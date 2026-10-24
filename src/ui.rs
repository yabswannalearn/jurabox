#[cfg(windows)]
mod app_picker;
#[cfg(windows)]
mod assignment_smoke;
#[cfg(windows)]
mod connection;
mod icons;
#[cfg(windows)]
mod launch;
#[cfg(windows)]
mod performance;
#[cfg(windows)]
mod preferences;
#[cfg(windows)]
mod service_forms;
#[cfg(windows)]
mod service_smoke;
#[cfg(windows)]
mod session_smoke;
mod smoke;
#[cfg(windows)]
mod window_picker;

use crate::projects::{Project, ProjectFile, ProjectStore};
use anyhow::{Context, Result};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};
use uuid::Uuid;

slint::include_modules!();

/// Preserve repeated UI items and focus when only a few rows change.
fn update_rows<T: Clone + PartialEq + 'static>(model: ModelRc<T>, rows: Vec<T>) -> ModelRc<T> {
    use slint::Model;
    if model.row_count() != rows.len() {
        return ModelRc::new(VecModel::from(rows));
    }
    for (index, row) in rows.into_iter().enumerate() {
        if model.row_data(index).as_ref() != Some(&row) {
            model.set_row_data(index, row);
        }
    }
    model
}

struct State {
    data: ProjectFile,
    selected: Option<Uuid>,
    store: Arc<ProjectStore>,
    #[cfg(windows)]
    workspace: crate::platform::workspace::Summary,
    #[cfg(windows)]
    launcher: Option<std::sync::mpsc::Sender<launch::Request>>,
    #[cfg(windows)]
    picker: Option<window_picker::Context>,
}

fn notice(window: &AppWindow, message: &str, error: bool) {
    window.set_notice(message.into());
    window.set_notice_error(error);
}

fn refresh(window: &AppWindow, state: &State) {
    window.set_start_with_windows(state.data.preferences.start_with_windows);
    window.set_mute_failures(state.data.preferences.mute_failures);
    window.set_resolve_confirmed(false);
    let rows: Vec<_> = state
        .data
        .projects
        .iter()
        .map(|project| ProjectRow {
            status: "".into(),
            name: project.name.clone().into(),
            folder: project
                .folder
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| "No folder".into())
                .into(),
        })
        .collect();
    window.set_projects(ModelRc::new(VecModel::from(rows)));
    let selected = state
        .data
        .projects
        .iter()
        .position(|project| Some(project.id) == state.selected);
    window.set_selected_index(selected.map_or(-1, |index| index as i32));
    window.set_editing_existing(selected.is_some());
    if let Some(index) = selected {
        let project = &state.data.projects[index];
        window.set_project_name(project.name.clone().into());
        window.set_project_folder(
            project
                .folder
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into(),
        );
        window.set_auto_stop_enabled(project.inactivity_minutes.is_some());
        window.set_auto_stop_minutes(project.inactivity_minutes.unwrap_or(15) as i32);
        let apps: Vec<_> = project
            .apps
            .iter()
            .map(|app| AppDraft {
                icon_key: icons::key(
                    &app.executable.to_string_lossy(),
                    app.app_id.as_deref().unwrap_or_default(),
                )
                .into(),
                icon: icons::get(&icons::key(
                    &app.executable.to_string_lossy(),
                    app.app_id.as_deref().unwrap_or_default(),
                )),
                id: app.id.to_string().into(),
                name: app.name.clone().into(),
                executable: app.executable.to_string_lossy().into_owned().into(),
                app_id: app.app_id.clone().unwrap_or_default().into(),
                arguments: app.arguments.clone().into(),
                new_console: app.new_console,
                shared: app.shared,
                advanced: false,
                zed: app.app_id.is_none() && crate::projects::is_zed(&app.executable),
                folder_mode: match &app.open_folder {
                    None => 0,
                    Some(crate::projects::OpenFolder::Project) => 1,
                    Some(crate::projects::OpenFolder::Custom(_)) => 2,
                },
                editor_folder: match &app.open_folder {
                    Some(crate::projects::OpenFolder::Custom(path)) => {
                        path.to_string_lossy().into_owned().into()
                    }
                    _ => "".into(),
                },
            })
            .collect();
        window.set_apps(ModelRc::new(VecModel::from(apps)));
        window.set_browser_enabled(project.browser.is_some());
        window.set_browser_executable(
            project
                .browser
                .as_ref()
                .map(|browser| browser.executable.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into(),
        );
        window.set_browser_urls(
            project
                .browser
                .as_ref()
                .map(|browser| browser.urls.join("\n"))
                .unwrap_or_default()
                .into(),
        );
    } else {
        window.set_project_name("".into());
        window.set_project_folder("".into());
        window.set_auto_stop_enabled(true);
        window.set_auto_stop_minutes(15);
        window.set_apps(ModelRc::new(VecModel::from(Vec::<AppDraft>::new())));
        window.set_browser_enabled(false);
        window.set_browser_executable("".into());
        window.set_browser_urls("".into());
    }
    window.set_dirty(false);
    window.set_browser_advanced(false);
    #[cfg(windows)]
    app_picker::browser_label(window);
    window.set_delete_pending(false);
    window.set_picker_visible(false);
    #[cfg(windows)]
    {
        service_forms::load(window, selected.map(|index| &state.data.projects[index]));
        launch::refresh_status(window, state);
    }
}

fn can_leave_form(window: &AppWindow) -> bool {
    if window.get_busy() {
        return false;
    }
    if window.get_window_picker_visible() || window.get_picker_visible() {
        notice(window, "Close the picker before changing projects.", true);
        return false;
    }
    if window.get_dirty() {
        notice(
            window,
            "Save or discard your changes before leaving this project.",
            true,
        );
        return false;
    }
    true
}

fn persist(
    window: &AppWindow,
    state: Arc<Mutex<State>>,
    next: ProjectFile,
    selection: Option<Uuid>,
    message: &'static str,
) {
    window.set_busy(true);
    notice(window, "Saving…", false);
    let store = state.lock().unwrap().store.clone();
    let weak = window.as_weak();
    std::thread::spawn(move || {
        let result = store.save(&next).map_err(|error| format!("{error:#}"));
        let _ = weak.upgrade_in_event_loop(move |window| {
            window.set_busy(false);
            match result {
                Ok(()) => {
                    let mut state = state.lock().unwrap();
                    state.data = next;
                    state.selected = selection;
                    refresh(&window, &state);
                    notice(&window, message, false);
                }
                Err(error) => notice(&window, &error, true),
            }
        });
    });
}

pub(super) fn show(window: &AppWindow) {
    if let Err(error) = window.show() {
        eprintln!("Cannot show Jurabox: {error}");
    }
    window.window().set_minimized(false);
    #[cfg(windows)]
    if let Err(error) = crate::platform::activate_management_window(window.window()) {
        eprintln!("Cannot bring the management window to this desktop: {error:#}");
    }
}

fn request_quit(window: &AppWindow) {
    if window.get_busy() || window.get_workspace_busy() {
        show(window);
        notice(
            window,
            "Wait for the current operation to finish before quitting.",
            true,
        );
    } else if window.get_managed_live() && !window.get_session_host_enabled() {
        show(window);
        notice(
            window,
            "Managed workloads or kept Compose containers still belong to this session. Choose Stop services and quit, or close their sessions first.",
            true,
        );
    } else if can_leave_form(window) {
        if window.get_session_host_enabled() && window.get_session_connected() {
            window.invoke_leave_and_quit();
        } else {
            let _ = slint::quit_event_loop();
        }
    } else {
        show(window);
    }
}

pub(super) fn create(store: ProjectStore) -> Result<(AppWindow, AppTray)> {
    create_with_host(store, false)
}

fn create_with_host(store: ProjectStore, persistent: bool) -> Result<(AppWindow, AppTray)> {
    let data = store.load()?;
    let selected = data.projects.first().map(|project| project.id);
    let state = Arc::new(Mutex::new(State {
        data,
        selected,
        store: Arc::new(store),
        #[cfg(windows)]
        workspace: Default::default(),
        #[cfg(windows)]
        launcher: None,
        #[cfg(windows)]
        picker: None,
    }));
    let window = AppWindow::new().context("Cannot create the Jurabox window.")?;
    let tray = AppTray::new().context("Cannot create the Jurabox tray icon.")?;
    #[cfg(windows)]
    icons::setup(&window);
    #[cfg(windows)]
    preferences::setup(&window, state.clone());
    window.set_session_host_enabled(persistent);
    refresh(&window, &state.lock().unwrap());
    #[cfg(windows)]
    launch::setup(&window, state.clone(), persistent);
    #[cfg(windows)]
    service_forms::setup(&window, state.clone());
    #[cfg(windows)]
    window_picker::setup(&window, state.clone());

    let weak = window.as_weak();
    let selected_state = state.clone();
    window.on_select_project(move |index| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if !can_leave_form(&window) {
            return;
        }
        let mut state = selected_state.lock().unwrap();
        let Some(project) = state.data.projects.get(index as usize) else {
            return;
        };
        state.selected = Some(project.id);
        refresh(&window, &state);
        notice(&window, "", false);
    });

    let weak = window.as_weak();
    let new_state = state.clone();
    window.on_new_project(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if !can_leave_form(&window) {
            return;
        }
        let mut state = new_state.lock().unwrap();
        state.selected = None;
        refresh(&window, &state);
        window.invoke_focus_name();
        notice(&window, "", false);
    });

    let weak = window.as_weak();
    let discard_state = state.clone();
    window.on_discard_changes(move || {
        if let Some(window) = weak.upgrade() {
            if window.get_busy() {
                return;
            }
            refresh(&window, &discard_state.lock().unwrap());
            notice(&window, "Changes discarded.", false);
        }
    });

    let weak = window.as_weak();
    let save_state = state.clone();
    window.on_save_project(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if window.get_busy() || window.get_window_picker_visible() || window.get_picker_visible() {
            return;
        }
        let state = save_state.lock().unwrap();
        let minutes = window
            .get_auto_stop_enabled()
            .then_some(window.get_auto_stop_minutes() as u32);
        let mut project = match Project::from_form(
            state.selected,
            window.get_project_name().as_str(),
            window.get_project_folder().as_str(),
            minutes,
        ) {
            Ok(project) => project,
            Err(error) => {
                notice(&window, &error.to_string(), true);
                return;
            }
        };
        #[cfg(windows)]
        if let Err(error) = launch::read_apps(&window, &mut project) {
            notice(&window, &format!("{error:#}"), true);
            return;
        }
        #[cfg(windows)]
        if let Err(error) = service_forms::read(&window, &mut project) {
            notice(&window, &format!("{error:#}"), true);
            return;
        }
        let id = project.id;
        let mut next = state.data.clone();
        next.upsert(project);
        drop(state);
        persist(
            &window,
            save_state.clone(),
            next,
            Some(id),
            "Project saved on this PC.",
        );
    });

    let weak = window.as_weak();
    let delete_state = state;
    window.on_delete_project(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if !can_leave_form(&window) || !window.get_delete_pending() {
            return;
        }
        let state = delete_state.lock().unwrap();
        let Some(id) = state.selected else {
            return;
        };
        #[cfg(windows)]
        if window.get_session_host_enabled() && !window.get_session_connected() {
            notice(
                &window,
                "Reconnect or resolve lost sessions before deleting a project.",
                true,
            );
            return;
        }
        #[cfg(windows)]
        if state.workspace.open.contains(&id) || window.get_workspace_busy() {
            notice(
                &window,
                "Close the project session before deleting its setup.",
                true,
            );
            return;
        }
        let mut next = state.data.clone();
        next.remove(id);
        let selection = next.projects.first().map(|project| project.id);
        drop(state);
        persist(
            &window,
            delete_state.clone(),
            next,
            selection,
            "Project removed. Its files are unchanged.",
        );
    });

    let weak = window.as_weak();
    window.on_hide_window(move || {
        if let Some(window) = weak.upgrade() {
            let _ = window.hide();
        }
    });
    window
        .window()
        .on_close_requested(|| slint::CloseRequestResponse::HideWindow);
    let weak = window.as_weak();
    window.on_quit(move || {
        if let Some(window) = weak.upgrade() {
            request_quit(&window);
        }
    });
    let weak = window.as_weak();
    tray.on_show_window(move || {
        if let Some(window) = weak.upgrade() {
            show(&window);
        }
    });
    let weak = window.as_weak();
    tray.on_new_project(move || {
        if let Some(window) = weak.upgrade() {
            show(&window);
            window.invoke_new_project();
        }
    });
    let weak = window.as_weak();
    tray.on_quit(move || {
        if let Some(window) = weak.upgrade() {
            request_quit(&window);
        }
    });
    let weak = window.as_weak();
    tray.on_stop_and_quit(move || {
        if let Some(window) = weak.upgrade() {
            window.invoke_stop_and_quit();
        }
    });
    Ok((window, tray))
}

pub fn run(start_hidden: bool) -> Result<()> {
    let startup = Instant::now();
    #[cfg(windows)]
    let Some(mut instance) = crate::platform::ManagerInstance::acquire()? else {
        return Ok(());
    };
    let path = ProjectStore::default_path()?;
    let (window, _tray) = create_with_host(ProjectStore::new(path), true)?;
    #[cfg(windows)]
    instance.listen(window.as_weak());
    if !start_hidden {
        window.show()?;
    }
    eprintln!(
        "Jurabox UI initialized in {} ms",
        startup.elapsed().as_millis()
    );
    slint::run_event_loop_until_quit()?;
    Ok(())
}

pub fn check() -> Result<()> {
    smoke::run()
}

#[cfg(windows)]
pub fn performance_check() -> Result<()> {
    performance::run()
}

pub fn service_check() -> Result<()> {
    #[cfg(windows)]
    {
        service_smoke::run()
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("Service UI checks require Windows 11.")
    }
}

#[cfg(windows)]
pub(crate) fn session_check() -> Result<()> {
    session_smoke::run()
}

pub fn assignment_check() -> Result<()> {
    #[cfg(windows)]
    {
        assignment_smoke::run()
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("The assignment exercise requires Windows 11.")
    }
}
