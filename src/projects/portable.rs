use super::*;
use std::collections::HashMap;

impl ProjectFile {
    /// Portable files contain saved configuration only. Sensitive free-form
    /// values are omitted unless the user explicitly opts in to including them.
    pub fn portable(&self, include_values: bool) -> Self {
        let mut exported = self.clone();
        exported.preferences = Default::default();
        if !include_values {
            for project in &mut exported.projects {
                for app in &mut project.apps {
                    app.arguments.clear();
                }
                project.browser = None;
                for workload in &mut project.workloads {
                    for value in workload.environment.values_mut() {
                        value.clear();
                    }
                    workload.start.arguments.clear();
                    if let Some(stop) = &mut workload.stop {
                        stop.arguments.clear();
                    }
                    if matches!(workload.readiness, Readiness::Http { .. }) {
                        workload.readiness = Readiness::ProcessRunning;
                    }
                }
            }
        }
        exported
    }

    pub fn import_copies(
        &mut self,
        imported: Self,
        old: Option<&Path>,
        new: Option<&Path>,
    ) -> Result<usize> {
        imported.validate()?;
        ensure!(
            old.is_some() == new.is_some(),
            "Set both the old and new folder prefixes, or leave both empty."
        );
        if let (Some(old), Some(new)) = (old, new) {
            ensure!(
                old.is_absolute() && new.is_absolute(),
                "Use full folder prefixes."
            );
        }
        let count = imported.projects.len();
        for mut project in imported.projects {
            project.id = Uuid::new_v4();
            project.name.push_str(" (imported)");
            let ids: HashMap<_, _> = project
                .workloads
                .iter()
                .map(|entry| (entry.id, Uuid::new_v4()))
                .collect();
            if let Some(folder) = &mut project.folder {
                remap(folder, old, new);
            }
            for app in &mut project.apps {
                app.id = Uuid::new_v4();
                remap(&mut app.executable, old, new);
                if let Some(OpenFolder::Custom(folder)) = &mut app.open_folder {
                    remap(folder, old, new);
                }
            }
            if let Some(browser) = &mut project.browser {
                remap(&mut browser.executable, old, new);
            }
            for entry in &mut project.workloads {
                entry.id = ids[&entry.id];
                for dependency in &mut entry.dependencies {
                    *dependency = ids[dependency];
                }
                remap(&mut entry.start.executable, old, new);
                if let Some(stop) = &mut entry.stop {
                    remap(&mut stop.executable, old, new);
                }
                if let Some(folder) = &mut entry.folder {
                    remap(folder, old, new);
                }
                if let Some(compose) = &mut entry.compose {
                    remap(&mut compose.file, old, new);
                }
            }
            project.validate()?;
            self.projects.push(project);
        }
        Ok(count)
    }
}

fn remap(path: &mut PathBuf, old: Option<&Path>, new: Option<&Path>) {
    let (Some(old), Some(new)) = (old, new) else {
        return;
    };
    let mut remaining = path.components();
    for expected in old.components() {
        let Some(actual) = remaining.next() else {
            return;
        };
        if actual.as_os_str().to_string_lossy().to_lowercase()
            != expected.as_os_str().to_string_lossy().to_lowercase()
        {
            return;
        }
    }
    *path = new.join(remaining.as_path());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_defaults_omit_free_form_values_and_import_preserves_existing_projects() {
        let mut file = ProjectFile::default();
        let mut project = Project::from_form(None, "Original", r"D:\old", None).unwrap();
        project.apps.push(AppConfig {
            id: Uuid::new_v4(),
            name: "App".into(),
            executable: r"D:\old\app.exe".into(),
            app_id: None,
            open_folder: None,
            arguments: "--token secret".into(),
            new_console: false,
            shared: false,
        });
        file.projects.push(project.clone());
        let exported = file.portable(false);
        assert!(exported.projects[0].apps[0].arguments.is_empty());
        let mut target = file.clone();
        target
            .import_copies(
                exported,
                Some(Path::new(r"D:\old")),
                Some(Path::new(r"E:\new")),
            )
            .unwrap();
        assert_eq!(target.projects[0], project);
        assert_ne!(target.projects[1].id, project.id);
        assert_eq!(
            target.projects[1].apps[0].executable,
            Path::new(r"E:\new\app.exe")
        );
    }
}
