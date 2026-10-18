use super::*;
use crate::platform::apps;
use slint::Model;

pub(super) fn browser_label(window: &AppWindow) {
    let value = window.get_browser_executable();
    let name = if value.trim().is_empty() {
        "Choose a browser"
    } else {
        apps::browser_name(std::path::Path::new(value.trim().trim_matches('"')))
            .unwrap_or("Custom browser")
    };
    window.set_browser_name(name.into());
}

fn add(window: &AppWindow, app: apps::InstalledApp) {
    if window.get_picking_browser() {
        if !app.is_browser() {
            window.set_picker_error(
                "Choose Google Chrome or Microsoft Edge for project links.".into(),
            );
            return;
        }
        window.set_browser_executable(app.executable.to_string_lossy().into_owned().into());
        browser_label(window);
        window.set_dirty(true);
    } else {
        let mut drafts = launch::form_apps(window);
        drafts.push(AppDraft {
            id: Uuid::new_v4().to_string().into(),
            name: app.name.clone().into(),
            executable: app.executable.to_string_lossy().into_owned().into(),
            app_id: app.app_id.unwrap_or_default().into(),
            arguments: app.arguments.into(),
            new_console: app.new_console,
            ..Default::default()
        });
        launch::set_apps(window, drafts);
    }
    window.set_picker_visible(false);
    notice(
        window,
        &format!("{} added. Save your project to keep this change.", app.name),
        false,
    );
}

pub(super) fn setup(window: &AppWindow) {
    let weak = window.as_weak();
    window.on_open_app_picker(move |browser| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if window.get_busy() || window.get_window_picker_visible() {
            return;
        }
        window.set_picking_browser(browser);
        window.set_app_search("".into());
        window.set_picker_error("".into());
        window.set_picker_visible(true);
        window.invoke_search_apps();
        window.invoke_focus_app_search();
    });
    // Take a read-only snapshot off the UI thread. Keystrokes filter this
    // snapshot, never traverse files or the registry on the management thread.
    let catalog = Arc::new(Mutex::new(Vec::<apps::InstalledApp>::new()));
    let worker_catalog = catalog.clone();
    let weak = window.as_weak();
    std::thread::spawn(move || {
        *worker_catalog.lock().unwrap() = apps::installed_apps();
        let _ = weak.upgrade_in_event_loop(|window| {
            window.set_apps_loading(false);
            window.invoke_search_apps();
        });
    });
    let weak = window.as_weak();
    window.on_search_apps(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let query = window.get_app_search().trim().to_lowercase();
        let browser = window.get_picking_browser();
        let mut seen_browsers = std::collections::HashSet::new();
        let rows: Vec<_> = catalog
            .lock()
            .unwrap()
            .iter()
            .filter_map(|app| {
                let name = if browser {
                    let name = apps::browser_name(&app.executable)?;
                    let key = app
                        .executable
                        .to_string_lossy()
                        .replace('/', "\\")
                        .to_lowercase();
                    if !seen_browsers.insert(key) {
                        return None;
                    }
                    name
                } else {
                    &app.name
                };
                name.to_lowercase().contains(&query).then_some((app, name))
            })
            .map(|(app, name)| InstalledRow {
                name: name.into(),
                executable: app.executable.to_string_lossy().into_owned().into(),
                app_id: app.app_id.clone().unwrap_or_default().into(),
                arguments: app.arguments.clone().into(),
                new_console: app.new_console,
            })
            .collect();
        window.set_installed_apps(ModelRc::new(VecModel::from(rows)));
    });
    let weak = window.as_weak();
    window.on_choose_app(move |index| {
        if let Some(window) = weak.upgrade()
            && let Some(app) = window.get_installed_apps().row_data(index as usize)
        {
            add(
                &window,
                apps::InstalledApp {
                    name: app.name.to_string(),
                    executable: app.executable.to_string().into(),
                    app_id: (!app.app_id.is_empty()).then(|| app.app_id.to_string()),
                    arguments: app.arguments.to_string(),
                    new_console: app.new_console,
                },
            );
        }
    });
    let weak = window.as_weak();
    window.on_browse_new_app(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        match launch::pick(&window, false).and_then(|path| {
            path.map(|path| apps::resolve_app(&path.to_string_lossy()))
                .transpose()
        }) {
            Ok(Some(app)) => add(&window, app),
            Ok(None) => {}
            Err(error) => window.set_picker_error(format!("{error:#}").into()),
        }
    });
    let weak = window.as_weak();
    window.on_browse_app(move |index| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        match launch::pick(&window, false).and_then(|path| {
            path.map(|path| apps::resolve_app(&path.to_string_lossy()))
                .transpose()
        }) {
            Ok(Some(app)) => {
                let mut drafts = launch::form_apps(&window);
                if let Some(draft) = drafts.get_mut(index as usize) {
                    draft.executable = app.executable.to_string_lossy().into_owned().into();
                    draft.app_id = "".into();
                    draft.arguments = app.arguments.into();
                    draft.new_console = app.new_console;
                    if draft.name.is_empty() {
                        draft.name = app.name.into();
                    }
                    launch::set_apps(&window, drafts);
                }
            }
            Ok(None) => {}
            Err(error) => notice(&window, &format!("{error:#}"), true),
        }
    });
    let weak = window.as_weak();
    window.on_browse_browser(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        match launch::pick(&window, false).and_then(|path| {
            path.map(|path| apps::resolve_app(&path.to_string_lossy()))
                .transpose()
        }) {
            Ok(Some(app)) if app.is_browser() => {
                window.set_browser_executable(app.executable.to_string_lossy().into_owned().into());
                browser_label(&window);
                window.set_dirty(true);
            }
            Ok(Some(_)) => notice(&window, "Choose Chrome or Edge for project links.", true),
            Ok(None) => {}
            Err(error) => notice(&window, &format!("{error:#}"), true),
        }
    });
    let weak = window.as_weak();
    window.on_browser_path_changed(move || {
        if let Some(window) = weak.upgrade() {
            browser_label(&window);
        }
    });
}
