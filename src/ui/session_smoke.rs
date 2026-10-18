use super::*;
use crate::{
    projects::WorkloadKind,
    services::{Action, fixture_config},
    session_host::{Client, Request},
};
use anyhow::ensure;
use slint::Model;
use std::{cell::RefCell, rc::Rc, time::Duration};

pub(super) fn run() -> Result<()> {
    let folder = tempfile::Builder::new()
        .prefix("jurabox reconnect UI 测试 ")
        .tempdir()?;
    let store = ProjectStore::new(folder.path().join("projects.json"));
    let journal = store.session_journal();
    let name = format!("Jurabox reconnect UI {}", Uuid::new_v4());
    let mut desktops = assignment_smoke::Desktops::new(name.clone())?;
    let mut project = Project::from_form(None, &name, "", None)?;
    let mut config = fixture_config(
        &folder.path().join("command"),
        "long",
        WorkloadKind::CommandService,
    )?;
    config.stop = None;
    let id = config.id;
    project.workloads.push(config);
    let mut data = ProjectFile::default();
    data.upsert(project.clone());
    store.save(&data)?;
    let cleanup = Cleanup {
        journal: journal.clone(),
        project: project.id,
        workload: id,
    };
    let mut client = Client::connect(&journal)?;
    let opened = client.request(Request::Open(project.clone(), false))?;
    ensure!(!opened.summary.error, "{}", opened.summary.message);
    client.request(Request::LeaveAndQuit)?;
    drop(client);

    let (window, _tray) = create_with_host(store, true)?;
    window
        .window()
        .set_size(slint::LogicalSize::new(820., 650.));
    window.show()?;
    let artifacts = std::env::current_dir()?.join("target/ui-check");
    std::fs::create_dir_all(&artifacts)?;
    let result = Rc::new(RefCell::new(None::<Result<()>>));
    let outcome = result.clone();
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    let start = Instant::now();
    let mut step = 0;
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(150),
        move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            if outcome.borrow().as_ref().is_some_and(Result::is_err) {
                if !window.get_workspace_busy() {
                    window.set_dirty(false);
                    window.invoke_quit();
                }
                return;
            }
            let mut next = || -> Result<()> {
                ensure!(
                    start.elapsed() < Duration::from_secs(35),
                    "Reconnect UI timed out at step {step}"
                );
                if window.get_workspace_busy() {
                    return Ok(());
                }
                let rows: Vec<_> = window.get_session_workloads().iter().collect();
                match step {
                    0 => {
                        if rows.is_empty() || !rows[0].output.contains("Fixture ready") {
                            return Ok(());
                        }
                        ensure!(
                            window.get_session_open()
                                && rows[0].live
                                && window.get_notice().contains("Reconnected"),
                            "UI did not reconnect to the existing service"
                        );
                        smoke::snapshot(&window, &artifacts.join("session-reconnected.png"))?;
                        window.invoke_stop_and_quit();
                    }
                    1 => {
                        ensure!(
                            window.get_notice_error()
                                && rows[0].live
                                && rows[0].status == "Stop failed",
                            "A failed stop did not keep the remote session open: {}",
                            window.get_notice()
                        );
                        smoke::snapshot(&window, &artifacts.join("session-stop-failed.png"))?;
                        window.invoke_service_action(id.to_string().into(), "force".into());
                    }
                    2 => {
                        ensure!(
                            !rows[0].live,
                            "Remote Force stop did not stop the owned job"
                        );
                        window.invoke_service_action(id.to_string().into(), "start".into());
                    }
                    3 => {
                        if !rows[0].live || !rows[0].output.contains("Fixture ready") {
                            return Ok(());
                        }
                        *outcome.borrow_mut() = Some(Ok(()));
                        window.invoke_quit();
                    }
                    _ => return Ok(()),
                }
                step += 1;
                Ok(())
            };
            if let Err(error) = next() {
                *outcome.borrow_mut() = Some(Err(error));
            }
        },
    );
    slint::run_event_loop_until_quit()?;
    timer.stop();
    result
        .borrow_mut()
        .take()
        .context("Reconnect UI ended before checks completed")??;
    let mut client = Client::connect(&journal)?;
    let response = client.request(Request::Snapshot)?;
    ensure!(
        response.summary.workloads[0].1.live,
        "UI leave-running exit stopped the service"
    );
    drop(client);
    drop(cleanup);
    desktops.restore()?;
    println!(
        "PASS: real UI reconnection, retained logs, failed stop-and-quit, remote Force/Start, and leave-running exit."
    );
    Ok(())
}

struct Cleanup {
    journal: std::path::PathBuf,
    project: Uuid,
    workload: Uuid,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Ok(mut client) = Client::connect(&self.journal) {
            let _ = client.request(Request::Service(
                self.project,
                self.workload,
                Action::ForceStop,
            ));
            let _ = client.request(Request::StopAndQuit);
        }
    }
}
