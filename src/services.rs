use crate::{
    platform::managed_process::ManagedProcess,
    projects::{Project, Readiness, WorkloadConfig, WorkloadKind},
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

mod check;
mod compose;
pub use compose::check::run as compose_check;
mod coordination;
mod fixture;
mod probe;
mod readiness_check;
pub(crate) use check::fixture_config;
pub use check::run as check;
pub use fixture::run as fixture;
pub(crate) use readiness_check::health_config;

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum Action {
    Start,
    Stop,
    ForceStop,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct View {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub status: String,
    pub detail: String,
    pub output: String,
    pub error: bool,
    pub live: bool,
    pub retained: bool,
    pub can_start: bool,
    pub can_stop: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Pending,
    Waiting,
    Checking,
    Starting,
    NotReady,
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
    readiness_deadline: Option<Instant>,
    ready: bool,
    attempt: Uuid,
    probe: Option<probe::Probe>,
    last_probe: Instant,
    preflight_clear: bool,
    compose: Option<compose::Runtime>,
}

fn active(process: &Option<ManagedProcess>) -> bool {
    process
        .as_ref()
        .is_some_and(|process| process.active().unwrap_or(true))
}

impl Entry {
    fn workload_live(&self) -> bool {
        self.compose
            .as_ref()
            .map_or_else(|| active(&self.process), compose::Runtime::live)
    }
    fn live(&self) -> bool {
        self.workload_live() || active(&self.stop_process)
    }

    fn poll(&mut self) {
        if let Some(compose) = &mut self.compose {
            compose.poll();
            if let Some(error) = compose.error().filter(|_| !compose.busy()) {
                self.ready = false;
                self.detail = error.to_owned();
                if self.deadline.is_none() {
                    self.status = if compose.live() {
                        Status::NotReady
                    } else {
                        Status::Failed
                    };
                    self.readiness_deadline = None;
                }
            } else if self.status == Status::Starting && compose.started() && compose.available() {
                if self.config.readiness == Readiness::ProcessRunning {
                    self.status = Status::Running;
                    self.ready = true;
                    self.detail = "The owned Compose containers are running.".into();
                } else if self.readiness_deadline.is_none() {
                    self.readiness_deadline = Some(
                        compose.started_at().unwrap_or_else(Instant::now)
                            + Duration::from_secs(self.config.readiness_timeout_seconds as u64),
                    );
                    self.last_probe = Instant::now() - Duration::from_secs(1);
                }
            } else if matches!(
                self.status,
                Status::Starting | Status::Running | Status::NotReady
            ) && compose.started()
                && !compose.available()
            {
                self.status = Status::Failed;
                self.ready = false;
                self.detail = "A Compose container exited or became unavailable. Other workloads stay running. Retry when ready.".into();
            }
        }
        let live = self.live();
        if let Some((completed, result)) = self.probe.as_ref().and_then(|probe| probe.result()) {
            let probe = self.probe.take().unwrap();
            if probe.attempt == self.attempt {
                if probe.preflight && self.status == Status::Checking {
                    if result.unwrap_or(false) {
                        self.status = Status::Failed;
                        self.detail = "The configured readiness endpoint is already accepting connections. No command started. Leave the other workload alone; edit the endpoint or resolve the conflict, then Retry.".into();
                    } else {
                        self.preflight_clear = true;
                        self.status = Status::Waiting;
                    }
                } else if !probe.preflight
                    && self.status == Status::Starting
                    && self.workload_live()
                    && self
                        .readiness_deadline
                        .is_some_and(|deadline| completed <= deadline)
                {
                    match result {
                        Ok(true) => {
                            self.status = Status::Running;
                            self.ready = true;
                            self.readiness_deadline = None;
                            self.detail = "Startup readiness passed.".into();
                        }
                        Ok(false) => {
                            self.detail =
                                "Waiting for the configured startup readiness condition…".into()
                        }
                        Err(error) => {
                            self.detail = format!("Readiness check has not passed: {error}")
                        }
                    }
                }
            }
        }
        if self
            .readiness_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.readiness_deadline = None;
            self.status = Status::NotReady;
            self.ready = false;
            self.detail = "Readiness timed out. The workload stays running. Retry checks this same instance; dependent services keep waiting.".into();
        }
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
        } else if self.compose.is_none()
            && matches!(
                self.status,
                Status::Running | Status::Starting | Status::NotReady
            )
        {
            match self
                .process
                .as_ref()
                .and_then(|process| process.exit_code().ok().flatten())
            {
                Some(code) if !live => {
                    self.ready = false;
                    self.readiness_deadline = None;
                    self.attempt = Uuid::new_v4();
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
                Some(code) if self.status == Status::Running => {
                    self.detail = format!(
                        "Launcher exited with code {code}; owned child processes are still running. No replacement will be started."
                    )
                }
                _ => {}
            }
        } else if self.status == Status::StopFailed && !live {
            self.status = Status::Stopped;
            self.detail = "The workload and its stop command have exited.".into();
        }
        if self.status == Status::Starting
            && self.readiness_deadline.is_some()
            && self.probe.is_none()
            && self.last_probe.elapsed() >= Duration::from_millis(500)
        {
            self.spawn_probe(false);
        }
        // Latch this only during polling, before taking the next view. If EOF
        // arrives later, one more poll still publishes the final captured bytes.
        self.capture_drained = [&self.process, &self.stop_process]
            .into_iter()
            .flatten()
            .all(|process| !process.output_pending());
    }

    fn spawn_probe(&mut self, preflight: bool) {
        if self.probe.is_some() {
            return;
        }
        self.last_probe = Instant::now();
        match probe::Probe::start(self.attempt, self.config.readiness.clone(), preflight) {
            Ok(probe) => {
                self.probe = Some(probe);
                if preflight {
                    self.status = Status::Checking;
                    self.detail =
                        "Checking for an existing workload at the configured endpoint…".into();
                }
            }
            Err(error) => {
                self.status = if preflight {
                    Status::Failed
                } else {
                    Status::NotReady
                };
                self.readiness_deadline = None;
                self.detail = format!("{error:#}");
            }
        }
    }

    fn own_ready(&self) -> bool {
        if self.config.kind == WorkloadKind::StartupAction {
            self.status == Status::Completed && !self.live()
        } else {
            self.ready
                && self.workload_live()
                && self.deadline.is_none()
                && self
                    .compose
                    .as_ref()
                    .is_none_or(compose::Runtime::available)
        }
    }

    fn queue_start(&mut self) -> String {
        self.poll();
        if self.compose.is_some()
            && (matches!(
                self.status,
                Status::NotReady | Status::StopFailed | Status::Failed
            ))
        {
            if let Some(compose) = &mut self.compose
                && let Err(error) = compose.start()
            {
                self.detail = format!("{error:#}");
                return self.detail.clone();
            }
            self.status = Status::Starting;
            self.ready = false;
            self.readiness_deadline = None;
            self.attempt = Uuid::new_v4();
            return format!(
                "Checking and starting only the owned containers for {}.",
                self.config.name
            );
        }
        if self.live() {
            if matches!(self.status, Status::NotReady | Status::StopFailed)
                && !active(&self.stop_process)
            {
                self.attempt = Uuid::new_v4();
                self.status = Status::Starting;
                self.ready = false;
                self.readiness_deadline = Some(
                    Instant::now()
                        + Duration::from_secs(self.config.readiness_timeout_seconds as u64),
                );
                self.last_probe = Instant::now() - Duration::from_secs(1);
                self.detail =
                    "Rechecking readiness of the existing workload. No duplicate started.".into();
                if self.config.readiness == Readiness::ProcessRunning {
                    self.status = Status::Running;
                    self.ready = self.config.kind == WorkloadKind::CommandService;
                    self.readiness_deadline = None;
                } else {
                    self.spawn_probe(false);
                }
            }
            return format!(
                "{} already has a running workload. No duplicate started.",
                self.config.name
            );
        }
        if !matches!(self.status, Status::Checking | Status::Waiting) {
            self.attempt = Uuid::new_v4();
            self.status = Status::Waiting;
            self.preflight_clear = false;
            self.detail = "Waiting for prerequisites…".into();
        }
        format!("Startup requested for {}.", self.config.name)
    }

    fn cancel_startup(&mut self) {
        self.attempt = Uuid::new_v4();
        self.readiness_deadline = None;
        self.ready = false;
        if matches!(self.status, Status::Waiting | Status::Checking) {
            self.status = Status::Stopped;
        }
    }

    fn start(&mut self) -> Result<String> {
        self.poll();
        if self.live() {
            return Ok(format!(
                "{} already has a running workload. No duplicate started.",
                self.config.name
            ));
        }
        if let Some(compose) = &mut self.compose {
            compose.start()?;
            self.status = Status::Starting;
            self.ready = false;
            self.detail = "Preparing the project's Compose containers…".into();
            return Ok(format!("Starting {} in Docker…", self.config.name));
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
                self.ready = self.config.kind == WorkloadKind::CommandService
                    && self.config.readiness == Readiness::ProcessRunning;
                if self.config.readiness != Readiness::ProcessRunning {
                    self.status = Status::Starting;
                    self.ready = false;
                    self.readiness_deadline = Some(
                        Instant::now()
                            + Duration::from_secs(self.config.readiness_timeout_seconds as u64),
                    );
                    self.last_probe = Instant::now() - Duration::from_secs(1);
                    self.detail = "Command started; checking startup readiness…".into();
                    self.spawn_probe(false);
                }
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
        self.cancel_startup();
        if let Some(compose) = &mut self.compose {
            compose.stop()?;
            self.status = Status::Stopping;
            self.deadline =
                Some(Instant::now() + Duration::from_secs(self.config.stop_timeout_seconds as u64));
            self.detail =
                "Sending normal stop signals to the owned containers. No automatic Force stop."
                    .into();
            return Ok(());
        }
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
        self.cancel_startup();
        self.status = Status::StopFailed;
        self.detail = "Force stop has not finished; the workload remains managed.".into();
        if let Some(compose) = &mut self.compose {
            compose.force()?;
        }
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
        if let Some(compose) = &self.compose {
            output = compose.output().to_owned();
        }
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
                WorkloadKind::DockerCompose => "Docker Compose",
            }
            .into(),
            status: match self.status {
                Status::Pending => "Not started",
                Status::Waiting => "Waiting",
                Status::Checking => "Checking endpoint",
                Status::Starting => "Starting",
                Status::NotReady => "Not ready",
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
            error: matches!(
                self.status,
                Status::Failed | Status::StopFailed | Status::NotReady
            ),
            live,
            retained: self
                .compose
                .as_ref()
                .is_some_and(compose::Runtime::retained),
            can_start: ((!live && !matches!(self.status, Status::Waiting | Status::Checking))
                || (matches!(
                    self.status,
                    Status::NotReady | Status::StopFailed | Status::Failed
                ) && !active(&self.stop_process)))
                && self.deadline.is_none(),
            can_stop: live
                || matches!(
                    self.status,
                    Status::Waiting | Status::Checking | Status::Starting
                ),
        }
    }
}

#[derive(Default)]
pub struct Services {
    entries: Vec<Entry>,
}

impl Services {
    pub fn new(project: &Project) -> Result<Self> {
        project.validate()?;
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
                    readiness_deadline: None,
                    ready: false,
                    attempt: Uuid::new_v4(),
                    probe: None,
                    last_probe: Instant::now() - Duration::from_secs(1),
                    preflight_clear: false,
                    compose: if config.kind == WorkloadKind::DockerCompose {
                        Some(compose::Runtime::new(
                            project.id,
                            config,
                            config.folder(project.folder.as_deref())?,
                        )?)
                    } else {
                        None
                    },
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self { entries })
    }

    pub fn start_all(&mut self) -> Vec<String> {
        for entry in &mut self.entries {
            entry.queue_start();
        }
        self.schedule()
    }

    pub fn views(&self) -> Vec<View> {
        let ready = self.ready_ids();
        self.entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let mut view = entry.view();
                let missing = self.missing_dependencies(index, &ready);
                if !missing.is_empty() && (entry.status == Status::Waiting || view.live) {
                    if entry.status == Status::Waiting {
                        view.detail = format!(
                            "Waiting for {}. Readiness timeout has not started.",
                            missing.join(", ")
                        );
                    } else {
                        view.detail.push_str(&format!(
                            " Prerequisite unavailable: {}. This workload stays running.",
                            missing.join(", ")
                        ));
                        view.error = true;
                    }
                }
                view
            })
            .collect()
    }
    pub fn live(&self) -> bool {
        self.entries.iter().any(Entry::live)
    }
    pub fn activity(&self) -> bool {
        self.live()
            || self.entries.iter().any(|entry| {
                !entry.capture_drained
                    || entry
                        .compose
                        .as_ref()
                        .is_some_and(compose::Runtime::activity)
                    || entry.probe.is_some()
                    || matches!(
                        entry.status,
                        Status::Running
                            | Status::Starting
                            | Status::Checking
                            | Status::NotReady
                            | Status::StopFailed
                    )
            })
    }
    pub fn poll(&mut self) {
        for entry in &mut self.entries {
            entry.poll();
        }
        self.schedule();
    }

    pub fn resume_monitoring(&self) {
        for entry in &self.entries {
            if let Some(compose) = &entry.compose {
                compose.monitor(true);
            }
        }
    }

    pub fn pause_monitoring(&mut self) {
        for entry in &mut self.entries {
            if let Some(compose) = &entry.compose {
                compose.monitor(false);
            }
            if matches!(entry.status, Status::Waiting | Status::Checking) {
                entry.cancel_startup();
                entry.detail = "Startup paused when Jurabox closed. Use Start to continue.".into();
            }
        }
    }

    pub fn act(&mut self, id: Uuid, action: Action) -> Result<String> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.config.id == id)
            .context("This workload is not in the open session. Relaunch the saved setup first.")?;
        if matches!(action, Action::Start) {
            let message = self.entries[index].queue_start();
            self.schedule();
            ensure!(
                self.entries[index].status != Status::Failed,
                "{}: {}",
                self.entries[index].config.name,
                self.entries[index].detail
            );
            return Ok(message);
        }
        if matches!(action, Action::Stop) {
            let dependents = self.live_dependents(id);
            ensure!(
                dependents.is_empty(),
                "Stop dependent workloads first: {}. No workload was stopped.",
                dependents.join(", ")
            );
        }
        let entry = &mut self.entries[index];
        match action {
            Action::Start => unreachable!(),
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
            entry.cancel_startup();
        }
        for index in self.shutdown_order() {
            let dependents = self.live_dependents(self.entries[index].config.id);
            if !dependents.is_empty() && self.entries[index].live() {
                self.entries[index].status = Status::StopFailed;
                self.entries[index].detail = format!(
                    "Kept running because dependent workloads have not stopped: {}.",
                    dependents.join(", ")
                );
                continue;
            }
            let entry = &mut self.entries[index];
            let _ = entry.request_stop();
            while entry.deadline.is_some() {
                thread::sleep(Duration::from_millis(50));
                entry.poll();
            }
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

    pub fn finish(&mut self) -> Result<()> {
        self.stop_all()?;
        for entry in &mut self.entries {
            if let Some(compose) = &mut entry.compose {
                compose.release()?;
            }
        }
        Ok(())
    }
}
