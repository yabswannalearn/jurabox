use super::*;
use crate::{
    projects::{CommandSpec, ComposeConfig, Readiness, WorkloadConfig, WorkloadKind},
    services::Action,
};
use slint::Model;
use std::path::PathBuf;

pub(super) fn load(window: &AppWindow, project: Option<&Project>) {
    let rows: Vec<_> = project
        .into_iter()
        .flat_map(|project| &project.workloads)
        .map(|entry| WorkloadDraft {
            id: entry.id.to_string().into(),
            name: entry.name.clone().into(),
            one_shot: entry.kind == WorkloadKind::StartupAction,
            compose: entry.kind == WorkloadKind::DockerCompose,
            compose_file: entry
                .compose
                .as_ref()
                .map(|value| value.file.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into(),
            docker_context: entry
                .compose
                .as_ref()
                .map(|value| value.context.clone())
                .unwrap_or_default()
                .into(),
            executable: entry.start.executable.to_string_lossy().into_owned().into(),
            arguments: entry.start.arguments.clone().into(),
            folder: entry
                .folder
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into(),
            stop_executable: entry
                .stop
                .as_ref()
                .map(|command| command.executable.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into(),
            stop_arguments: entry
                .stop
                .as_ref()
                .map(|command| command.arguments.clone())
                .unwrap_or_default()
                .into(),
            stop_timeout: entry.stop_timeout_seconds as i32,
            readiness: match entry.readiness {
                Readiness::ProcessRunning => 0,
                Readiness::Tcp { .. } => 1,
                Readiness::Http { .. } => 2,
            },
            readiness_endpoint: match &entry.readiness {
                Readiness::ProcessRunning => String::new(),
                Readiness::Tcp { address } => address.to_string(),
                Readiness::Http { url } => url.clone(),
            }
            .into(),
            readiness_timeout: entry.readiness_timeout_seconds as i32,
            dependencies: entry
                .dependencies
                .iter()
                .map(Uuid::to_string)
                .collect::<Vec<_>>()
                .join(" ")
                .into(),
        })
        .collect();
    window.set_workloads(ModelRc::new(VecModel::from(rows)));
}

pub(super) fn read(window: &AppWindow, project: &mut Project) -> Result<()> {
    project.workloads = window
        .get_workloads()
        .iter()
        .map(|row| {
            let stop = (!row.stop_executable.trim().is_empty() || !row.stop_arguments.is_empty())
                .then(|| CommandSpec {
                    executable: PathBuf::from(row.stop_executable.trim()),
                    arguments: row.stop_arguments.to_string(),
                });
            Ok(WorkloadConfig {
                id: Uuid::parse_str(&row.id)?,
                name: row.name.trim().into(),
                kind: if row.compose { WorkloadKind::DockerCompose } else if row.one_shot {
                    WorkloadKind::StartupAction
                } else {
                    WorkloadKind::CommandService
                },
                start: CommandSpec {
                    executable: PathBuf::from(row.executable.trim()),
                    arguments: row.arguments.to_string(),
                },
                folder: (!row.folder.trim().is_empty()).then(|| PathBuf::from(row.folder.trim())),
                stop,
                stop_timeout_seconds: row.stop_timeout as u32,
                readiness: match row.readiness {
                    0 => Readiness::ProcessRunning,
                    1 => Readiness::Tcp { address: row.readiness_endpoint.trim().parse().context("Enter a TCP IP address and port, for example 127.0.0.1:3000 or [::1]:3000.")? },
                    2 => Readiness::Http { url: row.readiness_endpoint.trim().into() },
                    _ => anyhow::bail!("Choose a startup readiness condition."),
                },
                readiness_timeout_seconds: row.readiness_timeout as u32,
                dependencies: row.dependencies.split_whitespace().map(Uuid::parse_str).collect::<std::result::Result<_, _>>()?,
                compose: row.compose.then(|| ComposeConfig { file: row.compose_file.trim().into(), context: row.docker_context.trim().into() }),
            })
        })
        .collect::<Result<_>>()?;
    project.validate()
}

pub(super) fn refresh_runtime(window: &AppWindow, state: &State) {
    window.set_managed_live(
        state
            .workspace
            .workloads
            .iter()
            .any(|(_, view)| view.live || view.retained),
    );
    let rows: Vec<_> = state
        .workspace
        .workloads
        .iter()
        .filter(|(project, _)| Some(*project) == state.selected)
        .map(|(_, view)| WorkloadRow {
            id: view.id.to_string().into(),
            name: view.name.clone().into(),
            kind: view.kind.clone().into(),
            status: view.status.clone().into(),
            detail: view.detail.clone().into(),
            output: view.output.clone().into(),
            error: view.error,
            live: view.live,
            can_start: view.can_start,
            can_stop: view.can_stop,
        })
        .collect();
    window.set_session_workloads(ModelRc::new(VecModel::from(rows)));
}

fn editable(window: &AppWindow) -> bool {
    !window.get_busy() && !window.get_window_picker_visible()
}

pub(super) fn setup(window: &AppWindow, state: Arc<Mutex<State>>) {
    let weak = window.as_weak();
    window.on_add_compose(move || {
        if let Some(window) = weak.upgrade()
            && editable(&window)
        {
            let executable = std::env::var_os("ProgramFiles")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join("Docker/Docker/resources/bin/docker.exe");
            let mut rows: Vec<_> = window.get_workloads().iter().collect();
            rows.push(WorkloadDraft {
                id: Uuid::new_v4().to_string().into(),
                name: "Docker Compose".into(),
                compose: true,
                executable: if executable.is_file() {
                    executable.to_string_lossy().into_owned().into()
                } else {
                    "".into()
                },
                stop_timeout: 10,
                readiness_timeout: 60,
                ..Default::default()
            });
            window.set_workloads(ModelRc::new(VecModel::from(rows)));
            window.set_dirty(true);
        }
    });
    window.on_has_dependency(|values, id| {
        values.split_whitespace().any(|value| value == id.as_str())
    });
    let weak = window.as_weak();
    window.on_toggle_dependency(move |index, id, checked| {
        if let Some(window) = weak.upgrade()
            && editable(&window)
        {
            let mut rows: Vec<_> = window.get_workloads().iter().collect();
            if let Some(row) = rows.get_mut(index as usize) {
                let mut values: Vec<_> = row
                    .dependencies
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect();
                values.retain(|value| value != id.as_str());
                if checked {
                    values.push(id.to_string());
                }
                row.dependencies = values.join(" ").into();
                window.set_workloads(ModelRc::new(VecModel::from(rows)));
                window.set_dirty(true);
            }
        }
    });
    let weak = window.as_weak();
    window.on_add_workload(move |one_shot| {
        if let Some(window) = weak.upgrade()
            && editable(&window)
        {
            let mut rows: Vec<_> = window.get_workloads().iter().collect();
            rows.push(WorkloadDraft {
                id: Uuid::new_v4().to_string().into(),
                name: if one_shot {
                    "Startup action"
                } else {
                    "Command service"
                }
                .into(),
                one_shot,
                stop_timeout: 10,
                readiness_timeout: 60,
                ..Default::default()
            });
            window.set_workloads(ModelRc::new(VecModel::from(rows)));
            window.set_dirty(true);
        }
    });
    let weak = window.as_weak();
    window.on_remove_workload(move |index| {
        if let Some(window) = weak.upgrade()
            && editable(&window)
        {
            let mut rows: Vec<_> = window.get_workloads().iter().collect();
            if index >= 0 && (index as usize) < rows.len() {
                let id = &rows[index as usize].id;
                if rows.iter().any(|row| {
                    row.dependencies
                        .split_whitespace()
                        .any(|value| value == id.as_str())
                }) {
                    notice(
                        &window,
                        "Remove this workload from other prerequisites before removing it.",
                        true,
                    );
                    return;
                }
                rows.remove(index as usize);
                window.set_workloads(ModelRc::new(VecModel::from(rows)));
                window.set_dirty(true);
            }
        }
    });
    let weak = window.as_weak();
    window.on_browse_workload(move |index, field| {
        if let Some(window) = weak.upgrade()
            && editable(&window)
        {
            match launch::pick(&window, field == 1) {
                Ok(Some(path)) => {
                    let mut rows: Vec<_> = window.get_workloads().iter().collect();
                    if let Some(row) = rows.get_mut(index as usize) {
                        let path = path.to_string_lossy().into_owned().into();
                        match field {
                            0 => row.executable = path,
                            1 => row.folder = path,
                            2 => row.stop_executable = path,
                            3 => row.compose_file = path,
                            _ => return,
                        }
                        window.set_workloads(ModelRc::new(VecModel::from(rows)));
                        window.set_dirty(true);
                    }
                }
                Ok(None) => {}
                Err(error) => notice(&window, &format!("{error:#}"), true),
            }
        }
    });
    let weak = window.as_weak();
    let service_state = state.clone();
    window.on_service_action(move |id, action| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if !editable(&window) || window.get_workspace_busy() {
            return;
        }
        let Ok(id) = Uuid::parse_str(&id) else {
            return;
        };
        let action = match action.as_str() {
            "start" => Action::Start,
            "stop" => Action::Stop,
            "force" => Action::ForceStop,
            _ => return,
        };
        let state = service_state.lock().unwrap();
        let Some(project) = state.selected else {
            return;
        };
        window.set_workspace_busy(true);
        notice(
            &window,
            match action {
                Action::Start => "Starting the session's saved command…",
                Action::Stop => "Requesting normal shutdown…",
                Action::ForceStop => "Force stopping this managed workload…",
            },
            false,
        );
        if state
            .launcher
            .as_ref()
            .unwrap()
            .send(launch::Request::Service(project, id, action))
            .is_err()
        {
            window.set_workspace_busy(false);
            notice(
                &window,
                "Workspace worker stopped. Restart Jurabox to continue.",
                true,
            );
        }
    });
    let weak = window.as_weak();
    window.on_stop_and_quit(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        // The tray action must make shutdown progress and any failure visible.
        if !window.window().is_visible() {
            show(&window);
        }
        if !can_leave_form(&window) || window.get_workspace_busy() {
            return;
        }
        window.set_workspace_busy(true);
        notice(
            &window,
            "Stopping managed workloads before quitting…",
            false,
        );
        if state
            .lock()
            .unwrap()
            .launcher
            .as_ref()
            .unwrap()
            .send(launch::Request::StopAndQuit)
            .is_err()
        {
            window.set_workspace_busy(false);
            notice(
                &window,
                "Workspace worker stopped. Jurabox remains open.",
                true,
            );
        }
    });
}
