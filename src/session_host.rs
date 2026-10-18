//! A single same-binary helper retains original ownership handles across UI exits.
//! Crash recovery uses write-ahead command capabilities, never PID/name adoption.
mod check;
mod identity;
mod pipe;
mod recovery_check;
pub use check::run as check;
pub use recovery_check::run as recovery_check;

use crate::{
    platform::workspace::{Summary, WindowChoices, WindowOption, Workspaces},
    projects::Project,
    services::Action,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::windows::{fs::OpenOptionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub enum Request {
    Snapshot,
    Open(Project, bool),
    Close(Uuid),
    Windows(Uuid, Uuid, Uuid),
    Assign(Uuid, Uuid),
    Unassign(Uuid, Uuid),
    Service(Uuid, Uuid, Action),
    LeaveAndQuit,
    StopAndQuit,
}

#[derive(Serialize, Deserialize)]
pub struct Response {
    pub summary: Summary,
    pub choices: Option<std::result::Result<Vec<WindowOption>, String>>,
    pub quit: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    identity: identity::Identity,
    generation: Uuid,
    token: Uuid,
    projects: Vec<Project>,
}

impl Journal {
    fn pipe_name(&self) -> String {
        format!(r"\\.\pipe\Jurabox.Session.{}", self.generation)
    }
    fn read(path: &Path) -> Result<Option<Self>> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= 4 * 1024 * 1024,
            "Session journal is too large; it has been preserved."
        );
        let journal: Self = serde_json::from_slice(&bytes).context(
            "Cannot read the session journal. It has been preserved; new launches are blocked.",
        )?;
        ensure!(
            matches!(journal.version, 1 | 2),
            "Unsupported session journal version; it has been preserved."
        );
        Ok(Some(journal))
    }
    fn save(&self, path: &Path) -> Result<()> {
        let bytes = serde_json::to_vec(self)?;
        ensure!(
            bytes.len() < 4 * 1024 * 1024,
            "Session journal exceeds 4 MiB; launch blocked."
        );
        let mut temporary =
            tempfile::NamedTempFile::new_in(path.parent().context("Missing session folder")?)?;
        temporary.write_all(&bytes)?;
        temporary.write_all(b"\n")?;
        temporary.as_file().sync_all()?;
        temporary.persist(path).map_err(|error| error.error)?;
        Ok(())
    }
    fn validate_owner(&self) -> Result<()> {
        let own = identity::inspect(std::process::id())?;
        ensure!(
            self.identity.executable == own.executable && self.identity.session == own.session,
            "The saved session belongs to another Jurabox installation or Windows session. Its record has been preserved."
        );
        ensure!(
            identity::inspect(self.identity.pid).is_ok_and(|identity| identity == self.identity),
            "The background session process is gone. Services may still be running. New launches are blocked; the session record has been preserved."
        );
        Ok(())
    }

    fn allow_recovery(&self) -> Result<()> {
        ensure!(
            self.version == 2,
            "The lost session uses an older record without command ownership capabilities. Its record has been preserved; new launches are blocked."
        );
        ensure!(
            self.identity.executable == identity::inspect(std::process::id())?.executable,
            "The saved session belongs to another Jurabox installation; its record has been preserved."
        );
        ensure!(
            !self.projects.iter().any(|project| project
                .workloads
                .iter()
                .any(|workload| workload.kind == crate::projects::WorkloadKind::DockerCompose)),
            "The lost session includes Docker containers. Docker crash recovery is not available yet; its record has been preserved and new launches are blocked."
        );
        Ok(())
    }
}

pub struct Client {
    file: File,
    journal: Journal,
}

impl Client {
    pub fn connect(path: &Path) -> Result<Self> {
        ensure!(
            path.is_absolute(),
            "The session journal must have an absolute path."
        );
        let mut launched: Option<std::process::Child> = None;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let mut may_spawn = true;
            if let Some(journal) = Journal::read(path)? {
                match journal.validate_owner() {
                    Ok(()) => {
                        may_spawn = false;
                        if let Ok(file) = pipe::connect(&journal.pipe_name(), &journal.identity) {
                            return Ok(Self { file, journal });
                        }
                        // A live owner may be completing a request or detaching. Never replace it.
                    }
                    Err(_) if !journal.projects.is_empty() => journal.allow_recovery()?,
                    Err(_) => {}
                }
            }
            if let Some(child) = &mut launched
                && child.try_wait()?.is_some()
            {
                let mut bytes = Vec::new();
                if let Some(error) = child.stderr.take() {
                    error.take(8192).read_to_end(&mut bytes)?;
                }
                bail!(
                    "The background session could not start. {}",
                    String::from_utf8_lossy(&bytes).trim()
                );
            }
            if launched.is_none() && may_spawn {
                fs::create_dir_all(path.parent().context("Missing session folder")?)?;
                launched = Some(
                    Command::new(std::env::current_exe()?)
                        .arg("_session-host")
                        .arg(path)
                        .creation_flags(0x08000000)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::piped())
                        .spawn()
                        .context("Cannot start the background session process.")?,
                );
            }
            ensure!(
                Instant::now() < deadline,
                "Cannot connect to the background session process. No operation was sent. Reopen Jurabox to retry."
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn request(&mut self, request: Request) -> Result<Response> {
        pipe::write(&mut self.file, &(self.journal.token, request)).context(
            "Lost the session connection. The operation may have started; it will not be replayed.",
        )?;
        pipe::read(&mut self.file)
            .context("Lost the session response. Services remain with the background process. Reopen Jurabox to reconnect; the operation will not be replayed.")
    }
}

#[derive(Clone)]
enum Event {
    Connected(u32),
    Disconnected,
    Exit,
    Command(Request, mpsc::SyncSender<Response>),
    Desktop(winvd::DesktopEvent),
}
impl From<winvd::DesktopEvent> for Event {
    fn from(event: winvd::DesktopEvent) -> Self {
        Self::Desktop(event)
    }
}

fn serve(mut pipe: File, journal: Journal, sender: mpsc::Sender<Event>) -> Result<()> {
    loop {
        if let Ok(pid) = pipe::accept(&pipe, &journal.identity) {
            // A valid token is required before the host resumes monitoring.
            let mut attached = false;
            while let Ok((token, request)) = pipe::read::<(Uuid, Request)>(&mut pipe) {
                if token != journal.token {
                    break;
                }
                if !attached {
                    sender.send(Event::Connected(pid))?;
                    attached = true;
                }
                let detach = matches!(request, Request::LeaveAndQuit);
                let stop_host = matches!(request, Request::StopAndQuit);
                let (reply, response) = mpsc::sync_channel(1);
                sender.send(Event::Command(request, reply))?;
                let response = response.recv()?;
                let quit = response.quit;
                let written = pipe::write(&mut pipe, &response);
                if quit && written.is_ok() {
                    pipe::drain(&pipe);
                }
                if stop_host && quit {
                    sender.send(Event::Exit)?;
                    return written;
                }
                if written.is_err() || (detach && quit) {
                    break;
                }
            }
            if attached {
                sender.send(Event::Disconnected)?;
            }
        }
        // Retain the only server instance across reconnects. Recreating it can
        // race the old client closing its end after a leave-running response.
        unsafe {
            windows::Win32::System::Pipes::DisconnectNamedPipe(
                windows::Win32::Foundation::HANDLE(
                    std::os::windows::io::AsRawHandle::as_raw_handle(&pipe),
                ),
            )?;
        }
    }
}

pub fn run(path: &Path) -> Result<()> {
    ensure!(
        path.is_absolute(),
        "The session journal must have an absolute path."
    );
    fs::create_dir_all(path.parent().context("Missing session folder")?)?;
    // The non-shared OS file handle is the authority for one host per settings file.
    // Stale lock files are harmless; only a live kernel handle holds this lock.
    let _lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(path.with_extension("lock"))
        .context("Another session host owns these settings.")?;
    let root = path.with_extension("sessions");
    let previous = Journal::read(path)?;
    let recovering = previous
        .as_ref()
        .is_some_and(|journal| !journal.projects.is_empty());
    let mut workspaces =
        if let Some(previous) = previous.filter(|journal| !journal.projects.is_empty()) {
            previous.allow_recovery()?;
            Workspaces::recover(root, &previous.projects)?
        } else {
            Workspaces::recorded(root)
        };
    let mut journal = Journal {
        version: 2,
        identity: identity::inspect(std::process::id())?,
        generation: Uuid::new_v4(),
        token: Uuid::new_v4(),
        projects: workspaces.setups(),
    };
    let pipe = pipe::create(&journal.pipe_name())?;
    journal.save(path)?;
    let (sender, receiver) = mpsc::channel();
    let _listener = winvd::listen_desktop_events(sender.clone());
    let pipe_journal = journal.clone();
    thread::spawn(move || {
        let _ = serve(pipe, pipe_journal, sender);
    });
    let mut summary = Summary::default();
    let mut recovery_notice = recovering.then(|| if journal.projects.is_empty() {
        "The previous sessions have ended. Choose a project to open; nothing was started automatically.".to_string()
    } else {
        "Recovered the original command sessions. Missing services stay stopped. Use Start or Retry when ready; previous output is unavailable.".to_string()
    });
    let mut choices: Option<(Uuid, WindowChoices)> = None;
    let mut attached = false;
    let mut next_poll = Instant::now();
    loop {
        if attached && workspaces.service_activity() && Instant::now() >= next_poll {
            summary.workloads = workspaces.poll_services();
            next_poll = Instant::now() + Duration::from_millis(500);
        }
        let event = if attached && workspaces.service_activity() {
            match receiver.recv_timeout(next_poll.saturating_duration_since(Instant::now())) {
                Ok(event) => event,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
        } else {
            receiver.recv()?
        };
        match event {
            Event::Exit => return Ok(()),
            Event::Connected(pid) => {
                attached = true;
                workspaces.set_manager_process(pid);
                workspaces.resume_monitoring();
                workspaces.poll_services();
                next_poll = Instant::now() + Duration::from_millis(500);
                summary = workspaces.desktop_changed();
                if let Some(message) = recovery_notice.take() {
                    summary.message = message;
                }
            }
            Event::Disconnected => {
                attached = false;
                choices = None;
                workspaces.pause_monitoring();
                if let Err(error) = workspaces.checkpoint() {
                    summary.message = format!(
                        "Cannot save recovery state: {error:#}. Reopen Jurabox to resolve it."
                    );
                    summary.error = true;
                }
                if journal.projects.is_empty() {
                    return Ok(());
                }
            }
            Event::Desktop(event) => {
                if attached
                    && matches!(
                        event,
                        winvd::DesktopEvent::DesktopChanged { .. }
                            | winvd::DesktopEvent::DesktopDestroyed { .. }
                    )
                {
                    let mut next = workspaces.desktop_changed();
                    if matches!(event, winvd::DesktopEvent::DesktopDestroyed { .. })
                        && next.message.is_empty()
                    {
                        next.message = "A desktop was removed. Open the affected project to restore its workspace; apps stay running.".into();
                    }
                    if next.message.is_empty() {
                        next.message = summary.message;
                        next.error = summary.error;
                    }
                    summary = next;
                }
            }
            Event::Command(request, reply) => {
                let mut quit = false;
                let mut options = None;
                let mut finish_host = false;
                let snapshot = matches!(request, Request::Snapshot);
                let save = !matches!(request, Request::Snapshot | Request::Windows(..));
                let result =
                    match request {
                        Request::Snapshot => Ok(String::new()),
                        Request::Open(project, relaunch) => {
                            // Record intent before any process or container can be created.
                            // A host crash during partial startup must block replacement.
                            if !journal.projects.iter().any(|entry| entry.id == project.id) {
                                journal.projects.push(project.clone());
                            }
                            match journal.save(path) {
                                Ok(()) => workspaces.open(project, relaunch),
                                Err(error) => Err(error
                                    .context("Cannot record session ownership; launch blocked.")),
                            }
                        }
                        Request::Close(id) => workspaces.close(id),
                        Request::Service(project, workload, action) => {
                            workspaces.service_action(project, workload, action)
                        }
                        Request::Windows(ticket, project, app) => {
                            options = Some(match workspaces.window_choices(project, app) {
                                Ok(snapshot) => {
                                    let options = snapshot.options.clone();
                                    choices = Some((ticket, snapshot));
                                    Ok(options)
                                }
                                Err(error) => {
                                    choices = None;
                                    Err(format!("{error:#}"))
                                }
                            });
                            Ok(String::new())
                        }
                        Request::Assign(ticket, choice) => match &choices {
                            Some((current, snapshot)) if *current == ticket => {
                                workspaces.assign_choice(snapshot, choice)
                            }
                            _ => bail_response(
                                "That window list expired. Refresh it and choose again.",
                            ),
                        },
                        Request::Unassign(project, app) => workspaces.unassign_window(project, app),
                        Request::LeaveAndQuit => {
                            quit = true;
                            Ok(
                            "Services left running. Monitoring resumes when Jurabox opens again."
                                .into(),
                        )
                        }
                        Request::StopAndQuit => workspaces.stop_for_exit().map(|()| {
                            quit = true;
                            finish_host = true;
                            "Managed services stopped.".into()
                        }),
                    };
                // Snapshot requests use cached metadata; only lifecycle changes scan windows.
                if !matches!(&result, Ok(message) if message.is_empty()) || options.is_some() {
                    summary = workspaces.summary();
                }
                if !snapshot {
                    summary.message.clear();
                    summary.error = false;
                    match result {
                        Ok(message) => summary.message = message,
                        Err(error) => {
                            summary.message = format!("{error:#}");
                            summary.error = true;
                        }
                    }
                }
                let setups = if finish_host {
                    Vec::new()
                } else {
                    workspaces.setups()
                };
                if save {
                    journal.projects = setups;
                    if let Err(error) = workspaces.checkpoint().and_then(|()| journal.save(path)) {
                        summary.message = format!(
                            "Cannot save the session record: {error:#}. Keep Jurabox open."
                        );
                        summary.error = true;
                        quit = false;
                    }
                }
                if quit {
                    attached = false;
                }
                let _ = reply.send(Response {
                    summary: summary.clone(),
                    choices: options,
                    quit,
                });
            }
        }
    }
}

fn bail_response(message: &str) -> Result<String> {
    bail!("{message}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_or_future_journals_are_preserved() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("session.json");
        let future = Journal {
            version: 999,
            identity: identity::inspect(std::process::id()).unwrap(),
            generation: Uuid::new_v4(),
            token: Uuid::new_v4(),
            projects: Vec::new(),
        };
        for bytes in [b"{".to_vec(), serde_json::to_vec(&future).unwrap()] {
            fs::write(&path, &bytes).unwrap();
            assert!(Client::connect(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}
