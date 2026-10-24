use super::*;

pub(super) enum Operation {
    Close,
    Relaunch(Project),
    Exit,
}

#[derive(Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionState {
    pub project: Uuid,
    pub setup_revision: u64,
    pub keep_running: bool,
    pub remaining_seconds: Option<u64>,
    pub busy: bool,
    pub automatically_stopped: bool,
}

pub fn setup_revision(project: &Project) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_vec(project)
        .unwrap_or_default()
        .hash(&mut hash);
    hash.finish()
}

/// Query the lock state and logon identity, rather than keyboard/mouse idle time.
pub fn interactive_session() -> Option<(bool, i64)> {
    use windows::{Win32::System::RemoteDesktop::*, core::PWSTR};
    // SAFETY: The WTS allocation is checked before reading and freed once.
    unsafe {
        let mut session = 0;
        ProcessIdToSessionId(std::process::id(), &mut session).ok()?;
        let mut buffer = PWSTR::null();
        let mut length = 0;
        WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            session,
            WTSSessionInfoEx,
            &mut buffer,
            &mut length,
        )
        .ok()?;
        let result = if !buffer.is_null() && length as usize >= std::mem::size_of::<WTSINFOEXW>() {
            let info = &*buffer.0.cast::<WTSINFOEXW>();
            (info.Level == 1).then(|| {
                let details = info.Data.WTSInfoExLevel1;
                (
                    details.SessionFlags == 1 && details.SessionState == WTSActive,
                    details.LogonTime,
                )
            })
        } else {
            None
        };
        WTSFreeMemory(buffer.0.cast());
        result
    }
}

impl Workspaces {
    pub fn session_states(&self) -> Vec<SessionState> {
        let now = Instant::now();
        let mut states: Vec<_> = self
            .sessions
            .iter()
            .map(|(id, session)| SessionState {
                project: *id,
                setup_revision: setup_revision(&session.setup),
                keep_running: session.policy.keep_running,
                remaining_seconds: session
                    .policy
                    .remaining(now, session.setup.inactivity_minutes),
                busy: self.operations.contains_key(id) || self.launches.busy(*id),
                automatically_stopped: session.automatic,
            })
            .collect();
        states.sort_by_key(|state| state.project);
        states
    }
    pub fn keep_running(&mut self, id: Uuid, keep: bool) -> Result<String> {
        let session = self
            .sessions
            .get_mut(&id)
            .context("Open the project first.")?;
        session.policy.keep(keep);
        self.checkpoint()?;
        Ok(if keep {
            "Keep running is on for this session."
        } else {
            "Keep running is off. Inactive projects get a fresh countdown."
        }
        .into())
    }
    pub fn close_async(&mut self, id: Uuid) -> Result<String> {
        let session = self
            .sessions
            .get_mut(&id)
            .context("This session is already closed.")?;
        session.services.begin_shutdown(false, true);
        self.operations.insert(id, Operation::Close);
        Ok("Closing the session. Normal shutdown is in progress…".into())
    }
    pub fn open_async(&mut self, project: Project, relaunch: bool) -> Result<String> {
        project.validate()?;
        ensure!(
            !self.operations.contains_key(&project.id),
            "Wait for this session's current operation to finish."
        );
        if relaunch && let Some(session) = self.sessions.get_mut(&project.id) {
            session.services.begin_shutdown(false, true);
            self.operations
                .insert(project.id, Operation::Relaunch(project));
            return Ok("Relaunch requested. Stopping the original session first…".into());
        }
        self.asynchronous_launch = true;
        let result = self.open(project, false);
        self.asynchronous_launch = false;
        self.poll_launches();
        result
    }
    pub fn begin_exit(&mut self) -> Result<String> {
        for (id, session) in &mut self.sessions {
            session.services.begin_shutdown(false, true);
            self.operations.insert(*id, Operation::Exit);
        }
        Ok("Stopping managed workloads before quitting…".into())
    }
    pub fn exit_result(&self) -> Option<Result<()>> {
        if !self.operations.is_empty() || self.recoveries.active() {
            return None;
        }
        Some(if self.sessions.is_empty() && self.blocked.is_empty() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Some workloads could not stop. Resolve the failure or explicitly Force stop, then try again."
            ))
        })
    }
    pub fn take_lifecycle_notice(&mut self) -> Option<(String, bool)> {
        self.lifecycle_notice.take()
    }
    pub(super) fn poll_lifecycle(&mut self) {
        self.poll_recovery();
        self.poll_launches();
        let completed: Vec<_> = self
            .operations
            .keys()
            .filter_map(|id| {
                self.sessions
                    .get(id)
                    .and_then(|session| session.services.shutdown_result())
                    .map(|result| (*id, result))
            })
            .collect();
        for (id, result) in completed {
            let operation = self.operations.remove(&id).unwrap();
            match result {
                Err(error) => {
                    self.sessions
                        .get_mut(&id)
                        .unwrap()
                        .services
                        .clear_shutdown();
                    self.lifecycle_notice =
                        Some((format!("Session remains open: {error:#}"), true));
                }
                Ok(()) => {
                    if !matches!(operation, Operation::Relaunch(_))
                        && let Err(error) = self.retire(id)
                    {
                        self.lifecycle_notice =
                            Some((format!("Cannot retire the session record: {error:#}"), true));
                        self.sessions
                            .get_mut(&id)
                            .unwrap()
                            .services
                            .clear_shutdown();
                        continue;
                    }
                    match operation {
                        Operation::Close | Operation::Exit => {
                            self.sessions.remove(&id);
                        }
                        Operation::Relaunch(project) => {
                            // Shutdown is complete, so activate can apply the new setup.
                            if let Err(error) = self.retire(id) {
                                self.lifecycle_notice = Some((
                                    format!("Cannot retire the original session: {error:#}"),
                                    true,
                                ));
                                self.sessions
                                    .get_mut(&id)
                                    .unwrap()
                                    .services
                                    .clear_shutdown();
                                continue;
                            }
                            let mut session = self.sessions.remove(&id).unwrap();
                            session.services.clear_shutdown();
                            self.asynchronous_launch = true;
                            let result = self.activate(&project, &mut session, true);
                            self.asynchronous_launch = false;
                            self.sessions.insert(id, session);
                            if let Err(error) = result {
                                self.lifecycle_notice = Some((format!("{error:#}"), true));
                            }
                        }
                    }
                    if self.lifecycle_notice.is_none() {
                        self.lifecycle_notice =
                            Some(("Session operation completed.".into(), false));
                    }
                }
            }
        }
        let now = Instant::now();
        let current = current_id().ok();
        let unlocked = interactive_session().is_some_and(|(unlocked, _)| unlocked);
        let desktops = winvd::get_desktops().ok().map(|desktops| {
            desktops
                .iter()
                .filter_map(|desktop| desktop.get_id().ok())
                .collect::<Vec<_>>()
        });
        for (id, session) in &mut self.sessions {
            if self.operations.contains_key(id) {
                session.policy.reset();
                continue;
            }
            let active = current.is_some() && session.desktop == current;
            let available = unlocked
                && current.is_some()
                && session.desktop.is_some()
                && desktops.as_ref().is_some_and(|desktops| {
                    session
                        .desktop
                        .is_some_and(|desktop| desktops.contains(&desktop))
                });
            if session
                .policy
                .observe(now, active, available, session.setup.inactivity_minutes)
                && session.services.eligible_automatic()
            {
                session.automatic = true;
                session.services.begin_shutdown(true, false);
            }
            if session.automatic
                && let Some(result) = session.services.shutdown_result()
            {
                if let Err(error) = result {
                    self.lifecycle_notice =
                        Some((format!("Auto-stop needs attention: {error:#}"), true));
                }
                if active {
                    session.services.resume_automatic();
                    session.automatic = session.services.automatically_stopped();
                } else {
                    session.services.clear_shutdown();
                }
            } else if session.automatic && active && !session.services.shutting_down() {
                session.services.resume_automatic();
                session.automatic = session.services.automatically_stopped();
            }
        }
    }
}
