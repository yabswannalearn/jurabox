use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;

mod workloads;
pub use workloads::{CommandSpec, ComposeConfig, Readiness, WorkloadConfig, WorkloadKind};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub folder: Option<PathBuf>,
    pub inactivity_minutes: Option<u32>,
    #[serde(default)]
    pub apps: Vec<AppConfig>,
    #[serde(default)]
    pub browser: Option<BrowserConfig>,
    #[serde(default)]
    pub workloads: Vec<WorkloadConfig>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub id: Uuid,
    pub name: String,
    pub executable: PathBuf,
    #[serde(default)]
    pub arguments: String,
    #[serde(default)]
    pub new_console: bool,
    #[serde(default)]
    pub shared: bool,
}

impl AppConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.name.trim().is_empty(), "Give each app a name.");
        ensure!(
            self.executable.is_absolute(),
            "Choose a full executable path for {}.",
            self.name
        );
        ensure!(
            self.executable
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exe")),
            "Choose an .exe application for {}. Run scripts through a configured terminal app.",
            self.name
        );
        ensure!(
            !self.arguments.contains('\0'),
            "App arguments cannot contain a null character."
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserConfig {
    pub executable: PathBuf,
    pub urls: Vec<String>,
}

impl BrowserConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.executable.is_absolute(),
            "Choose a full browser executable path."
        );
        let name = self
            .executable
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        ensure!(
            name == "chrome.exe" || name == "msedge.exe",
            "Choose Chrome or Edge for the project browser window."
        );
        ensure!(
            !self.urls.is_empty(),
            "Add at least one browser link, or turn the browser window off."
        );
        for value in &self.urls {
            let url =
                url::Url::parse(value).with_context(|| format!("Enter a full web URL: {value}"))?;
            ensure!(
                matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
                "Use http:// or https:// web links: {value}"
            );
        }
        Ok(())
    }
}

impl Project {
    pub fn from_form(
        id: Option<Uuid>,
        name: &str,
        folder: &str,
        inactivity_minutes: Option<u32>,
    ) -> Result<Self> {
        let project = Self {
            id: id.unwrap_or_else(Uuid::new_v4),
            name: name.trim().to_owned(),
            folder: (!folder.trim().is_empty()).then(|| PathBuf::from(folder.trim())),
            inactivity_minutes,
            apps: Vec::new(),
            browser: None,
            workloads: Vec::new(),
        };
        project.validate()?;
        Ok(project)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(!self.name.trim().is_empty(), "Enter a project name.");
        if let Some(minutes) = self.inactivity_minutes {
            ensure!(
                minutes > 0 && minutes <= i32::MAX as u32,
                "Choose an inactivity timeout of at least one minute, or turn it off."
            );
        }
        let mut ids = std::collections::HashSet::new();
        for app in &self.apps {
            app.validate()?;
            ensure!(
                !app.id.is_nil(),
                "Each app entry needs a nonempty identity."
            );
            ensure!(ids.insert(app.id), "Each app entry needs its own identity.");
        }
        if let Some(browser) = &self.browser {
            browser.validate()?;
        }
        for workload in &self.workloads {
            workload.validate(self.folder.as_deref())?;
            ensure!(
                !workload.id.is_nil() && ids.insert(workload.id),
                "Each app, service, and startup action needs its own nonempty identity."
            );
        }
        workloads::validate_dependencies(&self.workloads)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectFile {
    pub version: u32,
    pub projects: Vec<Project>,
}

impl Default for ProjectFile {
    fn default() -> Self {
        Self {
            version: 5,
            projects: Vec::new(),
        }
    }
}

impl ProjectFile {
    pub fn upsert(&mut self, project: Project) {
        if let Some(existing) = self
            .projects
            .iter_mut()
            .find(|entry| entry.id == project.id)
        {
            *existing = project;
        } else {
            self.projects.push(project);
        }
    }

    pub fn remove(&mut self, id: Uuid) {
        self.projects.retain(|project| project.id != id);
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 5,
            "This project file uses unsupported version {}. Keep the file and use a compatible Jurabox version.",
            self.version
        );
        let mut ids = std::collections::HashSet::new();
        for project in &self.projects {
            project.validate()?;
            ensure!(
                ids.insert(project.id),
                "The project file contains a duplicate project identity."
            );
        }
        Ok(())
    }
}

pub struct ProjectStore {
    path: PathBuf,
}

impl ProjectStore {
    #[cfg(windows)]
    pub fn session_journal(&self) -> PathBuf {
        self.path.with_extension("session-host.json")
    }

    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> Result<PathBuf> {
        let directory = std::env::var_os("LOCALAPPDATA")
            .context("Windows did not provide a local application data folder.")?;
        let directory = PathBuf::from(directory);
        ensure!(
            directory.is_absolute(),
            "Windows local application data folder is not an absolute path."
        );
        Ok(directory.join("Jurabox").join("projects.json"))
    }

    pub fn load(&self) -> Result<ProjectFile> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ProjectFile::default());
            }
            Err(error) => {
                return Err(error).with_context(|| format!("Cannot read {}", self.path.display()));
            }
        };
        let mut data: ProjectFile = serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "Cannot read the project settings in {}. The file has been preserved.",
                self.path.display()
            )
        })?;
        if matches!(data.version, 1..=4) {
            data.version = 5;
        }
        data.validate()?;
        Ok(data)
    }

    pub fn save(&self, data: &ProjectFile) -> Result<()> {
        data.validate()?;
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent).context("Cannot create the Jurabox settings folder.")?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .context("Cannot create a temporary settings file.")?;
        serde_json::to_writer_pretty(&mut temporary, data)?;
        temporary.write_all(b"\n")?;
        temporary
            .as_file()
            .sync_all()
            .context("Cannot flush project settings to disk.")?;
        temporary
            .persist(&self.path)
            .map_err(|error| error.error)
            .with_context(|| {
                format!(
                    "Cannot replace {}. The previous settings have been preserved.",
                    self.path.display()
                )
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str) -> Project {
        Project::from_form(None, name, "", Some(15)).unwrap()
    }

    #[test]
    fn projects_round_trip_with_optional_folders_and_off_policy() {
        let directory = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(directory.path().join("nested/projects.json"));
        assert_eq!(store.load().unwrap(), ProjectFile::default());
        let mut data = ProjectFile::default();
        data.upsert(project("Jurabox"));
        data.upsert(Project::from_form(None, " Research ", "D:\\研究\\Notes", None).unwrap());
        store.save(&data).unwrap();
        assert_eq!(store.load().unwrap(), data);
        assert_eq!(data.projects[1].name, "Research");
    }

    #[test]
    fn renaming_updates_the_same_project() {
        let mut data = ProjectFile::default();
        let original = project("Before");
        data.upsert(original.clone());
        data.upsert(Project::from_form(Some(original.id), "After", "", Some(20)).unwrap());
        assert_eq!(data.projects.len(), 1);
        assert_eq!(data.projects[0].id, original.id);
        assert_eq!(data.projects[0].name, "After");
    }

    #[test]
    fn version_one_loads_without_rewriting_until_save() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("projects.json");
        let old = format!(
            r#"{{"version":1,"projects":[{{"id":"{}","name":"Existing","folder":null,"inactivity_minutes":15}}]}}"#,
            Uuid::new_v4()
        );
        fs::write(&path, &old).unwrap();
        let store = ProjectStore::new(path.clone());
        let data = store.load().unwrap();
        assert_eq!(data.version, 5);
        assert!(data.projects[0].apps.is_empty() && data.projects[0].browser.is_none());
        assert_eq!(fs::read_to_string(&path).unwrap(), old);
        store.save(&data).unwrap();
        assert_eq!(store.load().unwrap(), data);
    }

    #[test]
    fn app_and_browser_setup_round_trip_with_validation() {
        let directory = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(directory.path().join("projects.json"));
        let mut entry = project("Tools");
        entry.apps.push(AppConfig {
            id: Uuid::new_v4(),
            name: "Editor".into(),
            executable: r"C:\Apps\editor.exe".into(),
            arguments: r#"--new-window "{folder}""#.into(),
            new_console: false,
            shared: true,
        });
        entry.browser = Some(BrowserConfig {
            executable: r"C:\Apps\chrome.exe".into(),
            urls: vec!["https://example.com/?q=notes".into()],
        });
        let mut data = ProjectFile::default();
        data.upsert(entry);
        store.save(&data).unwrap();
        assert_eq!(store.load().unwrap(), data);
        data.projects[0].browser.as_mut().unwrap().urls = vec!["file:///C:/private.txt".into()];
        assert!(store.save(&data).is_err());
        data.projects[0].browser = None;
        data.projects[0].apps[0].executable = "relative.exe".into();
        assert!(store.save(&data).is_err());
    }

    #[test]
    fn malformed_settings_are_reported_without_overwriting() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("projects.json");
        fs::write(&path, b"incomplete settings").unwrap();
        assert!(ProjectStore::new(path.clone()).load().is_err());
        assert_eq!(fs::read(path).unwrap(), b"incomplete settings");
    }

    #[test]
    fn invalid_save_preserves_previous_settings() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("projects.json");
        let store = ProjectStore::new(path.clone());
        let mut data = ProjectFile::default();
        data.upsert(project("Valid"));
        store.save(&data).unwrap();
        let original = fs::read(&path).unwrap();
        data.projects[0].name.clear();
        assert!(store.save(&data).is_err());
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn unsupported_versions_and_duplicate_ids_are_rejected() {
        let mut data = ProjectFile {
            version: 6,
            ..Default::default()
        };
        assert!(data.validate().is_err());
        data.version = 5;
        let entry = project("First");
        data.projects.extend([entry.clone(), entry]);
        assert!(data.validate().is_err());
    }

    #[test]
    fn deleting_a_saved_project_preserves_its_folder_and_files() {
        let directory = tempfile::tempdir().unwrap();
        let folder = directory.path().join("user-project");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("work.txt"), "keep this").unwrap();
        let store = ProjectStore::new(directory.path().join("projects.json"));
        let entry = Project::from_form(None, "Work", &folder.to_string_lossy(), Some(15)).unwrap();
        let mut data = ProjectFile::default();
        data.upsert(entry.clone());
        store.save(&data).unwrap();
        data.remove(entry.id);
        store.save(&data).unwrap();
        assert!(store.load().unwrap().projects.is_empty());
        assert_eq!(
            fs::read_to_string(folder.join("work.txt")).unwrap(),
            "keep this"
        );
    }
}
