use super::*;
use crate::session_host::{Client, Request as HostRequest};
use std::{path::PathBuf, sync::mpsc, time::Duration};

/// Only the UI's background thread blocks on IPC. Failed mutations are never replayed.
pub(super) fn start(
    window: &AppWindow,
    state: Arc<Mutex<State>>,
    receiver: mpsc::Receiver<launch::Request>,
    journal: PathBuf,
) {
    let weak = window.as_weak();
    window.set_workspace_busy(true);
    std::thread::spawn(move || {
        let mut client = match Client::connect(&journal) {
            Ok(client) => Some(client),
            Err(error) => {
                failed(&weak, format!("{error:#}"));
                None
            }
        };
        if let Some(client) = &mut client {
            match client.request(HostRequest::Snapshot) {
                Ok(mut response) => {
                    if !response.summary.open.is_empty() && response.summary.message.is_empty() {
                        response.summary.message =
                            "Reconnected to running sessions. No apps were restarted.".into();
                    }
                    publish(&weak, &state, response, None, true);
                }
                Err(error) => failed(&weak, format!("{error:#}")),
            }
        }
        let mut last_summary = None;
        loop {
            let request = match receiver.recv_timeout(Duration::from_millis(500)) {
                Ok(request) => request,
                Err(mpsc::RecvTimeoutError::Timeout) if client.is_some() => launch::Request::Poll,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => break,
            };
            let mut ticket = None;
            let poll = matches!(request, launch::Request::Poll);
            let request = match request {
                launch::Request::Open(project, relaunch) => HostRequest::Open(project, relaunch),
                launch::Request::Close(project) => HostRequest::Close(project),
                launch::Request::Service(project, service, action) => {
                    HostRequest::Service(project, service, action)
                }
                launch::Request::Windows(id, project, app) => {
                    ticket = Some((id, false));
                    HostRequest::Windows(id, project, app)
                }
                launch::Request::Assign(id, choice) => {
                    ticket = Some((id, true));
                    HostRequest::Assign(id, choice)
                }
                launch::Request::Unassign(project, app) => HostRequest::Unassign(project, app),
                launch::Request::StopAndQuit => HostRequest::StopAndQuit,
                launch::Request::LeaveAndQuit => HostRequest::LeaveAndQuit,
                launch::Request::Poll => HostRequest::Snapshot,
                launch::Request::Desktop(_) => continue,
            };
            // After connection loss, an explicit user action can reconnect. We
            // discard expired picker tokens and never automatically retry a mutation.
            if client.is_none() {
                match Client::connect(&journal) {
                    Ok(connected) => client = Some(connected),
                    Err(error) => {
                        failed(&weak, format!("{error:#}"));
                        continue;
                    }
                }
            }
            match client.as_mut().unwrap().request(request) {
                Ok(response) => {
                    let quit = response.quit;
                    if !poll || last_summary.as_ref() != Some(&response.summary) {
                        last_summary = Some(response.summary.clone());
                        publish(&weak, &state, response, ticket, !poll);
                    }
                    if quit {
                        client = None;
                    }
                }
                Err(error) => {
                    client = None;
                    failed(&weak, format!("{error:#}"));
                }
            }
        }
    });
}

fn failed(weak: &slint::Weak<AppWindow>, message: String) {
    let _ = weak.upgrade_in_event_loop(move |window| {
        window.set_workspace_busy(false);
        window.set_session_connected(false);
        notice(&window, &message, true);
    });
}

fn publish(
    weak: &slint::Weak<AppWindow>,
    state: &Arc<Mutex<State>>,
    response: crate::session_host::Response,
    ticket: Option<(Uuid, bool)>,
    completed: bool,
) {
    let state = state.clone();
    let _ = weak.upgrade_in_event_loop(move |window| {
        window.set_session_connected(!response.quit);
        if completed {
            window.set_workspace_busy(false);
        }
        let mut state = state.lock().unwrap();
        if let Some((ticket, assignment)) = ticket {
            if assignment {
                window_picker::completed(
                    &window,
                    &mut state,
                    ticket,
                    !response.summary.error,
                    &response.summary,
                );
            } else if let Some(choices) = response.choices {
                window_picker::loaded(&window, &mut state, ticket, choices);
            }
        }
        state.workspace = response.summary;
        launch::refresh_status(&window, &state);
        if !state.workspace.message.is_empty() {
            notice(&window, &state.workspace.message, state.workspace.error);
        }
        if response.quit {
            if window.get_dirty() {
                notice(
                    &window,
                    "Save or discard your changes before quitting.",
                    true,
                );
            } else {
                let _ = slint::quit_event_loop();
            }
        }
    });
}
