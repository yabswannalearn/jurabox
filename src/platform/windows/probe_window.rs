use anyhow::{Context, Result, bail};
use std::{
    sync::mpsc,
    thread::{self, JoinHandle},
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
            GetMessageW, GetWindowThreadProcessId, IsWindow, MSG, PostMessageW, PostQuitMessage,
            RegisterClassW, TranslateMessage, WINDOW_EX_STYLE, WM_CLOSE, WM_DESTROY, WNDCLASSW,
            WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        },
    },
    core::w,
};

/// The helper owns only its own window; no user app is selected or closed.
pub struct ProbeWindow {
    hwnd: HWND,
    owner_thread: u32,
    thread: Option<JoinHandle<Result<(), String>>>,
}

impl ProbeWindow {
    pub fn create() -> Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            let result = window_loop(&sender);
            if let Err(error) = &result {
                let _ = sender.send(Err(error.clone()));
            }
            result
        });
        match receiver
            .recv()
            .context("test window thread stopped before creating its window")?
        {
            Ok(handle) => {
                let hwnd = HWND(handle as *mut _);
                // SAFETY: The helper thread just created this window. Windows returns
                // its owning thread ID; no application memory is borrowed or retained.
                let owner_thread = unsafe { GetWindowThreadProcessId(hwnd, None) };
                Ok(Self {
                    hwnd,
                    owner_thread,
                    thread: Some(thread),
                })
            }
            Err(error) => {
                let _ = thread.join();
                bail!("cannot create test window: {error}")
            }
        }
    }

    pub fn handle(&self) -> HWND {
        self.hwnd
    }

    pub fn is_alive(&self) -> bool {
        let mut process_id = 0;
        // SAFETY: These queries accept a window handle. The output pointer refers to
        // a live stack variable. Checking owner IDs also rejects a recycled handle.
        unsafe {
            IsWindow(self.hwnd).as_bool()
                && GetWindowThreadProcessId(self.hwnd, Some(&mut process_id)) == self.owner_thread
                && process_id == std::process::id()
        }
    }

    pub fn close(&mut self) -> Result<()> {
        if let Some(thread) = self.thread.take() {
            if self.is_alive() {
                // SAFETY: This is the helper's own window, with a live message loop.
                if let Err(error) =
                    unsafe { PostMessageW(self.hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)) }
                {
                    self.thread = Some(thread);
                    return Err(error).context("cannot close the owned test window");
                }
            }
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("test window thread panicked"))?
                .map_err(anyhow::Error::msg)?;
        }
        Ok(())
    }
}

impl Drop for ProbeWindow {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            eprintln!("Test window cleanup failed: {error:#}");
        }
    }
}

fn window_loop(sender: &mpsc::SyncSender<Result<isize, String>>) -> Result<(), String> {
    // SAFETY: All window creation, dispatch, and destruction happen on this thread.
    // The class name and title are static UTF-16 strings, and no borrowed data is
    // passed through the window procedure or retained by Windows.
    unsafe {
        let instance = GetModuleHandleW(None).map_err(|error| error.to_string())?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: w!("JuraboxCompatibilityProbe"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_win32().to_string());
        }
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class.lpszClassName,
            w!("Jurabox compatibility check - temporary test window"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            600,
            180,
            None,
            None,
            instance,
            None,
        )
        .map_err(|error| error.to_string())?;
        if sender.send(Ok(hwnd.0 as isize)).is_err() {
            let _ = DestroyWindow(hwnd);
            return Err("test window receiver disconnected".into());
        }
        let mut message = MSG::default();
        loop {
            match GetMessageW(&mut message, None, 0, 0).0 {
                -1 => {
                    let error = windows::core::Error::from_win32().to_string();
                    let _ = DestroyWindow(hwnd);
                    return Err(error);
                }
                0 => return Ok(()),
                _ => {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: Windows invokes this procedure for a window owned by this thread.
    unsafe {
        match message {
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}
