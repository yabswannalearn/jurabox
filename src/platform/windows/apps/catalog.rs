use super::{InstalledApp, registry_apps};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, path::PathBuf};
use windows::{
    Win32::{
        Foundation::RPC_E_CHANGED_MODE,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize, IPersistFile, STGM_READ,
        },
        UI::Shell::{
            AO_NOERRORUI, ApplicationActivationManager, BHID_EnumItems, FOLDERID_AppsFolder,
            IApplicationActivationManager, IEnumShellItems, IShellItem, IShellItem2, IShellLinkW,
            KF_FLAG_DEFAULT,
            PropertiesSystem::{PROPERTYKEY, PSGetPropertyKeyFromName},
            SHGetKnownFolderItem, SIGDN_NORMALDISPLAY, ShellLink,
        },
    },
    core::{HSTRING, Interface, PWSTR},
};

struct Apartment(bool);
impl Apartment {
    fn new() -> Result<Self> {
        // Desktop integration may already have entered the MTA. Keep that
        // apartment; only balance COM initializations owned by this guard.
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result == RPC_E_CHANGED_MODE {
            return Ok(Self(false));
        }
        result.ok()?;
        Ok(Self(true))
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

fn owned_string(value: PWSTR) -> Result<String> {
    // Shell strings are copied before releasing their COM task allocation.
    let result = unsafe { value.to_string() };
    unsafe {
        CoTaskMemFree(Some(value.0.cast()));
    }
    Ok(result?)
}

fn key(name: &str) -> Result<PROPERTYKEY> {
    let mut value = PROPERTYKEY::default();
    unsafe {
        PSGetPropertyKeyFromName(&HSTRING::from(name), &mut value)?;
    }
    Ok(value)
}

fn property(item: &IShellItem2, key: &PROPERTYKEY) -> Option<String> {
    owned_string(unsafe { item.GetString(key).ok()? }).ok()
}

fn shell_apps() -> Result<Vec<InstalledApp>> {
    let _com = Apartment::new()?;
    // AppsFolder is Windows' installed-app catalog, including Start Menu and
    // packaged apps. This enumerates metadata only; no app or script is run.
    let folder: IShellItem =
        unsafe { SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)? };
    let items: IEnumShellItems = unsafe { folder.BindToHandler(None, &BHID_EnumItems)? };
    let app_id_key = key("System.AppUserModel.ID")?;
    let target_key = key("System.Link.TargetParsingPath")?;
    let arguments_key = key("System.Link.Arguments")?;
    let mut result = Vec::new();
    for _ in 0..4096 {
        let mut next = [None];
        unsafe {
            items.Next(&mut next, None)?;
        }
        let Some(item) = next[0].take() else {
            break;
        };
        let Ok(name) = (unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY) })
            .map_err(anyhow::Error::from)
            .and_then(owned_string)
        else {
            continue;
        };
        let Ok(item) = item.cast::<IShellItem2>() else {
            continue;
        };
        let app_id = property(&item, &app_id_key).filter(|value| value.contains('!'));
        if let Some(app_id) = app_id {
            result.push(InstalledApp {
                name,
                executable: PathBuf::new(),
                app_id: Some(app_id),
                arguments: String::new(),
                new_console: false,
            });
        } else if let Some(target) = property(&item, &target_key) {
            let executable = PathBuf::from(target.trim_matches('"'));
            if executable.is_file()
                && executable
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
            {
                result.push(InstalledApp::desktop(
                    name,
                    executable,
                    property(&item, &arguments_key).unwrap_or_default(),
                ));
            }
        }
    }
    Ok(result)
}

pub fn installed_apps() -> Vec<InstalledApp> {
    let shell = shell_apps().unwrap_or_else(|error| {
        eprintln!("Windows app catalog unavailable; using App Paths: {error:#}");
        Vec::new()
    });
    let mut found = BTreeMap::new();
    let packaged_names: Vec<_> = shell
        .iter()
        .filter(|app| app.app_id.is_some())
        .map(|app| app.name.to_lowercase())
        .collect();
    for app in shell.into_iter().chain(registry_apps()) {
        // Packaged desktop apps can appear twice. Prefer the stable Windows ID
        // over an executable path containing a changing package version.
        if app.app_id.is_none()
            && app
                .executable
                .to_string_lossy()
                .to_lowercase()
                .contains("\\windowsapps\\")
            && packaged_names.contains(&app.name.to_lowercase())
        {
            continue;
        }
        let identity = app.app_id.clone().unwrap_or_else(|| {
            format!(
                "{}\0{}",
                super::super::app_windows::path_key(&app.executable),
                app.arguments
            )
        });
        found.entry(identity).or_insert(app);
    }
    let mut apps: Vec<_> = found.into_values().collect();
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps
}

/// Accept Explorer's quoted "Copy as path" text and ordinary .lnk shortcuts.
/// Read shortcut metadata without Resolve, execution, network search or updates.
pub fn resolve_app(value: &str) -> Result<InstalledApp> {
    let path = PathBuf::from(value.trim().trim_matches('"'));
    ensure!(
        path.is_absolute() && path.is_file(),
        "Choose an installed app, or browse for an app or shortcut."
    );
    let name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
    {
        let _com = Apartment::new()?;
        let link: IShellLinkW =
            unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)? };
        let file: IPersistFile = link.cast()?;
        let mut target = vec![0; 32768];
        let mut arguments = vec![0; 32768];
        unsafe {
            file.Load(&HSTRING::from(path.as_os_str()), STGM_READ)?;
            link.GetPath(&mut target, std::ptr::null_mut(), 0)?;
            link.GetArguments(&mut arguments)?;
        }
        let target = PathBuf::from(wide_text(&target));
        ensure!(
            target.is_file()
                && target
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe")),
            "This shortcut cannot be used here. Select the app from Add app instead."
        );
        return Ok(InstalledApp::desktop(name, target, wide_text(&arguments)));
    }
    ensure!(
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe")),
        "Choose an app (.exe) or a Start Menu shortcut (.lnk)."
    );
    Ok(InstalledApp::desktop(name, path, String::new()))
}

fn wide_text(value: &[u16]) -> String {
    String::from_utf16_lossy(
        &value[..value
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(value.len())],
    )
}

pub fn activate(app_id: &str, arguments: &str) -> Result<()> {
    let _com = Apartment::new()?;
    // Let Windows activate the registered package; never store its versioned
    // installation path. The returned PID is not service ownership evidence.
    let manager: IApplicationActivationManager =
        unsafe { CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_INPROC_SERVER)? };
    unsafe {
        manager.ActivateApplication(
            &HSTRING::from(app_id),
            &HSTRING::from(arguments),
            AO_NOERRORUI,
        )
    }
    .context("Windows could not open this app. Check that it is still installed.")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_windows_app_reports_activation_failure() {
        assert!(activate("Jurabox.NonexistentPackage_123!Missing", "").is_err());
    }

    #[test]
    fn activation_preserves_an_existing_multithreaded_com_apartment() {
        use windows::Win32::System::Com::COINIT_MULTITHREADED;
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
        }
        let result = activate("Jurabox.NonexistentPackage_123!Missing", "").unwrap_err();
        assert!(
            result
                .to_string()
                .contains("Windows could not open this app")
        );
        unsafe {
            CoUninitialize();
        }
    }

    #[test]
    fn quoted_shortcuts_resolve_without_launching_and_preserve_arguments() {
        let directory = tempfile::tempdir().unwrap();
        let shortcut = directory.path().join("Example editor.lnk");
        let executable = std::env::current_exe().unwrap();
        let arguments = r#"--new-window "D:\Project folder""#;
        {
            let _com = Apartment::new().unwrap();
            let link: IShellLinkW =
                unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap() };
            unsafe {
                link.SetPath(&HSTRING::from(executable.as_os_str()))
                    .unwrap();
                link.SetArguments(&HSTRING::from(arguments)).unwrap();
                link.cast::<IPersistFile>()
                    .unwrap()
                    .Save(&HSTRING::from(shortcut.as_os_str()), true)
                    .unwrap();
            }
        }
        let app = resolve_app(&format!("\"{}\"", shortcut.display())).unwrap();
        assert_eq!(app.name, "Example editor");
        assert_eq!(app.executable, executable);
        assert_eq!(app.arguments, arguments);
        assert!(!app.new_console);
        assert!(resolve_app("relative.exe").is_err());
        std::fs::write(&shortcut, "broken shortcut").unwrap();
        assert!(resolve_app(&shortcut.to_string_lossy()).is_err());
    }
}
