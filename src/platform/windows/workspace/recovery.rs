use super::*;
use crate::recovery::{Record, project_path};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use std::{
    collections::{HashSet, VecDeque},
    sync::mpsc,
};
type Recovered = (Project, Result<Option<Session>>);
pub(super) struct Pending {
    queued: VecDeque<Project>,
    active: HashSet<Uuid>,
    sender: mpsc::Sender<Recovered>,
    receiver: mpsc::Receiver<Recovered>,
}
impl Default for Pending {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            queued: Default::default(),
            active: Default::default(),
            sender,
            receiver,
        }
    }
}
impl Pending {
    pub fn active(&self) -> bool {
        !self.queued.is_empty() || !self.active.is_empty()
    }
    pub fn busy(&self, id: Uuid) -> bool {
        self.active.contains(&id) || self.queued.iter().any(|project| project.id == id)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    setup: Project,
    generation: Uuid,
    desktop: Option<Uuid>,
    entries: Vec<Entry>,
    #[serde(default)]
    keep_running: bool,
    #[serde(default)]
    logon_time: Option<i64>,
    #[serde(default)]
    closed: bool,
}

pub(super) fn save(
    root: &Path,
    setup: &Project,
    generation: Uuid,
    desktop: Option<GUID>,
    entries: &[Entry],
    keep_running: bool,
) -> Result<()> {
    Record::new(project_path(root, setup.id)).save(&Saved {
        version: 1,
        setup: setup.clone(),
        generation,
        desktop: desktop.map(|id| Uuid::from_u128(id.to_u128())),
        entries: entries.to_vec(),
        keep_running,
        logon_time: super::interactive_session().map(|(_, logon)| logon),
        closed: false,
    })
}

pub(super) fn mark_closed(root: &Path, id: Uuid, generation: Uuid) -> Result<()> {
    let mut record = Record::new(project_path(root, id));
    let mut saved: Saved = record
        .read()?
        .context("Missing workspace record; close has been deferred.")?;
    ensure!(
        saved.version == 1 && saved.setup.id == id && saved.generation == generation,
        "Workspace record changed; its evidence has been preserved."
    );
    saved.closed = true;
    record.save(&saved)
}

impl Workspaces {
    pub fn recover_async(root: PathBuf, projects: &[Project]) -> Result<Self> {
        let mut workspaces = Self::recorded(root);
        for project in projects {
            workspaces.blocked.insert(
                project.id,
                (
                    project.clone(),
                    "Verifying this session's original ownership records...".into(),
                ),
            );
            workspaces.recoveries.queued.push_back(project.clone());
        }
        workspaces.poll_recovery();
        Ok(workspaces)
    }
    pub(super) fn poll_recovery(&mut self) {
        while let Ok((project, result)) = self.recoveries.receiver.try_recv() {
            self.recoveries.active.remove(&project.id);
            match result {
                Ok(session) => {
                    self.blocked.remove(&project.id);
                    if let Some(session) = session {
                        self.sessions.insert(project.id, session);
                    }
                    self.lifecycle_notice = Some((
                        format!(
                            "Recovered {}. No replacement workloads were launched.",
                            project.name
                        ),
                        false,
                    ));
                }
                Err(error) => {
                    self.blocked
                        .insert(project.id, (project.clone(), format!("{error:#}")));
                    self.lifecycle_notice = Some((
                        format!(
                            "{} needs recovery: {error:#}. Its records were preserved.",
                            project.name
                        ),
                        true,
                    ));
                }
            }
        }
        while self.recoveries.active.len() < 4 {
            let Some(project) = self.recoveries.queued.pop_front() else {
                break;
            };
            self.recoveries.active.insert(project.id);
            let sender = self.recoveries.sender.clone();
            let root = self.recovery_root.as_ref().unwrap().clone();
            std::thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    Self::recover_session(&root, &project)
                }))
                .unwrap_or_else(|_| {
                    Err(anyhow::anyhow!(
                        "Recovery worker failed. Original records were preserved."
                    ))
                });
                let _ = sender.send((project, result));
            });
        }
    }

    pub fn recorded(root: PathBuf) -> Self {
        if let Err(error) = super::cleanup::prune(&root) {
            eprintln!("Runtime retention cleanup deferred: {error:#}");
        }
        Self {
            recovery_root: Some(root),
            ..Default::default()
        }
    }

    pub fn recover(root: PathBuf, projects: &[Project]) -> Result<Self> {
        let mut workspaces = Self::recorded(root.clone());
        for project in projects {
            match Self::recover_session(&root, project) {
                Ok(Some(session)) => {
                    workspaces.sessions.insert(project.id, session);
                }
                Ok(None) => {}
                Err(error) => {
                    workspaces
                        .blocked
                        .insert(project.id, (project.clone(), format!("{error:#}")));
                }
            }
        }
        Ok(workspaces)
    }

    fn recover_session(root: &Path, project: &Project) -> Result<Option<Session>> {
        let saved = Record::new(project_path(root, project.id)).read::<Saved>()?
            .context("Missing workspace recovery record. Original work remains unmanaged until resolved.")?;
        ensure!(
            saved.version == 1 && saved.setup.id == project.id && !saved.generation.is_nil(),
            "Unsupported or mismatched workspace record; it has been preserved."
        );
        saved.setup.validate()?;
        if saved.closed {
            return Ok(None);
        }
        let new_logon = saved
            .logon_time
            .zip(super::interactive_session().map(|(_, logon)| logon))
            .is_some_and(|(old, current)| old > 0 && current > 0 && old != current);
        let services = Services::recorded_with_logon(
            &saved.setup,
            &root.join(saved.generation.to_string()),
            true,
            new_logon,
        )?;
        let desktops = vd(winvd::get_desktops())?;
        let desktop = saved
            .desktop
            .map(|id| GUID::from_u128(id.as_u128()))
            .filter(|id| {
                desktops
                    .iter()
                    .any(|desktop| desktop.get_id().ok() == Some(*id))
            });
        let configured = configured_apps(&saved.setup);
        let mut entries = saved.entries;
        let mut ids = std::collections::HashSet::new();
        ensure!(
            entries
                .iter()
                .all(|entry| ids.insert(entry.config.id) && configured.contains(&entry.config)),
            "Window assignments do not match the original session setup."
        );
        for entry in &mut entries {
            if !entry.window.as_ref().is_some_and(AppWindow::is_alive) {
                entry.window = None;
            }
        }
        if desktop.is_none()
            && !services.live()
            && !services.retained()
            && entries.iter().all(|entry| entry.window.is_none())
        {
            mark_closed(root, project.id, saved.generation)?;
            super::cleanup::mark(root, saved.generation)?;
            return Ok(None);
        }
        for config in configured {
            if !entries.iter().any(|entry| entry.config.id == config.id) {
                entries.push(Entry {
                    config,
                    window: None,
                });
            }
        }
        let mut policy = super::inactivity::Policy::default();
        policy.keep(saved.keep_running);
        let automatic = services.automatically_stopped();
        Ok(Some(Session {
            generation: saved.generation,
            setup: saved.setup,
            desktop,
            entries,
            services,
            policy,
            automatic,
        }))
    }

    pub fn retry_recovery(&mut self, id: Uuid) -> Result<String> {
        ensure!(!self.recoveries.busy(id), "Recovery is still in progress.");
        let project = self
            .blocked
            .get(&id)
            .context("This project does not need recovery.")?
            .0
            .clone();
        self.recoveries.queued.push_back(project);
        self.poll_recovery();
        Ok("Verifying the original session...".into())
    }

    /// Explicitly abandon lifecycle control, after the UI explains that any
    /// uncertain work stays running and a later Open can create replacements.
    pub fn leave_unmanaged(&mut self, id: Uuid) -> Result<String> {
        ensure!(
            !self.recoveries.busy(id),
            "Wait for verification before resolving this session."
        );
        ensure!(
            self.blocked.contains_key(&id),
            "This project does not need resolution."
        );
        let root = self.recovery_root.as_ref().context("No recovery folder")?;
        let archive = root.join("resolved").join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&archive)?;
        let original = project_path(root, id);
        if original.is_file() {
            std::fs::copy(&original, archive.join(format!("{id}.json")))?;
        }
        self.blocked.remove(&id);
        Ok("Original evidence archived. Existing work is left unmanaged. Inspect it before opening a new session.".into())
    }

    pub fn checkpoint(&mut self) -> Result<()> {
        if let Some(root) = &self.recovery_root {
            for session in self.sessions.values_mut() {
                session.services.checkpoint()?;
                if !session.generation.is_nil() {
                    save(
                        root,
                        &session.setup,
                        session.generation,
                        session.desktop,
                        &session.entries,
                        session.policy.keep_running,
                    )?;
                }
            }
        }
        Ok(())
    }
}
