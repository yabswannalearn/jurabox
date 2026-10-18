use super::{
    Snapshot,
    cli::{Cli, args},
    model::{self, Container, Plan},
};
use crate::{
    platform::managed_process::{ManagedProcess, OUTPUT_LIMIT},
    projects::WorkloadConfig,
};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

const INSPECT: &str = r#"{"Id":{{json .Id}},"Created":{{json .Created}},"Labels":{{json .Config.Labels}},"State":{{json .State}},"StopSignal":{{json .Config.StopSignal}},"Restart":{{json .HostConfig.RestartPolicy.Name}}}"#;

pub(super) fn namespace(project: Uuid, workload: Uuid) -> String {
    format!(
        "jurabox-{}-{}",
        &project.simple().to_string()[..16],
        workload.simple()
    )
}

pub(super) struct Controller {
    cli: Cli,
    config: WorkloadConfig,
    project: String,
    namespace: String,
    owner: String,
    engine: Option<String>,
    snapshot: Option<tempfile::TempDir>,
    plan: Option<Plan>,
    containers: BTreeMap<String, Container>,
    logs: BTreeMap<String, ManagedProcess>,
    uncertain: bool,
    attempted: bool,
    started: bool,
    started_at: Option<Instant>,
    stopping: Option<Instant>,
    note: String,
}

impl Controller {
    pub fn new(
        project: Uuid,
        config: WorkloadConfig,
        folder: &Path,
        cancel: Arc<AtomicBool>,
    ) -> Self {
        Self {
            cli: Cli::new(&config.start.executable, folder, cancel),
            namespace: namespace(project, config.id),
            config,
            project: project.to_string(),
            owner: Uuid::new_v4().to_string(),
            engine: None,
            snapshot: None,
            plan: None,
            containers: BTreeMap::new(),
            logs: BTreeMap::new(),
            uncertain: false,
            attempted: false,
            started: false,
            started_at: None,
            stopping: None,
            note: String::new(),
        }
    }
    fn engine(&mut self) -> Result<()> {
        if self.cli.host.is_empty() {
            let compose = self
                .config
                .compose
                .as_ref()
                .context("Missing Compose setup")?;
            let mut command = args(&["context", "inspect"]);
            if !compose.context.trim().is_empty() {
                command.push(compose.context.clone());
            }
            let value: Value = serde_json::from_str(&self.cli.run(&command, 10)?)?;
            let host = value[0]["Endpoints"]["docker"]["Host"]
                .as_str()
                .context("Docker context has no endpoint")?;
            ensure!(
                host.starts_with("npipe:////./pipe/") && !host.contains(['\0', '\r', '\n']),
                "Choose a local Docker Desktop context. Remote Docker endpoints are not supported yet."
            );
            self.cli.host = host.into();
        }
        let info: Vec<String> = serde_json::from_str(&self.cli.run(
            &args(&["info", "--format", "[{{json .ID}},{{json .OSType}}]"]),
            10,
        )?)?;
        ensure!(
            info.len() == 2 && !info[0].is_empty() && info[1] == "linux",
            "Use Docker Desktop with its Linux engine."
        );
        if let Some(engine) = &self.engine {
            ensure!(
                engine == &info[0],
                "Docker engine changed. Existing ownership cannot be verified; containers are left alone."
            );
        } else {
            self.engine = Some(info[0].clone());
        }
        Ok(())
    }
    fn compose_args(&self, file: &Path) -> Vec<String> {
        vec![
            "compose".into(),
            "--ansi".into(),
            "never".into(),
            "--project-name".into(),
            self.namespace.clone(),
            "--project-directory".into(),
            self.cli.folder.to_string_lossy().into_owned(),
            "--file".into(),
            file.to_string_lossy().into_owned(),
        ]
    }
    fn list(&mut self, filters: &[String]) -> Result<Vec<String>> {
        let mut command = args(&[
            "container",
            "ls",
            "--all",
            "--no-trunc",
            "--format",
            "{{.ID}}",
        ]);
        for filter in filters {
            command.extend(["--filter".into(), filter.clone()]);
        }
        let output = self.cli.run(&command, 10)?;
        let ids: Vec<_> = output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.trim().to_owned())
            .collect();
        ensure!(
            ids.iter()
                .all(|id| id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
                && ids.len() <= 32,
            "Docker returned an unexpected container list."
        );
        Ok(ids)
    }
    fn inspect(&mut self, ids: &[String]) -> Result<Vec<Container>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut command = args(&["container", "inspect", "--format", INSPECT]);
        command.extend_from_slice(ids);
        let output = self.cli.run(&command, 10)?;
        let values = output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<std::result::Result<Vec<Container>, _>>()?;
        ensure!(
            values.len() == ids.len() && values.iter().all(|value| ids.contains(&value.id)),
            "Docker returned mismatched container identities."
        );
        Ok(values)
    }
    fn verify(&self, container: &Container) -> Result<()> {
        let labels = &container.labels;
        ensure!(
            labels.get(model::OWNER) == Some(&self.owner)
                && labels.get(model::PROJECT) == Some(&self.project)
                && labels.get(model::WORKLOAD) == Some(&self.config.id.to_string())
                && labels.get("com.docker.compose.project") == Some(&self.namespace),
            "An unverified container is using this namespace. It is left alone."
        );
        ensure!(
            self.plan
                .as_ref()
                .is_some_and(|plan| plan.order.iter().any(|name| name == container.service())),
            "Docker returned an unexpected Compose service."
        );
        if let Some(previous) = self.containers.get(&container.id) {
            ensure!(
                previous.created == container.created,
                "Container identity changed; refusing lifecycle control."
            );
        }
        Ok(())
    }
    pub fn refresh(&mut self) -> Result<()> {
        self.cli.reap()?;
        if !self.attempted {
            return Ok(());
        }
        self.uncertain = true;
        self.engine()?;
        let ids = self.list(&[format!(
            "label=com.docker.compose.project={}",
            self.namespace
        )])?;
        let values = self.inspect(&ids)?;
        let mut seen = BTreeMap::new();
        for value in values {
            self.verify(&value)?;
            ensure!(
                !seen
                    .values()
                    .any(|other: &Container| other.service() == value.service()),
                "Multiple containers use the same Compose service; no replacement will be started."
            );
            seen.insert(value.id.clone(), value);
        }
        let missing = self.containers.keys().any(|id| !seen.contains_key(id));
        self.containers = seen;
        self.uncertain = false;
        ensure!(
            !missing,
            "An owned container was removed outside Jurabox. Relaunch to rebuild the session."
        );
        Ok(())
    }
    pub fn start(&mut self) -> Result<()> {
        self.stopping = None;
        self.engine()?;
        if self.plan.is_none() {
            let compose_file = self
                .config
                .compose
                .as_ref()
                .context("Missing Compose setup")?
                .file
                .clone();
            ensure!(
                compose_file.is_file(),
                "Compose file does not exist: {}",
                compose_file.display()
            );
            let runtime_directory = crate::projects::ProjectStore::default_path()?
                .parent()
                .context("No Jurabox data folder")?
                .join("runtime");
            fs::create_dir_all(&runtime_directory)?;
            let directory = tempfile::Builder::new()
                .prefix("compose-")
                .tempdir_in(runtime_directory)?;
            let file = directory.path().join("compose.json");
            let mut command = self.compose_args(&compose_file);
            command.extend(args(&["config", "--format", "json", "--output"]));
            command.push(file.to_string_lossy().into_owned());
            self.cli.run(&command, 30)?;
            ensure!(
                fs::metadata(&file)?.len() <= 1024 * 1024,
                "Resolved Compose setup exceeds 1 MiB."
            );
            let document = serde_json::from_slice(&fs::read(&file)?)?;
            let plan = model::plan(
                document,
                &self.namespace,
                &self.owner,
                &self.project,
                &self.config.id.to_string(),
            )?;
            fs::write(&file, serde_json::to_vec(&plan.document)?)?;
            self.plan = Some(plan);
            self.snapshot = Some(directory);
        }
        if !self.attempted {
            let existing = self.list(&[
                format!("label={}={}", model::PROJECT, self.project),
                format!("label={}={}", model::WORKLOAD, self.config.id),
            ])?;
            let namespace = self.list(&[format!(
                "label=com.docker.compose.project={}",
                self.namespace
            )])?;
            ensure!(
                existing.is_empty() && namespace.is_empty(),
                "Containers from another or older session already exist. They remain unmanaged; no duplicate was created. Resolve them in Docker before launching again."
            );
            self.attempted = true;
            self.uncertain = true;
            let file = self.snapshot.as_ref().unwrap().path().join("compose.json");
            let mut command = self.compose_args(&file);
            command.extend(args(&[
                "create",
                "--no-build",
                "--no-recreate",
                "--pull",
                "missing",
                "--quiet-pull",
            ]));
            let creation = self.cli.run(&command, 180);
            let discovery = self.refresh();
            self.note = creation
                .as_ref()
                .map(Clone::clone)
                .unwrap_or_else(|error| format!("{error:#}"));
            creation?;
            discovery?;
        } else {
            self.refresh()?;
        }
        let order = self.plan.as_ref().unwrap().order.clone();
        ensure!(
            self.containers.len() == order.len(),
            "Some Compose containers are missing. Relaunch after resolving the failed creation; no partial replacement will be created."
        );
        self.started_at = Some(Instant::now());
        let deadline = self.started_at.unwrap()
            + Duration::from_secs(self.config.readiness_timeout_seconds as u64);
        let mut failures = Vec::new();
        for name in order {
            let result = (|| -> Result<()> {
                let dependencies = self.plan.as_ref().unwrap().dependencies[&name].clone();
                loop {
                    let mut ready = true;
                    for (dependency, condition) in &dependencies {
                        let item = self
                            .containers
                            .values()
                            .find(|item| item.service() == dependency)
                            .context("Missing Compose prerequisite")?;
                        ensure!(
                            item.active() || item.state.exit_code == 0,
                            "Compose prerequisite {dependency} exited with code {}",
                            item.state.exit_code
                        );
                        ready &= match condition.as_str() {
                            "service_healthy" => {
                                item.state.running
                                    && item
                                        .state
                                        .health
                                        .as_ref()
                                        .is_some_and(|health| health.status == "healthy")
                            }
                            "service_completed_successfully" => {
                                !item.active()
                                    && item.state.exit_code == 0
                                    && self.logs.contains_key(&item.id)
                            }
                            _ => item.active(),
                        };
                    }
                    if ready {
                        break;
                    }
                    ensure!(
                        Instant::now() < deadline,
                        "Compose prerequisite for {name} did not become ready. Existing containers stay running; Retry uses the same containers."
                    );
                    thread::sleep(Duration::from_millis(250));
                    self.refresh()?;
                }
                let item = self
                    .containers
                    .values()
                    .find(|item| item.service() == name)
                    .unwrap()
                    .clone();
                item.normal_signal()?;
                let finite_completed = self.plan.as_ref().unwrap().finite.contains(&name)
                    && !item.active()
                    && item.state.exit_code == 0
                    && self.logs.contains_key(&item.id);
                if !item.active() && !finite_completed {
                    self.cli.run(&args(&["container", "start", &item.id]), 30)?;
                }
                if !self.logs.contains_key(&item.id) || self.logs[&item.id].exit_code()?.is_some() {
                    let process = self.cli.spawn(&args(&[
                        "container",
                        "logs",
                        "--follow",
                        "--tail",
                        "200",
                        &item.id,
                    ]))?;
                    self.logs.insert(item.id.clone(), process);
                }
                self.refresh()?;
                Ok(())
            })();
            if let Err(error) = result {
                failures.push(format!("{name}: {error:#}"));
            }
        }
        self.started = failures.is_empty();
        ensure!(failures.is_empty(), "{}", failures.join("\n"));
        Ok(())
    }
    pub fn stop(&mut self) -> Result<()> {
        self.stopping = Some(Instant::now());
        if !self.attempted {
            return Ok(());
        }
        self.refresh()?;
        let mut order = self.plan.as_ref().unwrap().order.clone();
        order.reverse();
        let mut failures = Vec::new();
        let deadline =
            Instant::now() + Duration::from_secs(self.config.stop_timeout_seconds as u64);
        for service in order {
            let Some(item) = self
                .containers
                .values()
                .find(|item| item.service() == service)
                .cloned()
            else {
                continue;
            };
            if !item.active() {
                continue;
            }
            if self.containers.values().any(|other| {
                other.active()
                    && self
                        .plan
                        .as_ref()
                        .unwrap()
                        .depends_on(other.service(), &service)
            }) {
                failures.push(format!(
                    "{service}: kept running because a dependent has not stopped"
                ));
                continue;
            }
            let result = (|| -> Result<()> {
                ensure!(
                    !item.state.paused,
                    "{service} is paused. Resume it in Docker or use explicit Force stop."
                );
                let signal = item.normal_signal()?.to_owned();
                self.cli.run(
                    &args(&["container", "kill", "--signal", &signal, &item.id]),
                    10,
                )?;
                // A prerequisite remains alive if its dependent does not finish.
                loop {
                    self.refresh()?;
                    if !self.containers.get(&item.id).is_some_and(Container::active) {
                        break;
                    }
                    ensure!(
                        Instant::now() < deadline,
                        "{service} did not stop. Its prerequisites stay running. Force stop remains explicit."
                    );
                    thread::sleep(Duration::from_millis(100));
                }
                Ok(())
            })();
            if let Err(error) = result {
                failures.push(format!("{error:#}"));
            }
        }
        ensure!(failures.is_empty(), "{}", failures.join("\n"));
        Ok(())
    }
    pub fn force(&mut self) -> Result<()> {
        self.cli.cancel_client()?;
        if !self.attempted {
            return Ok(());
        }
        self.refresh()?;
        let ids: Vec<_> = self
            .containers
            .values()
            .filter(|item| item.active())
            .map(|item| item.id.clone())
            .collect();
        if !ids.is_empty() {
            let mut command = args(&["container", "kill", "--signal", "SIGKILL"]);
            command.extend(ids);
            self.cli.run(&command, 10)?;
        }
        self.refresh()?;
        ensure!(
            !self.containers.values().any(Container::active),
            "Some owned containers are still running."
        );
        self.stopping = None;
        Ok(())
    }
    pub fn release(&mut self) -> Result<()> {
        if !self.attempted {
            return Ok(());
        }
        self.refresh()?;
        ensure!(
            !self.containers.values().any(Container::active),
            "Stop the owned containers before ending the session."
        );
        for item in self.containers.values().cloned().collect::<Vec<_>>() {
            self.verify(&item)?;
            self.cli.run(&args(&["container", "rm", &item.id]), 10)?;
            self.containers.remove(&item.id);
        }
        self.close_log_clients();
        self.attempted = false;
        self.started = false;
        Ok(())
    }
    pub fn close_log_clients(&mut self) {
        for (_, process) in std::mem::take(&mut self.logs) {
            if process.active().unwrap_or(false) {
                let _ = process.force_stop();
            }
        }
    }
    pub fn available(&self) -> bool {
        self.started
            && !self.uncertain
            && self.plan.as_ref().is_some_and(|plan| {
                self.containers.len() == plan.order.len()
                    && self.containers.values().all(|item| {
                        if plan.finite.iter().any(|name| name == item.service()) {
                            !item.active() && item.state.exit_code == 0
                        } else {
                            item.state.running && !item.state.paused && !item.state.restarting
                        }
                    })
            })
    }
    pub fn activity(&self) -> bool {
        self.uncertain
            || self.containers.values().any(Container::active)
            || self.cli.pending.is_some()
            || self.logs.values().any(ManagedProcess::output_pending)
    }
    pub fn poll_interval(&self) -> Duration {
        if self.stopping.is_some_and(|time| {
            time.elapsed() < Duration::from_secs(self.config.stop_timeout_seconds as u64 + 2)
        }) {
            Duration::from_millis(250)
        } else {
            Duration::from_secs(2)
        }
    }
    pub fn snapshot(&self, error: Option<String>) -> Snapshot {
        let mut output = self.note.clone();
        for (id, process) in &self.logs {
            output.push_str(&format!(
                "\n[{}]\n{}",
                self.containers
                    .get(id)
                    .map(Container::service)
                    .unwrap_or("container"),
                process.output()
            ));
        }
        if output.len() > OUTPUT_LIMIT {
            let mut at = output.len() - OUTPUT_LIMIT;
            while !output.is_char_boundary(at) {
                at += 1;
            }
            output = format!("[Earlier Compose output omitted]\n{}", &output[at..]);
        }
        Snapshot {
            live: self.uncertain
                || self.containers.values().any(Container::active)
                || self.cli.pending.is_some(),
            retained: self.attempted,
            available: self.available(),
            started: self.started,
            started_at: self.started_at,
            activity: self.activity(),
            error,
            output,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn container_control_requires_the_same_owner_labels_and_creation_identity() {
        let directory = tempfile::tempdir().unwrap();
        let config = crate::services::check::fixture_config(
            directory.path(),
            "long",
            crate::projects::WorkloadKind::CommandService,
        )
        .unwrap();
        let mut controller = Controller::new(
            Uuid::new_v4(),
            config,
            directory.path(),
            Arc::new(AtomicBool::new(false)),
        );
        controller.plan = Some(
            model::plan(
                json!({"services":{"server":{"image":"test"}}}),
                &controller.namespace,
                &controller.owner,
                &controller.project,
                &controller.config.id.to_string(),
            )
            .unwrap(),
        );
        let mut item: Container = serde_json::from_value(json!({"Id":"a".repeat(64),"Created":"original","Labels":{model::OWNER:controller.owner,model::PROJECT:controller.project,model::WORKLOAD:controller.config.id.to_string(),"com.docker.compose.project":controller.namespace,"com.docker.compose.service":"server"},"State":{"Running":true,"Paused":false,"Restarting":false,"ExitCode":0},"StopSignal":"SIGTERM","Restart":"no"})).unwrap();
        assert!(controller.verify(&item).is_ok());
        for signal in ["KILL", "SIGKILL", "9", "09", "+9"] {
            item.stop_signal = signal.into();
            assert!(item.normal_signal().is_err());
        }
        item.stop_signal = "SIGINT".into();
        assert!(item.normal_signal().is_ok());
        item.restart = "always".into();
        assert!(item.normal_signal().is_err());
        item.restart = "no".into();
        controller.containers.insert(item.id.clone(), item.clone());
        item.created = "replacement".into();
        assert!(controller.verify(&item).is_err());
        item.created = "original".into();
        item.labels
            .insert(model::OWNER.into(), "someone-else".into());
        assert!(controller.verify(&item).is_err());
    }
}
