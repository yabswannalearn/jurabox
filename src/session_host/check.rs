use super::*;
use crate::{
    projects::{CommandSpec, ComposeConfig, WorkloadConfig, WorkloadKind},
    services::fixture_config,
};

pub fn run() -> Result<()> {
    let temporary = tempfile::Builder::new()
        .prefix("jurabox session 测试 ")
        .tempdir()?;
    let path = temporary.path().join("projects.session-host.json");
    let starting_desktop =
        winvd::get_current_desktop().map_err(|error| anyhow::anyhow!("{error:?}"))?;
    let before = winvd::get_desktops().map_err(|error| anyhow::anyhow!("{error:?}"))?;
    let before_ids: Vec<_> = before
        .iter()
        .map(|desktop| desktop.get_id().unwrap())
        .collect();
    let mut fixture = Fixture {
        path: path.clone(),
        desktop: None,
        starting_desktop,
    };
    let folder = temporary.path().join("service");
    let config = fixture_config(&folder, "long", WorkloadKind::CommandService)?;
    let service_id = config.id;
    let desktop_name = format!("Jurabox reconnect check {}", Uuid::new_v4());
    let mut project = Project::from_form(None, &desktop_name, "", None)?;
    project.workloads.push(config);
    let action_folder = temporary.path().join("gate");
    let action = fixture_config(
        &action_folder,
        "delayed-success",
        WorkloadKind::StartupAction,
    )?;
    let waiting_folder = temporary.path().join("waiting");
    let mut waiting = fixture_config(&waiting_folder, "long", WorkloadKind::CommandService)?;
    waiting.dependencies.push(action.id);
    project.workloads.extend([action, waiting]);
    let compose = if std::env::var_os("JURABOX_CHECK_DOCKER").is_some() {
        let config = compose_fixture(temporary.path())?;
        let id = config.id;
        project.workloads.push(config);
        Some(id)
    } else {
        None
    };
    let mut client = Client::connect(&path)?;
    let host = Journal::read(&path)?.unwrap();
    let response = client.request(Request::Open(project.clone(), false))?;
    fixture.desktop = winvd::get_desktops().ok().and_then(|desktops| {
        desktops.into_iter().find(|desktop| {
            desktop.get_id().is_ok_and(|id| !before_ids.contains(&id))
                && desktop.get_name().ok().as_deref() == Some(&desktop_name)
        })
    });
    ensure!(!response.summary.error, "{}", response.summary.message);
    let first = wait(&mut client, |summary| {
        summary
            .workloads
            .first()
            .is_some_and(|(_, view)| view.live && !view.output.is_empty())
            && compose.is_none_or(|id| {
                summary.workloads.iter().any(|(_, view)| {
                    view.id == id
                        && view.status == "Running"
                        && view.output.contains("reconnect-compose-ready")
                })
            })
    })?;
    let container = compose.map(|id| container_id(&project, id)).transpose()?;
    let output = first.workloads[0].1.output.clone();
    let detail = first.workloads[0].1.detail.clone();
    ensure!(
        client.request(Request::LeaveAndQuit)?.quit,
        "Leave-running exit refused"
    );
    drop(client);
    fs::write(action_folder.join("allow-action"), "ready")?;
    thread::sleep(Duration::from_millis(150));
    ensure!(
        identity::inspect(host.identity.pid)? == host.identity,
        "Host did not survive UI exit"
    );
    let valid = Journal::read(&path)?.unwrap();
    let mut wrong_identity = valid.clone();
    wrong_identity.identity.created ^= 1;
    wrong_identity.save(&path)?;
    ensure!(
        Client::connect(&path).is_err(),
        "A live PID with the wrong creation identity was accepted"
    );
    valid.save(&path)?;
    let mut rejected = Client::connect(&path)?;
    rejected.journal.token = Uuid::new_v4();
    ensure!(
        rejected.request(Request::Snapshot).is_err(),
        "An unauthenticated session client was accepted"
    );
    drop(rejected);
    let mut client = Client::connect(&path)?;
    ensure!(
        Journal::read(&path)?.unwrap().generation == host.generation,
        "A replacement host was created"
    );
    let response = client.request(Request::Snapshot)?;
    if let Some(id) = compose {
        ensure!(
            Some(container_id(&project, id)?) == container,
            "Reconnect replaced a Compose container"
        );
    }
    ensure!(
        !response.summary.workloads[2].1.live
            && response.summary.workloads[2].1.status == "Stopped",
        "Reconnect launched a previously queued dependent without explicit Start"
    );
    ensure!(
        response.summary.open == vec![project.id],
        "Session setup was lost"
    );
    ensure!(
        response.summary.workloads[0].1.live
            && response.summary.workloads[0]
                .1
                .output
                .contains(output.trim()),
        "Output/control was not retained"
    );
    ensure!(
        response.summary.workloads[0].1.detail == detail,
        "The running command identity changed"
    );
    // A broken transport models abrupt manager exit; there is no detach request.
    drop(client);
    thread::sleep(Duration::from_millis(150));
    let mut client = Client::connect(&path)?;
    let reopened = client.request(Request::Open(project.clone(), false))?;
    ensure!(
        reopened.summary.workloads[0].1.detail == detail,
        "Switch restarted an owned service"
    );
    let rejected_choice = client.request(Request::Assign(Uuid::new_v4(), Uuid::new_v4()))?;
    ensure!(
        rejected_choice.summary.error,
        "An expired picker was accepted"
    );
    let refresh = client.request(Request::Snapshot)?;
    ensure!(
        refresh.summary.error && refresh.summary.message == rejected_choice.summary.message,
        "A status refresh erased the session error"
    );
    client.request(Request::Open(project.clone(), false))?;
    ensure!(
        fs::read_dir(&folder)?
            .filter_map(Result::ok)
            .filter(|entry| entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "started"))
            .count()
            == 1,
        "Reconnect or Switch created a second command instance"
    );
    // Save a different launch setup; a recovered session must retain its old stop command.
    let mut changed = project.clone();
    changed.workloads[0].stop = None;
    client.request(Request::Open(changed, false))?;
    let response = client.request(Request::Service(project.id, service_id, Action::Stop))?;
    ensure!(
        !response.summary.error,
        "Recovered normal stop failed: {}",
        response.summary.message
    );
    if let Some(id) = compose {
        let response = client.request(Request::Service(project.id, id, Action::Stop))?;
        ensure!(
            !response.summary.error,
            "Recovered Compose Stop failed: {}",
            response.summary.message
        );
    }
    wait(&mut client, |summary| {
        summary.workloads.iter().all(|(_, view)| !view.live)
    })?;
    drop(client);
    thread::sleep(Duration::from_millis(100));
    let mut client = Client::connect(&path)?;
    let stopped = client.request(Request::Snapshot)?;
    ensure!(
        !stopped.summary.workloads[0].1.live,
        "Manual stop was forgotten after reconnect"
    );
    for _ in 0..5 {
        ensure!(
            client.request(Request::LeaveAndQuit)?.quit,
            "Quit response was lost"
        );
        drop(client);
        client = Client::connect(&path)?;
        ensure!(
            !client.request(Request::Snapshot)?.summary.workloads[0]
                .1
                .live,
            "Rapid reconnect lost a manual stop"
        );
    }
    if let Some(id) = compose {
        let file = project
            .workloads
            .iter()
            .find(|config| config.id == id)
            .unwrap()
            .compose
            .as_ref()
            .unwrap()
            .file
            .clone();
        fs::write(file, "invalid edited source file")?;
        let response = client.request(Request::Service(project.id, id, Action::Start))?;
        ensure!(!response.summary.error, "{}", response.summary.message);
        wait(&mut client, |summary| {
            summary
                .workloads
                .iter()
                .any(|(_, view)| view.id == id && view.status == "Running")
        })?;
        ensure!(
            Some(container_id(&project, id)?) == container,
            "Recovered Compose Start replaced the original container"
        );
        let response = client.request(Request::Service(project.id, id, Action::Stop))?;
        ensure!(!response.summary.error, "{}", response.summary.message);
        println!(
            "PASS: Compose reconnect, retained container identity, original snapshot after source edits, and remote Stop/Start."
        );
    }
    let response = client.request(Request::StopAndQuit)?;
    ensure!(
        response.quit && !response.summary.error,
        "Stop-and-quit did not finish"
    );
    drop(client);
    let deadline = Instant::now() + Duration::from_secs(5);
    while identity::inspect(host.identity.pid).is_ok_and(|current| current == host.identity) {
        ensure!(Instant::now() < deadline, "Stopped host did not exit");
        thread::sleep(Duration::from_millis(50));
    }
    ensure!(
        Journal::read(&path)?.unwrap().projects.is_empty(),
        "Clean exit left lost sessions"
    );
    // A stale PID or damaged record must never cause process-name/PID adoption.
    let mut stale = host;
    stale.identity.created ^= 1;
    stale.projects.push(project);
    stale.save(&path)?;
    let bytes = fs::read(&path)?;
    ensure!(
        Client::connect(&path).is_err(),
        "Lost host was silently replaced"
    );
    ensure!(
        fs::read(&path)? == bytes,
        "Uncertain ownership record was overwritten"
    );
    println!(
        "PASS: leave-running, abrupt disconnect, verified host/session/output reconnection, original setup, manual-stop retention, clean exit, and lost-host refusal."
    );
    // Restore this fixture's desktop before the independent real-UI exercise.
    drop(fixture);
    crate::ui::session_check()
}

fn compose_fixture(folder: &Path) -> Result<WorkloadConfig> {
    let file = folder.join("reconnect-compose.json");
    // No networks/volumes are allocated by this fixture; the image's anonymous
    // database-volume location is overridden with tmpfs, as in compose-check.
    fs::write(
        &file,
        serde_json::to_vec(&serde_json::json!({"services":{"server":{
            "image":"postgres:16-alpine", "network_mode":"none", "tmpfs":["/var/lib/postgresql/data"],
            "entrypoint":["/bin/sh","-c"], "command":["trap 'exit 0' TERM; echo reconnect-compose-ready; for i in $$(seq 60); do sleep 1; done"]
        }}}))?,
    )?;
    Ok(WorkloadConfig {
        id: Uuid::new_v4(),
        name: "Reconnect Compose fixture".into(),
        kind: WorkloadKind::DockerCompose,
        start: CommandSpec {
            executable: PathBuf::from(
                std::env::var_os("ProgramFiles").context("Missing Program Files")?,
            )
            .join("Docker/Docker/resources/bin/docker.exe"),
            arguments: String::new(),
        },
        folder: Some(folder.into()),
        stop: None,
        stop_timeout_seconds: 6,
        readiness: Default::default(),
        readiness_timeout_seconds: 30,
        dependencies: Vec::new(),
        compose: Some(ComposeConfig {
            file,
            context: "desktop-linux".into(),
        }),
    })
}

fn container_id(project: &Project, id: Uuid) -> Result<String> {
    let config = project
        .workloads
        .iter()
        .find(|config| config.id == id)
        .unwrap();
    let output = Command::new(&config.start.executable)
        .args([
            "--context",
            "desktop-linux",
            "container",
            "ls",
            "--all",
            "--quiet",
            "--no-trunc",
            "--filter",
            &format!("label=io.jurabox.project={}", project.id),
            "--filter",
            &format!("label=io.jurabox.workload={id}"),
        ])
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .output()?;
    ensure!(
        output.status.success(),
        "Cannot inspect the private diagnostic container"
    );
    let id = String::from_utf8(output.stdout)?.trim().to_owned();
    ensure!(
        id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Expected exactly one private diagnostic container"
    );
    Ok(id)
}

fn wait(client: &mut Client, condition: impl Fn(&Summary) -> bool) -> Result<Summary> {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        let response = client.request(Request::Snapshot)?;
        ensure!(!response.summary.error, "{}", response.summary.message);
        if condition(&response.summary) {
            return Ok(response.summary);
        }
        ensure!(
            Instant::now() < deadline,
            "Timed out waiting for the private session fixture"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

struct Fixture {
    path: PathBuf,
    desktop: Option<winvd::Desktop>,
    starting_desktop: winvd::Desktop,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Only this check's private host/journal and newly created project are used.
        if let Ok(Some(journal)) = Journal::read(&self.path)
            && journal.validate_owner().is_ok()
            && let Ok(mut client) = Client::connect(&self.path)
        {
            for project in &journal.projects {
                for workload in &project.workloads {
                    let _ = client.request(Request::Service(
                        project.id,
                        workload.id,
                        Action::ForceStop,
                    ));
                }
            }
            let _ = client.request(Request::StopAndQuit);
        }
        let _ = winvd::switch_desktop(self.starting_desktop);
        if let Some(desktop) = self.desktop {
            let _ = winvd::remove_desktop(desktop, self.starting_desktop);
        }
    }
}
