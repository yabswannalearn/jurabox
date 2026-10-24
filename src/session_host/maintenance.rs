use super::*;

#[derive(Serialize, Deserialize)]
struct Owner {
    version: u32,
    pid: u32,
    created: u64,
    saved_seconds: u64,
}

fn root() -> Result<PathBuf> {
    Ok(crate::projects::ProjectStore::default_path()?
        .parent()
        .context("Missing data folder")?
        .join("runtime"))
}

pub fn snapshot_directory() -> Result<tempfile::TempDir> {
    let root = root()?;
    fs::create_dir_all(&root)?;
    let directory = tempfile::Builder::new()
        .prefix("compose-")
        .tempdir_in(&root)?;
    let identity = identity::inspect(std::process::id())?;
    crate::recovery::Record::new(directory.path().join("owner.json")).save(&Owner {
        version: 1,
        pid: identity.pid,
        created: identity.created,
        saved_seconds: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    })?;
    Ok(directory)
}

pub(super) fn prune_snapshots() -> Result<()> {
    use windows::Win32::Foundation::ERROR_INVALID_PARAMETER;
    let root = root()?;
    if !root.exists() {
        return Ok(());
    }
    let root = root.canonicalize()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if !kind.is_dir()
            || kind.is_symlink()
            || !entry.file_name().to_string_lossy().starts_with("compose-")
        {
            continue;
        }
        let path = entry.path().canonicalize()?;
        if path.parent() != Some(root.as_path()) {
            continue;
        }
        let owner = crate::recovery::Record::new(path.join("owner.json")).read::<Owner>();
        let Ok(Some(owner)) = owner else {
            continue;
        };
        if owner.version != 1
            || owner.pid == 0
            || now.saturating_sub(owner.saved_seconds) < 24 * 60 * 60
        {
            continue;
        }
        let gone = match identity::inspect(owner.pid) {
            Ok(identity) => identity.created != owner.created,
            Err(error) => error
                .downcast_ref::<windows::core::Error>()
                .is_some_and(|error| {
                    error.code() == windows::core::HRESULT::from_win32(ERROR_INVALID_PARAMETER.0)
                }),
        };
        if gone {
            fs::remove_dir_all(path)?;
        }
    }
    Ok(())
}
