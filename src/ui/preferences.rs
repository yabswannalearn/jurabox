use super::*;
use crate::projects::Preferences;
use std::path::PathBuf;

pub(super) fn setup(window: &AppWindow, state: Arc<Mutex<State>>) {
    let weak = window.as_weak();
    let archive_state = state.clone();
    window.on_archive_session_record(move || {
        let Some(window) = weak.upgrade() else { return; };
        let state = archive_state.lock().unwrap(); let journal = state.store.session_journal();
        let sender = state.launcher.clone(); let weak = window.as_weak(); window.set_workspace_busy(true);
        std::thread::spawn(move || {
            let result = crate::session_host::archive_lost_record(&journal);
            let _ = weak.upgrade_in_event_loop(move |window| { window.set_workspace_busy(false); window.set_resolve_confirmed(false);
                match result { Ok(()) => { notice(&window, "Original runtime evidence archived. Existing work stays unmanaged. Inspect it before opening a new session.", false);
                    if let Some(sender) = sender { let _ = sender.send(launch::Request::Poll); } },
                    Err(error) => notice(&window, &format!("{error:#}"), true) }
            });
        });
    });
    let weak = window.as_weak();
    let current = state.clone();
    window.on_save_preferences(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if !can_leave_form(&window) {
            return;
        }
        let preferences = Preferences {
            start_with_windows: window.get_start_with_windows(),
            mute_failures: window.get_mute_failures(),
        };
        let current = current.clone();
        let weak = window.as_weak();
        window.set_busy(true);
        std::thread::spawn(move || {
            let mut next = current.lock().unwrap().data.clone();
            let old = next.preferences.clone();
            let store = current.lock().unwrap().store.clone();
            next.preferences = preferences.clone();
            let result = (|| -> Result<()> {
                if preferences.start_with_windows != old.start_with_windows {
                    crate::platform::preferences::startup(preferences.start_with_windows)?;
                }
                if let Err(error) = store.save(&next) {
                    if preferences.start_with_windows != old.start_with_windows {
                        let _ = crate::platform::preferences::startup(old.start_with_windows);
                    }
                    return Err(error);
                }
                current.lock().unwrap().data = next;
                Ok(())
            })();
            let _ = weak.upgrade_in_event_loop(move |window| {
                window.set_busy(false);
                match result {
                    Ok(()) => notice(&window, "Preferences saved.", false),
                    Err(error) => notice(&window, &format!("{error:#}"), true),
                }
            });
        });
    });
    let weak = window.as_weak();
    window.on_browse_import(move || {
        if let Some(window) = weak.upgrade() {
            match launch::pick(&window, false) {
                Ok(Some(path)) => {
                    window.set_import_path(path.to_string_lossy().into_owned().into())
                }
                Ok(None) => {}
                Err(error) => notice(&window, &format!("{error:#}"), true),
            }
        }
    });
    let weak = window.as_weak();
    let current = state.clone();
    window.on_import_projects(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if !can_leave_form(&window) {
            return;
        }
        let path = PathBuf::from(window.get_import_path().as_str());
        let old = (!window.get_remap_from().is_empty())
            .then(|| PathBuf::from(window.get_remap_from().as_str()));
        let new = (!window.get_remap_to().is_empty())
            .then(|| PathBuf::from(window.get_remap_to().as_str()));
        let mut next = current.lock().unwrap().data.clone();
        let selection = current.lock().unwrap().selected;
        let current = current.clone();
        let weak = window.as_weak();
        window.set_busy(true);
        std::thread::spawn(move || {
            let result = ProjectStore::new(path)
                .load()
                .and_then(|imported| next.import_copies(imported, old.as_deref(), new.as_deref()));
            let _ = weak.upgrade_in_event_loop(move |window| match result {
                Ok(_) => persist(
                    &window,
                    current,
                    next,
                    selection,
                    "Project copies imported. Review paths and commands before opening.",
                ),
                Err(error) => {
                    window.set_busy(false);
                    notice(&window, &format!("{error:#}"), true);
                }
            });
        });
    });
    let weak = window.as_weak();
    window.on_export_projects(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if !can_leave_form(&window) {
            return;
        }
        let folder = match launch::pick(&window, true) {
            Ok(Some(folder)) => folder,
            Ok(None) => return,
            Err(error) => {
                notice(&window, &format!("{error:#}"), true);
                return;
            }
        };
        let data = state
            .lock()
            .unwrap()
            .data
            .portable(window.get_export_values());
        let file = folder.join(format!(
            "jurabox-projects-{}.json",
            &Uuid::new_v4().simple().to_string()[..8]
        ));
        let weak = window.as_weak();
        window.set_busy(true);
        std::thread::spawn(move || {
            let result = ProjectStore::new(file.clone()).save(&data);
            let _ = weak.upgrade_in_event_loop(move |window| {
                window.set_busy(false);
                match result {
                    Ok(()) => notice(
                        &window,
                        &format!("Exported saved projects to {}", file.display()),
                        false,
                    ),
                    Err(error) => notice(&window, &format!("{error:#}"), true),
                }
            });
        });
    });
}

pub(super) fn notify_changes(
    window: &AppWindow,
    previous: &crate::platform::workspace::Summary,
    next: &crate::platform::workspace::Summary,
) {
    if window.get_mute_failures() || !window.get_session_host_enabled() {
        return;
    }
    let new_failure = next.workloads.iter().find(|(project, view)| {
        view.error
            && !previous
                .workloads
                .iter()
                .any(|(old_project, old)| old_project == project && old.id == view.id && old.error)
    });
    if let Some((_, view)) = new_failure {
        crate::platform::preferences::notify_failure(&format!("{}: {}", view.name, view.detail));
    } else if next.error && (!previous.error || previous.message != next.message) {
        crate::platform::preferences::notify_failure(&next.message);
    }
}
