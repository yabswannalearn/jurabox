use super::*;
use std::{
    collections::{HashSet, VecDeque},
    sync::mpsc,
};

struct Pending {
    project: Project,
    generation: Uuid,
    config: AppConfig,
}
struct Completed {
    pending: Pending,
    key: String,
    result: Result<Option<AppWindow>>,
}

pub(super) struct Launches {
    queued: VecDeque<Pending>,
    running: HashSet<String>,
    projects: HashMap<Uuid, usize>,
    sender: mpsc::Sender<Completed>,
    receiver: mpsc::Receiver<Completed>,
}
impl Default for Launches {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            queued: Default::default(),
            running: Default::default(),
            projects: Default::default(),
            sender,
            receiver,
        }
    }
}
impl Launches {
    pub fn busy(&self, project: Uuid) -> bool {
        self.projects.get(&project).is_some_and(|count| *count > 0)
    }
    pub fn active(&self) -> bool {
        !self.queued.is_empty() || !self.running.is_empty()
    }
    pub fn queue(&mut self, project: &Project, generation: Uuid, config: &AppConfig) {
        *self.projects.entry(project.id).or_default() += 1;
        self.queued.push_back(Pending {
            project: project.clone(),
            generation,
            config: config.clone(),
        });
    }
    fn key(pending: &Pending) -> String {
        pending
            .config
            .app_id
            .clone()
            .unwrap_or_else(|| app_windows::path_key(&pending.config.executable))
    }
    fn start(&mut self) {
        while self.running.len() < 4 {
            let Some(index) = self
                .queued
                .iter()
                .position(|pending| !self.running.contains(&Self::key(pending)))
            else {
                break;
            };
            let pending = self.queued.remove(index).unwrap();
            let key = Self::key(&pending);
            self.running.insert(key.clone());
            let sender = self.sender.clone();
            thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    launch_app(&pending.config, &pending.project)
                }))
                .unwrap_or_else(|_| {
                    Err(anyhow::anyhow!(
                        "App discovery failed. Check the app before retrying."
                    ))
                });
                let _ = sender.send(Completed {
                    pending,
                    key,
                    result,
                });
            });
        }
    }
}

impl Workspaces {
    pub(super) fn poll_launches(&mut self) {
        while let Ok(completed) = self.launches.receiver.try_recv() {
            let pending = completed.pending;
            self.launches.running.remove(&completed.key);
            if let Some(count) = self.launches.projects.get_mut(&pending.project.id) {
                *count = count.saturating_sub(1);
            }
            let Some(mut session) = self.sessions.remove(&pending.project.id) else {
                continue;
            };
            if session.generation == pending.generation {
                let result = completed.result.and_then(|window| {
                    let window = window.context(
                        "No unique window was found. Check the app, then Assign open window.",
                    )?;
                    assignment::check_automatic_sharing(
                        &window,
                        pending.config.shared,
                        self,
                        &session.entries,
                        &session.setup.name,
                    )?;
                    if let Some(desktop) = session.desktop.and_then(|id| {
                        winvd::get_desktops()
                            .ok()?
                            .into_iter()
                            .find(|desktop| desktop.get_id().ok() == Some(id))
                    }) {
                        move_window(&window, desktop, false)?;
                    }
                    if let Some(entry) = session.entries.iter_mut().find(|entry| {
                        entry.config.id == pending.config.id
                            && same_launch(&entry.config, &pending.config)
                    }) {
                        entry.window = Some(window);
                    }
                    Ok(())
                });
                if let Err(error) = result {
                    self.lifecycle_notice =
                        Some((format!("{}: {error:#}", pending.config.name), true));
                } else if self.lifecycle_notice.is_none() {
                    self.lifecycle_notice =
                        Some((format!("{} opened.", pending.config.name), false));
                }
            }
            self.sessions.insert(pending.project.id, session);
            if let Err(error) = self.checkpoint() {
                self.lifecycle_notice = Some((
                    format!("Cannot save window assignment: {error:#}. Keep Jurabox open."),
                    true,
                ));
            }
        }
        self.launches.start();
    }
}
