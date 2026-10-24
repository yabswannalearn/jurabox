#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::{
    ManagerInstance, OwnedProbe, activate_management_window, desktop_check, probe_window, status,
    workspace_check,
};
#[cfg(windows)]
pub use windows::{apps, managed_process, metrics, preferences, workspace};

#[cfg(not(windows))]
pub fn status() -> anyhow::Result<()> {
    anyhow::bail!("this first compatibility tool requires Windows 11")
}

#[cfg(not(windows))]
pub fn desktop_check() -> anyhow::Result<()> {
    anyhow::bail!("this first compatibility tool requires Windows 11")
}

#[cfg(not(windows))]
pub fn workspace_check() -> anyhow::Result<()> {
    anyhow::bail!("This check requires Windows 11.")
}
#[cfg(not(windows))]
pub fn probe_window() -> anyhow::Result<()> {
    anyhow::bail!("This check requires Windows 11.")
}
