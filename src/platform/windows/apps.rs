use anyhow::{Context, Result, bail};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
mod catalog;
pub use catalog::{activate, installed_apps, resolve_app};
use windows::{
    Win32::{
        Foundation::{
            ERROR_CANCELLED, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, HLOCAL, HWND, LocalFree,
        },
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoTaskMemFree, CoUninitialize,
            },
            Registry::{
                HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ, RegCloseKey,
                RegEnumKeyW, RegGetValueW, RegOpenKeyExW,
            },
        },
        UI::Shell::{
            CommandLineToArgvW, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS,
            FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
        },
    },
    core::{HSTRING, PCWSTR},
};

#[derive(Clone, Debug)]
pub struct InstalledApp {
    pub name: String,
    pub executable: PathBuf,
    pub app_id: Option<String>,
    pub arguments: String,
    pub new_console: bool,
}

fn registry_apps() -> Vec<InstalledApp> {
    let mut found = BTreeMap::new();
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for key in [
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths",
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths",
        ] {
            // SAFETY: The registry is opened read-only. All names and data buffers are
            // owned locally; the key is closed after enumeration and nothing is changed.
            unsafe {
                let mut handle = HKEY::default();
                if RegOpenKeyExW(root, &HSTRING::from(key), 0, KEY_READ, &mut handle)
                    != ERROR_SUCCESS
                {
                    continue;
                }
                for index in 0..4096 {
                    let mut name = [0u16; 512];
                    let result = RegEnumKeyW(handle, index, Some(&mut name));
                    if result == ERROR_NO_MORE_ITEMS {
                        break;
                    }
                    if result != ERROR_SUCCESS {
                        continue;
                    }
                    let name = String::from_utf16_lossy(
                        &name[..name
                            .iter()
                            .position(|value| *value == 0)
                            .unwrap_or(name.len())],
                    );
                    let mut value = vec![0u16; 32768];
                    let mut bytes = (value.len() * 2) as u32;
                    if RegGetValueW(
                        handle,
                        &HSTRING::from(&name),
                        PCWSTR::null(),
                        RRF_RT_REG_SZ,
                        None,
                        Some(value.as_mut_ptr().cast()),
                        Some(&mut bytes),
                    ) != ERROR_SUCCESS
                    {
                        continue;
                    }
                    let path = String::from_utf16_lossy(
                        &value[..value
                            .iter()
                            .position(|value| *value == 0)
                            .unwrap_or(value.len())],
                    );
                    add(
                        &mut found,
                        name.trim_end_matches(".exe").to_owned(),
                        PathBuf::from(path.trim_matches('"')),
                    );
                }
                let _ = RegCloseKey(handle);
            }
        }
    }
    if let Some(system) = std::env::var_os("SystemRoot") {
        let directory = PathBuf::from(system).join("System32");
        add(
            &mut found,
            "Command Prompt".into(),
            directory.join("cmd.exe"),
        );
        add(
            &mut found,
            "Windows PowerShell".into(),
            directory.join("WindowsPowerShell/v1.0/powershell.exe"),
        );
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let directory = PathBuf::from(local);
        add(
            &mut found,
            "Zed".into(),
            directory.join("Programs/Zed/zed.exe"),
        );
    }
    if let Some(programs) = std::env::var_os("ProgramFiles") {
        let directory = PathBuf::from(programs);
        add(
            &mut found,
            "Docker Desktop".into(),
            directory.join("Docker/Docker/Docker Desktop.exe"),
        );
        add(
            &mut found,
            "PowerShell".into(),
            directory.join("PowerShell/7/pwsh.exe"),
        );
    }
    let mut apps: Vec<_> = found.into_values().collect();
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps
}

fn add(found: &mut BTreeMap<String, InstalledApp>, name: String, executable: PathBuf) {
    if executable.is_file()
        && executable
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        found
            .entry(executable.to_string_lossy().to_lowercase())
            .or_insert_with(|| InstalledApp::desktop(name, executable, String::new()));
    }
}

impl InstalledApp {
    fn desktop(name: String, executable: PathBuf, arguments: String) -> Self {
        let new_console = executable.file_name().is_some_and(|name| {
            matches!(
                name.to_string_lossy().to_ascii_lowercase().as_str(),
                "cmd.exe" | "powershell.exe" | "pwsh.exe"
            )
        });
        Self {
            name,
            executable,
            app_id: None,
            arguments,
            new_console,
        }
    }

    pub fn is_browser(&self) -> bool {
        browser_name(&self.executable).is_some()
    }
}

pub fn browser_name(path: &Path) -> Option<&'static str> {
    match path
        .file_name()?
        .to_string_lossy()
        .to_ascii_lowercase()
        .as_str()
    {
        "chrome.exe" => Some("Google Chrome"),
        "msedge.exe" => Some("Microsoft Edge"),
        _ => None,
    }
}

pub fn parse_arguments(value: &str) -> Result<Vec<String>> {
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    let command = HSTRING::from(format!("jurabox.exe {value}"));
    // SAFETY: CommandLineToArgvW receives a valid UTF-16 string and returns a
    // counted allocation. Copy each string before freeing the allocation once.
    unsafe {
        let mut count = 0;
        let values = CommandLineToArgvW(&command, &mut count);
        if values.is_null() {
            bail!("Cannot parse the app arguments.");
        }
        let result = std::slice::from_raw_parts(values, count as usize)
            .iter()
            .skip(1)
            .map(|value| value.to_string())
            .collect::<std::result::Result<Vec<_>, _>>();
        let _ = LocalFree(HLOCAL(values.cast()));
        Ok(result?)
    }
}

pub fn pick_path(owner: HWND, folder: bool) -> Result<Option<PathBuf>> {
    // The dialog is modal and uses the management window as its owner. Its
    // native modal loop handles interaction while the user chooses a path.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .context("Cannot initialize the Windows path picker.")?;
        struct ComGuard;
        impl Drop for ComGuard {
            fn drop(&mut self) {
                unsafe {
                    CoUninitialize();
                }
            }
        }
        let _com = ComGuard;
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        let options = if folder {
            FOS_FORCEFILESYSTEM | FOS_PICKFOLDERS
        } else {
            FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST
        };
        dialog.SetOptions(options)?;
        if let Err(error) = dialog.Show(owner) {
            if error.code() == windows::core::HRESULT::from_win32(ERROR_CANCELLED.0) {
                return Ok(None);
            }
            return Err(error.into());
        }
        let selected = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        let value = selected.to_string();
        CoTaskMemFree(Some(selected.0.cast()));
        Ok(Some(PathBuf::from(value?)))
    }
}

/// Interactive shells need fresh console handles, rather than inherited or null
/// standard streams from the tray application. Preserve cmd.exe's own argument
/// grammar by passing the configured argument text directly to CreateProcessW.
pub fn start_console(
    executable: &std::path::Path,
    arguments: &str,
    folder: Option<&std::path::Path>,
) -> Result<()> {
    use windows::Win32::{
        Foundation::{BOOL, CloseHandle},
        System::Threading::{
            CREATE_NEW_CONSOLE, CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
        },
    };
    use windows::core::PWSTR;
    // CreateProcess accepts forward slashes, but cmd.exe interprets a slash in
    // its own argv[0] as an option separator and can silently corrupt /c scripts.
    let native_path: PathBuf = executable.components().collect();
    let executable = HSTRING::from(native_path.as_os_str());
    let mut command: Vec<u16> = std::iter::once(b'"' as u16)
        .chain(executable.as_wide().iter().copied())
        .chain([b'"' as u16, b' ' as u16])
        .chain(arguments.encode_utf16())
        .chain(Some(0))
        .collect();
    let folder = folder.map(|path| HSTRING::from(path.as_os_str()));
    let mut process = PROCESS_INFORMATION::default();
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: Input strings/buffers outlive CreateProcessW. No handles are
    // inherited; Windows supplies the console's standard handles. Returned process
    // and thread handles are closed without terminating the user's application.
    unsafe {
        CreateProcessW(
            &executable,
            PWSTR(command.as_mut_ptr()),
            None,
            None,
            BOOL(0),
            CREATE_NEW_CONSOLE,
            None,
            folder
                .as_ref()
                .map_or(PCWSTR::null(), |value| PCWSTR(value.as_ptr())),
            &startup,
            &mut process,
        )?;
        let _ = CloseHandle(process.hThread);
        let _ = CloseHandle(process.hProcess);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_arguments_preserve_spaces_empty_arguments_and_backslashes() {
        assert_eq!(
            parse_arguments(r#"--new-window "D:\My Project" "" plain"#).unwrap(),
            vec!["--new-window", r"D:\My Project", "", "plain"]
        );
        assert!(parse_arguments("").unwrap().is_empty());
    }
}
