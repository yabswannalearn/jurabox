use super::*;

#[derive(Clone)]
pub struct SessionApp {
    pub project: Uuid,
    pub app: Uuid,
    pub name: String,
    pub title: Option<String>,
    pub executable: Option<String>,
    pub shared: bool,
}

#[derive(Clone)]
pub struct WindowOption {
    pub id: Uuid,
    pub title: String,
    pub executable: String,
    pub assignment: String,
    pub available: bool,
}

/// Short-lived chooser snapshot. The UI receives opaque option IDs, never handles.
pub struct WindowChoices {
    pub options: Vec<WindowOption>,
    project: Uuid,
    app: Uuid,
    windows: HashMap<Uuid, AppWindow>,
}

impl Workspaces {
    fn entry(&self, project: Uuid, app: Uuid) -> Result<&Entry> {
        self.sessions
            .get(&project)
            .context("Open this project before assigning a window.")?
            .entries
            .iter()
            .find(|entry| entry.config.id == app)
            .context("This app is not in the current session. Relaunch the saved setup first.")
    }

    pub fn window_choices(&self, project: Uuid, app: Uuid) -> Result<WindowChoices> {
        let shared = self.entry(project, app)?.config.shared;
        let mut result = WindowChoices {
            options: Vec::new(),
            project,
            app,
            windows: HashMap::new(),
        };
        for window in app_windows::all()?
            .into_iter()
            .filter(|window| window.process != std::process::id())
        {
            let id = Uuid::new_v4();
            let owners: Vec<_> = self
                .sessions
                .iter()
                .flat_map(|(project_id, session)| {
                    session
                        .entries
                        .iter()
                        .filter(|entry| entry.window.as_ref() == Some(&window))
                        .map(|entry| {
                            (
                                format!("{} / {}", session.setup.name, entry.config.name),
                                *project_id,
                                entry.config.id,
                                entry.config.shared,
                            )
                        })
                })
                .collect();
            let available = owners
                .iter()
                .all(|(_, owner_project, owner_app, owner_shared)| {
                    (*owner_project == project && *owner_app == app) || (shared && *owner_shared)
                });
            result.options.push(WindowOption {
                id,
                title: window.title(),
                executable: window.executable.clone(),
                assignment: owners
                    .iter()
                    .map(|(name, ..)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                available,
            });
            result.windows.insert(id, window);
        }
        result
            .options
            .sort_by_key(|option| option.title.to_lowercase());
        Ok(result)
    }

    pub fn assign_choice(&mut self, choices: &WindowChoices, choice: Uuid) -> Result<String> {
        let window = choices
            .windows
            .get(&choice)
            .context("That window list is out of date. Refresh and choose again.")?;
        self.assign_window(choices.project, choices.app, window.clone())
    }

    pub(super) fn assign_window(
        &mut self,
        project: Uuid,
        app: Uuid,
        window: AppWindow,
    ) -> Result<String> {
        ensure!(
            window.process != std::process::id(),
            "Choose another application's window."
        );
        ensure!(
            window.is_alive(),
            "That window has closed or changed identity. Refresh and choose again."
        );
        let entry = self.entry(project, app)?;
        let name = entry.config.name.clone();
        let shared = entry.config.shared;
        let others = self.sessions.iter().flat_map(|(project_id, session)| {
            session
                .entries
                .iter()
                .filter(move |entry| *project_id != project || entry.config.id != app)
                .map(|entry| (session.setup.name.as_str(), entry))
        });
        check_sharing(&window, shared, others)?;

        // Shared windows follow the actual active project, independently of which
        // project's settings the user currently selected in the management window.
        let target_project = if shared {
            self.summary().active.unwrap_or(project)
        } else {
            project
        };
        let target = self.sessions.get(&target_project).unwrap().desktop;
        let desktop = if let Some(id) = target {
            Some(vd(winvd::get_desktops())?.into_iter().find(|desktop| desktop.get_id().ok() == Some(id))
                .context("The workspace desktop was removed. Open the project again before assigning a window.")?)
        } else {
            None
        };
        self.sessions
            .get_mut(&project)
            .unwrap()
            .entries
            .iter_mut()
            .find(|entry| entry.config.id == app)
            .unwrap()
            .window = Some(window.clone());
        if let Some(desktop) = desktop {
            move_window(&window, desktop, false).context(
                "Window assigned, but desktop movement failed. Try Switch to project again.",
            )?;
        }
        Ok(format!(
            "Window assigned to {name}. Its launch settings are unchanged."
        ))
    }

    pub fn unassign_window(&mut self, project: Uuid, app: Uuid) -> Result<String> {
        self.entry(project, app)?;
        self.sessions
            .get_mut(&project)
            .unwrap()
            .entries
            .iter_mut()
            .find(|entry| entry.config.id == app)
            .unwrap()
            .window = None;
        Ok("Window unassigned. The app stays open.".into())
    }
}

fn check_sharing<'a>(
    window: &AppWindow,
    shared: bool,
    entries: impl Iterator<Item = (&'a str, &'a Entry)>,
) -> Result<()> {
    for (project, entry) in entries {
        if entry.window.as_ref() == Some(window) {
            ensure!(
                shared && entry.config.shared,
                "This window is assigned to {project} / {}. Choose a different window, or enable Shared app in both saved setups and relaunch.",
                entry.config.name
            );
        }
    }
    Ok(())
}

pub(super) fn check_automatic_sharing<'a>(
    window: &AppWindow,
    shared: bool,
    workspaces: &'a Workspaces,
    next: &'a [Entry],
    name: &'a str,
) -> Result<()> {
    check_sharing(
        window,
        shared,
        workspaces
            .sessions
            .values()
            .flat_map(|session| {
                session
                    .entries
                    .iter()
                    .map(|entry| (session.setup.name.as_str(), entry))
            })
            .chain(next.iter().map(|entry| (name, entry))),
    )
}
