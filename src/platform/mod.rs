#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::{desktop_check, status};

#[cfg(not(windows))]
pub fn status() -> anyhow::Result<()> {
    anyhow::bail!("this first compatibility tool requires Windows 11")
}

#[cfg(not(windows))]
pub fn desktop_check() -> anyhow::Result<()> {
    anyhow::bail!("this first compatibility tool requires Windows 11")
}
