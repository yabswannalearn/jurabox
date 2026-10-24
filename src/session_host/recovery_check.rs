//! Uses only private journals and creation-owned child handles to model a host crash.
use super::*;
use crate::{projects::WorkloadKind, services::fixture_config};
use std::process::Child;

pub fn run() -> Result<()> {
    let folder = tempfile::Builder::new()
        .prefix("jurabox recovery 测试 ")
        .tempdir()?;
    let path = folder.path().join("projects.session-host.json");
    let starting = winvd::get_current_desktop().map_err(|error| anyhow::anyhow!("{error:?}"))?;
    let mut fixture = Fixture {
        path: path.clone(),
        host: None,
        desktop: None,
        starting,
    };
    fixture.spawn()?;
    let mut client = Client::connect(&path)?;
    let mut project = Project::from_form(None, &format!("Recovery {}", Uuid::new_v4()), "", None)?;
    let mut probe = crate::platform::OwnedProbe::start()?;
    project.apps.push(crate::projects::AppConfig {
        open_folder: None,
        app_id: None,
        id: Uuid::new_v4(),
        name: "Recovery test window".into(),
        executable: probe.executable().into(),
        arguments: "_probe-window".into(),
        new_console: false,
        shared: true,
    });
    let child_folder = folder.path().join("child");
    let child = fixture_config(&child_folder, "parent-exit", WorkloadKind::CommandService)?;
    let stopped_folder = folder.path().join("stopped");
    let stopped = fixture_config(&stopped_folder, "long", WorkloadKind::CommandService)?;
    let completed_folder = folder.path().join("completed");
    let completed = fixture_config(&completed_folder, "success", WorkloadKind::StartupAction)?;
    let missing_folder = folder.path().join("missing");
    let missing = fixture_config(&missing_folder, "long", WorkloadKind::CommandService)?;
    let gate_folder = folder.path().join("gate");
    let gate = fixture_config(&gate_folder, "delayed-success", WorkloadKind::StartupAction)?;
    let waiting_folder = folder.path().join("waiting");
    let mut waiting = fixture_config(&waiting_folder, "long", WorkloadKind::CommandService)?;
    waiting.dependencies.push(gate.id);
    let mut unready = fixture_config(
        &folder.path().join("unready"),
        "long",
        WorkloadKind::CommandService,
    )?;
    unready.readiness = crate::projects::Readiness::Tcp {
        address: "127.0.0.1:1".parse()?,
    };
    let stop_command_folder = folder.path().join("stop-command");
    fs::create_dir_all(&stop_command_folder)?;
    let mut interrupted_stop = fixture_config(
        &folder.path().join("interrupted-stop"),
        "long",
        WorkloadKind::CommandService,
    )?;
    interrupted_stop.stop.as_mut().unwrap().arguments = format!(
        r#"_service-fixture long "{}""#,
        stop_command_folder.display()
    );
    project.workloads = vec![
        child.clone(),
        stopped.clone(),
        completed,
        missing.clone(),
        gate,
        waiting,
        unready.clone(),
        interrupted_stop.clone(),
    ];
    let compose = if std::env::var_os("JURABOX_CHECK_DOCKER").is_some() {
        let config = super::check::compose_fixture(folder.path())?;
        let id = config.id;
        project.workloads.push(config);
        Some(id)
    } else {
        None
    };
    let response = client.request(Request::Open(project.clone(), false))?;
    fixture.desktop = winvd::get_desktops().ok().and_then(|desktops| {
        desktops
            .into_iter()
            .find(|desktop| desktop.get_name().ok().as_deref() == Some(&project.name))
    });
    ensure!(!response.summary.error, "{}", response.summary.message);
    wait(&mut client, |summary| {
        summary.workloads[2].1.status == "Completed"
            && child_folder.join("child-pid").is_file()
            && summary.workloads[6].1.live
            && compose.is_none_or(|id| {
                summary.workloads.iter().any(|(_, view)| {
                    view.id == id
                        && view.status == "Running"
                        && view.output.contains("reconnect-compose-ready")
                })
            })
    })?;
    let container = compose
        .map(|id| super::check::container_id(&project, id))
        .transpose()?;
    if let Some(id) = compose {
        let config = project
            .workloads
            .iter()
            .find(|config| config.id == id)
            .unwrap();
        fs::write(
            &config.compose.as_ref().unwrap().file,
            "invalid source after launch",
        )?;
    }
    let response = client.request(Request::Service(project.id, stopped.id, Action::Stop))?;
    ensure!(!response.summary.error, "{}", response.summary.message);
    let response = client.request(Request::Service(
        project.id,
        interrupted_stop.id,
        Action::Stop,
    ))?;
    ensure!(
        response.summary.error && response.summary.workloads[7].1.live,
        "The interrupted-stop fixture did not retain its workload and stop command"
    );
    let old_host = Journal::read(&path)?.unwrap();
    fixture.crash()?;
    drop(client);
    // Make one recorded job disappear while the host is down. Recovery must
    // report it missing and must not silently replay its startup command.
    fs::write(missing_folder.join("stop"), "stop")?;
    fs::write(gate_folder.join("allow-action"), "go")?;
    thread::sleep(Duration::from_millis(200));
    fixture.spawn()?;
    let mut client = Client::connect(&path)?;
    let recovered = client.request(Request::Snapshot)?;
    ensure!(!recovered.summary.error, "{}", recovered.summary.message);
    if let Some(id) = compose {
        wait(&mut client, |summary| {
            summary.workloads.iter().any(|(_, view)| {
                view.id == id
                    && view.status == "Running"
                    && view.output.contains("reconnect-compose-ready")
            })
        })?;
        ensure!(
            Some(super::check::container_id(&project, id)?) == container,
            "Cold recovery replaced the original Compose container"
        );
    }
    ensure!(
        Journal::read(&path)?.unwrap().generation != old_host.generation,
        "Host was not replaced"
    );
    ensure!(
        recovered.summary.active == Some(project.id),
        "Original desktop link was lost"
    );
    ensure!(
        recovered.summary.apps[0].title.is_some()
            && probe.is_alive()
            && probe.desktop()?
                == fixture
                    .desktop
                    .unwrap()
                    .get_id()
                    .map_err(|error| anyhow::anyhow!("{error:?}"))?,
        "Original verified window assignment was lost"
    );
    ensure!(
        recovered.summary.workloads[0].1.live,
        "Surviving child job was lost"
    );
    ensure!(
        recovered.summary.workloads[0]
            .1
            .output
            .contains("unavailable"),
        "Lost output was not explained"
    );
    ensure!(
        recovered.summary.workloads[1].1.status == "Stopped",
        "Manual stop was forgotten"
    );
    ensure!(
        recovered.summary.workloads[2].1.status == "Completed",
        "Completed action was forgotten"
    );
    ensure!(
        !recovered.summary.workloads[3].1.live
            && recovered.summary.workloads[3].1.status == "Failed",
        "Missing service was restarted or reported as healthy"
    );
    ensure!(
        recovered.summary.workloads[5].1.status == "Stopped"
            && !waiting_folder.join("stopped").exists(),
        "A queued dependent was automatically launched"
    );
    ensure!(
        recovered.summary.workloads[6].1.status == "Not ready",
        "Network readiness was trusted after recovery"
    );
    ensure!(
        recovered.summary.workloads[7].1.status == "Stop failed"
            && recovered.summary.workloads[7].1.live,
        "An interrupted stop was forgotten"
    );
    let response = client.request(Request::Service(
        project.id,
        interrupted_stop.id,
        Action::Stop,
    ))?;
    ensure!(
        response.summary.error && started(&stop_command_folder)? == 1,
        "Retry duplicated a surviving stop command"
    );
    let response = client.request(Request::Service(
        project.id,
        interrupted_stop.id,
        Action::ForceStop,
    ))?;
    ensure!(
        !response.summary.error && !response.summary.workloads[7].1.live,
        "Force stop did not end both recovered command jobs"
    );
    let response = client.request(Request::Service(project.id, unready.id, Action::Start))?;
    ensure!(!response.summary.error, "{}", response.summary.message);
    ensure!(
        started(&child_folder)? == 1
            && started(&stopped_folder)? == 1
            && started(&missing_folder)? == 1
            && started(&waiting_folder)? == 0,
        "Recovery created a duplicate or queued command"
    );
    // Saving a changed definition must not replace the original shutdown setup.
    let mut changed = project.clone();
    changed.workloads[0].stop = None;
    client.request(Request::Open(changed, false))?;
    let stopped_child = client.request(Request::Service(project.id, child.id, Action::Stop))?;
    ensure!(
        !stopped_child.summary.error && !stopped_child.summary.workloads[0].1.live,
        "Original stop command failed after child-only recovery: {}",
        stopped_child.summary.message
    );
    let force = client.request(Request::Service(project.id, unready.id, Action::ForceStop))?;
    ensure!(
        !force.summary.error && !force.summary.workloads[6].1.live,
        "Recovered Force stop failed"
    );
    let response = client.request(Request::Service(project.id, missing.id, Action::Start))?;
    ensure!(!response.summary.error, "{}", response.summary.message);
    wait(&mut client, |summary| {
        summary.workloads[3].1.live && started(&missing_folder).ok() == Some(2)
    })?;
    ensure!(
        started(&missing_folder)? == 2,
        "Explicit Retry did not launch the missing command"
    );
    println!(
        "PASS: real host crash, child-only job recovery, original desktop/setup, manual stops, completed actions, missing jobs, readiness Retry, explicit Force/Start, and no automatic launches."
    );

    // Model the resources absent after reboot/logoff without rebooting the user's
    // computer: stop everything, remove only this check's desktop, lose the host.
    for config in &project.workloads {
        client.request(Request::Service(project.id, config.id, Action::ForceStop))?;
    }
    let root = path.with_extension("sessions");
    let manifest = crate::recovery::project_path(&root, project.id);
    let original = fs::read(&manifest)?;
    let mut future: serde_json::Value = serde_json::from_slice(&original)?;
    future["version"] = 999.into();
    fs::write(&manifest, serde_json::to_vec(&future)?)?;
    let refused = Workspaces::recover(root.clone(), std::slice::from_ref(&project));
    ensure!(
        refused?
            .summary()
            .recovery
            .iter()
            .any(|problem| problem.project == project.id),
        "Future recovery record was not blocked at project scope"
    );
    ensure!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&manifest)?)?["version"] == 999,
        "Future record was overwritten"
    );
    fs::write(&manifest, original)?;
    probe.close()?;
    fixture.crash()?;
    drop(client);
    winvd::switch_desktop(starting).map_err(|error| anyhow::anyhow!("{error:?}"))?;
    if let Some(desktop) = fixture.desktop.take() {
        winvd::remove_desktop(desktop, starting).map_err(|error| anyhow::anyhow!("{error:?}"))?;
    }
    fixture.spawn()?;
    let mut client = Client::connect(&path)?;
    let empty = client.request(Request::Snapshot)?;
    if let Some(id) = compose {
        ensure!(
            empty.summary.open == vec![project.id]
                && empty
                    .summary
                    .workloads
                    .iter()
                    .any(|(_, view)| { view.id == id && !view.live && view.status == "Stopped" })
                && Some(super::check::container_id(&project, id)?) == container,
            "Recovery did not preserve the stopped owned Compose container"
        );
        let response = client.request(Request::Close(project.id))?;
        ensure!(!response.summary.error, "{}", response.summary.message);
        ensure!(
            super::check::container_id(&project, id)?.is_empty(),
            "Close did not release the private Compose fixture"
        );
        println!(
            "PASS: mixed command/Compose cold recovery, original container/logs/frozen plan, stopped-container recovery and explicit release."
        );
    } else {
        ensure!(
            empty.summary.open.is_empty() && empty.summary.workloads.is_empty(),
            "Ended sessions were relaunched after resource loss"
        );
    }
    ensure!(
        client.request(Request::StopAndQuit)?.quit,
        "Empty recovered host did not quit"
    );
    drop(client);
    fixture.wait_exit()?;
    println!(
        "PASS: unsupported records preserved; reboot-like loss of jobs/windows/desktops leaves projects closed for explicit Open."
    );
    Ok(())
}

fn started(folder: &Path) -> Result<usize> {
    Ok(fs::read_dir(folder)?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "started")
        })
        .count())
}

fn wait(client: &mut Client, condition: impl Fn(&Summary) -> bool) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        let response = client.request(Request::Snapshot)?;
        ensure!(!response.summary.error, "{}", response.summary.message);
        if condition(&response.summary) {
            return Ok(());
        }
        ensure!(
            Instant::now() < deadline,
            "Recovery fixture timed out: {}",
            response
                .summary
                .workloads
                .iter()
                .map(|(_, view)| format!("{}: {} ({})", view.name, view.status, view.detail))
                .collect::<Vec<_>>()
                .join("; ")
        );
        thread::sleep(Duration::from_millis(25));
    }
}

struct Fixture {
    path: PathBuf,
    host: Option<Child>,
    desktop: Option<winvd::Desktop>,
    starting: winvd::Desktop,
}
impl Fixture {
    fn spawn(&mut self) -> Result<()> {
        let child = Command::new(std::env::current_exe()?)
            .arg("_session-host")
            .arg(&self.path)
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let id = child.id();
        self.host = Some(child);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if Journal::read(&self.path)?.is_some_and(|journal| journal.identity.pid == id) {
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "Private recovery host did not start"
            );
            thread::sleep(Duration::from_millis(25));
        }
    }
    fn crash(&mut self) -> Result<()> {
        let mut host = self.host.take().context("Missing private fixture host")?;
        // std::process::Child keeps the creation-returned kernel handle. No
        // process-name lookup or PID-based termination is used by this check.
        host.kill()?;
        host.wait()?;
        Ok(())
    }
    fn wait_exit(&mut self) -> Result<()> {
        if let Some(host) = &mut self.host {
            let deadline = Instant::now() + Duration::from_secs(5);
            while host.try_wait()?.is_none() {
                ensure!(
                    Instant::now() < deadline,
                    "Private recovery host did not exit"
                );
                thread::sleep(Duration::from_millis(25));
            }
        }
        self.host = None;
        Ok(())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if Journal::read(&self.path)
            .ok()
            .flatten()
            .is_some_and(|journal| !journal.projects.is_empty())
            && let Ok(mut client) = Client::connect(&self.path)
        {
            if let Ok(response) = client.request(Request::Snapshot) {
                for (project, view) in response.summary.workloads {
                    let _ = client.request(Request::Service(project, view.id, Action::ForceStop));
                }
            }
            let _ = client.request(Request::StopAndQuit);
        }
        let _ = winvd::switch_desktop(self.starting);
        if let Some(desktop) = self.desktop {
            let _ = winvd::remove_desktop(desktop, self.starting);
        }
    }
}
