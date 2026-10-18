use super::*;
mod assignment;
mod browser;
mod console;
use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE},
};

struct Cleanup {
    original: Desktop,
    desktops: Vec<Desktop>,
    windows: Vec<AppWindow>,
    probe_folder: tempfile::TempDir,
}

impl Cleanup {
    fn capture(&mut self, workspaces: &Workspaces) {
        for session in workspaces.sessions.values() {
            if let Some(id) = session.desktop {
                let desktop = Desktop::from(id);
                if !self.desktops.contains(&desktop) {
                    self.desktops.push(desktop);
                }
            }
            self.windows.extend(
                session
                    .entries
                    .iter()
                    .filter_map(|entry| entry.window.clone()),
            );
        }
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        for window in &self.windows {
            if window.is_alive() {
                // SAFETY: Only windows launched by this exercise are retained.
                unsafe {
                    let _ = PostMessageW(window.hwnd(), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
        }
        let _ = winvd::switch_desktop(self.original);
        for desktop in &self.desktops {
            if !winvd::get_desktops()
                .unwrap_or_default()
                .iter()
                .any(|entry| entry.get_id().ok() == desktop.get_id().ok())
            {
                continue;
            }
            if let Err(error) = winvd::remove_desktop(*desktop, self.original) {
                eprintln!("Workspace check cleanup: {error:?}");
            }
        }
        let executable = self.probe_folder.path().join("jurabox-owned-probe.exe");
        let key = app_windows::path_key(&executable);
        let result = super::super::wait_until("owned probe process exit", || {
            // A unique copied executable prevents the shared-app test from ever
            // adopting a pre-existing Jurabox management window. Find late windows
            // only from this exercise's private copy during failure cleanup.
            for window in app_windows::all()?
                .into_iter()
                .filter(|window| window.executable == key)
            {
                if window.is_alive() {
                    // SAFETY: This executable exists only in our owned temporary folder.
                    unsafe {
                        PostMessageW(window.hwnd(), WM_CLOSE, WPARAM(0), LPARAM(0))?;
                    }
                }
            }
            match std::fs::remove_file(&executable) {
                Ok(()) => Ok(true),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Ok(false),
                Err(error) => Err(error.into()),
            }
        });
        if let Err(error) = result {
            eprintln!("Probe process cleanup failed: {error:#}");
        }
    }
}

/// Uses only projects/desktops and child windows created by this exercise.
pub(super) fn run() -> Result<()> {
    let original = vd(winvd::get_current_desktop())?;
    let original_ids = super::super::desktop_ids()?;
    let probe_folder = tempfile::tempdir()?;
    let probe_executable = probe_folder.path().join("jurabox-owned-probe.exe");
    std::fs::copy(std::env::current_exe()?, &probe_executable)?;
    let mut cleanup = Cleanup {
        original,
        desktops: Vec::new(),
        windows: Vec::new(),
        probe_folder,
    };
    let mut workspaces = Workspaces::default();
    let mut project = Project::from_form(None, "Jurabox launch check", "", Some(15))?;
    project.apps.push(AppConfig {
        app_id: None,
        id: Uuid::new_v4(),
        name: "Owned test app".into(),
        executable: probe_executable.clone(),
        arguments: "_probe-window".into(),
        new_console: false,
        shared: true,
    });
    let browser_pages = if let Some(executable) = std::env::var_os("JURABOX_CHECK_BROWSER") {
        let pages = browser::BrowserPages::start()?;
        project.browser = Some(crate::projects::BrowserConfig {
            executable: executable.into(),
            urls: pages.urls.clone(),
        });
        Some(pages)
    } else {
        None
    };
    project.validate()?;
    // A failed entry must not prevent an independent app from launching.
    let missing = AppConfig {
        app_id: None,
        id: Uuid::new_v4(),
        name: "Missing test app".into(),
        executable: std::env::temp_dir().join(format!("jurabox-missing-{}.exe", Uuid::new_v4())),
        arguments: String::new(),
        new_console: false,
        shared: false,
    };
    project.apps.push(missing);
    let result = workspaces.open(project.clone(), false);
    cleanup.capture(&workspaces);
    ensure!(result.is_err(), "Missing executable was not reported.");
    let first = workspaces.sessions[&project.id].entries[0]
        .window
        .clone()
        .context("No launched test window")?;
    ensure!(
        first.process != std::process::id(),
        "Test window must be in an external child process."
    );
    let desktop_one = workspaces.sessions[&project.id].desktop.unwrap();
    ensure!(
        super::super::window_desktop_id(first.hwnd())? == desktop_one,
        "Child window was not assigned."
    );
    project.apps.pop();
    let browser_window = project.browser.as_ref().and_then(|_| {
        workspaces.sessions[&project.id]
            .entries
            .last()
            .and_then(|entry| entry.window.clone())
    });
    if let Some(pages) = &browser_pages {
        ensure!(
            workspaces.sessions[&project.id]
                .entries
                .last()
                .unwrap()
                .window
                .is_some(),
            "Browser window was not assigned."
        );
        super::super::wait_until("both local browser links", || Ok(pages.both_loaded()))?;
        println!("PASS real browser new window, two local HTTP links, and desktop assignment");
    }
    println!("PASS partial launch failure preserves the successful external test app");
    let other = Project::from_form(None, "Jurabox second check", "", None)?;
    let result = workspaces.open(other.clone(), false);
    cleanup.capture(&workspaces);
    result?;
    let desktop_two = workspaces.sessions[&other.id].desktop.unwrap();
    ensure!(
        super::super::window_desktop_id(first.hwnd())? == desktop_two && first.is_alive(),
        "Shared window did not follow activation."
    );
    ensure!(
        workspaces.summary().active == Some(other.id),
        "Active project not tracked."
    );
    println!(
        "PASS external child launch, exact executable window assignment, two desktops, shared movement, active identity"
    );
    workspaces.open(project.clone(), false)?;
    ensure!(
        workspaces.sessions[&project.id].entries[0].window.as_ref() == Some(&first),
        "Switch duplicated the app."
    );
    let mut changed = project.clone();
    changed.name = "Saved change".into();
    changed.apps[0].name = "Renamed app label".into();
    workspaces.open(changed.clone(), false)?;
    ensure!(
        workspaces.sessions[&project.id].setup.name == project.name,
        "Switch applied saved settings early."
    );
    workspaces.open(changed.clone(), true)?;
    if let Some(browser_window) = &browser_window {
        ensure!(
            workspaces.sessions[&project.id]
                .entries
                .last()
                .unwrap()
                .window
                .as_ref()
                == Some(browser_window),
            "Relaunch duplicated an unchanged browser window."
        );
    }
    ensure!(
        workspaces.sessions[&project.id].setup.name == changed.name
            && workspaces.sessions[&project.id].entries[0].window.as_ref() == Some(&first),
        "Relaunch did not apply setup while preserving unchanged apps."
    );
    // SAFETY: This is the child test window this exercise launched and verified.
    unsafe {
        PostMessageW(first.hwnd(), WM_CLOSE, WPARAM(0), LPARAM(0))?;
    }
    super::super::wait_until("owned child close", || Ok(!first.is_alive()))?;
    workspaces.open(changed.clone(), false)?;
    ensure!(
        !first.is_alive()
            && workspaces.sessions[&project.id].entries[0].window.as_ref() == Some(&first),
        "Switch reopened a manually closed app."
    );
    let result = workspaces.open(changed.clone(), true);
    cleanup.capture(&workspaces);
    result?;
    let replacement = workspaces.sessions[&project.id].entries[0]
        .window
        .clone()
        .context("Explicit relaunch did not reopen the app")?;
    ensure!(
        replacement.is_alive() && replacement != first,
        "Relaunch reused a dead identity."
    );
    // External desktop switches use the same event handler as native Win+Tab.
    vd(winvd::switch_desktop(Desktop::from(desktop_two)))?;
    let summary = workspaces.desktop_changed();
    ensure!(
        summary.active == Some(other.id)
            && super::super::window_desktop_id(replacement.hwnd())? == desktop_two,
        "Native switch did not update the active project and shared window."
    );
    vd(winvd::remove_desktop(
        Desktop::from(desktop_one),
        Desktop::from(desktop_two),
    ))?;
    let result = workspaces.open(changed.clone(), false);
    cleanup.capture(&workspaces);
    result?;
    ensure!(
        workspaces.sessions[&project.id].desktop != Some(desktop_one)
            && workspaces.sessions[&project.id].entries[0].window.as_ref() == Some(&replacement),
        "Deleted desktop recovery duplicated the app or retained the lost desktop."
    );
    println!("PASS external desktop switches and deleted-desktop recreation preserve live apps");
    workspaces.close(project.id)?;
    ensure!(
        replacement.is_alive() && !workspaces.summary().open.contains(&project.id),
        "Close killed an app or retained the session."
    );
    println!(
        "PASS switch without duplicates, session snapshot, deliberate relaunch, manually closed apps, close keeps app alive"
    );
    assignment::run(&mut workspaces, &mut cleanup, &replacement, &first, &other)?;
    let console_window = console::run(&mut workspaces, &mut cleanup)?;
    drop(cleanup);
    ensure!(
        !probe_executable.exists(),
        "The owned probe process or executable could not be cleaned up."
    );
    super::super::wait_until("desktop cleanup", || {
        Ok(super::super::desktop_ids()? == original_ids && current_id()? == vd(original.get_id())?)
    })?;
    super::super::wait_until("child cleanup", || Ok(!replacement.is_alive()))?;
    if let Some(window) = console_window {
        super::super::wait_until("owned console cleanup", || Ok(!window.is_alive()))?;
    }
    if let Some(browser_window) = browser_window {
        super::super::wait_until("owned browser window cleanup", || {
            Ok(!browser_window.is_alive())
        })?;
        println!("PASS unchanged browser reuse and owned browser window cleanup");
    }
    println!("PASS original desktop list and active desktop restored; owned child windows closed");
    Ok(())
}
