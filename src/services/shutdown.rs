use super::*;
use std::collections::HashSet;

pub(super) struct Shutdown {
    automatic: bool,
    pending: HashSet<Uuid>,
    release: bool,
    cleaning: bool,
    complete: bool,
    error: Option<String>,
}

impl Services {
    pub fn automatically_stopped(&self) -> bool {
        self.entries.iter().any(|entry| entry.auto_stopped)
    }
    pub fn retained(&self) -> bool {
        self.entries.iter().any(|entry| {
            entry
                .compose
                .as_ref()
                .is_some_and(compose::Runtime::retained)
        })
    }
    /// Advance all independent shutdown branches without blocking the host.
    pub fn begin_shutdown(&mut self, automatic: bool, release: bool) {
        if self
            .shutdown
            .as_ref()
            .is_some_and(|operation| !operation.complete)
        {
            return;
        }
        let mut pending = HashSet::new();
        for entry in &mut self.entries {
            if automatic && entry.config.kind == WorkloadKind::StartupAction {
                continue;
            }
            let eligible = entry.live()
                || matches!(
                    entry.status,
                    Status::Waiting | Status::Checking | Status::Starting
                );
            if automatic && eligible && entry.config.kind != WorkloadKind::StartupAction {
                entry.auto_stopped = true;
            }
            if !automatic {
                entry.auto_stopped = false;
            }
            if !automatic
                || matches!(
                    entry.status,
                    Status::Waiting | Status::Checking | Status::Starting
                )
            {
                entry.cancel_startup();
            }
            if eligible {
                pending.insert(entry.config.id);
            }
        }
        self.shutdown = Some(Shutdown {
            automatic,
            pending,
            release,
            cleaning: false,
            complete: false,
            error: None,
        });
        self.advance_shutdown();
    }

    pub fn shutdown_result(&self) -> Option<Result<()>> {
        let operation = self.shutdown.as_ref()?;
        operation.complete.then(|| match &operation.error {
            Some(error) => Err(anyhow::anyhow!(error.clone())),
            None => Ok(()),
        })
    }

    pub fn clear_shutdown(&mut self) {
        self.shutdown = None;
    }
    pub fn shutting_down(&self) -> bool {
        self.shutdown
            .as_ref()
            .is_some_and(|operation| !operation.complete)
    }

    pub fn eligible_automatic(&self) -> bool {
        self.entries.iter().any(|entry| {
            entry.config.kind != WorkloadKind::StartupAction
                && (entry.live()
                    || matches!(
                        entry.status,
                        Status::Waiting | Status::Checking | Status::Starting
                    ))
        })
    }
    pub fn resume_automatic(&mut self) {
        if self.shutting_down() {
            return;
        }
        self.shutdown = None;
        for entry in &mut self.entries {
            if !entry.auto_stopped || active(&entry.stop_process) || entry.deadline.is_some() {
                continue;
            }
            if !entry.live() && entry.status == Status::Stopped {
                entry.auto_stopped = false;
                entry.queue_start();
            } else if entry.workload_live() {
                // Recheck the same owned instance after an interrupted/failed
                // normal stop. Never create a duplicate of a surviving workload.
                entry.auto_stopped = false;
                entry.status = Status::NotReady;
                entry.queue_start();
            }
        }
        self.schedule();
    }

    pub fn request_action(&mut self, id: Uuid, action: Action) -> Result<String> {
        ensure!(
            !matches!(action, Action::Start) || !self.shutting_down(),
            "Wait for normal shutdown to finish before starting this service."
        );
        if matches!(action, Action::ForceStop) {
            let entry = self
                .entries
                .iter_mut()
                .find(|entry| entry.config.id == id)
                .context("Workload is not in this session")?;
            entry.auto_stopped = false;
            entry.cancel_startup();
            if let Some(compose) = &mut entry.compose {
                compose.begin_force()?;
            }
            for process in [&entry.process, &entry.stop_process].into_iter().flatten() {
                if process.active()? {
                    process.force_stop()?;
                }
            }
            entry.status = Status::Stopping;
            entry.deadline = Some(Instant::now() + Duration::from_secs(5));
            entry.detail =
                "Explicit Force stop requested for this workload and its owned children.".into();
            return Ok(entry.detail.clone());
        }
        if !matches!(action, Action::Stop) {
            return self.act(id, action);
        }
        let dependents = self.live_dependents(id);
        ensure!(
            dependents.is_empty(),
            "Stop dependent workloads first: {}",
            dependents.join(", ")
        );
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.config.id == id)
            .context("Workload is not in this session")?;
        entry.auto_stopped = false;
        entry.request_stop()?;
        Ok(format!(
            "Normal shutdown requested for {}.",
            entry.config.name
        ))
    }

    pub(super) fn advance_shutdown(&mut self) {
        let Some(mut operation) = self.shutdown.take() else {
            return;
        };
        if operation.complete {
            self.shutdown = Some(operation);
            return;
        }
        if !operation.cleaning {
            for index in self.shutdown_order() {
                let id = self.entries[index].config.id;
                if operation.pending.contains(&id) && self.live_dependents(id).is_empty() {
                    operation.pending.remove(&id);
                    let _ = self.entries[index].request_stop();
                }
            }
            let waiting = self.entries.iter().any(|entry| {
                entry.deadline.is_some()
                    || entry.compose.as_ref().is_some_and(compose::Runtime::busy)
            }) || (operation.automatic
                && !operation.pending.is_empty()
                && self
                    .entries
                    .iter()
                    .any(|entry| entry.config.kind == WorkloadKind::StartupAction && entry.live()));
            if !waiting {
                let failures: Vec<_> = self
                    .entries
                    .iter()
                    .filter(|entry| {
                        entry.live()
                            && (!operation.automatic
                                || entry.config.kind != WorkloadKind::StartupAction)
                    })
                    .map(|entry| format!("{}: {}", entry.config.name, entry.detail))
                    .collect();
                if !failures.is_empty() {
                    operation.error = Some(failures.join("\n"));
                    operation.complete = true;
                } else if operation.release {
                    operation.cleaning = true;
                    for entry in &mut self.entries {
                        if let Some(compose) = &mut entry.compose
                            && let Err(error) = compose.begin_release()
                        {
                            operation.error = Some(format!("{error:#}"));
                        }
                    }
                } else {
                    operation.complete = true;
                }
            }
        }
        if operation.cleaning
            && !self
                .entries
                .iter()
                .any(|entry| entry.compose.as_ref().is_some_and(compose::Runtime::busy))
        {
            let failures: Vec<_> = self
                .entries
                .iter()
                .filter_map(|entry| entry.compose.as_ref().and_then(compose::Runtime::error))
                .collect();
            if !failures.is_empty() {
                operation.error = Some(failures.join("\n"));
            }
            operation.complete = true;
        }
        self.shutdown = Some(operation);
    }
}
