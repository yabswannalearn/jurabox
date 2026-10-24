use super::*;
use crate::recovery::Record;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    workload: Uuid,
    status: Status,
    job: Option<Uuid>,
    stop_job: Option<Uuid>,
    #[serde(default)]
    auto_stopped: bool,
}

impl Entry {
    pub(super) fn persist(&mut self) -> Result<()> {
        if let Some(record) = &mut self.record {
            record.save(&Saved {
                version: 1,
                workload: self.config.id,
                status: self.status,
                job: self.job,
                stop_job: self.stop_job,
                auto_stopped: self.auto_stopped,
            })?;
        }
        Ok(())
    }

    fn restore(&mut self, saved: Saved) -> Result<()> {
        ensure!(
            saved.version == 1 && saved.workload == self.config.id,
            "Unsupported or mismatched service recovery record; it has been preserved."
        );
        ensure!(
            saved.job.is_none_or(|id| !id.is_nil())
                && saved.stop_job.is_none_or(|id| !id.is_nil())
                && (saved.job.is_none() || saved.job != saved.stop_job),
            "Invalid command ownership capabilities."
        );
        self.job = saved.job;
        self.stop_job = saved.stop_job;
        self.auto_stopped = saved.auto_stopped && self.config.kind != WorkloadKind::StartupAction;
        let record = self
            .record
            .as_ref()
            .context("Missing service recovery location")?;
        self.process = saved
            .job
            .map(|claim| ManagedProcess::recover(claim, &record.claim_path(claim)))
            .transpose()?
            .flatten();
        self.stop_process = saved
            .stop_job
            .map(|claim| ManagedProcess::recover(claim, &record.claim_path(claim)))
            .transpose()?
            .flatten();
        self.recovered = self.config.kind != WorkloadKind::DockerCompose;
        self.ready = false;
        self.status = if active(&self.stop_process) {
            Status::StopFailed
        } else if self.workload_live() {
            if matches!(
                saved.status,
                Status::Stopping | Status::StopFailed | Status::Stopped
            ) {
                Status::StopFailed
            } else if self.config.kind != WorkloadKind::StartupAction
                && self.config.readiness == Readiness::ProcessRunning
                && self
                    .compose
                    .as_ref()
                    .is_none_or(compose::Runtime::available)
            {
                self.ready = true;
                Status::Running
            } else {
                Status::NotReady
            }
        } else {
            match saved.status {
                Status::Completed if self.config.kind == WorkloadKind::StartupAction => {
                    Status::Completed
                }
                Status::Stopped | Status::Pending | Status::Waiting | Status::Checking => {
                    Status::Stopped
                }
                _ => Status::Failed,
            }
        };
        self.detail = if active(&self.stop_process) {
            "Recovered the original stop command. Wait for it to exit, then Retry Stop, or explicitly Force stop."
        } else if self.workload_live() {
            if self.status == Status::StopFailed {
                "The original command is still running after an interrupted stop. Retry Stop or explicitly Force stop."
            } else if self.status == Status::NotReady {
                "Recovered the original command job. Retry checks this same job; no replacement was started."
            } else {
                "Recovered the original command job, including surviving children. No replacement was started."
            }
        } else if self.status == Status::Completed {
            "This startup action had already completed successfully. It was not run again."
        } else if self.status == Status::Stopped {
            "Left stopped after recovery. Use Start when ready."
        } else {
            "The original command job is gone. Its final exit code is unknown. Nothing was restarted; use Retry when ready."
        }.into();
        Ok(())
    }
}

impl Services {
    /// Each session gets a fresh directory. All initial records are flushed before
    /// the workspace manifest commits it or any startup scheduling is permitted.
    pub fn recorded(project: &Project, folder: &Path, restore: bool) -> Result<Self> {
        Self::recorded_with_logon(project, folder, restore, false)
    }
    pub fn recorded_with_logon(
        project: &Project,
        folder: &Path,
        restore: bool,
        new_logon: bool,
    ) -> Result<Self> {
        let mut services = Self::new(project)?;
        for entry in &mut services.entries {
            if let Some(compose) = &mut entry.compose {
                compose.record(
                    folder.join(format!("{}.compose.json", entry.config.id)),
                    restore,
                )?;
            }
            let mut record = Record::new(folder.join(format!("{}.json", entry.config.id)));
            if restore {
                let mut saved = record
                    .read::<Saved>()?
                    .context("Missing service recovery record; replacement is blocked.")?;
                if new_logon && entry.config.kind != WorkloadKind::DockerCompose {
                    // A different verified logon cannot retain the original user
                    // session processes. Docker has separate engine ownership.
                    saved.job = None;
                    saved.stop_job = None;
                    saved.auto_stopped = false;
                    if saved.status != Status::Completed {
                        saved.status = Status::Stopped;
                    }
                }
                entry.record = Some(record);
                entry.restore(saved)?;
            } else {
                entry.record = Some(record);
            }
            if !restore {
                entry.persist()?;
            }
        }
        Ok(services)
    }

    pub fn checkpoint(&mut self) -> Result<()> {
        for entry in &mut self.entries {
            entry.persist()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verified_new_logon_leaves_missing_commands_stopped_without_replacement() {
        let folder = tempfile::tempdir().unwrap();
        let config = fixture_config(
            &folder.path().join("service"),
            "long",
            WorkloadKind::CommandService,
        )
        .unwrap();
        let mut project = Project::from_form(None, "Previous logon", "", None).unwrap();
        let id = config.id;
        project.workloads.push(config);
        let records = folder.path().join("records");
        Services::recorded(&project, &records, false).unwrap();
        let path = records.join(format!("{id}.json"));
        let mut record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        record["job"] = Uuid::new_v4().to_string().into();
        record["status"] = "Running".into();
        std::fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(Services::recorded(&project, &records, true).is_err());
        let recovered = Services::recorded_with_logon(&project, &records, true, true).unwrap();
        assert!(!recovered.live());
        assert_eq!(recovered.views()[0].status, "Stopped");
        assert!(recovered.entries[0].process.is_none());
    }

    #[test]
    fn ownership_write_failure_prevents_command_creation() {
        let folder = tempfile::tempdir().unwrap();
        let config = fixture_config(
            &folder.path().join("service"),
            "long",
            WorkloadKind::CommandService,
        )
        .unwrap();
        let mut project = Project::from_form(None, "Write failure", "", None).unwrap();
        project.workloads.push(config);
        let mut services = Services::new(&project).unwrap();
        let file = folder.path().join("not-a-directory");
        std::fs::write(&file, "original").unwrap();
        let entry = &mut services.entries[0];
        entry.record = Some(Record::new(file.join("service.json")));
        assert!(entry.start().is_err());
        assert!(entry.process.is_none());
        assert_eq!(entry.status, Status::Failed);
        assert_eq!(std::fs::read_to_string(file).unwrap(), "original");
    }

    #[test]
    fn missing_or_future_service_record_blocks_recovery() {
        let folder = tempfile::tempdir().unwrap();
        let config = fixture_config(
            &folder.path().join("service"),
            "long",
            WorkloadKind::CommandService,
        )
        .unwrap();
        let mut project = Project::from_form(None, "Missing record", "", None).unwrap();
        let id = config.id;
        project.workloads.push(config);
        let records = folder.path().join("records");
        Services::recorded(&project, &records, false).unwrap();
        let path = records.join(format!("{id}.json"));
        let mut saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        saved["version"] = 999.into();
        let bytes = serde_json::to_vec(&saved).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(Services::recorded(&project, &records, true).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_file(&path).unwrap();
        assert!(Services::recorded(&project, &records, true).is_err());
    }
}
