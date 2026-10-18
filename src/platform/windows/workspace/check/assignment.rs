use super::*;

pub(super) fn run(
    workspaces: &mut Workspaces,
    cleanup: &mut Cleanup,
    window: &AppWindow,
    dead: &AppWindow,
    other: &Project,
) -> Result<()> {
    let mut project = Project::from_form(None, "Jurabox manual assignment check", "", None)?;
    let app = AppConfig {
        app_id: None,
        id: Uuid::new_v4(),
        name: "Brokered app slot".into(),
        executable: cleanup.probe_folder.path().join("missing-broker.exe"),
        arguments: String::new(),
        new_console: false,
        shared: true,
    };
    project.apps.push(app.clone());
    let result = workspaces.open(project.clone(), false);
    cleanup.capture(workspaces);
    ensure!(
        result.is_err(),
        "The missing launch executable was not reported"
    );
    let choices = workspaces.window_choices(project.id, app.id)?;
    let choice = choices
        .options
        .iter()
        .find(|choice| choice.executable == window.executable)
        .context("Owned fixture absent from window picker")?;
    ensure!(
        choice.available,
        "An unassigned fixture was incorrectly blocked"
    );
    ensure!(
        workspaces.assign_choice(&choices, Uuid::new_v4()).is_err(),
        "Unknown window tokens were accepted"
    );
    workspaces.assign_choice(&choices, choice.id)?;
    ensure!(
        workspaces.sessions[&project.id].entries[0].window.as_ref() == Some(window),
        "Manual window identity was not retained"
    );
    ensure!(
        window.executable != app_windows::path_key(&app.executable),
        "Fixture must represent a different host executable"
    );
    // Renaming the title must not invalidate a process/window identity.
    // SAFETY: The caller passes only its own verified external fixture window.
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(
            window.hwnd(),
            &windows::core::HSTRING::from("Jurabox renamed assignment fixture"),
        )?;
    }
    ensure!(
        window.is_alive() && window.title() == "Jurabox renamed assignment fixture",
        "A title change invalidated the assigned window"
    );
    // SAFETY: Hide/show only the verified private fixture, never a user window.
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
            window.hwnd(),
            windows::Win32::UI::WindowsAndMessaging::SW_HIDE,
        );
    }
    ensure!(
        app_windows::handles()?.contains(&window.handle) && !app_windows::all()?.contains(window),
        "The launch snapshot lost a hidden existing window"
    );
    // SAFETY: Restore the same still-owned fixture without activating another app.
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
            window.hwnd(),
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE,
        );
    }
    workspaces.open(project.clone(), true)?;
    ensure!(
        workspaces.sessions[&project.id].entries[0].window.as_ref() == Some(window),
        "Relaunch did not preserve the manually assigned host window"
    );

    let mut private_project = other.clone();
    let mut private_app = app.clone();
    private_app.id = Uuid::new_v4();
    private_app.shared = false;
    private_project.apps.push(private_app.clone());
    ensure!(
        workspaces.open(private_project.clone(), true).is_err(),
        "The private slot should report its missing executable"
    );
    let blocked = workspaces.window_choices(private_project.id, private_app.id)?;
    let blocked_choice = blocked
        .options
        .iter()
        .find(|choice| choice.executable == window.executable)
        .unwrap();
    ensure!(
        !blocked_choice.available
            && workspaces
                .assign_choice(&blocked, blocked_choice.id)
                .is_err(),
        "A private slot took over another session's window"
    );
    ensure!(
        workspaces.sessions[&private_project.id].entries[0]
            .window
            .is_none(),
        "Rejected assignment changed a binding"
    );
    // A shared window follows the actual active project, not the selected settings.
    let active = workspaces.sessions[&private_project.id].desktop.unwrap();
    workspaces.assign_choice(&choices, choice.id)?;
    ensure!(
        super::super::super::window_desktop_id(window.hwnd())? == active,
        "Shared manual assignment ignored the active project"
    );
    let mut shared_project = private_project.clone();
    shared_project.apps[0].shared = true;
    ensure!(
        workspaces.open(shared_project.clone(), true).is_err(),
        "The broker slot still has a missing launch executable"
    );
    let shared = workspaces.window_choices(shared_project.id, private_app.id)?;
    let shared_choice = shared
        .options
        .iter()
        .find(|choice| choice.executable == window.executable)
        .unwrap();
    ensure!(
        shared_choice.available,
        "Two shared entries could not reuse the same window"
    );
    workspaces.assign_choice(&shared, shared_choice.id)?;
    workspaces.unassign_window(shared_project.id, private_app.id)?;
    ensure!(
        workspaces.open(private_project.clone(), true).is_err(),
        "The private broker slot still has a missing launch executable"
    );
    workspaces.unassign_window(project.id, app.id)?;
    ensure!(window.is_alive(), "Unassign closed the app");
    let available = workspaces.window_choices(private_project.id, private_app.id)?;
    let available_choice = available
        .options
        .iter()
        .find(|choice| choice.executable == window.executable)
        .unwrap();
    workspaces.assign_choice(&available, available_choice.id)?;
    let private_desktop = workspaces.sessions[&private_project.id].desktop.unwrap();
    workspaces.open(project.clone(), false)?;
    ensure!(
        super::super::super::window_desktop_id(window.hwnd())? == private_desktop,
        "A private manually assigned window followed another project"
    );
    ensure!(
        workspaces
            .assign_window(project.id, app.id, dead.clone())
            .is_err(),
        "A closed fixture was accepted"
    );
    let mut automatic = Project::from_form(None, "Jurabox sharing conflict check", "", None)?;
    automatic.apps.push(AppConfig {
        app_id: None,
        id: Uuid::new_v4(),
        executable: cleanup.probe_folder.path().join("jurabox-owned-probe.exe"),
        arguments: "_probe-window".into(),
        name: "Automatic shared attempt".into(),
        shared: true,
        new_console: false,
    });
    let result = workspaces.open(automatic.clone(), false);
    cleanup.capture(workspaces);
    ensure!(
        result.is_err()
            && workspaces.sessions[&automatic.id].entries[0]
                .window
                .is_none(),
        "Automatic shared reuse took over a private assigned window"
    );
    println!(
        "PASS manual host assignment, title changes, opaque tokens, unchanged relaunch, private/automatic sharing conflicts, shared linking, active movement, and unassign preserves the app"
    );
    Ok(())
}
