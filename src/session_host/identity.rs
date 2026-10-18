use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;
use windows::{
    Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::RemoteDesktop::ProcessIdToSessionId,
        System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
    },
    core::PWSTR,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Identity {
    pub pid: u32,
    pub created: u64,
    pub executable: String,
    pub session: u32,
}

pub(super) fn inspect(pid: u32) -> Result<Identity> {
    // SAFETY: Query-only access, local bounded buffers, and the handle is always closed.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?;
        let result: Result<Identity> = (|| {
            let mut name = vec![0u16; 32768];
            let mut length = name.len() as u32;
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_FORMAT(0),
                PWSTR(name.as_mut_ptr()),
                &mut length,
            )?;
            let mut created = FILETIME::default();
            let mut exited = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user)?;
            ensure!(
                exited.dwHighDateTime == 0 && exited.dwLowDateTime == 0,
                "The session process has exited."
            );
            let mut session = 0;
            ProcessIdToSessionId(pid, &mut session)?;
            Ok(Identity {
                pid,
                created: (u64::from(created.dwHighDateTime) << 32)
                    | u64::from(created.dwLowDateTime),
                executable: path_key(Path::new(&String::from_utf16_lossy(
                    &name[..length as usize],
                ))),
                session,
            })
        })();
        let _ = CloseHandle(handle);
        result.context("Cannot verify the session process identity.")
    }
}

fn path_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('/', "\\")
        .to_lowercase()
}
