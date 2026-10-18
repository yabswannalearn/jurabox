use super::*;
use crate::{platform::managed_process::OUTPUT_LIMIT, projects::CommandSpec};
use std::{fs, path::Path};

pub(super) fn wait(label: &str, mut condition: impl FnMut() -> Result<bool>) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if condition()? {
            return Ok(());
        }
        ensure!(Instant::now() < deadline, "Timed out: {label}");
        thread::sleep(Duration::from_millis(25));
    }
}

pub(crate) fn fixture_config(
    folder: &Path,
    mode: &str,
    kind: WorkloadKind,
) -> Result<WorkloadConfig> {
    fs::create_dir_all(folder)?;
    let executable = std::env::current_exe()?;
    Ok(WorkloadConfig {
        id: Uuid::new_v4(),
        name: format!("Fixture {mode}"),
        kind,
        start: CommandSpec {
            executable: executable.clone(),
            arguments: format!(r#"_service-fixture {mode} "{{folder}}""#),
        },
        folder: Some(folder.to_path_buf()),
        stop: Some(CommandSpec {
            executable,
            arguments: r#"_service-fixture stop "{folder}""#.into(),
        }),
        stop_timeout_seconds: 1,
        readiness: Default::default(),
        readiness_timeout_seconds: 60,
        dependencies: Vec::new(),
        compose: None,
    })
}

pub(super) struct OwnedCheck(pub(super) Services);
impl OwnedCheck {
    pub(super) fn new(configs: Vec<WorkloadConfig>) -> Result<Self> {
        let mut project = Project::from_form(None, "Private service fixture", "", None)?;
        project.workloads = configs;
        project.validate()?;
        Ok(Self(Services::new(&project)?))
    }
}
impl Drop for OwnedCheck {
    fn drop(&mut self) {
        // Every entry was created in this diagnostic scope. Cleanup never searches
        // for matching process names, ports, windows, or existing user workloads.
        for entry in &mut self.0.entries {
            if entry.live()
                && let Err(error) = entry.force_stop()
            {
                eprintln!("Owned service fixture cleanup: {error:#}");
            }
        }
    }
}

pub fn run() -> Result<()> {
    let temporary = tempfile::Builder::new()
        .prefix("jurabox service 测试 ")
        .tempdir()?;
    let root = temporary.path();
    let system =
        PathBuf::from(std::env::var_os("SystemRoot").context("Windows system folder unavailable")?);
    for (name, executable, arguments) in [
        (
            "CMD",
            system.join("System32/cmd.exe"),
            r#"/d /s /c "echo shell stdout & echo shell stderr 1>&2""#,
        ),
        (
            "PowerShell",
            system.join("System32/WindowsPowerShell/v1.0/powershell.exe"),
            r#"-NoLogo -NoProfile -NonInteractive -Command "[Console]::WriteLine('shell stdout'); [Console]::Error.WriteLine('shell stderr'); exit 0""#,
        ),
    ] {
        let mut config = fixture_config(&root.join(name), "success", WorkloadKind::StartupAction)?;
        config.start = CommandSpec {
            executable,
            arguments: arguments.into(),
        };
        let mut owned = OwnedCheck::new(vec![config])?;
        ensure!(
            owned.0.start_all().is_empty(),
            "{name} fixture failed to launch"
        );
        wait("real shell startup action and combined output", || {
            owned.0.poll();
            Ok(!owned.0.activity())
        })?;
        let row = &owned.0.views()[0];
        ensure!(
            row.status == "Completed"
                && row.output.contains("shell stdout")
                && row.output.contains("shell stderr"),
            "{name} argument handling or output capture failed: {} / {}",
            row.detail,
            row.output
        );
    }
    println!(
        "PASS real CMD and PowerShell actions with raw shell arguments and combined stdout/stderr"
    );
    {
        let config = fixture_config(
            &root.join("final-output"),
            "success",
            WorkloadKind::StartupAction,
        )?;
        let mut owned = OwnedCheck::new(vec![config])?;
        ensure!(
            owned.0.start_all().is_empty(),
            "Finite output fixture failed to start"
        );
        wait("final output capture before event-only idle", || {
            owned.0.poll();
            Ok(!owned.0.activity())
        })?;
        let row = &owned.0.views()[0];
        ensure!(
            row.status == "Completed" && row.output.contains("Action finished successfully."),
            "The last output was lost when the command exited"
        );
        println!("PASS final startup-action output is drained before returning to idle");
    }
    let outside_config =
        fixture_config(&root.join("outside"), "long", WorkloadKind::CommandService)?;
    let mut outside = OwnedCheck::new(vec![outside_config])?;
    ensure!(
        outside.0.start_all().is_empty(),
        "Outside fixture failed to start"
    );

    {
        let service = fixture_config(&root.join("normal"), "long", WorkloadKind::CommandService)?;
        let id = service.id;
        let action = fixture_config(&root.join("action"), "success", WorkloadKind::StartupAction)?;
        let unexpected = fixture_config(
            &root.join("unexpected"),
            "success",
            WorkloadKind::CommandService,
        )?;
        let mut missing =
            fixture_config(&root.join("missing"), "long", WorkloadKind::CommandService)?;
        missing.start.executable = root.join("missing.exe");
        let mut owned = OwnedCheck::new(vec![service, action, unexpected, missing])?;
        ensure!(
            owned.0.start_all().len() == 1,
            "Partial start did not report the missing executable"
        );
        wait("action completion and service exit distinction", || {
            owned.0.poll();
            let rows = owned.0.views();
            Ok(rows[0].output.contains("Fixture ready")
                && rows[1].status == "Completed"
                && rows[2].status == "Failed")
        })?;
        let pid = owned.0.entries[0].process.as_ref().unwrap().id;
        owned.0.act(id, Action::Start)?;
        ensure!(
            owned.0.entries[0].process.as_ref().unwrap().id == pid,
            "Duplicate start replaced a live service"
        );
        let recorded = fs::read_to_string(root.join("normal").join(format!("{pid}.started")))?;
        ensure!(
            Path::new(&recorded).canonicalize()? == root.join("normal").canonicalize()?,
            "The service used an implicit working folder"
        );
        owned.0.act(id, Action::Stop)?;
        ensure!(
            !owned.0.views()[0].live && owned.0.views()[0].status == "Stopped",
            "Normal stop did not finish"
        );
        owned.0.act(id, Action::Start)?;
        ensure!(
            owned.0.entries[0].process.as_ref().unwrap().id != pid,
            "Explicit Start did not create a new owned attempt"
        );
        println!(
            "PASS independent partial startup, finite-action completion, unexpected service exit, duplicate prevention, explicit working folder, normal stop, and manual Start"
        );
    }
    {
        let config = fixture_config(&root.join("flaky"), "flaky", WorkloadKind::CommandService)?;
        let id = config.id;
        let mut owned = OwnedCheck::new(vec![config])?;
        ensure!(
            owned.0.start_all().is_empty(),
            "Flaky fixture could not launch"
        );
        wait("initial service failure", || {
            owned.0.poll();
            Ok(owned.0.views()[0].status == "Failed")
        })?;
        let first = owned.0.entries[0].process.as_ref().unwrap().id;
        thread::sleep(Duration::from_millis(150));
        owned.0.poll();
        ensure!(
            owned.0.entries[0].process.as_ref().unwrap().id == first,
            "The service retried automatically"
        );
        owned.0.act(id, Action::Start)?;
        wait("manual Retry", || {
            Ok(owned.0.views()[0].output.contains("Fixture ready"))
        })?;
        ensure!(
            owned.0.entries[0].process.as_ref().unwrap().id != first && owned.0.live(),
            "Retry did not recover the failed attempt"
        );
        println!("PASS failed service stays failed until explicit Retry");
    }
    for scenario in ["missing-stop", "failed-stop", "timeout", "hanging-stop"] {
        let mut config =
            fixture_config(&root.join(scenario), "long", WorkloadKind::CommandService)?;
        match scenario {
            "missing-stop" => config.stop = None,
            "failed-stop" => {
                config.stop.as_mut().unwrap().arguments =
                    r#"_service-fixture fail "{folder}""#.into()
            }
            "timeout" => {
                config.stop.as_mut().unwrap().arguments =
                    r#"_service-fixture success "{folder}""#.into()
            }
            "hanging-stop" => {
                config.stop.as_mut().unwrap().arguments =
                    r#"_service-fixture long "{folder}""#.into()
            }
            _ => unreachable!(),
        }
        let id = config.id;
        let mut owned = OwnedCheck::new(vec![config])?;
        ensure!(
            owned.0.start_all().is_empty(),
            "Stop fixture could not launch"
        );
        wait("stop fixture ready", || {
            Ok(owned.0.views()[0].output.contains("Fixture ready"))
        })?;
        let pid = owned.0.entries[0].process.as_ref().unwrap().id;
        ensure!(
            owned.0.act(id, Action::Stop).is_err(),
            "A missing/failed/timed-out stop was silently accepted"
        );
        ensure!(
            owned.0.live() && owned.0.views()[0].status == "Stop failed",
            "Stop failure terminated or lost the workload"
        );
        owned.0.act(id, Action::Start)?;
        ensure!(
            owned.0.entries[0].process.as_ref().unwrap().id == pid,
            "Retry duplicated a still-running workload"
        );
        if scenario == "hanging-stop" {
            ensure!(
                active(&owned.0.entries[0].stop_process),
                "Timed-out stop command was terminated automatically"
            );
        }
        owned.0.act(id, Action::ForceStop)?;
        ensure!(
            !owned.0.live() && outside.0.live(),
            "Force stop escaped its owned workload"
        );
    }
    println!(
        "PASS missing/failed/timed-out normal stop leaves workloads managed and running; explicit Force stop covers owned stop commands and preserves another workload"
    );
    {
        let config = fixture_config(
            &root.join("tree"),
            "parent-exit",
            WorkloadKind::CommandService,
        )?;
        let id = config.id;
        let mut owned = OwnedCheck::new(vec![config])?;
        ensure!(
            owned.0.start_all().is_empty(),
            "Process tree fixture failed"
        );
        wait("owned child after launcher exit", || {
            owned.0.poll();
            Ok(owned.0.entries[0]
                .process
                .as_ref()
                .unwrap()
                .exit_code()?
                .is_some()
                && owned.0.views()[0].output.contains("Fixture ready"))
        })?;
        let pid = owned.0.entries[0].process.as_ref().unwrap().id;
        ensure!(owned.0.live(), "Launcher exit lost its child processes");
        owned.0.act(id, Action::Start)?;
        ensure!(
            owned.0.entries[0].process.as_ref().unwrap().id == pid,
            "Launcher exit allowed a duplicate workload"
        );
        owned.0.act(id, Action::ForceStop)?;
        ensure!(
            !owned.0.live() && outside.0.live(),
            "Owned process-tree force stop was not isolated"
        );
        println!(
            "PASS atomic job ownership, surviving children after launcher exit, duplicate prevention, and scoped process-tree Force stop"
        );
    }
    {
        let config = fixture_config(&root.join("output"), "flood", WorkloadKind::CommandService)?;
        let mut owned = OwnedCheck::new(vec![config])?;
        ensure!(owned.0.start_all().is_empty(), "Output fixture failed");
        wait("bounded output tail", || {
            Ok(owned.0.views()[0].output.contains("Fixture ready"))
        })?;
        let output = owned.0.views()[0].output.clone();
        ensure!(
            output.contains("Earlier output omitted") && output.len() <= OUTPUT_LIMIT * 3 + 256,
            "Output retention was unbounded"
        );
        println!("PASS bounded output for large Unicode output without line breaks");
    }
    ensure!(
        outside.0.live(),
        "Another workload was affected by the exercise"
    );
    println!("Service checks passed; only owned finite fixtures were launched.");
    super::readiness_check::run(root)?;
    Ok(())
}
