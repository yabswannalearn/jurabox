use anyhow::{Context, Result};
use std::{collections::HashSet, path::Path};
use windows::{
    Win32::{
        Foundation::{BOOL, CloseHandle, FILETIME, HWND, LPARAM},
        System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GW_OWNER, GWL_EXSTYLE, GetClassNameW, GetShellWindow, GetWindow,
            GetWindowLongW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
            WS_EX_TOOLWINDOW,
        },
    },
    core::PWSTR,
};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppWindow {
    pub handle: isize,
    pub process: u32,
    thread: u32,
    created: u64,
    pub executable: String,
}

pub fn path_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('/', "\\")
        .to_lowercase()
}

impl AppWindow {
    pub fn hwnd(&self) -> HWND {
        HWND(self.handle as *mut _)
    }

    pub fn is_alive(&self) -> bool {
        inspect(self.hwnd()).is_some_and(|window| window == *self)
    }

    pub fn title(&self) -> String {
        let mut title = [0u16; 1024];
        // SAFETY: Windows copies a bounded caption into our local buffer. Titles
        // are display metadata, never part of the process/window identity check.
        let length = unsafe { GetWindowTextW(self.hwnd(), &mut title) } as usize;
        if length == 0 {
            "Untitled window".into()
        } else {
            String::from_utf16_lossy(&title[..length])
        }
    }
}

fn inspect(hwnd: HWND) -> Option<AppWindow> {
    // SAFETY: Only metadata is queried. The process handle has query-only rights,
    // all buffers are local, and the handle is closed on every result path.
    unsafe {
        let mut process = 0;
        let thread = GetWindowThreadProcessId(hwnd, Some(&mut process));
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process).ok()?;
        let result = (|| {
            let mut name = vec![0u16; 32768];
            let mut length = name.len() as u32;
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_FORMAT(0),
                PWSTR(name.as_mut_ptr()),
                &mut length,
            )
            .ok()?;
            let mut created = FILETIME::default();
            let mut exited = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user).ok()?;
            Some(AppWindow {
                handle: hwnd.0 as isize,
                process,
                thread,
                created: ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64,
                executable: path_key(Path::new(&String::from_utf16_lossy(
                    &name[..length as usize],
                ))),
            })
        })();
        let _ = CloseHandle(handle);
        result
    }
}

unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: EnumWindows invokes this synchronously with the pointer to our local
    // vector. No pointer is retained. Only visible, unowned application windows qualify.
    unsafe {
        if IsWindowVisible(hwnd).as_bool()
            && hwnd != GetShellWindow()
            && GetWindow(hwnd, GW_OWNER).is_err()
            && GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 == 0
            && !shell_surface(hwnd)
            && let Some(window) = inspect(hwnd)
        {
            (&mut *(data.0 as *mut Vec<AppWindow>)).push(window);
        }
    }
    BOOL(1)
}

unsafe fn shell_surface(hwnd: HWND) -> bool {
    let mut class = [0u16; 256];
    // SAFETY: This is a bounded class-name query; no window messages are sent.
    let length = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
    matches!(
        String::from_utf16_lossy(&class[..length]).as_str(),
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
    )
}

pub fn all() -> Result<Vec<AppWindow>> {
    let mut result = Vec::new();
    // SAFETY: The vector outlives the synchronous callback invocation.
    unsafe {
        EnumWindows(
            Some(collect),
            LPARAM(&mut result as *mut Vec<AppWindow> as isize),
        )
    }
    .context("Cannot inspect application windows.")?;
    Ok(result)
}

unsafe extern "system" fn collect_handle(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: EnumWindows calls synchronously with our local set. Include hidden
    // windows so showing an existing window cannot be mistaken for creating one.
    unsafe {
        (&mut *(data.0 as *mut HashSet<isize>)).insert(hwnd.0 as isize);
    }
    BOOL(1)
}

pub fn handles() -> Result<HashSet<isize>> {
    let mut result = HashSet::new();
    // SAFETY: The set outlives every synchronous callback and contains only values.
    unsafe {
        EnumWindows(
            Some(collect_handle),
            LPARAM(&mut result as *mut HashSet<isize> as isize),
        )
    }
    .context("Cannot inspect existing window identities.")?;
    Ok(result)
}
