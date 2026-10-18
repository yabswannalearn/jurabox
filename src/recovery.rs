//! Private runtime records, separate from editable project definitions.
use anyhow::{Context, Result, ensure};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const LIMIT: usize = 4 * 1024 * 1024;

pub struct Record {
    path: PathBuf,
    last: Option<Vec<u8>>,
}

impl Record {
    pub fn claim_path(&self, claim: uuid::Uuid) -> PathBuf {
        self.path
            .with_extension("claims")
            .join(format!("{claim}.json"))
    }
    pub fn new(path: PathBuf) -> Self {
        Self { path, last: None }
    }

    pub fn read<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.take(LIMIT as u64 + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= LIMIT,
            "Recovery record is too large; it has been preserved."
        );
        let value = serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "Cannot read recovery record {}; it has been preserved.",
                self.path.display()
            )
        })?;
        self.last = Some(bytes);
        Ok(Some(value))
    }

    pub fn save<T: Serialize>(&mut self, value: &T) -> Result<()> {
        let bytes = serde_json::to_vec(value)?;
        ensure!(
            bytes.len() <= LIMIT,
            "Recovery record exceeds 4 MiB; no launch is allowed."
        );
        if self.last.as_ref() == Some(&bytes) {
            return Ok(());
        }
        let parent = self.path.parent().context("Missing recovery folder")?;
        std::fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&self.path).map_err(|error| error.error)?;
        self.last = Some(bytes);
        Ok(())
    }
}

pub fn project_path(root: &Path, id: uuid::Uuid) -> PathBuf {
    root.join(format!("{id}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn damaged_or_oversize_records_are_preserved() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("record.json");
        let mut record = Record::new(path.clone());
        record.save(&vec!["original"]).unwrap();
        assert!(record.save(&"x".repeat(LIMIT)).is_err());
        assert_eq!(
            record.read::<Vec<String>>().unwrap().unwrap(),
            vec!["original"]
        );
        std::fs::write(&path, "{").unwrap();
        assert!(record.read::<serde_json::Value>().is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "{");
    }
}
