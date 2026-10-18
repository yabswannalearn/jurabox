use crate::{
    platform::managed_process::ManagedProcess,
    projects::{Project, WorkloadConfig, WorkloadKind},
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

mod check;
mod fixture;
pub(crate) use check::fixture_config;
pub use check::run as check;
pub use fixture::run as fixture;

#[derive(Clone, Copy)]
pub enum Action {
    Start,
    Stop,
    ForceStop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub status: String,
    pub detail: String,
    pub output: String,
    pub error: bool,
    pub live: bool,
    pub can_start: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Pending,
    Running,
    Completed,
    Failed,
    Stopping,
    StopFailed,
    Stopped,
}

struct Entry {
    config: WorkloadConfig,
    folder: PathBuf,
    process: Option<ManagedProcess>,
    stop_process: Option<ManagedProcess>,
    status: Status,
    detail: String,
    deadline: Option<Instant>,
    capture_drained: bool,
}

fn active(process: &Option<ManagedProcess>) -> bool {
    process
        .as_ref()
        .is_some_and(|process| process.active().unwrap_or(true))
}

impl Entry {
    fn live(&self) -> bool {
        active(&self.process) || active(&self.stop_process)
    }

    fn poll(&mut self) {
        let live = self.live();
        if self.deadline.is_some() {
            if !live {
                self.status = Status::Stopped;
                self.detail = "Stopped normally.".into();
                self.deadline = None;
            } else if self
                .stop_process
                .as_ref()
                .and_then(|process| process.exit_code().ok().flatten())
                .is_some_and(|code| code != 0)
            {
                self.status = Status::StopFailed;
                self.detail = "The stop command failed. The workload is still running; check its output or explicitly Force stop.".into();
                self.deadline = None;
            } else if Instant::now() >= self.deadline.unwrap() {
                self.status = Status::StopFailed;
                self.detail = "Shutdown timed out. The workload or stop command is still running. Retry Stop or explicitly Force stop.".into();
                self.deadline = None;
            }
        } else if self.status == Status::Running {
            match self
                .process
                .as_ref()
                .and_then(|process| process.exit_code().ok().flatten())
            {
                Some(code) if !live => {
                    if self.config.kind == WorkloadKind::StartupAction && code == 0 {
                        self.status = Status::Completed;
                        self.detail = "Startup action completed successfully.".into();
                    } else {
                        self.status = Status::Failed;
                        self.detail = format!(
                            "Command exited with code {code}. {}",
                            if self.config.kind == WorkloadKind::CommandService {
                                "This service was expected to keep running. Retry when ready."
                            } else {
                                "Check the output, then Retry."
                            }
                        );
                    }
                }
                Some(code) => {
                    self.detail = format!(
                        "Launcher exited with code {code}; owned child processes are still running. No replacement will be started."
                    )
                }
                None => {}
            }
        } else if self.status == Status::StopFailed && !live {
            self.status = Status::Stopped;
            self.detail = "The workload and its stop command have exited.".into();
        }
        // Latch this only during polling, before taking the next view. If EOF
        // arrives later, one more poll still publishes the final captured bytes.
        self.capture_drained = [&self.process, &self.stop_process]
            .into_iter()
            .flatten()
            .all(|process| !process.output_pending());
    }

    fn start(&mut self) -> Result<String> {
        self.poll();
        if self.live() {
            return Ok(format!(
                "{} already has a running workload. No duplicate started.",
                self.config.name
            ));
        }
        self.process = None;
        self.stop_process = None;
        self.deadline = None;
        self.capture_drained = true;
        let arguments = self
            .config
            .start
            .arguments
            .replace("{folder}", &self.folder.to_string_lossy());
        match ManagedProcess::start(&self.config.start.executable, &arguments, &self.folder) {
            Ok(process) => {
                self.detail = "Command started. Readiness: process running.".into();
                self.process = Some(process);
                self.capture_drained = false;
                self.status = Status::Running;
                Ok(format!("{} started.", self.config.name))
            }
            Err(error) => {
                self.status = Status::Failed;
                self.detail = format!("{error:#}");
                Err(error).with_context(|| format!("Cannot start {}", self.config.name))
            }
        }
    }

    fn request_stop(&mut self) -> Result<()> {
        self.poll();
        if !self.live() {
            self.status = Status::Stopped;
            return Ok(());
        }
        if active(&self.stop_process) {
            bail!(
                "{}: the previous stop command is still running. Wait or explicitly Force stop.",
                self.config.name
            );
        }
        let Some(stop) = &self.config.stop else {
            self.status = Status::StopFailed;
            self.detail = "No normal stop command is configured in this session. The workload stays running; explicitly Force stop if needed.".into();
            bail!("{}: {}", self.config.name, self.detail);
        };
        let arguments = stop
            .arguments
            .replace("{folder}", &self.folder.to_string_lossy());
        match ManagedProcess::start(&stop.executable, &arguments, &self.folder) {
            Ok(process) => {
                self.stop_process = Some(process);
                self.capture_drained = false;
            }
            Err(error) => {
                self.status = Status::StopFailed;
                self.detail = format!(
                    "Cannot run the stop command: {error:#}. The workload remains managed."
                );
                return Err(error).with_context(|| format!("Cannot stop {}", self.config.name));
            }
        }
        self.status = Status::Stopping;
        self.deadline =
            Some(Instant::now() + Duration::from_secs(self.config.stop_timeout_seconds as u64));
        self.detail = "Normal shutdown requested. No automatic force escalation.".into();
        Ok(())
    }

    fn force_stop(&mut self) -> Result<String> {
        for process in [&self.process, &self.stop_process].into_iter().flatten() {
            if process.active()? {
                process.force_stop()?;
            }
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.live() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        ensure!(
            !self.live(),
            "Force stop has not finished. The workload remains managed."
        );
        self.deadline = None;
        self.status = Status::Stopped;
        self.detail = "Force stopped this workload and its owned child processes.".into();
        Ok(format!("{} force stopped.", self.config.name))
    }

    fn view(&self) -> View {
        let mut output = self
            .process
            .as_ref()
            .map(ManagedProcess::output)
            .unwrap_or_default();
        if let Some(stop) = &self.stop_process {
            let text = stop.output();
            if !text.is_empty() {
                output.push_str("\n[Stop command]\n");
                output.push_str(&text);
            }
        }
        let live = self.live();
        View {
            id: self.config.id,
            name: self.config.name.clone(),
            kind: match self.config.kind {
                WorkloadKind::CommandService => "Command service",
                WorkloadKind::StartupAction => "Startup action",
            }
            .into(),
            status: match self.status {
                Status::Pending => "Not started",
                Status::Running => "Running",
                Status::Completed => "Completed",
                Status::Failed => "Failed",
                Status::Stopping => "Stopping",
                Status::StopFailed => "Stop failed",
                Status::Stopped => "Stopped",
            }
            .into(),
            detail: self.detail.clone(),
            output,
            error: matches!(self.status, Status::Failed | Status::StopFailed),
            live,
            can_start: !live && self.deadline.is_none(),
        }
    }
}

#[derive(Default)]
pub struct Services {
    entries: Vec<Entry>,
}

impl Services {
    pub fn new(project: &Project) -> Result<Self> {
        let entries = project
            .workloads
            .iter()
            .map(|config| {
                Ok(Entry {
                    config: config.clone(),
                    folder: config.folder(project.folder.as_deref())?.to_path_buf(),
                    process: None,
                    stop_process: None,
                    status: Status::Pending,
                    detail: "Uses this project session's saved setup.".into(),
                    deadline: None,
                    capture_drained: true,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self { entries })
    }

    pub fn start_all(&mut self) -> Vec<String> {
        self.entries
            .iter_mut()
            .filter_map(|entry| entry.start().err().map(|error| format!("{error:#}")))
            .collect()
    }

    pub fn views(&self) -> Vec<View> {
        self.entries.iter().map(Entry::view).collect()
    }
    pub fn live(&self) -> bool {
        self.entries.iter().any(Entry::live)
    }
    pub fn activity(&self) -> bool {
        self.live() || self.entries.iter().any(|entry| !entry.capture_drained)
    }
    pub fn poll(&mut self) {
        for entry in &mut self.entries {
            entry.poll();
        }
    }

    pub fn act(&mut self, id: Uuid, action: Action) -> Result<String> {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.config.id == id)
            .context("This workload is not in the open session. Relaunch the saved setup first.")?;
        match action {
            Action::Start => entry.start(),
            Action::ForceStop => entry.force_stop(),
            Action::Stop => {
                entry.request_stop()?;
                while entry.deadline.is_some() {
                    thread::sleep(Duration::from_millis(50));
                    entry.poll();
                }
                ensure!(!entry.live(), "{}: {}", entry.config.name, entry.detail);
                Ok(format!("{} stopped.", entry.config.name))
            }
        }
    }

    pub fn stop_all(&mut self) -> Result<()> {
        for entry in &mut self.entries {
            let _ = entry.request_stop();
        }
        while self.entries.iter().any(|entry| entry.deadline.is_some()) {
            thread::sleep(Duration::from_millis(50));
            self.poll();
        }
        let failures: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.live())
            .map(|entry| format!("{}: {}", entry.config.name, entry.detail))
            .collect();
        ensure!(failures.is_empty(), "{}", failures.join("\n"));
        Ok(())
    }
}
