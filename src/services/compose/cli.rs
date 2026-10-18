use crate::platform::managed_process::ManagedProcess;
use anyhow::{Context, Result, ensure};
use std::{
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub(super) struct Cli {
    pub executable: PathBuf,
    pub folder: PathBuf,
    pub host: String,
    pub cancel: Arc<AtomicBool>,
    pub pending: Option<ManagedProcess>,
}

// CommandLineToArgvW/MS CRT quoting, with no shell involved.
pub(super) fn quote(value: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for ch in value.chars() {
        if ch == '\\' {
            slashes += 1;
            continue;
        }
        if ch == '"' {
            result.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
        } else {
            result.extend(std::iter::repeat_n('\\', slashes));
        }
        slashes = 0;
        result.push(ch);
    }
    result.extend(std::iter::repeat_n('\\', slashes * 2));
    result.push('"');
    result
}

impl Cli {
    pub fn new(executable: &Path, folder: &Path, cancel: Arc<AtomicBool>) -> Self {
        Self {
            executable: executable.into(),
            folder: folder.into(),
            host: String::new(),
            cancel,
            pending: None,
        }
    }
    pub fn spawn(&self, args: &[String]) -> Result<ManagedProcess> {
        let mut values = Vec::new();
        if !self.host.is_empty() {
            values.extend(["--host".into(), self.host.clone()]);
        }
        values.extend_from_slice(args);
        let arguments = values
            .iter()
            .map(|value| quote(value))
            .collect::<Vec<_>>()
            .join(" ");
        let mut environment = Vec::new();
        for (key, value) in std::env::vars_os() {
            let upper = key.to_string_lossy().to_ascii_uppercase();
            if upper.starts_with("COMPOSE_")
                || [
                    "DOCKER_HOST",
                    "DOCKER_CONTEXT",
                    "DOCKER_TLS_VERIFY",
                    "DOCKER_CERT_PATH",
                    "DOCKER_API_VERSION",
                ]
                .contains(&upper.as_str())
            {
                continue;
            }
            environment.extend(key.encode_wide());
            environment.push('=' as u16);
            environment.extend(value.encode_wide());
            environment.push(0);
        }
        environment.push(0);
        ManagedProcess::start_environment(
            &self.executable,
            &arguments,
            &self.folder,
            Some(&environment),
        )
    }
    pub fn run(&mut self, args: &[String], seconds: u64) -> Result<String> {
        self.reap()?;
        ensure!(
            self.pending.is_none(),
            "The previous Docker command is still running. No duplicate command started; wait or use Force stop."
        );
        let process = self.spawn(args)?;
        self.pending = Some(process);
        let deadline = Instant::now() + Duration::from_secs(seconds);
        loop {
            let process = self.pending.as_ref().unwrap();
            if self.cancel.swap(false, Ordering::SeqCst) {
                process
                    .force_stop()
                    .context("Cannot cancel the owned Docker client")?;
                anyhow::bail!(
                    "Docker client cancelled by explicit Force stop; container ownership will be checked again."
                );
            }
            if !process.active()? && !process.output_pending() {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "Docker command has not finished. Its client remains tracked; wait or use Force stop."
            );
            thread::sleep(Duration::from_millis(25));
        }
        let process = self.pending.take().unwrap();
        let output = process.output();
        ensure!(
            process.exit_code()? == Some(0),
            "Docker command failed: {output}"
        );
        ensure!(
            !output.contains("[Earlier output omitted:"),
            "Docker returned too much structured output; no result can be trusted."
        );
        Ok(output)
    }
    pub fn reap(&mut self) -> Result<()> {
        if self
            .pending
            .as_ref()
            .is_some_and(|process| !process.active().unwrap_or(true) && !process.output_pending())
        {
            self.pending = None;
        }
        Ok(())
    }
    pub fn cancel_client(&mut self) -> Result<()> {
        self.cancel.store(false, Ordering::SeqCst);
        if let Some(process) = &self.pending {
            if process.active()? {
                process.force_stop()?;
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            while process.active()? {
                ensure!(Instant::now() < deadline, "Docker client is still running.");
                thread::sleep(Duration::from_millis(25));
            }
        }
        self.pending = None;
        Ok(())
    }
}

pub(super) fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn docker_arguments_round_trip_without_a_shell() {
        let values = [
            "",
            "with spaces",
            r"D:\测试 folder\",
            "a\"b",
            "$(not executed)",
        ];
        let command = values
            .iter()
            .map(|value| quote(value))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            crate::platform::apps::parse_arguments(&command).unwrap(),
            values
        );
    }
}
