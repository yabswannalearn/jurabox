use super::{
    check::{OwnedCheck, fixture_config, wait},
    *,
};
use std::{fs, net::TcpListener, path::Path};

pub(crate) fn health_config(folder: &Path, http: bool) -> Result<WorkloadConfig> {
    let mut config = fixture_config(folder, "health", WorkloadKind::CommandService)?;
    // Reserve an ephemeral loopback port, then release it for this owned fixture.
    // A concurrent claimant is a reported conflict, never a workload to adopt.
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    fs::write(folder.join("listen-address"), address.to_string())?;
    config.readiness = if http {
        Readiness::Http {
            url: format!("http://{address}/health?project=fixture"),
        }
    } else {
        Readiness::Tcp { address }
    };
    config.readiness_timeout_seconds = 2;
    Ok(config)
}

fn until(owned: &mut OwnedCheck, index: usize, status: &str) -> Result<()> {
    wait(status, || {
        owned.0.poll();
        Ok(owned.0.views()[index].status == status)
    })
}

pub(super) fn run(root: &Path) -> Result<()> {
    {
        let first_folder = root.join("tcp-prerequisite");
        let first = health_config(&first_folder, false)?;
        let second_folder = root.join("tcp-dependent");
        let mut second = health_config(&second_folder, false)?;
        second.readiness_timeout_seconds = 1;
        second.dependencies.push(first.id);
        let mut cancelled = fixture_config(
            &root.join("cancelled"),
            "success",
            WorkloadKind::StartupAction,
        )?;
        cancelled.dependencies.push(first.id);
        let cancelled_id = cancelled.id;
        let mut owned = OwnedCheck::new(vec![second, first, cancelled])?;
        owned.0.start_all();
        until(&mut owned, 1, "Starting")?;
        owned.0.act(cancelled_id, Action::Stop)?;
        thread::sleep(Duration::from_millis(1200));
        owned.0.poll();
        ensure!(
            owned.0.views()[0].status == "Waiting"
                && owned.0.entries[0].readiness_deadline.is_none()
                && owned.0.entries[0].process.is_none(),
            "A dependency wait consumed the command's startup timeout or launched it early"
        );
        fs::write(first_folder.join("allow-ready"), "ready")?;
        until(&mut owned, 0, "Starting")?;
        fs::write(second_folder.join("allow-ready"), "ready")?;
        until(&mut owned, 0, "Running")?;
        ensure!(
            owned.0.views()[2].status == "Stopped" && owned.0.entries[2].process.is_none(),
            "A cancelled waiting action launched automatically"
        );
        fs::write(
            second_folder.join("require-live-folder"),
            first_folder.to_string_lossy().as_bytes(),
        )?;
        ensure!(
            owned
                .0
                .act(owned.0.entries[1].config.id, Action::Stop)
                .is_err()
                && owned.0.entries[1].live(),
            "Stopping a live prerequisite was allowed"
        );
        owned.0.stop_all()?;
        ensure!(
            first_folder.join("stopped").exists() && second_folder.join("stopped").exists(),
            "Shutdown did not finish in dependency order"
        );
        println!(
            "PASS delayed TCP readiness, unordered dependencies, per-command timeout, cancellation, prerequisite stop guard, and reverse shutdown"
        );
    }
    {
        let folder = root.join("http-prerequisite");
        let first = health_config(&folder, true)?;
        let first_id = first.id;
        let mut action = fixture_config(
            &root.join("http-action"),
            "success",
            WorkloadKind::StartupAction,
        )?;
        action.dependencies.push(first.id);
        let mut dependent = fixture_config(
            &root.join("http-dependent"),
            "long",
            WorkloadKind::CommandService,
        )?;
        dependent.dependencies.push(action.id);
        let mut owned = OwnedCheck::new(vec![first, action, dependent])?;
        owned.0.start_all();
        until(&mut owned, 0, "Starting")?;
        let pid = owned.0.entries[0].process.as_ref().unwrap().id;
        fs::write(folder.join("allow-ready"), "ready")?;
        until(&mut owned, 0, "Not ready")?;
        ensure!(
            owned.0.entries[0].live() && owned.0.entries[1].process.is_none(),
            "503 or timeout released a prerequisite or killed its process"
        );
        fs::write(folder.join("http-status"), "302")?;
        owned.0.act(first_id, Action::Start)?;
        until(&mut owned, 0, "Not ready")?;
        ensure!(
            fs::read_to_string(folder.join("last-request"))?
                .starts_with("GET /health?project=fixture "),
            "HTTP redirects were followed or the query was lost"
        );
        fs::write(folder.join("http-status"), "204")?;
        owned.0.act(first_id, Action::Start)?;
        until(&mut owned, 2, "Running")?;
        ensure!(
            owned.0.entries[0].process.as_ref().unwrap().id == pid
                && owned.0.views()[1].status == "Completed",
            "Retry duplicated the process or a finite prerequisite was released before completion"
        );
        fs::write(folder.join("http-status"), "503")?;
        thread::sleep(Duration::from_millis(700));
        owned.0.poll();
        ensure!(
            owned.0.views()[0].status == "Running",
            "Startup readiness became an unsolicited continuous health monitor"
        );

        let mut conflict = health_config(&root.join("conflict"), true)?;
        conflict.readiness = owned.0.entries[0].config.readiness.clone();
        let mut other = OwnedCheck::new(vec![conflict])?;
        other.0.start_all();
        until(&mut other, 0, "Failed")?;
        ensure!(
            other.0.entries[0].process.is_none() && owned.0.entries[0].live(),
            "Endpoint conflict launched a duplicate or touched the existing workload"
        );
        owned.0.act(first_id, Action::ForceStop)?;
        owned.0.poll();
        ensure!(
            owned.0.views()[2].live
                && owned.0.views()[2].error
                && owned.0.views()[2]
                    .detail
                    .contains("Prerequisite unavailable"),
            "A lost transitive prerequisite stopped its dependent or was hidden"
        );
        println!(
            "PASS HTTP 503/302/204, query preservation, no redirects, timeout retention, same-instance Retry, finite prerequisites, endpoint conflicts, and non-cascading dependency loss"
        );
    }
    {
        let first = fixture_config(
            &root.join("stop-parent"),
            "long",
            WorkloadKind::CommandService,
        )?;
        let mut second = fixture_config(
            &root.join("stop-child"),
            "long",
            WorkloadKind::CommandService,
        )?;
        second.dependencies.push(first.id);
        second.stop = None;
        let third = fixture_config(
            &root.join("stop-independent"),
            "long",
            WorkloadKind::CommandService,
        )?;
        let mut owned = OwnedCheck::new(vec![first, second, third])?;
        owned.0.start_all();
        wait("stop fixtures ready", || {
            Ok(owned
                .0
                .views()
                .iter()
                .all(|row| row.output.contains("Fixture ready")))
        })?;
        ensure!(
            owned.0.stop_all().is_err()
                && owned.0.entries[0].live()
                && owned.0.entries[1].live()
                && !owned.0.entries[2].live(),
            "Failed dependent stop lost a prerequisite or prevented independent shutdown"
        );
        println!(
            "PASS failed dependent shutdown preserves its prerequisites while independent work stops"
        );
    }
    {
        let folder = root.join("failed-action");
        let mut first = fixture_config(&folder, "fail", WorkloadKind::StartupAction)?;
        let first_id = first.id;
        let mut second = fixture_config(
            &root.join("after-action"),
            "success",
            WorkloadKind::StartupAction,
        )?;
        second.dependencies.push(first_id);
        let mut owned = OwnedCheck::new(vec![first.clone(), second])?;
        owned.0.start_all();
        until(&mut owned, 0, "Failed")?;
        ensure!(
            owned.0.views()[1].status == "Waiting",
            "Failed action released its dependent"
        );
        // Diagnostic-only replacement of the command simulates fixing the script
        // in place; no saved/live configuration behavior is changed in production.
        first.start.arguments = r#"_service-fixture success "{folder}""#.into();
        owned.0.entries[0].config = first;
        owned.0.act(first_id, Action::Start)?;
        until(&mut owned, 1, "Completed")?;
        println!(
            "PASS failed finite prerequisite waits for explicit Retry, then queued dependents resume"
        );
    }
    {
        let config = health_config(&root.join("cancel-preflight"), true)?;
        let id = config.id;
        let mut owned = OwnedCheck::new(vec![config])?;
        owned.0.start_all();
        let before = Instant::now();
        owned.0.act(id, Action::Stop)?;
        ensure!(
            before.elapsed() < Duration::from_millis(200),
            "Endpoint probe blocked the workspace actor"
        );
        wait("discard cancelled probe", || {
            owned.0.poll();
            Ok(!owned.0.activity())
        })?;
        ensure!(
            owned.0.entries[0].process.is_none() && owned.0.views()[0].status == "Stopped",
            "A stale preflight completion launched a cancelled workload"
        );
        println!("PASS nonblocking preflight and stale probe cancellation");
    }
    Ok(())
}
