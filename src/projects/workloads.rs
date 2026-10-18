use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadKind {
    CommandService,
    StartupAction,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandSpec {
    pub executable: PathBuf,
    #[serde(default)]
    pub arguments: String,
}

impl CommandSpec {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.executable.is_absolute()
                && self
                    .executable
                    .extension()
                    .is_some_and(|value| value.eq_ignore_ascii_case("exe")),
            "Choose a full .exe path. Run scripts through an explicitly configured shell."
        );
        ensure!(
            !self.arguments.contains('\0'),
            "Command arguments cannot contain a null character."
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadConfig {
    pub id: Uuid,
    pub name: String,
    pub kind: WorkloadKind,
    pub start: CommandSpec,
    #[serde(default)]
    pub folder: Option<PathBuf>,
    #[serde(default)]
    pub stop: Option<CommandSpec>,
    #[serde(default = "stop_timeout")]
    pub stop_timeout_seconds: u32,
}

fn stop_timeout() -> u32 {
    10
}

impl WorkloadConfig {
    pub fn folder<'a>(&'a self, project_folder: Option<&'a Path>) -> Result<&'a Path> {
        self.folder
            .as_deref()
            .or(project_folder)
            .context("Set a working folder for each service/action, or give its project a folder.")
    }

    pub fn validate(&self, project_folder: Option<&Path>) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty(),
            "Give each service or startup action a name."
        );
        self.start.validate().with_context(|| self.name.clone())?;
        ensure!(
            self.folder(project_folder)?.is_absolute(),
            "Choose a full working folder path for {}.",
            self.name
        );
        if let Some(stop) = &self.stop {
            stop.validate()
                .with_context(|| format!("Stop command for {}", self.name))?;
        }
        ensure!(
            (1..=3600).contains(&self.stop_timeout_seconds),
            "Choose a shutdown timeout between 1 and 3600 seconds for {}.",
            self.name
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> WorkloadConfig {
        WorkloadConfig {
            id: Uuid::new_v4(),
            name: "Development server".into(),
            kind: WorkloadKind::CommandService,
            start: CommandSpec {
                executable: r"C:\Tools\server.exe".into(),
                arguments: "--port 4000".into(),
            },
            folder: None,
            stop: None,
            stop_timeout_seconds: 10,
        }
    }

    #[test]
    fn commands_require_an_explicit_working_location_and_valid_stop_configuration() {
        let mut entry = config();
        assert!(entry.validate(None).is_err());
        assert!(entry.validate(Some(Path::new(r"D:\Work"))).is_ok());
        entry.folder = Some("relative".into());
        assert!(entry.validate(Some(Path::new(r"D:\Work"))).is_err());
        entry.folder = Some(r"D:\Service".into());
        entry.stop = Some(CommandSpec {
            executable: "taskkill.exe".into(),
            arguments: String::new(),
        });
        assert!(entry.validate(None).is_err());
    }

    #[test]
    fn typed_workloads_round_trip_and_version_two_migrates_without_a_write() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("projects.json");
        let mut project = Project::from_form(None, "Work", r"D:\Work", None).unwrap();
        let mut action = config();
        action.kind = WorkloadKind::StartupAction;
        project.workloads = vec![config(), action];
        let mut file = ProjectFile::default();
        file.upsert(project);
        let store = ProjectStore::new(path.clone());
        store.save(&file).unwrap();
        assert_eq!(store.load().unwrap(), file);
        let old = format!(
            r#"{{"version":2,"projects":[{{"id":"{}","name":"Existing","folder":null,"inactivity_minutes":null,"apps":[],"browser":null}}]}}"#,
            Uuid::new_v4()
        );
        fs::write(&path, &old).unwrap();
        let migrated = store.load().unwrap();
        assert_eq!(migrated.version, 3);
        assert!(migrated.projects[0].workloads.is_empty());
        assert_eq!(fs::read_to_string(path).unwrap(), old);
    }
}
