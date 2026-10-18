use super::*;
use crate::{projects::WorkloadKind, services::fixture_config};
use anyhow::{bail, ensure};
use slint::Model;
use std::{cell::RefCell, rc::Rc, time::Duration};

/// Own temporary project/settings and finite child workloads only.
pub(super) fn run() -> Result<()> {
    let temporary = tempfile::Builder::new()
        .prefix("jurabox service UI ")
        .tempdir()?;
    let name = format!("Jurabox service UI check {}", Uuid::new_v4());
    let mut desktops = assignment_smoke::Desktops::new(name.clone())?;
    let path = temporary.path().join("projects.json");
    let mut project = Project::from_form(None, &name, &temporary.path().to_string_lossy(), None)?;
    let first = fixture_config(
        &temporary.path().join("normal"),
        "long",
        WorkloadKind::CommandService,
    )?;
    let mut second = fixture_config(
        &temporary.path().join("refuses"),
        "long",
        WorkloadKind::CommandService,
    )?;
    second.name = "Needs explicit force".into();
    second.stop = None;
    let first_id = first.id.to_string();
    let second_id = second.id.to_string();
    project.workloads = vec![first, second];
    let mut data = ProjectFile::default();
    data.upsert(project);
    let store = ProjectStore::new(path.clone());
    store.save(&data)?;
    let (window, _tray) = create(store)?;
    window
        .window()
        .set_size(slint::LogicalSize::new(820., 650.));
    window.show()?;
    let artifacts = std::env::current_dir()?.join("target/ui-check");
    std::fs::create_dir_all(&artifacts)?;
    let result = Rc::new(RefCell::new(None::<Result<()>>));
    let outcome = result.clone();
    let weak = window.as_weak();
    let started = Instant::now();
    let timer = slint::Timer::default();
    let mut step = 0;
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(200), move || {
        let Some(window) = weak.upgrade() else { return; };
        // Failures still clean up only the fixture rows from this private project.
        if outcome.borrow().as_ref().is_some_and(Result::is_err) {
            if !window.get_workspace_busy() {
                if let Some(row) = window.get_session_workloads().iter().find(|row| row.live) {
                    window.invoke_service_action(row.id, "force".into());
                } else { let _ = slint::quit_event_loop(); }
            }
            return;
        }
        let mut next = || -> Result<()> {
            ensure!(started.elapsed() < Duration::from_secs(45), "Service UI check timed out at step {step}");
            if window.get_busy() || window.get_workspace_busy() { return Ok(()); }
            let rows: Vec<_> = window.get_session_workloads().iter().collect();
            let first_row = || rows.iter().find(|row| row.id == first_id);
            let second_row = || rows.iter().find(|row| row.id == second_id);
            match step {
                0 => {
                    ensure!(!window.get_session_open() && !window.get_managed_live(), "Configuration selection launched services");
                    window.invoke_add_workload(true);
                    let drafts = window.get_workloads(); let mut action = drafts.row_data(2).context("Startup action form was not added")?;
                    action.name = "Prepare project".into();
                    action.executable = std::env::current_exe()?.to_string_lossy().into_owned().into();
                    action.arguments = "_service-fixture success \"{folder}\"".into();
                    drafts.set_row_data(2, action); window.set_dirty(true); window.invoke_save_project();
                }
                1 => {
                    let data = ProjectStore::new(path.clone()).load()?;
                    ensure!(data.version == 3 && data.projects[0].workloads.len() == 3 && data.projects[0].workloads[2].kind == WorkloadKind::StartupAction, "Typed workload form did not persist");
                    // Scroll to inspect the saved command editor before launch.
                    window.window().dispatch_event_with_result(slint::platform::WindowEvent::PointerScrolled { position: slint::LogicalPosition::new(600., 350.), delta_x: 0., delta_y: -420. })?;
                }
                2 => {
                    smoke::snapshot(&window, &artifacts.join("service-editor.png"))?;
                    window.window().dispatch_event_with_result(slint::platform::WindowEvent::PointerScrolled { position: slint::LogicalPosition::new(600., 350.), delta_x: 0., delta_y: 10000. })?;
                    window.invoke_open_project();
                }
                3 => {
                    if rows.len() != 3 || !rows.iter().any(|row| row.status == "Completed") || !first_row().is_some_and(|row| row.output.contains("Fixture ready")) || !second_row().is_some_and(|row| row.live) { return Ok(()); }
                    ensure!(window.get_session_open() && window.get_managed_live(), "Service session did not become live");
                    smoke::snapshot(&window, &artifacts.join("services-running.png"))?;
                    window.invoke_quit();
                    ensure!(window.get_notice_error() && window.get_notice().contains("Managed workloads"), "Quit did not protect live output capture");
                    window.invoke_service_action(first_id.clone().into(), "stop".into());
                }
                4 => {
                    ensure!(first_row().is_some_and(|row| row.status == "Stopped") && second_row().is_some_and(|row| row.live), "Manual stop affected another service");
                    window.invoke_open_project();
                }
                5 => {
                    ensure!(first_row().is_some_and(|row| row.status == "Stopped"), "Switch reopened a manually stopped service");
                    window.invoke_service_action(first_id.clone().into(), "start".into());
                }
                6 => {
                    if !first_row().is_some_and(|row| row.live && row.output.contains("Fixture ready")) { return Ok(()); }
                    let drafts = window.get_workloads();
                    let mut first = drafts.row_data(0).unwrap(); first.name = "Updated service".into(); drafts.set_row_data(0, first);
                    let mut second = drafts.row_data(1).unwrap();
                    second.stop_executable = std::env::current_exe()?.to_string_lossy().into_owned().into();
                    second.stop_arguments = "_service-fixture stop \"{folder}\"".into(); drafts.set_row_data(1, second);
                    window.set_dirty(true); window.invoke_save_project();
                }
                7 => {
                    ensure!(first_row().is_some_and(|row| row.name != "Updated service"), "Save changed the live session setup");
                    window.invoke_relaunch_project();
                }
                8 => {
                    ensure!(window.get_notice_error() && window.get_session_open() && second_row().is_some_and(|row| row.status == "Stop failed" && row.live), "Relaunch applied the saved stop command to the old session or lost its workload");
                    window.window().dispatch_event_with_result(slint::platform::WindowEvent::PointerScrolled { position: slint::LogicalPosition::new(750., 200.), delta_x: 0., delta_y: -320. })?;
                    window.invoke_close_project();
                }
                9 => {
                    ensure!(window.get_notice_error() && window.get_session_open() && window.get_managed_live(), "Close lost a workload that refused shutdown");
                    smoke::snapshot(&window, &artifacts.join("services-stop-failed.png"))?;
                    window.invoke_hide_window();
                    window.invoke_stop_and_quit();
                }
                10 => {
                    ensure!(window.window().is_visible() && window.get_notice_error() && window.get_managed_live(), "Stop-and-quit hid a stop failure or exited despite it");
                    window.invoke_service_action(second_id.clone().into(), "force".into());
                }
                11 => {
                    ensure!(!window.get_managed_live(), "Explicit Force stop did not finish");
                    window.invoke_relaunch_project();
                }
                12 => {
                    if !first_row().is_some_and(|row| row.name == "Updated service" && row.live) || !second_row().is_some_and(|row| row.live && row.output.contains("Fixture ready")) || !rows.iter().any(|row| row.status == "Completed") { return Ok(()); }
                    *outcome.borrow_mut() = Some(Ok(()));
                    window.invoke_stop_and_quit();
                }
                13 => {
                    ensure!(!window.get_managed_live() && !window.get_notice_error(), "Normal stop-and-quit failed after relaunch");
                    let _ = slint::quit_event_loop();
                }
                _ => bail!("Unexpected service UI check step"),
            }
            step += 1;
            Ok(())
        };
        if let Err(error) = next() { *outcome.borrow_mut() = Some(Err(error)); }
    });
    slint::run_event_loop_until_quit()?;
    timer.stop();
    let restoration = desktops.restore();
    let outcome = result
        .borrow_mut()
        .take()
        .context("Service UI check ended early")?;
    outcome?;
    restoration?;
    ensure!(
        !window.get_managed_live(),
        "Service UI check left owned workloads running"
    );
    println!(
        "PASS real workload form save, startup completion, session output, manual stop/start, switch respects manual stop, saved/session setup separation, failed relaunch/Close/quit retention, explicit Force stop, successful relaunch and stop-and-quit, and desktop cleanup"
    );
    Ok(())
}
