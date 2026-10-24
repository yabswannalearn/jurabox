//! Only explicitly retired session generations are eligible for retention cleanup.
use super::*;
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(serde::Serialize, serde::Deserialize)]
struct Retired {
    version: u32,
    generation: Uuid,
    retired_seconds: u64,
}

pub(super) fn mark(root: &Path, generation: Uuid) -> Result<()> {
    crate::recovery::Record::new(root.join(generation.to_string()).join("retired.json")).save(
        &Retired {
            version: 1,
            generation,
            retired_seconds: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        },
    )
}

pub(super) fn prune(root: &Path) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    let root = root.canonicalize()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let Ok(generation) = Uuid::parse_str(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        let path = entry.path().canonicalize()?;
        if path.parent() != Some(root.as_path()) || !path.starts_with(&root) {
            continue;
        }
        let marker = crate::recovery::Record::new(path.join("retired.json")).read::<Retired>();
        if let Ok(Some(marker)) = marker
            && marker.version == 1
            && marker.generation == generation
            && now.saturating_sub(marker.retired_seconds) > 7 * 24 * 60 * 60
        {
            // These jobs and containers were verified stopped before retirement.
            // Unknown, live, resolved, malformed, and unsealed generations stay.
            fs::remove_dir_all(path)?;
        }
    }
    Ok(())
}

impl Workspaces {
    pub(super) fn retire(&self, id: Uuid) -> Result<()> {
        if self
            .sessions
            .get(&id)
            .is_some_and(|session| session.generation.is_nil())
        {
            return Ok(());
        }
        if let Some(root) = &self.recovery_root
            && let Some(session) = self.sessions.get(&id)
        {
            recovery::mark_closed(root, id, session.generation)?;
            cleanup::mark(root, session.generation)?;
        }
        Ok(())
    }
}
