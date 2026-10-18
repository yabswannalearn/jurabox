use super::*;

/// Optional real console test. The unique title is generated here, and the /c
/// command exits on its own after 20 seconds even if the exercise is interrupted.
pub(super) fn run(workspaces: &mut Workspaces, cleanup: &mut Cleanup) -> Result<Option<AppWindow>> {
    if std::env::var("JURABOX_CHECK_CONSOLE").as_deref() != Ok("1") {
        return Ok(None);
    }
    let title = format!("JuraboxConsoleFixture-{}", Uuid::new_v4());
    let mut project = Project::from_form(None, &title, "", None)?;
    project.folder = Some(cleanup.probe_folder.path().to_path_buf());
    let executable = std::path::PathBuf::from(
        std::env::var_os("SystemRoot").context("Windows system folder unavailable")?,
    )
    .join("System32/cmd.exe");
    let app = AppConfig {
        app_id: None,
        id: Uuid::new_v4(),
        name: "CMD fixture".into(),
        executable,
        arguments: format!(
            r#"/d /s /c "title {title} & echo started>console-started.txt & "%SystemRoot%\System32\timeout.exe" /t 20 /nobreak >console-wait.txt 2>&1""#
        ),
        new_console: true,
        shared: false,
    };
    project.apps.push(app.clone());
    let before = app_windows::handles()?;
    let launch = workspaces.open(project.clone(), false);
    cleanup.capture(workspaces);
    let mut choice = None;
    let mut snapshot = None;
    super::super::super::wait_until("uniquely titled CMD fixture", || {
        let fresh: Vec<_> = app_windows::all()?
            .into_iter()
            .filter(|window| !before.contains(&window.handle) && window.title().contains(&title))
            .collect();
        ensure!(
            fresh.len() <= 1,
            "Multiple console fixture windows appeared; none will be selected automatically."
        );
        let current = workspaces.window_choices(project.id, app.id)?;
        choice = fresh
            .first()
            .and_then(|window| {
                current.options.iter().find(|option| {
                    option.title.contains(&title) && option.executable == window.executable
                })
            })
            .map(|option| option.id);
        if let Some(window) = fresh.first() {
            cleanup.windows.push(window.clone());
        }
        snapshot = Some(current);
        Ok(choice.is_some())
    })
    .with_context(|| {
        let started =
            std::fs::read_to_string(cleanup.probe_folder.path().join("console-started.txt"))
                .unwrap_or_default();
        let output = std::fs::read(cleanup.probe_folder.path().join("console-wait.txt"))
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        format!("Console startup: {launch:?}; marker={started:?}; wait output={output:?}")
    })?;
    let result = workspaces.assign_choice(snapshot.as_ref().unwrap(), choice.unwrap());
    cleanup.capture(workspaces);
    result?;
    let window = workspaces.sessions[&project.id].entries[0]
        .window
        .clone()
        .context("Console window was not assigned")?;
    ensure!(
        super::super::super::window_desktop_id(window.hwnd())?
            == workspaces.sessions[&project.id].desktop.unwrap(),
        "The real console window was not moved to its project"
    );
    workspaces.open(project.clone(), true)?;
    ensure!(
        workspaces.sessions[&project.id].entries[0].window.as_ref() == Some(&window),
        "Relaunch duplicated the manually assigned terminal host"
    );
    workspaces.unassign_window(project.id, app.id)?;
    ensure!(window.is_alive(), "Unassign closed the real console");
    println!(
        "PASS real CMD console creation, terminal-host manual assignment, desktop movement, unchanged relaunch, and unassign keeps the console open"
    );
    Ok(Some(window))
}
