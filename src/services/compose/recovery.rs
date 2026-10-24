use super::{
    controller::Controller,
    model::{Container, Plan},
};
use crate::recovery::Record;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    project: String,
    workload: uuid::Uuid,
    owner: String,
    host: String,
    engine: Option<String>,
    plan: Option<Plan>,
    containers: BTreeMap<String, Container>,
    attempted: bool,
    started: bool,
}

impl Controller {
    pub(super) fn attach_record(&mut self, path: PathBuf, restore: bool) -> Result<()> {
        let mut record = Record::new(path);
        if restore {
            let saved: Saved = record
                .read()?
                .context("Missing Compose ownership record; replacement is blocked.")?;
            ensure!(
                saved.version == 1
                    && saved.project == self.project
                    && saved.workload == self.config.id,
                "Mismatched or unsupported Compose record; it has been preserved."
            );
            ensure!(
                uuid::Uuid::parse_str(&saved.owner).is_ok_and(|owner| !owner.is_nil())
                    && (saved.host.is_empty() || saved.host.starts_with("npipe:////./pipe/"))
                    && !saved.host.contains(['\0', '\r', '\n']),
                "Invalid Compose ownership capability."
            );
            self.owner = saved.owner;
            self.cli.host = saved.host;
            self.engine = saved.engine;
            self.plan = saved.plan;
            self.containers = saved.containers;
            self.attempted = saved.attempted;
            self.started = saved.started;
            if let Some(plan) = &self.plan {
                // Reconstruct the frozen, ownership-labelled setup. Never read the
                // user's edited Compose source while recovering a session.
                let directory = crate::session_host::snapshot_directory()?;
                std::fs::write(
                    directory.path().join("compose.json"),
                    serde_json::to_vec(&plan.document)?,
                )?;
                self.snapshot = Some(directory);
            }
        }
        self.record = Some(record);
        if restore {
            self.refresh()?;
            self.follow_logs()?;
        }
        self.persist()
    }

    pub(super) fn persist(&mut self) -> Result<()> {
        if let Some(record) = &mut self.record {
            record.save(&Saved {
                version: 1,
                project: self.project.clone(),
                workload: self.config.id,
                owner: self.owner.clone(),
                host: self.cli.host.clone(),
                engine: self.engine.clone(),
                plan: self.plan.clone(),
                containers: self.containers.clone(),
                attempted: self.attempted,
                started: self.started,
            })?;
        }
        Ok(())
    }
}
