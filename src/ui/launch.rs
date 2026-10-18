use super::*;
use crate::{
    platform::{apps, workspace::Workspaces},
    projects::{AppConfig, BrowserConfig},
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::Model;
use std::{path::PathBuf, sync::mpsc};
use windows::Win32::Foundation::HWND;

#[derive(Clone)]
pub(super) enum Request {
    Open(Project, bool),
    Close(Uuid),
    Desktop(winvd::DesktopEvent),
    Windows(Uuid, Uuid, Uuid),
    Assign(Uuid, Uuid),
    Unassign(Uuid, Uuid),
    Service(Uuid, Uuid, crate::services::Action),
    Poll,
    StopAndQuit,
    LeaveAndQuit,
}

impl From<winvd::DesktopEvent> for Request {
    fn from(event: winvd::DesktopEvent) -> Self {
        Self::Desktop(event)
    }
}

pub(super) fn refresh_status(window: &AppWindow, state: &State) {
    window.set_session_open(
        state
            .selected
            .is_some_and(|id| state.workspace.open.contains(&id)),
    );
    let name = state
        .workspace
        .active
        .and_then(|id| state.data.projects.iter().find(|project| project.id == id))
        .map(|project| format!("Active: {}", project.name))
        .unwrap_or_else(|| "No project active".into());
    window.set_active_workspace(name.into());
    let apps: Vec<_> = state
        .workspace
        .apps
        .iter()
        .filter(|entry| Some(entry.project) == state.selected)
        .map(|entry| SessionAppRow {
            id: entry.app.to_string().into(),
            name: entry.name.clone().into(),
            assigned: entry.title.is_some(),
            title: entry.title.clone().unwrap_or_default().into(),
            executable: entry.executable.clone().unwrap_or_default().into(),
            shared: entry.shared,
        })
        .collect();
    window.set_session_apps(ModelRc::new(VecModel::from(apps)));
    service_forms::refresh_runtime(window, state);
}

pub(super) fn read_apps(window: &AppWindow, project: &mut Project) -> Result<()> {
    project.apps = window
        .get_apps()
        .iter()
        .map(|app| {
            let mut executable = PathBuf::from(app.executable.trim().trim_matches('"'));
            let mut arguments = app.arguments.to_string();
            let mut new_console = app.new_console;
            if executable
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
            {
                let resolved = apps::resolve_app(&app.executable)?;
                executable = resolved.executable;
                if arguments.trim().is_empty() {
                    arguments = resolved.arguments;
                }
                new_console |= resolved.new_console;
            }
            Ok(AppConfig {
                open_folder: match app.folder_mode {
                    0 => None,
                    1 => Some(crate::projects::OpenFolder::Project),
                    2 => Some(crate::projects::OpenFolder::Custom(PathBuf::from(
                        app.editor_folder.trim().trim_matches('"'),
                    ))),
                    _ => anyhow::bail!("Choose a project to open in Zed's Advanced settings."),
                },
                app_id: (!app.app_id.is_empty()).then(|| app.app_id.to_string()),
                id: Uuid::parse_str(&app.id)?,
                name: app.name.trim().into(),
                executable,
                arguments,
                new_console,
                shared: app.shared,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    project.browser = window.get_browser_enabled().then(|| BrowserConfig {
        executable: PathBuf::from(window.get_browser_executable().trim().trim_matches('"')),
        urls: window
            .get_browser_urls()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect(),
    });
    project.validate()
}

pub(super) fn form_apps(window: &AppWindow) -> Vec<AppDraft> {
    window.get_apps().iter().collect()
}

pub(super) fn set_apps(window: &AppWindow, apps: Vec<AppDraft>) {
    window.set_apps(ModelRc::new(VecModel::from(apps)));
    window.set_dirty(true);
}

fn add_app(window: &AppWindow, name: String, executable: String) {
    let mut apps = form_apps(window);
    apps.push(AppDraft {
        id: Uuid::new_v4().to_string().into(),
        name: name.into(),
        executable: executable.into(),
        advanced: true,
        ..Default::default()
    });
    set_apps(window, apps);
}

pub(super) fn pick(window: &AppWindow, folder: bool) -> Result<Option<PathBuf>> {
    let handle = window.window().window_handle();
    let RawWindowHandle::Win32(handle) = handle.window_handle()?.as_raw() else {
        anyhow::bail!("A Windows window handle is required.");
    };
    apps::pick_path(HWND(handle.hwnd.get() as *mut _), folder)
}

pub(super) fn setup(window: &AppWindow, state: Arc<Mutex<State>>, persistent: bool) {
    let (sender, receiver) = mpsc::channel::<Request>();
    state.lock().unwrap().launcher = Some(sender.clone());
    if persistent {
        let journal = state.lock().unwrap().store.session_journal();
        super::connection::start(window, state.clone(), receiver, journal);
    } else {
        let weak = window.as_weak();
        let worker_state = state.clone();
        std::thread::spawn(move || {
            let _listener = winvd::listen_desktop_events(sender);
            if let Err(error) = &_listener {
                let message = format!(
                    "Desktop event tracking unavailable ({error:?}). Open/Switch remains available, but native switches cannot be tracked."
                );
                let _ = weak.upgrade_in_event_loop(move |window| notice(&window, &message, true));
            }
            let mut workspaces = Workspaces::default();
            let mut choices: Option<(Uuid, crate::platform::workspace::WindowChoices)> = None;
            let mut last_workloads = Vec::new();
            loop {
                let request = if workspaces.service_activity() {
                    match receiver.recv_timeout(std::time::Duration::from_millis(500)) {
                        Ok(request) => request,
                        Err(mpsc::RecvTimeoutError::Timeout) => Request::Poll,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                } else {
                    match receiver.recv() {
                        Ok(request) => request,
                        Err(_) => break,
                    }
                };
                let mut summary;
                let mut picker_completion = None;
                let mut quit = false;
                let completed = match request {
                    Request::LeaveAndQuit => continue,
                    Request::Open(project, relaunch) => {
                        let result = workspaces.open(project, relaunch);
                        summary = workspaces.summary();
                        match result {
                            Ok(message) => summary.message = message,
                            Err(error) => {
                                summary.message = format!("{error:#}");
                                summary.error = true;
                            }
                        }
                        true
                    }
                    Request::Close(id) => {
                        let result = workspaces.close(id);
                        summary = workspaces.summary();
                        match result {
                            Ok(message) => summary.message = message,
                            Err(error) => {
                                summary.message = format!("{error:#}");
                                summary.error = true;
                            }
                        }
                        true
                    }
                    Request::Service(project, service, action) => {
                        let result = workspaces.service_action(project, service, action);
                        summary = workspaces.summary();
                        match result {
                            Ok(message) => summary.message = message,
                            Err(error) => {
                                summary.message = format!("{error:#}");
                                summary.error = true;
                            }
                        }
                        true
                    }
                    Request::StopAndQuit => {
                        let result = workspaces.stop_for_exit();
                        summary = workspaces.summary();
                        match result {
                            Ok(()) => {
                                quit = true;
                                summary.message = "Managed workloads stopped.".into();
                            }
                            Err(error) => {
                                summary.message = format!("{error:#}");
                                summary.error = true;
                            }
                        }
                        true
                    }
                    Request::Poll => {
                        let workloads = workspaces.poll_services();
                        if workloads != last_workloads {
                            last_workloads = workloads.clone();
                            let state = worker_state.clone();
                            let _ = weak.upgrade_in_event_loop(move |window| {
                                let mut state = state.lock().unwrap();
                                state.workspace.workloads = workloads;
                                service_forms::refresh_runtime(&window, &state);
                            });
                        }
                        continue;
                    }
                    Request::Windows(ticket, project, app) => {
                        let result = match workspaces.window_choices(project, app) {
                            Ok(snapshot) => {
                                let options = snapshot.options.clone();
                                choices = Some((ticket, snapshot));
                                Ok(options)
                            }
                            Err(error) => {
                                choices = None;
                                Err(format!("{error:#}"))
                            }
                        };
                        let state = worker_state.clone();
                        let _ = weak.upgrade_in_event_loop(move |window| {
                            window_picker::loaded(
                                &window,
                                &mut state.lock().unwrap(),
                                ticket,
                                result,
                            );
                        });
                        continue;
                    }
                    Request::Assign(ticket, choice) => {
                        let result =
                            match choices.as_ref().filter(|(current, _)| *current == ticket) {
                                Some((_, choices)) => workspaces.assign_choice(choices, choice),
                                None => Err(anyhow::anyhow!(
                                    "That window list is out of date. Refresh and choose again."
                                )),
                            };
                        picker_completion = Some((ticket, result.is_ok()));
                        summary = workspaces.summary();
                        match result {
                            Ok(message) => summary.message = message,
                            Err(error) => {
                                summary.message = format!("{error:#}");
                                summary.error = true;
                            }
                        }
                        true
                    }
                    Request::Unassign(project, app) => {
                        let result = workspaces.unassign_window(project, app);
                        summary = workspaces.summary();
                        match result {
                            Ok(message) => summary.message = message,
                            Err(error) => {
                                summary.message = format!("{error:#}");
                                summary.error = true;
                            }
                        }
                        true
                    }
                    Request::Desktop(event) => {
                        if !matches!(
                            event,
                            winvd::DesktopEvent::DesktopChanged { .. }
                                | winvd::DesktopEvent::DesktopDestroyed { .. }
                        ) {
                            continue;
                        }
                        summary = workspaces.desktop_changed();
                        if matches!(event, winvd::DesktopEvent::DesktopDestroyed { .. }) {
                            summary.message = "A desktop was removed. Open an affected project to restore its workspace; apps stay running.".into();
                        }
                        false
                    }
                };
                last_workloads = summary.workloads.clone();
                let state = worker_state.clone();
                let _ = weak.upgrade_in_event_loop(move |window| {
                    if completed {
                        window.set_workspace_busy(false);
                    }
                    let mut state = state.lock().unwrap();
                    if let Some((ticket, success)) = picker_completion {
                        window_picker::completed(&window, &mut state, ticket, success, &summary);
                    }
                    state.workspace = summary;
                    refresh_status(&window, &state);
                    if !state.workspace.message.is_empty() {
                        notice(&window, &state.workspace.message, state.workspace.error);
                    }
                    if quit {
                        if window.get_dirty() {
                            notice(
                                &window,
                                "Services stopped. Save or discard your changes before quitting.",
                                true,
                            );
                        } else {
                            let _ = slint::quit_event_loop();
                        }
                    }
                });
            }
        });
    }

    let weak = window.as_weak();
    let leave_state = state.clone();
    window.on_leave_and_quit(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if !can_leave_form(&window) || window.get_workspace_busy() {
            return;
        }
        let state = leave_state.lock().unwrap();
        window.set_workspace_busy(true);
        if state
            .launcher
            .as_ref()
            .unwrap()
            .send(Request::LeaveAndQuit)
            .is_err()
        {
            window.set_workspace_busy(false);
            notice(
                &window,
                "Cannot contact the session worker. Reopen Jurabox to reconnect.",
                true,
            );
        }
    });

    for relaunch in [false, true] {
        let weak = window.as_weak();
        let state = state.clone();
        let callback = move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            if !can_leave_form(&window) || window.get_workspace_busy() {
                return;
            }
            let state = state.lock().unwrap();
            let Some(project) = state
                .data
                .projects
                .iter()
                .find(|project| Some(project.id) == state.selected)
                .cloned()
            else {
                return;
            };
            window.set_workspace_busy(true);
            notice(
                &window,
                if relaunch {
                    "Applying saved setup…"
                } else {
                    "Opening workspace…"
                },
                false,
            );
            if state
                .launcher
                .as_ref()
                .unwrap()
                .send(Request::Open(project, relaunch))
                .is_err()
            {
                window.set_workspace_busy(false);
                notice(
                    &window,
                    "The workspace worker stopped. Restart Jurabox to continue.",
                    true,
                );
            }
        };
        if relaunch {
            window.on_relaunch_project(callback);
        } else {
            window.on_open_project(callback);
        }
    }
    let weak = window.as_weak();
    let close_state = state;
    window.on_close_project(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if !can_leave_form(&window) || window.get_workspace_busy() {
            return;
        }
        let state = close_state.lock().unwrap();
        if let Some(id) = state.selected {
            window.set_workspace_busy(true);
            if state
                .launcher
                .as_ref()
                .unwrap()
                .send(Request::Close(id))
                .is_err()
            {
                window.set_workspace_busy(false);
                notice(
                    &window,
                    "The workspace worker stopped. Restart Jurabox to continue.",
                    true,
                );
            }
        }
    });

    let weak = window.as_weak();
    window.on_add_app(move || {
        if let Some(window) = weak.upgrade() {
            add_app(&window, String::new(), String::new());
        }
    });
    let weak = window.as_weak();
    window.on_remove_app(move |index| {
        if let Some(window) = weak.upgrade() {
            let mut apps = form_apps(&window);
            if (index as usize) < apps.len() {
                apps.remove(index as usize);
                set_apps(&window, apps);
            }
        }
    });
    super::app_picker::setup(window);
    let weak = window.as_weak();
    window.on_browse_folder(move || {
        if let Some(window) = weak.upgrade() {
            match pick(&window, true) {
                Ok(Some(path)) => {
                    window.set_project_folder(path.to_string_lossy().into_owned().into());
                    window.set_dirty(true);
                }
                Ok(None) => {}
                Err(error) => notice(&window, &format!("{error:#}"), true),
            }
        }
    });
}
