use super::*;
use crate::{
    projects::{CommandSpec, ComposeConfig, Project, WorkloadKind},
    services::{Action, Services},
};
use std::{fs, path::PathBuf, thread};

fn wait(services: &mut Services, index: usize, status: &str) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        services.poll();
        if services.views()[index].status == status {
            return Ok(());
        }
        ensure!(
            Instant::now() < deadline,
            "Waiting for {status}: {}",
            services.views()[index].detail
        );
        thread::sleep(Duration::from_millis(50));
    }
}

pub(crate) fn fixture(folder: &Path) -> Result<WorkloadConfig> {
    fs::create_dir_all(folder)?;
    let file = folder.join("compose.json");
    fs::write(
        &file,
        r#"{"services":{"server":{"image":"postgres:16-alpine","entrypoint":["/bin/sh","-c"],"command":["trap 'echo normal-stop; exit 0' TERM; echo compose-ready; for i in $$(seq 60); do sleep 1; done"],"volumes":["data:/scratch"],"stop_signal":"SIGTERM"}},"volumes":{"data":{}}}"#,
    )?;
    use_ephemeral_image_storage(&file)?;
    let executable = std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .context("Missing Program Files")?
        .join("Docker/Docker/resources/bin/docker.exe");
    Ok(WorkloadConfig {
        environment: Default::default(),
        id: Uuid::new_v4(),
        name: "Compose fixture".into(),
        kind: WorkloadKind::DockerCompose,
        start: CommandSpec {
            executable,
            arguments: String::new(),
        },
        folder: Some(folder.into()),
        stop: None,
        stop_timeout_seconds: 6,
        readiness: Default::default(),
        readiness_timeout_seconds: 10,
        dependencies: Vec::new(),
        compose: Some(ComposeConfig {
            file,
            context: String::new(),
        }),
    })
}

struct Owned(Services, Option<(cli::Cli, String)>);

fn use_ephemeral_image_storage(file: &Path) -> Result<()> {
    let mut document: serde_json::Value = serde_json::from_slice(&fs::read(file)?)?;
    for service in document["services"]
        .as_object_mut()
        .context("No fixture services")?
        .values_mut()
    {
        // These diagnostics use only the image's shell, not its database volume.
        service["tmpfs"] = serde_json::json!(["/var/lib/postgresql/data"]);
    }
    fs::write(file, serde_json::to_vec(&document)?)?;
    Ok(())
}

struct UnverifiedFixture {
    cli: cli::Cli,
    id: String,
}
impl UnverifiedFixture {
    fn new(owned: &Owned) -> Result<Self> {
        let (original, namespace) = owned.1.as_ref().unwrap();
        let mut cli = cli::Cli::new(
            &original.executable,
            &original.folder,
            Arc::new(AtomicBool::new(false)),
        );
        cli.host = original.host.clone();
        let id = cli
            .run(
                &cli::args(&[
                    "container",
                    "create",
                    "--tmpfs",
                    "/var/lib/postgresql/data",
                    "--label",
                    &format!("com.docker.compose.project={namespace}"),
                    "--entrypoint",
                    "/bin/true",
                    "postgres:16-alpine",
                ]),
                10,
            )?
            .trim()
            .to_owned();
        ensure!(
            id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "Unexpected diagnostic container identity"
        );
        Ok(Self { cli, id })
    }
    fn remove(&mut self) -> Result<()> {
        if !self.id.is_empty() {
            self.cli
                .run(&cli::args(&["container", "rm", &self.id]), 10)?;
            self.id.clear();
        }
        Ok(())
    }
}
impl Drop for UnverifiedFixture {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}
impl Owned {
    fn new(project: &Project, cleanup: bool) -> Result<Self> {
        let config = &project.workloads[0];
        let mut cli = cli::Cli::new(
            &config.start.executable,
            config.folder(project.folder.as_deref())?,
            Arc::new(AtomicBool::new(false)),
        );
        let context: serde_json::Value =
            serde_json::from_str(&cli.run(&cli::args(&["context", "inspect"]), 10)?)?;
        cli.host = context[0]["Endpoints"]["docker"]["Host"]
            .as_str()
            .context("No Docker endpoint")?
            .into();
        let namespace = controller::namespace(project.id, config.id);
        Ok(Self(
            Services::new(project)?,
            cleanup.then_some((cli, namespace)),
        ))
    }
    fn ids(&mut self) -> Result<Vec<String>> {
        let (cli, namespace) = self.1.as_mut().context("No owned diagnostic namespace")?;
        let result = cli.run(
            &cli::args(&[
                "container",
                "ls",
                "--all",
                "--no-trunc",
                "--filter",
                &format!("label=com.docker.compose.project={namespace}"),
                "--format",
                "{{.ID}}",
            ]),
            10,
        )?;
        Ok(result.lines().map(str::to_owned).collect())
    }
    fn cleanup(&mut self) -> Result<()> {
        self.0.finish()?;
        if let Some((mut cli, namespace)) = self.1.take() {
            for (kind, name) in [
                ("network", format!("{namespace}_default")),
                ("volume", format!("{namespace}_data")),
            ] {
                if let Ok(output) = cli.run(
                    &cli::args(&[kind, "inspect", "--format", "{{json .Labels}}", &name]),
                    10,
                ) {
                    let labels: serde_json::Value = serde_json::from_str(&output)?;
                    ensure!(
                        labels["com.docker.compose.project"] == namespace,
                        "Refusing cleanup of an unverified diagnostic resource"
                    );
                    cli.run(&cli::args(&[kind, "rm", &name]), 10)?;
                }
            }
        }
        Ok(())
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        for row in self.0.views() {
            if row.live {
                let _ = self.0.act(row.id, Action::ForceStop);
            }
        }
        if let Err(error) = self.cleanup() {
            eprintln!("Owned Compose fixture cleanup: {error:#}");
        }
    }
}

fn conditions(folder: &Path) -> Result<()> {
    let config = fixture(folder)?;
    fs::write(
        &config.compose.as_ref().unwrap().file,
        serde_json::to_vec(&serde_json::json!({
            "services": {
            "db": {"image":"postgres:16-alpine", "stop_signal":"SIGTERM", "entrypoint":["/bin/sh","-c"], "command":["trap 'exit 0' TERM; touch /tmp/ready; echo db-ready; for i in $$(seq 60); do sleep 1; done"], "healthcheck":{"test":["CMD","test","-f","/tmp/ready"],"interval":"1s","timeout":"1s","retries":3}},
                "init": {"image":"postgres:16-alpine", "entrypoint":["/bin/sh","-c"], "command":["echo init-finished; exit 0"]},
            "web": {"image":"postgres:16-alpine", "stop_signal":"SIGTERM", "entrypoint":["/bin/sh","-c"], "command":["trap 'exit 0' TERM; echo web-ready; echo literal=$$LITERAL; for i in $$(seq 60); do sleep 1; done"], "environment":{"LITERAL":"$$HOME"}, "depends_on":{"db":{"condition":"service_healthy"},"init":{"condition":"service_completed_successfully"}}}
            }
        }))?,
    )?;
    use_ephemeral_image_storage(&config.compose.as_ref().unwrap().file)?;
    let mut project = Project::from_form(None, "Private Compose conditions exercise", "", None)?;
    let mut action = crate::services::fixture_config(
        &folder.join("action"),
        "success",
        WorkloadKind::StartupAction,
    )?;
    action.dependencies.push(config.id);
    project.workloads = vec![config, action];
    let mut owned = Owned::new(&project, true)?;
    owned.0.start_all();
    wait(&mut owned.0, 1, "Completed")?;
    let deadline = Instant::now() + Duration::from_secs(8);
    while !owned.0.views()[0].output.contains("literal=$HOME") {
        ensure!(
            Instant::now() < deadline,
            "Literal Compose environment was changed or output was lost: {}",
            owned.0.views()[0].output
        );
        thread::sleep(Duration::from_millis(100));
        owned.0.poll();
    }
    owned.cleanup()?;
    println!(
        "PASS Compose healthy/completed prerequisites, Jurabox dependent action, preserved literal dollars, and scoped refusal of unverified containers"
    );
    Ok(())
}

fn startup_exit(folder: &Path) -> Result<()> {
    let config = fixture(folder)?;
    let file = &config.compose.as_ref().unwrap().file;
    let mut document: serde_json::Value = serde_json::from_slice(&fs::read(file)?)?;
    document["services"]["server"]["command"] =
        serde_json::json!(["echo intentional-startup-exit; exit 7"]);
    fs::write(file, serde_json::to_vec(&document)?)?;
    let id = config.id;
    let mut project = Project::from_form(None, "Private startup exit exercise", "", None)?;
    project.workloads.push(config);
    let mut owned = Owned::new(&project, true)?;
    owned.0.start_all();
    wait(&mut owned.0, 0, "Failed")?;
    let ids = owned.ids()?;
    ensure!(
        !owned.0.views()[0].live && owned.0.views()[0].retained,
        "Startup exit was hidden or lost ownership"
    );
    owned.0.act(id, Action::Start)?;
    wait(&mut owned.0, 0, "Failed")?;
    ensure!(ids == owned.ids()?, "Retry replaced an exited container");
    owned.cleanup()?;
    println!("PASS startup exit reports Failed and explicit Retry reuses the exited container");
    Ok(())
}

pub fn run() -> Result<()> {
    let temporary = tempfile::Builder::new()
        .prefix("jurabox compose 测试 ")
        .tempdir()?;
    let mut config = fixture(temporary.path())?;
    // Isolated names and labels ensure these diagnostics never select existing
    // user containers, even if they use the same image or service name.
    let mut project = Project::from_form(None, "Private Compose exercise", "", None)?;
    let project_id = project.id;
    let id = config.id;
    project.workloads.push(config.clone());
    let mut owned = Owned::new(&project, true)?;
    ensure!(
        owned.0.start_all().is_empty(),
        "Could not queue Compose start"
    );
    wait(&mut owned.0, 0, "Running")?;
    ensure!(
        owned.0.views()[0].live && owned.0.views()[0].retained,
        "Containers were not retained as owned work"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !owned.0.views()[0].output.contains("compose-ready") {
        ensure!(
            Instant::now() < deadline,
            "Container output was not captured"
        );
        thread::sleep(Duration::from_millis(200));
        owned.0.poll();
    }
    owned.0.act(id, Action::Start)?;
    let original_ids = owned.ids()?;
    ensure!(owned.0.views()[0].live, "Start lost an existing container");
    // A second unverified session must not adopt this one's namespace.
    let mut other = Owned::new(&project, false)?;
    other.0.start_all();
    wait(&mut other.0, 0, "Failed")?;
    ensure!(
        other.0.views()[0]
            .detail
            .contains("another or older session")
            && owned.0.views()[0].live,
        "The second session adopted or altered the first one's containers"
    );
    owned.0.act(id, Action::Stop)?;
    ensure!(
        owned.0.views()[0].status == "Stopped" && owned.0.views()[0].retained,
        "Normal Stop removed ownership or failed"
    );
    // Editing the source after launch cannot replace the session's snapshot.
    fs::write(
        &config.compose.as_ref().unwrap().file,
        "invalid source edited after launch",
    )?;
    owned.0.act(id, Action::Start)?;
    wait(&mut owned.0, 0, "Running")?;
    ensure!(
        original_ids == owned.ids()?,
        "Stop/Start replaced containers instead of reusing them"
    );
    {
        let mut unverified = UnverifiedFixture::new(&owned)?;
        ensure!(
            owned.0.act(id, Action::ForceStop).is_err(),
            "Force stop accepted an unverified namespace member"
        );
        let running = unverified.cli.run(
            &cli::args(&[
                "container",
                "inspect",
                "--format",
                "{{json .State.Running}}",
                &original_ids[0],
            ]),
            10,
        )?;
        ensure!(
            running.trim() == "true",
            "Ownership failure stopped an existing container"
        );
        unverified.remove()?;
    }
    owned.0.finish()?;
    ensure!(
        !owned.0.views()[0].retained,
        "Ending the session left its containers owned"
    );
    println!(
        "PASS real Compose creation, container ownership, duplicate prevention, output, normal Stop/Start, frozen setup, unverified-session conflict, and session cleanup"
    );
    owned.cleanup()?;
    conditions(&temporary.path().join("conditions"))?;
    startup_exit(&temporary.path().join("startup-exit"))?;

    config = fixture(&temporary.path().join("stubborn"))?;
    fs::write(
        &config.compose.as_ref().unwrap().file,
        r#"{"services":{"server":{"image":"postgres:16-alpine","entrypoint":["/bin/sh","-c"],"command":["trap '' TERM; echo stubborn-ready; for i in $$(seq 60); do sleep 1; done"],"stop_signal":"SIGTERM"}}}"#,
    )?;
    use_ephemeral_image_storage(&config.compose.as_ref().unwrap().file)?;
    config.stop_timeout_seconds = 1;
    let id = config.id;
    let mut project = Project::from_form(None, "Private stubborn Compose exercise", "", None)?;
    project.workloads.push(config);
    let mut stubborn = Owned::new(&project, true)?;
    stubborn.0.start_all();
    wait(&mut stubborn.0, 0, "Running")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while !stubborn.0.views()[0].output.contains("stubborn-ready") {
        ensure!(
            Instant::now() < deadline,
            "Stubborn fixture did not initialize"
        );
        thread::sleep(Duration::from_millis(100));
        stubborn.0.poll();
    }
    ensure!(
        stubborn.0.act(id, Action::Stop).is_err() && stubborn.0.views()[0].live,
        "Normal Stop forced a stubborn container to exit"
    );
    stubborn.0.act(id, Action::ForceStop)?;
    stubborn.0.finish()?;
    stubborn.cleanup()?;
    println!(
        "PASS normal stop timeout retains the container; explicit Force stop terminates only verified IDs"
    );
    println!(
        "Compose checks passed for private project {project_id}; private containers, networks, and volumes cleaned up."
    );
    Ok(())
}
