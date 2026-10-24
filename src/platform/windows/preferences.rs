use anyhow::{Context, Result};
use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HWND},
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
            RegCreateKeyExW, RegDeleteValueW, RegSetValueExW,
        },
        UI::{
            Shell::{NIF_INFO, NIIF_WARNING, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW},
            WindowsAndMessaging::{FindWindowExW, GetWindowThreadProcessId, HWND_MESSAGE},
        },
    },
    core::w,
};

pub fn startup(enabled: bool) -> Result<()> {
    // SAFETY: Only Jurabox's own HKCU Run value is edited; the handle is closed.
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            0,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()?;
        let result: Result<()> = (|| {
            if enabled {
                let command = format!("\"{}\" --tray", std::env::current_exe()?.display());
                let bytes: Vec<u8> = command
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect();
                RegSetValueExW(key, w!("Jurabox"), 0, REG_SZ, Some(&bytes)).ok()?;
            } else {
                let result = RegDeleteValueW(key, w!("Jurabox"));
                if result != ERROR_FILE_NOT_FOUND && result != ERROR_SUCCESS {
                    result.ok()?;
                }
            }
            Ok(())
        })();
        let _ = RegCloseKey(key);
        result.context("Cannot update Start with Windows.")
    }
}

/// Slint 1.18.1's pinned Windows tray adapter owns this local message window
/// and icon ID. Failure to find it simply leaves the in-app error visible.
pub fn notify_failure(message: &str) {
    unsafe {
        let mut after = HWND::default();
        while let Ok(hwnd) = FindWindowExW(HWND_MESSAGE, after, w!("SlintSystemTrayWindow"), None) {
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == std::process::id() {
                let mut data = NOTIFYICONDATAW {
                    cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                    hWnd: hwnd,
                    uID: 1,
                    uFlags: NIF_INFO,
                    dwInfoFlags: NIIF_WARNING,
                    ..Default::default()
                };
                copy_text(&mut data.szInfoTitle, "Jurabox needs attention");
                copy_text(&mut data.szInfo, message);
                let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
                break;
            }
            after = hwnd;
        }
    }
}

fn copy_text<const N: usize>(buffer: &mut [u16; N], text: &str) {
    let mut index = 0;
    for ch in text.chars() {
        let mut encoded = [0; 2];
        let value = ch.encode_utf16(&mut encoded);
        if index + value.len() >= N {
            break;
        }
        buffer[index..index + value.len()].copy_from_slice(value);
        index += value.len();
    }
}
