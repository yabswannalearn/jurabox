use super::*;
use crate::{
    projects::WorkloadKind,
    recovery::{Record, project_path},
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    setup: Project,
    generation: Uuid,
    desktop: Option<Uuid>,
    entries: Vec<Entry>,
}

pub(super) fn save(
    root: &Path,
    setup: &Project,
    generation: Uuid,
    desktop: Option<GUID>,
    entries: &[Entry],
) -> Result<()> {
    Record::new(project_path(root, setup.id)).save(&Saved {
        version: 1,
        setup: setup.clone(),
        generation,
        desktop: desktop.map(|id| Uuid::from_u128(id.to_u128())),
        entries: entries.to_vec(),
    })
}

impl Workspaces {
    pub fn recorded(root: PathBuf) -> Self {
        Self {
            recovery_root: Some(root),
            ..Default::default()
        }
    }

    pub fn recover(root: PathBuf, projects: &[Project]) -> Result<Self> {
        ensure!(
            !projects.iter().any(|project| project
                .workloads
                .iter()
                .any(|workload| workload.kind == WorkloadKind::DockerCompose)),
            "The lost session includes Docker containers. Docker crash recovery is not available yet; its record has been preserved and new launches are blocked."
        );
        let desktops = vd(winvd::get_desktops())?;
        let desktop_ids: Vec<_> = desktops
            .iter()
            .filter_map(|desktop| desktop.get_id().ok())
            .collect();
        let mut workspaces = Self::recorded(root.clone());
        for project in projects {
            let saved = Record::new(project_path(&root, project.id)).read::<Saved>()?
                .context("Missing workspace recovery record; the session has been preserved and replacements are blocked.")?;
            ensure!(
                saved.version == 1 && saved.setup.id == project.id && !saved.generation.is_nil(),
                "Unsupported or mismatched workspace recovery record; it has been preserved."
            );
            saved.setup.validate()?;
            ensure!(
                !saved
                    .setup
                    .workloads
                    .iter()
                    .any(|config| config.kind == WorkloadKind::DockerCompose),
                "This original session includes Docker containers; crash recovery is not available yet."
            );
            let services =
                Services::recorded(&saved.setup, &root.join(saved.generation.to_string()), true)?;
            let desktop = saved
                .desktop
                .map(|id| GUID::from_u128(id.as_u128()))
                .filter(|id| desktop_ids.contains(id));
            let mut entries = saved.entries;
            let configured = configured_apps(&saved.setup);
            let mut entry_ids = std::collections::HashSet::new();
            ensure!(
                entries.iter().all(|entry| entry_ids.insert(entry.config.id)
                    && configured.iter().any(|config| config == &entry.config)),
                "Workspace app assignments do not match the original setup; its record has been preserved."
            );
            for entry in &mut entries {
                if !entry.window.as_ref().is_some_and(AppWindow::is_alive) {
                    entry.window = None;
                }
            }
            // Services::recorded has verified quiescence, including completion
            // seals for jobs whose names disappeared. If the windows/desktop are
            // also gone, end the session without launching anything. An unsealed
            // missing job fails recovery above, including after a sudden reboot.
            if desktop.is_none()
                && !services.live()
                && entries.iter().all(|entry| entry.window.is_none())
            {
                continue;
            }
            // A crash during GUI launch can leave no assignment metadata. Show
            // the configured entries, but require explicit window assignment.
            for config in configured {
                if !entries.iter().any(|entry| entry.config.id == config.id) {
                    entries.push(Entry {
                        config,
                        window: None,
                    });
                }
            }
            workspaces.sessions.insert(
                project.id,
                Session {
                    generation: saved.generation,
                    setup: saved.setup,
                    desktop,
                    entries,
                    services,
                },
            );
        }
        Ok(workspaces)
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
                    )?;
                }
            }
        }
        Ok(())
    }
}
