use crate::projects::WorkloadConfig;
use anyhow::{Context, Result, ensure};
use std::{
    collections::HashSet,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use uuid::Uuid;

pub(crate) mod check;
mod cli;
mod controller;
mod model;
mod recovery;

#[derive(Clone, Default)]
struct Snapshot {
    live: bool,
    retained: bool,
    available: bool,
    started: bool,
    started_at: Option<Instant>,
    activity: bool,
    error: Option<String>,
    output: String,
}
#[derive(Clone)]
enum Request {
    Start,
    Stop,
    Force,
    Release,
    Monitor(bool),
    Shutdown,
    Record(std::path::PathBuf, bool),
}
struct Event {
    snapshot: Snapshot,
    completed: Option<Uuid>,
}

pub(super) struct Runtime {
    sender: mpsc::Sender<(Uuid, Request)>,
    receiver: mpsc::Receiver<Event>,
    pending: HashSet<Uuid>,
    snapshot: Snapshot,
    cancel: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Runtime {
    pub fn new(project: Uuid, config: &WorkloadConfig, folder: &Path) -> Result<Self> {
        let (sender, requests) = mpsc::channel();
        let (events, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let mut controller =
            controller::Controller::new(project, config.clone(), folder, cancel.clone());
        let worker = std::thread::Builder::new()
            .name("compose-workload".into())
            .spawn(move || {
                let mut last_error = None;
                let mut monitoring = true;
                loop {
                    let request = if monitoring && controller.activity() {
                        match requests.recv_timeout(controller.poll_interval()) {
                            Ok(request) => Some(request),
                            Err(mpsc::RecvTimeoutError::Timeout) => None,
                            Err(_) => break,
                        }
                    } else {
                        match requests.recv() {
                            Ok(request) => Some(request),
                            Err(_) => break,
                        }
                    };
                    let result = match request.as_ref().map(|(_, request)| request) {
                        Some(Request::Record(path, restore)) => {
                            controller.attach_record(path.clone(), *restore)
                        }
                        Some(Request::Start) => controller.start(),
                        Some(Request::Stop) => controller.stop(),
                        Some(Request::Force) => controller.force(),
                        Some(Request::Release) => controller.release(),
                        Some(Request::Shutdown) => break,
                        Some(Request::Monitor(enabled)) => {
                            monitoring = *enabled;
                            if !enabled {
                                continue;
                            }
                            controller.refresh()
                        }
                        None => controller.refresh(),
                    };
                    if request.is_some() || result.is_err() || controller.available() {
                        last_error = result.err().map(|error| format!("{error:#}"));
                    }
                    let event = Event {
                        snapshot: controller.snapshot(last_error.clone()),
                        completed: request.map(|(id, _)| id),
                    };
                    if events.send(event).is_err() {
                        break;
                    }
                }
                // Dropping the manager never grants permission to kill containers.
                controller.close_log_clients();
            })
            .context("Cannot start the Compose worker")?;
        Ok(Self {
            sender,
            receiver,
            pending: HashSet::new(),
            snapshot: Snapshot::default(),
            cancel,
            worker: Some(worker),
        })
    }
    fn request(&mut self, request: Request) -> Result<Uuid> {
        let id = Uuid::new_v4();
        self.sender
            .send((id, request))
            .context("Compose worker stopped; ownership is uncertain")?;
        self.pending.insert(id);
        Ok(id)
    }
    pub fn record(&mut self, path: std::path::PathBuf, restore: bool) -> Result<()> {
        let id = self.request(Request::Record(path, restore))?;
        self.wait(id)
    }
    pub fn monitor(&self, enabled: bool) {
        // This changes observation only, never service ownership or lifecycle.
        // It must not count as an outstanding Start/Stop operation.
        let _ = self.sender.send((Uuid::nil(), Request::Monitor(enabled)));
    }
    pub fn poll(&mut self) {
        loop {
            match self.receiver.try_recv() {
                Ok(event) => {
                    self.snapshot = event.snapshot;
                    if let Some(id) = event.completed {
                        self.pending.remove(&id);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if self.snapshot.retained || !self.pending.is_empty() {
                        self.snapshot.live = true;
                        self.snapshot.error = Some("Compose worker stopped. Ownership is uncertain; do not start replacements.".into());
                    }
                    break;
                }
            }
        }
    }
    pub fn start(&mut self) -> Result<()> {
        self.snapshot.error = None;
        self.request(Request::Start).map(|_| ())
    }
    pub fn stop(&mut self) -> Result<()> {
        self.request(Request::Stop).map(|_| ())
    }
    fn wait(&mut self, id: Uuid) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            self.poll();
            if !self.pending.contains(&id) {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "Docker operation is still pending. Its containers remain managed."
            );
            std::thread::sleep(Duration::from_millis(25));
        }
        ensure!(
            self.snapshot.error.is_none(),
            "{}",
            self.snapshot.error.as_deref().unwrap_or_default()
        );
        Ok(())
    }
    pub fn force(&mut self) -> Result<()> {
        self.cancel.store(true, Ordering::SeqCst);
        let id = self.request(Request::Force)?;
        self.wait(id)
    }
    pub fn release(&mut self) -> Result<()> {
        let id = self.request(Request::Release)?;
        self.wait(id)
    }
    pub fn live(&self) -> bool {
        // Outstanding operations retain lifecycle ownership until acknowledged.
        self.snapshot.live || !self.pending.is_empty()
    }
    pub fn begin_release(&mut self) -> Result<()> {
        self.snapshot.error = None;
        self.request(Request::Release).map(|_| ())
    }
    pub fn begin_force(&mut self) -> Result<()> {
        self.cancel.store(true, Ordering::SeqCst);
        self.request(Request::Force).map(|_| ())
    }
    pub fn busy(&self) -> bool {
        !self.pending.is_empty()
    }
    pub fn retained(&self) -> bool {
        self.snapshot.retained
    }
    pub fn available(&self) -> bool {
        self.snapshot.available && self.pending.is_empty() && self.snapshot.error.is_none()
    }
    pub fn started(&self) -> bool {
        self.snapshot.started && self.pending.is_empty()
    }
    pub fn started_at(&self) -> Option<Instant> {
        self.snapshot.started_at
    }
    pub fn activity(&self) -> bool {
        self.snapshot.activity || !self.pending.is_empty()
    }
    pub fn error(&self) -> Option<&str> {
        self.snapshot.error.as_deref()
    }
    pub fn output(&self) -> &str {
        &self.snapshot.output
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // Session removal follows successful finish/release. Wait for the worker
        // to close its read-only log clients and delete its private snapshot before
        // a normal host exit can end the process. Crashes still require recovery.
        let _ = self.sender.send((Uuid::nil(), Request::Shutdown));
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
