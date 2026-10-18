use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadKind {
    CommandService,
    StartupAction,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Readiness {
    #[default]
    ProcessRunning,
    Tcp {
        address: std::net::SocketAddr,
    },
    Http {
        url: String,
    },
}

fn readiness_timeout() -> u32 {
    60
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
    #[serde(default)]
    pub readiness: Readiness,
    #[serde(default = "readiness_timeout")]
    pub readiness_timeout_seconds: u32,
    #[serde(default)]
    pub dependencies: Vec<Uuid>,
}

fn stop_timeout() -> u32 {
    10
}

pub(super) fn validate_dependencies(entries: &[WorkloadConfig]) -> Result<()> {
    fn visit(
        id: Uuid,
        entries: &[WorkloadConfig],
        visiting: &mut std::collections::HashSet<Uuid>,
        done: &mut std::collections::HashSet<Uuid>,
    ) -> Result<()> {
        if done.contains(&id) {
            return Ok(());
        }
        let entry = entries
            .iter()
            .find(|entry| entry.id == id)
            .context("A service dependency does not exist in this project.")?;
        ensure!(
            visiting.insert(id),
            "Service dependencies contain a cycle involving {}.",
            entry.name
        );
        let mut unique = std::collections::HashSet::new();
        for dependency in &entry.dependencies {
            ensure!(*dependency != id, "{} cannot depend on itself.", entry.name);
            ensure!(
                unique.insert(*dependency),
                "{} lists the same dependency twice.",
                entry.name
            );
            visit(*dependency, entries, visiting, done)?;
        }
        visiting.remove(&id);
        done.insert(id);
        Ok(())
    }
    let mut done = std::collections::HashSet::new();
    for entry in entries {
        visit(
            entry.id,
            entries,
            &mut std::collections::HashSet::new(),
            &mut done,
        )?;
    }
    Ok(())
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
        ensure!(
            (1..=3600).contains(&self.readiness_timeout_seconds),
            "Choose a readiness timeout between 1 and 3600 seconds for {}.",
            self.name
        );
        if let Readiness::Tcp { address } = self.readiness {
            ensure!(
                address.port() != 0 && !address.ip().is_unspecified(),
                "Use a specific TCP address and a port above zero for {}.",
                self.name
            );
        }
        if let Readiness::Http { url } = &self.readiness {
            let url = url::Url::parse(url).context("Enter a full HTTP(S) health URL.")?;
            ensure!(
                matches!(url.scheme(), "http" | "https")
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none(),
                "Use an HTTP(S) health URL without embedded credentials."
            );
        }
        ensure!(
            self.kind != WorkloadKind::StartupAction || self.readiness == Readiness::ProcessRunning,
            "Startup actions become ready after successful completion; choose Process running for {}.",
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
            readiness: Readiness::default(),
            readiness_timeout_seconds: 60,
            dependencies: Vec::new(),
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
    fn readiness_requires_a_specific_endpoint_and_a_finite_timeout() {
        let mut entry = config();
        entry.folder = Some(r"D:\Work".into());
        for address in ["0.0.0.0:3000", "127.0.0.1:0", "[::]:3000"] {
            entry.readiness = Readiness::Tcp {
                address: address.parse().unwrap(),
            };
            assert!(entry.validate(None).is_err());
        }
        for url in [
            "file:///C:/health",
            "http://user:pass@localhost/health",
            "not a URL",
        ] {
            entry.readiness = Readiness::Http { url: url.into() };
            assert!(entry.validate(None).is_err());
        }
        entry.readiness = Readiness::Http {
            url: "https://localhost:3000/health?ready=yes".into(),
        };
        assert!(entry.validate(None).is_ok());
        entry.kind = WorkloadKind::StartupAction;
        assert!(entry.validate(None).is_err());
        entry.kind = WorkloadKind::CommandService;
        entry.readiness_timeout_seconds = 0;
        assert!(entry.validate(None).is_err());
    }

    #[test]
    fn dependency_graph_rejects_unknown_self_duplicate_and_cyclic_edges() {
        let mut first = config();
        let mut second = config();
        first.dependencies = vec![second.id];
        assert!(validate_dependencies(&[first.clone(), second.clone()]).is_ok());
        second.name = "Renamed prerequisite".into();
        assert!(validate_dependencies(&[first.clone(), second.clone()]).is_ok());
        second.dependencies = vec![first.id];
        assert!(validate_dependencies(&[first.clone(), second.clone()]).is_err());
        first.dependencies = vec![first.id];
        assert!(validate_dependencies(&[first.clone()]).is_err());
        first.dependencies = vec![Uuid::new_v4()];
        assert!(validate_dependencies(&[first.clone()]).is_err());
        first.dependencies = vec![second.id, second.id];
        second.dependencies.clear();
        assert!(validate_dependencies(&[first, second]).is_err());
    }

    #[test]
    fn version_three_workloads_default_to_process_readiness_without_rewriting_disk() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("projects.json");
        let mut project = Project::from_form(None, "Existing", r"D:\Work", None).unwrap();
        project.workloads.push(config());
        let mut file = ProjectFile::default();
        file.upsert(project);
        let mut old = serde_json::to_value(&file).unwrap();
        old["version"] = 3.into();
        let workload = old["projects"][0]["workloads"][0].as_object_mut().unwrap();
        for key in ["readiness", "readiness_timeout_seconds", "dependencies"] {
            workload.remove(key);
        }
        let original = serde_json::to_string(&old).unwrap();
        fs::write(&path, &original).unwrap();
        let store = ProjectStore::new(path.clone());
        let mut migrated = store.load().unwrap();
        assert_eq!(migrated.version, 4);
        assert_eq!(
            migrated.projects[0].workloads[0].readiness,
            Readiness::ProcessRunning
        );
        assert_eq!(
            migrated.projects[0].workloads[0].readiness_timeout_seconds,
            60
        );
        assert!(migrated.projects[0].workloads[0].dependencies.is_empty());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        migrated.projects[0].workloads[0].readiness = Readiness::Tcp {
            address: "127.0.0.1:3000".parse().unwrap(),
        };
        let mut dependent = config();
        dependent
            .dependencies
            .push(migrated.projects[0].workloads[0].id);
        migrated.projects[0].workloads.push(dependent);
        store.save(&migrated).unwrap();
        assert_eq!(store.load().unwrap(), migrated);
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
        assert_eq!(migrated.version, 4);
        assert!(migrated.projects[0].workloads.is_empty());
        assert_eq!(fs::read_to_string(path).unwrap(), old);
    }
}
