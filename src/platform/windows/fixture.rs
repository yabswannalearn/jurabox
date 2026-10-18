use super::{
    app_windows::{self, AppWindow},
    vd, wait_until,
};
use anyhow::{Context, Result, ensure};
use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};
use windows::{
    Win32::{
        Foundation::{LPARAM, WPARAM},
        UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE},
    },
    core::GUID,
};

/// A diagnostic-only external window from a private executable copy. No existing
/// user application can match its executable path or child-process identity.
pub struct OwnedProbe {
    directory: tempfile::TempDir,
    executable: PathBuf,
    child: Child,
    window: Option<AppWindow>,
}

impl OwnedProbe {
    pub fn start() -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("jurabox-ui-probe.exe");
        std::fs::copy(std::env::current_exe()?, &executable)?;
        let child = Command::new(&executable)
            .arg("_probe-window")
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let mut probe = Self {
            directory,
            executable,
            child,
            window: None,
        };
        wait_until("owned UI fixture window", || {
            probe.window = app_windows::all()?.into_iter().find(|window| {
                window.process == probe.child.id()
                    && window.executable == app_windows::path_key(&probe.executable)
            });
            Ok(probe.window.is_some())
        })?;
        Ok(probe)
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }
    pub fn is_alive(&self) -> bool {
        self.window.as_ref().is_some_and(AppWindow::is_alive)
    }
    pub fn desktop(&self) -> Result<GUID> {
        let window = self
            .window
            .as_ref()
            .context("The UI fixture has no window")?;
        vd(vd(winvd::get_desktop_by_window(window.hwnd()))?.get_id())
    }
    pub fn close(&mut self) -> Result<()> {
        // Also find a late fixture window during failure cleanup, scoped to our
        // private executable and the child whose handle this guard owns.
        for window in app_windows::all()?.into_iter().filter(|window| {
            window.process == self.child.id()
                && window.executable == app_windows::path_key(&self.executable)
        }) {
            if window.is_alive() {
                // SAFETY: This process/window identity belongs to our own fixture.
                unsafe {
                    PostMessageW(window.hwnd(), WM_CLOSE, WPARAM(0), LPARAM(0))?;
                }
            }
        }
        wait_until("owned UI fixture process exit", || {
            Ok(self.child.try_wait()?.is_some())
        })?;
        ensure!(
            !self.is_alive(),
            "The owned UI fixture window is still open"
        );
        Ok(())
    }
}

impl Drop for OwnedProbe {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            eprintln!(
                "UI fixture cleanup failed in {}: {error:#}",
                self.directory.path().display()
            );
        }
    }
}
