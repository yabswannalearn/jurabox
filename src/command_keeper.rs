//! One query-only helper per launching host pins named command jobs. Windows
//! removes a temporary object's name at last-handle close, before its last process
//! necessarily exits. A durable completion seal distinguishes quiescence from
//! lost ownership evidence. This helper never launches or terminates workloads.
use crate::{
    platform::managed_process::{job_active, open_recorded_job},
    recovery::Record,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    os::windows::io::OwnedHandle,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Mutex, OnceLock, mpsc},
    thread,
    time::Duration,
};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Seal {
    version: u32,
    claim: Uuid,
    finished: bool,
}

pub fn read_seal(path: &Path, claim: Uuid) -> Result<bool> {
    let saved = Record::new(path.into()).read::<Seal>()?.context(
        "Missing command completion record; ownership is uncertain and replacement is blocked.",
    )?;
    ensure!(
        saved.version == 1 && saved.claim == claim,
        "Invalid command completion record; it has been preserved."
    );
    Ok(saved.finished)
}

pub fn finish(path: &Path, claim: Uuid) -> Result<()> {
    if read_seal(path, claim)? {
        return Ok(());
    }
    Record::new(path.into()).save(&Seal {
        version: 1,
        claim,
        finished: true,
    })
}

#[derive(Serialize, Deserialize)]
enum Request {
    Register(Uuid, PathBuf),
    Arm(Uuid),
}
#[derive(Serialize, Deserialize)]
struct Reply {
    claim: Uuid,
    error: Option<String>,
}
static HOST: OnceLock<Mutex<Option<Host>>> = OnceLock::new();
struct Host {
    input: ChildStdin,
    replies: mpsc::Receiver<Reply>,
    _child: Child,
}

fn send(writer: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}
fn frames<R: BufRead>(mut reader: R, mut receive: impl FnMut(&[u8]) -> Result<()>) -> Result<()> {
    loop {
        let mut bytes = Vec::new();
        // Bound each private local frame before deserialization/allocation.
        let count =
            std::io::Read::take(&mut reader, 16 * 1024 + 1).read_until(b'\n', &mut bytes)?;
        if count == 0 {
            return Ok(());
        }
        ensure!(
            count <= 16 * 1024 && bytes.last() == Some(&b'\n'),
            "Invalid command-keeper frame."
        );
        receive(&bytes)?;
    }
}

impl Host {
    fn spawn() -> Result<Self> {
        let mut child = Command::new(std::env::current_exe()?)
            .arg("_command-keeper")
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let input = child.stdin.take().context("Missing keeper input")?;
        let output = child.stdout.take().context("Missing keeper output")?;
        let (sender, replies) = mpsc::channel();
        thread::Builder::new()
            .name("command-keeper-replies".into())
            .spawn(move || {
                let _ = frames(BufReader::new(output), |bytes| {
                    sender.send(serde_json::from_slice(bytes)?)?;
                    Ok(())
                });
            })?;
        Ok(Self {
            input,
            replies,
            _child: child,
        })
    }
}

pub struct Registration(Uuid);
impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(host) = HOST.get()
            && let Ok(mut host) = host.lock()
            && let Some(host) = host.as_mut()
        {
            // Creation has either completed or failed. EOF also arms pending
            // registrations if the owner dies at any point in CreateProcessW.
            let _ = send(&mut host.input, &Request::Arm(self.0));
        }
    }
}

pub fn register(claim: Uuid, path: &Path) -> Result<Registration> {
    ensure!(
        !path.exists(),
        "A completion record already exists; command launch blocked."
    );
    Record::new(path.into()).save(&Seal {
        version: 1,
        claim,
        finished: false,
    })?;
    let mut slot = HOST
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| anyhow::anyhow!("Command keeper lock failed"))?;
    if slot.is_none() {
        *slot = Some(Host::spawn()?);
    }
    let host = slot.as_mut().unwrap();
    let result = (|| {
        send(&mut host.input, &Request::Register(claim, path.into()))?;
        let reply = host
            .replies
            .recv_timeout(Duration::from_secs(5))
            .context("Command keeper did not confirm ownership; nothing was launched")?;
        ensure!(
            reply.claim == claim,
            "Mismatched command keeper acknowledgement"
        );
        if let Some(error) = reply.error {
            bail!("{error}");
        }
        Ok(Registration(claim))
    })();
    if result.is_err() {
        *slot = None;
    }
    result
}

struct Lease {
    job: OwnedHandle,
    path: PathBuf,
    armed: bool,
}
enum Event {
    Request(Request),
    Gone,
}

pub fn run() -> Result<()> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = frames(std::io::stdin().lock(), |bytes| {
            sender.send(Event::Request(serde_json::from_slice(bytes)?))?;
            Ok(())
        });
        let _ = sender.send(Event::Gone);
    });
    let mut leases: HashMap<Uuid, Lease> = HashMap::new();
    let mut attached = true;
    let mut output = std::io::stdout().lock();
    loop {
        let event = if leases.is_empty() {
            receiver
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        } else {
            receiver.recv_timeout(Duration::from_millis(250))
        };
        match event {
            Ok(Event::Request(Request::Register(claim, path))) => {
                let result = (|| -> Result<()> {
                    ensure!(!leases.contains_key(&claim), "Command already registered");
                    ensure!(
                        path.is_absolute() && !read_seal(&path, claim)?,
                        "Invalid command completion record"
                    );
                    let job = open_recorded_job(claim, 0x0004)?
                        .context("The original job disappeared before launch")?;
                    leases.insert(
                        claim,
                        Lease {
                            job,
                            path,
                            armed: false,
                        },
                    );
                    Ok(())
                })();
                if send(
                    &mut output,
                    &Reply {
                        claim,
                        error: result.err().map(|error| format!("{error:#}")),
                    },
                )
                .is_err()
                {
                    attached = false;
                }
            }
            Ok(Event::Request(Request::Arm(claim))) => {
                if let Some(lease) = leases.get_mut(&claim) {
                    lease.armed = true;
                }
            }
            Ok(Event::Gone) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                attached = false;
                for lease in leases.values_mut() {
                    lease.armed = true;
                }
                // Avoid spinning on a permanently disconnected receiver.
                thread::sleep(Duration::from_millis(250));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        leases.retain(|claim, lease| {
            // Query failures retain ownership. Seal verified quiescence while
            // the exact job is open. A failed seal remains uncertain on disk and
            // blocks later recovery, but must not keep an idle helper forever.
            if lease.armed && job_active(&lease.job).is_ok_and(|active| !active) {
                let _ = finish(&lease.path, *claim);
                false
            } else {
                true
            }
        });
        if !attached && leases.is_empty() {
            return Ok(());
        }
    }
}
