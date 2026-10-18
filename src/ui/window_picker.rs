use super::launch::Request;
use super::*;
use crate::platform::workspace::{Summary, WindowOption};

pub(super) struct Context {
    pub ticket: Uuid,
    project: Uuid,
    app: Uuid,
    options: Vec<WindowOption>,
}

pub(super) fn filter(window: &AppWindow, state: &State) {
    let query = window.get_window_search().to_lowercase();
    let rows: Vec<_> = state
        .picker
        .as_ref()
        .into_iter()
        .flat_map(|picker| &picker.options)
        .filter(|option| {
            format!(
                "{} {} {}",
                option.title, option.executable, option.assignment
            )
            .to_lowercase()
            .contains(&query)
        })
        .map(|option| WindowOptionRow {
            id: option.id.to_string().into(),
            title: option.title.clone().into(),
            executable: option.executable.clone().into(),
            assignment: option.assignment.clone().into(),
            available: option.available,
        })
        .collect();
    window.set_window_options(ModelRc::new(VecModel::from(rows)));
}

pub(super) fn loaded(
    window: &AppWindow,
    state: &mut State,
    ticket: Uuid,
    result: std::result::Result<Vec<WindowOption>, String>,
) {
    let Some(picker) = &mut state.picker else {
        return;
    };
    if picker.ticket != ticket || !window.get_window_picker_visible() {
        return;
    }
    window.set_window_picker_loading(false);
    match result {
        Ok(options) => {
            picker.options = options;
            window.set_window_picker_error("".into());
        }
        Err(error) => {
            picker.options.clear();
            window.set_window_picker_error(error.into());
        }
    }
    filter(window, state);
}

pub(super) fn completed(
    window: &AppWindow,
    state: &mut State,
    ticket: Uuid,
    success: bool,
    summary: &Summary,
) {
    if state
        .picker
        .as_ref()
        .is_none_or(|picker| picker.ticket != ticket)
    {
        return;
    }
    window.set_window_picker_loading(false);
    window.set_window_picker_assigning(false);
    if success {
        cancel(window, state);
    } else {
        window.set_window_picker_error(summary.message.clone().into());
    }
}

fn cancel(window: &AppWindow, state: &mut State) {
    state.picker = None;
    window.set_window_picker_visible(false);
    window.set_window_picker_loading(false);
    window.set_window_picker_assigning(false);
    window.set_window_options(ModelRc::new(VecModel::from(Vec::<WindowOptionRow>::new())));
}

fn refresh(window: &AppWindow, state: &mut State) {
    let Some(picker) = &mut state.picker else {
        return;
    };
    if window.get_window_picker_assigning() {
        return;
    }
    picker.ticket = Uuid::new_v4();
    picker.options.clear();
    window.set_window_picker_loading(true);
    window.set_window_picker_error("".into());
    filter(window, state);
    let picker = state.picker.as_ref().unwrap();
    if state
        .launcher
        .as_ref()
        .unwrap()
        .send(Request::Windows(picker.ticket, picker.project, picker.app))
        .is_err()
    {
        window.set_window_picker_loading(false);
        window.set_window_picker_error(
            "The workspace worker stopped. Restart Jurabox to continue.".into(),
        );
    }
}

pub(super) fn setup(window: &AppWindow, state: Arc<Mutex<State>>) {
    let weak = window.as_weak();
    let picker_state = state.clone();
    window.on_show_window_picker(move |app| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if !can_leave_form(&window) || window.get_workspace_busy() {
            return;
        }
        let Ok(app) = Uuid::parse_str(&app) else {
            return;
        };
        let mut state = picker_state.lock().unwrap();
        let Some(project) = state.selected else {
            return;
        };
        let Some(entry) = state
            .workspace
            .apps
            .iter()
            .find(|entry| entry.project == project && entry.app == app)
        else {
            notice(
                &window,
                "Open the project session before assigning a window.",
                true,
            );
            return;
        };
        window.set_window_picker_name(entry.name.clone().into());
        window.set_window_search("".into());
        window.set_window_picker_visible(true);
        state.picker = Some(Context {
            ticket: Uuid::new_v4(),
            project,
            app,
            options: Vec::new(),
        });
        refresh(&window, &mut state);
        window.invoke_focus_window_search();
    });
    let weak = window.as_weak();
    let picker_state = state.clone();
    window.on_refresh_window_picker(move || {
        if let Some(window) = weak.upgrade() {
            refresh(&window, &mut picker_state.lock().unwrap());
        }
    });
    let weak = window.as_weak();
    let picker_state = state.clone();
    window.on_filter_windows(move || {
        if let Some(window) = weak.upgrade() {
            filter(&window, &picker_state.lock().unwrap());
        }
    });
    let weak = window.as_weak();
    let picker_state = state.clone();
    window.on_assign_window(move |choice| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if window.get_window_picker_loading() || !window.get_window_picker_visible() {
            return;
        }
        let Ok(choice) = Uuid::parse_str(&choice) else {
            return;
        };
        let state = picker_state.lock().unwrap();
        let Some(picker) = &state.picker else {
            return;
        };
        window.set_window_picker_loading(true);
        window.set_window_picker_assigning(true);
        window.set_workspace_busy(true);
        if state
            .launcher
            .as_ref()
            .unwrap()
            .send(Request::Assign(picker.ticket, choice))
            .is_err()
        {
            window.set_workspace_busy(false);
            window.set_window_picker_loading(false);
            window.set_window_picker_assigning(false);
            window.set_window_picker_error(
                "The workspace worker stopped. Restart Jurabox to continue.".into(),
            );
        }
    });
    let weak = window.as_weak();
    let picker_state = state.clone();
    window.on_cancel_window_picker(move || {
        if let Some(window) = weak.upgrade()
            && !window.get_window_picker_assigning()
        {
            cancel(&window, &mut picker_state.lock().unwrap());
        }
    });
    let weak = window.as_weak();
    window.on_unassign_window(move |app| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if !can_leave_form(&window) || window.get_workspace_busy() {
            return;
        }
        let Ok(app) = Uuid::parse_str(&app) else {
            return;
        };
        let state = state.lock().unwrap();
        let Some(project) = state.selected else {
            return;
        };
        window.set_workspace_busy(true);
        if state
            .launcher
            .as_ref()
            .unwrap()
            .send(Request::Unassign(project, app))
            .is_err()
        {
            window.set_workspace_busy(false);
            notice(
                &window,
                "The workspace worker stopped. Restart Jurabox to continue.",
                true,
            );
        }
    });
}
