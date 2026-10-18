use super::{
    app_windows::{self, AppWindow},
    apps, current_id, vd,
};
use crate::projects::{AppConfig, Project};
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::HashMap,
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use windows::core::GUID;
use winvd::Desktop;

mod check;
pub fn check() -> Result<()> {
    check::run()
}

#[derive(Clone, Default)]
pub struct Summary {
    pub open: Vec<Uuid>,
    pub active: Option<Uuid>,
    pub message: String,
    pub error: bool,
}

struct Entry {
    config: AppConfig,
    window: Option<AppWindow>,
}

struct Session {
    setup: Project,
    desktop: Option<GUID>,
    entries: Vec<Entry>,
}

#[derive(Default)]
pub struct Workspaces {
    sessions: HashMap<Uuid, Session>,
}

impl Workspaces {
    pub fn summary(&self) -> Summary {
        let current = current_id().ok();
        Summary {
            open: self.sessions.keys().copied().collect(),
            active: current.and_then(|current| {
                self.sessions
                    .iter()
                    .find_map(|(id, session)| (session.desktop == Some(current)).then_some(*id))
            }),
            ..Default::default()
        }
    }

    pub fn close(&mut self, id: Uuid) {
        self.sessions.remove(&id);
    }

    pub fn open(&mut self, project: Project, relaunch: bool) -> Result<String> {
        project.validate()?;
        let existing = self.sessions.contains_key(&project.id);
        let mut session = self
            .sessions
            .remove(&project.id)
            .unwrap_or_else(|| Session {
                setup: project.clone(),
                desktop: None,
                entries: Vec::new(),
            });
        // Always retain the session, including partial launches and integration failures.
        let result = self.activate(&project, &mut session, !existing || relaunch);
        self.sessions.insert(project.id, session);
        result
    }

    fn activate(
        &mut self,
        project: &Project,
        session: &mut Session,
        launch: bool,
    ) -> Result<String> {
        let mut problems = Vec::new();
        let desktop = match desktop_for(session.desktop, &project.name) {
            Ok(desktop) => {
                session.desktop = Some(vd(desktop.get_id())?);
                if let Err(error) = vd(winvd::switch_desktop(desktop)) {
                    problems.push(format!("Desktop switch failed: {error:#}"));
                }
                Some(desktop)
            }
            Err(error) => {
                problems.push(format!(
                    "Desktop management unavailable; new launches use the current desktop: {error:#}"
                ));
                None
            }
        };
        if launch {
            let entries = configured_apps(project);
            let mut next = Vec::new();
            for config in entries {
                let previous = session.entries.iter().find(|entry| {
                    same_launch(&entry.config, &config)
                        && session.setup.folder == project.folder
                        && entry.window.as_ref().is_some_and(AppWindow::is_alive)
                });
                let window = if let Some(previous) = previous {
                    previous.window.clone()
                } else {
                    match launch_app(&config, project) {
                        Ok(window) => {
                            if window.is_none() {
                                problems.push(format!("{}: launch requested, but no unique window could be assigned. Check the app before relaunching; it may use a broker or need a new-window argument.", config.name));
                            }
                            window
                        }
                        Err(error) => {
                            problems.push(format!("{}: {error:#}", config.name));
                            None
                        }
                    }
                };
                next.push(Entry { config, window });
            }
            session.entries = next;
            session.setup = project.clone();
        }
        if let Some(desktop) = desktop {
            for entry in &session.entries {
                if let Some(window) = &entry.window
                    && window.is_alive()
                    && let Err(error) = move_window(window, desktop, launch)
                {
                    problems.push(format!(
                        "{}: cannot move its window: {error:#}",
                        entry.config.name
                    ));
                }
            }
            self.move_shared(desktop, &mut problems);
        }
        if problems.is_empty() {
            Ok(if launch {
                format!(
                    "{} is open. GUI apps stay running when you leave.",
                    project.name
                )
            } else {
                format!(
                    "Switched to {}. Closed apps stay closed.",
                    session.setup.name
                )
            })
        } else {
            bail!("{}", problems.join("\n"))
        }
    }

    fn move_shared(&self, desktop: Desktop, problems: &mut Vec<String>) {
        for entry in self
            .sessions
            .values()
            .flat_map(|session| &session.entries)
            .filter(|entry| entry.config.shared)
        {
            if let Some(window) = &entry.window
                && window.is_alive()
                && let Err(error) = vd(winvd::move_window_to_desktop(desktop, &window.hwnd()))
            {
                problems.push(format!(
                    "{}: shared window move failed: {error:#}",
                    entry.config.name
                ));
            }
        }
    }

    pub fn desktop_changed(&self) -> Summary {
        let mut summary = self.summary();
        if summary.active.is_some()
            && let Ok(desktop) = winvd::get_current_desktop()
        {
            let mut errors = Vec::new();
            self.move_shared(desktop, &mut errors);
            if !errors.is_empty() {
                summary.message = errors.join("\n");
                summary.error = true;
            }
        }
        summary
    }
}

fn desktop_for(id: Option<GUID>, name: &str) -> Result<Desktop> {
    if let Some(id) = id {
        for desktop in vd(winvd::get_desktops())? {
            if vd(desktop.get_id())? == id {
                return Ok(desktop);
            }
        }
    }
    let desktop = vd(winvd::create_desktop())?;
    // Naming is cosmetic; a name failure must not lose the newly created GUID.
    if let Err(error) = vd(desktop.set_name(name)) {
        eprintln!("Cannot name workspace desktop: {error:#}");
    }
    Ok(desktop)
}

fn configured_apps(project: &Project) -> Vec<AppConfig> {
    let mut entries = project.apps.clone();
    if let Some(browser) = &project.browser {
        // A stable reserved ID is local to each session, never a shared service identity.
        entries.push(AppConfig {
            id: Uuid::nil(),
            name: "Project browser".into(),
            executable: browser.executable.clone(),
            arguments: browser.urls.join("\n"),
            new_console: false,
            shared: false,
        });
    }
    entries
}

fn same_launch(left: &AppConfig, right: &AppConfig) -> bool {
    left.id == right.id
        && left.executable == right.executable
        && left.arguments == right.arguments
        && left.new_console == right.new_console
        && left.shared == right.shared
}

fn launch_app(config: &AppConfig, project: &Project) -> Result<Option<AppWindow>> {
    ensure!(
        config.executable.is_file(),
        "Executable not found: {}",
        config.executable.display()
    );
    if config.shared {
        let key = app_windows::path_key(&config.executable);
        let matching: Vec<_> = app_windows::all()?
            .into_iter()
            .filter(|window| window.executable == key)
            .collect();
        ensure!(
            matching.len() <= 1,
            "Multiple matching windows are open. Close extra windows before assigning this shared app."
        );
        if let Some(window) = matching.into_iter().next() {
            return Ok(Some(window));
        }
    }
    let before = app_windows::all()?;
    let mut command = Command::new(&config.executable);
    if config.id.is_nil() {
        let browser = project.browser.as_ref().context("Missing browser setup.")?;
        command.arg("--new-window").args(&browser.urls);
    } else {
        let mut args = apps::parse_arguments(&config.arguments)?;
        for argument in &mut args {
            if argument.contains("{folder}") {
                let folder = project
                    .folder
                    .as_ref()
                    .context("This app uses {folder}; choose a project folder first.")?;
                *argument = argument.replace("{folder}", &folder.to_string_lossy());
            }
        }
        command.args(args);
    }
    if let Some(folder) = &project.folder {
        command.current_dir(folder);
    }
    if config.new_console {
        let arguments = if config.arguments.contains("{folder}") {
            config.arguments.replace(
                "{folder}",
                &project
                    .folder
                    .as_ref()
                    .context("Choose a project folder for {folder}.")?
                    .to_string_lossy(),
            )
        } else {
            config.arguments.clone()
        };
        apps::start_console(&config.executable, &arguments, project.folder.as_deref())
            .context("Windows could not start this console application.")?;
    } else {
        command.creation_flags(0x08000000);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let _child = command
            .spawn()
            .context("Windows could not start this application.")?;
    }
    let key = app_windows::path_key(&config.executable);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let windows = app_windows::all()?;
        let matching: Vec<_> = windows
            .into_iter()
            .filter(|window| window.executable == key && !before.contains(window))
            .collect();
        if matching.len() == 1 {
            let window = matching.into_iter().next().unwrap();
            return Ok(Some(window));
        }
        if matching.len() > 1 || Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn move_window(window: &AppWindow, desktop: Desktop, newly_launched: bool) -> Result<()> {
    let deadline = Instant::now()
        + if newly_launched {
            Duration::from_secs(2)
        } else {
            Duration::ZERO
        };
    loop {
        ensure!(
            window.is_alive(),
            "The application window closed before assignment."
        );
        match vd(winvd::move_window_to_desktop(desktop, &window.hwnd())) {
            Ok(()) => return Ok(()),
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
}
