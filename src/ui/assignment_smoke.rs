use super::*;
use anyhow::{bail, ensure};
use slint::Model;
use std::{cell::RefCell, rc::Rc, time::Duration};
use windows::core::GUID;

fn vd<T>(result: winvd::Result<T>) -> Result<T> {
    result.map_err(|error| anyhow::anyhow!("Desktop API: {error:?}"))
}

pub(super) struct Desktops {
    original: winvd::Desktop,
    original_ids: Vec<GUID>,
    name: String,
    assigned: Option<GUID>,
    restored: bool,
}

impl Desktops {
    pub(super) fn new(name: String) -> Result<Self> {
        Ok(Self {
            original: vd(winvd::get_current_desktop())?,
            original_ids: vd(winvd::get_desktops())?
                .into_iter()
                .map(|desktop| vd(desktop.get_id()))
                .collect::<Result<_>>()?,
            name,
            assigned: None,
            restored: false,
        })
    }
    pub(super) fn restore(&mut self) -> Result<()> {
        vd(winvd::switch_desktop(self.original))?;
        for desktop in vd(winvd::get_desktops())? {
            let id = vd(desktop.get_id())?;
            if !self.original_ids.contains(&id)
                && (self.assigned == Some(id)
                    || desktop.get_name().ok().as_deref() == Some(&self.name))
            {
                vd(winvd::remove_desktop(desktop, self.original))?;
            }
        }
        let ids: Vec<_> = vd(winvd::get_desktops())?
            .into_iter()
            .map(|desktop| vd(desktop.get_id()))
            .collect::<Result<_>>()?;
        ensure!(
            ids == self.original_ids
                && vd(vd(winvd::get_current_desktop())?.get_id())? == vd(self.original.get_id())?,
            "Original desktops were not restored"
        );
        self.restored = true;
        Ok(())
    }
}

impl Drop for Desktops {
    fn drop(&mut self) {
        if !self.restored
            && let Err(error) = self.restore()
        {
            eprintln!("Assignment UI desktop cleanup failed: {error:#}");
        }
    }
}

/// Explicit live exercise: real worker callbacks, a private external fixture, and
/// only the desktop created by this unique test project. No user app is adopted.
pub(super) fn run() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let name = format!("Jurabox assignment UI check {}", Uuid::new_v4());
    let desktops = Rc::new(RefCell::new(Desktops::new(name.clone())?));
    let probe = Rc::new(RefCell::new(crate::platform::OwnedProbe::start()?));
    let mut project = Project::from_form(None, &name, "", None)?;
    let app = crate::projects::AppConfig {
        open_folder: None,
        app_id: None,
        id: Uuid::new_v4(),
        name: "Terminal / brokered app".into(),
        executable: temporary.path().join("missing-broker.exe"),
        arguments: String::new(),
        new_console: false,
        shared: false,
    };
    project.apps.push(app.clone());
    let mut data = ProjectFile::default();
    data.upsert(project);
    let store = ProjectStore::new(temporary.path().join("projects.json"));
    store.save(&data)?;
    let (window, _tray) = create(store)?;
    window
        .window()
        .set_size(slint::LogicalSize::new(820., 650.));
    window.show()?;
    let artifacts = std::env::current_dir()?.join("target/ui-check");
    std::fs::create_dir_all(&artifacts)?;
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    let result = Rc::new(RefCell::new(None));
    let outcome = result.clone();
    let worker_probe = probe.clone();
    let worker_desktops = desktops.clone();
    let started = Instant::now();
    let mut step = 0;
    let mut choice = slint::SharedString::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(200),
        move || {
            let mut run_step = || -> Result<bool> {
                ensure!(
                    started.elapsed() < Duration::from_secs(45),
                    "Assignment UI check timed out at step {step}"
                );
                let window = weak
                    .upgrade()
                    .context("The assignment UI window disappeared")?;
                if window.get_workspace_busy() || window.get_window_picker_loading() {
                    return Ok(false);
                }
                match step {
                    0 => window.invoke_open_project(),
                    1 => {
                        ensure!(
                            window.get_session_open() && window.get_notice_error(),
                            "Expected an open session with an unassigned failed app"
                        );
                        show(&window);
                        window.invoke_show_window_picker(app.id.to_string().into());
                        ensure!(
                            window.get_window_picker_visible(),
                            "The window picker did not open"
                        );
                        window.invoke_new_project();
                        ensure!(
                            window.get_selected_index() == 0,
                            "Project selection changed behind the window picker"
                        );
                    }
                    2 => {
                        window.window().dispatch_event_with_result(
                            slint::platform::WindowEvent::KeyPressed {
                                text: "jurabox-ui-probe.exe".into(),
                            },
                        )?;
                        ensure!(
                            window.get_window_search() == "jurabox-ui-probe.exe"
                                && !window.get_dirty(),
                            "Picker keyboard input reached the settings form"
                        );
                        ensure!(
                            window.get_window_options().row_count() == 1,
                            "Search did not isolate the private fixture"
                        );
                        choice = window.get_window_options().row_data(0).unwrap().id;
                    }
                    3 => {
                        super::smoke::snapshot(&window, &artifacts.join("window-picker.png"))?;
                        window.invoke_assign_window(choice.clone());
                    }
                    4 => {
                        ensure!(
                            !window.get_window_picker_visible()
                                && window.get_session_apps().row_data(0).unwrap().assigned,
                            "Assignment did not reach the live session"
                        );
                        let desktop = worker_probe.borrow().desktop()?;
                        ensure!(
                            !worker_desktops.borrow().original_ids.contains(&desktop),
                            "The fixture did not move to the test project desktop"
                        );
                        worker_desktops.borrow_mut().assigned = Some(desktop);
                        let expected = worker_probe
                            .borrow()
                            .executable()
                            .canonicalize()?
                            .to_string_lossy()
                            .trim_start_matches(r"\\?\")
                            .to_lowercase();
                        ensure!(
                            window
                                .get_session_apps()
                                .row_data(0)
                                .unwrap()
                                .executable
                                .as_str()
                                == expected,
                            "The actual host executable was not retained"
                        );
                        super::smoke::snapshot(&window, &artifacts.join("assigned-window.png"))?;
                        window.invoke_relaunch_project();
                    }
                    5 => {
                        ensure!(
                            !window.get_notice_error() && worker_probe.borrow().is_alive(),
                            "Relaunch failed to reuse the manually assigned host"
                        );
                        window.invoke_unassign_window(app.id.to_string().into());
                    }
                    6 => {
                        ensure!(
                            !window.get_session_apps().row_data(0).unwrap().assigned
                                && worker_probe.borrow().is_alive(),
                            "Unassign closed the app or retained the binding"
                        );
                        window.invoke_show_window_picker(app.id.to_string().into());
                        window.invoke_cancel_window_picker();
                        ensure!(
                            !window.get_window_picker_visible(),
                            "Cancel during discovery did not close the picker"
                        );
                        window.invoke_show_window_picker(app.id.to_string().into());
                    }
                    7 => {
                        window.set_window_search("jurabox-ui-probe.exe".into());
                        window.invoke_filter_windows();
                        choice = window
                            .get_window_options()
                            .row_data(0)
                            .context("The fixture vanished before the stale-choice test")?
                            .id;
                        worker_probe.borrow_mut().close()?;
                        window.invoke_assign_window(choice.clone());
                    }
                    8 => {
                        ensure!(
                            window.get_window_picker_visible()
                                && !window.get_window_picker_error().is_empty()
                                && !window.get_session_apps().row_data(0).unwrap().assigned,
                            "A closed-window choice was accepted or its error was hidden"
                        );
                        super::smoke::snapshot(
                            &window,
                            &artifacts.join("window-picker-expired.png"),
                        )?;
                        window.invoke_refresh_window_picker();
                    }
                    9 => {
                        ensure!(
                            window.get_window_options().row_count() == 0
                                && window.get_window_picker_error().is_empty(),
                            "Refresh did not remove the closed fixture"
                        );
                        window.invoke_cancel_window_picker();
                        window.invoke_quit();
                        return Ok(true);
                    }
                    _ => bail!("Unexpected assignment UI check step"),
                }
                step += 1;
                Ok(false)
            };
            match run_step() {
                Ok(true) => *outcome.borrow_mut() = Some(Ok(())),
                Ok(false) => {}
                Err(error) => {
                    *outcome.borrow_mut() = Some(Err(error));
                    let _ = slint::quit_event_loop();
                }
            }
        },
    );
    slint::run_event_loop_until_quit()?;
    timer.stop();
    let cleanup = probe.borrow_mut().close();
    let restoration = desktops.borrow_mut().restore();
    result
        .borrow_mut()
        .take()
        .context("Assignment UI check ended early")??;
    cleanup?;
    restoration?;
    println!(
        "PASS real picker discovery/search, keyboard isolation, manual host assignment, unchanged relaunch, unassign, cancel during discovery, stale choices, refresh, and owned fixture/desktop cleanup"
    );
    Ok(())
}
