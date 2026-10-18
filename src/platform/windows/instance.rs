use anyhow::{Context, Result};
use slint::Weak;
use std::{
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    thread::JoinHandle,
};
use windows::{
    Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0},
        System::Threading::{
            CreateEventW, CreateMutexW, INFINITE, SetEvent, WaitForMultipleObjects,
        },
    },
    core::HSTRING,
};

pub struct ManagerInstance {
    _mutex: OwnedHandle,
    show: OwnedHandle,
    stop: OwnedHandle,
    thread: Option<JoinHandle<()>>,
}

impl ManagerInstance {
    pub fn acquire() -> Result<Option<Self>> {
        Self::acquire_named("Jurabox.Manager.v1")
    }

    pub(crate) fn acquire_named(name: &str) -> Result<Option<Self>> {
        // SAFETY: Owned names, default per-user security, and owned handles retained
        // until after the waiting thread is stopped. No application pointers are shared.
        unsafe {
            let event_name = HSTRING::from(format!("Local\\{name}.Show"));
            let mutex_name = HSTRING::from(format!("Local\\{name}"));
            let show = CreateEventW(None, false, false, &event_name)
                .context("Cannot create the Jurabox activation event.")?;
            let show = OwnedHandle::from_raw_handle(show.0);
            let mutex = CreateMutexW(None, false, &mutex_name);
            // Read last-error before dropping name storage or calling another API.
            let already_running = GetLastError() == ERROR_ALREADY_EXISTS;
            let mutex = mutex.context("Cannot create the Jurabox instance guard.")?;
            let mutex = OwnedHandle::from_raw_handle(mutex.0);
            if already_running {
                SetEvent(HANDLE(show.as_raw_handle()))?;
                return Ok(None);
            }
            let stop = CreateEventW(None, true, false, None)?;
            Ok(Some(Self {
                _mutex: mutex,
                show,
                stop: OwnedHandle::from_raw_handle(stop.0),
                thread: None,
            }))
        }
    }

    pub fn listen(&mut self, window: Weak<crate::ui::AppWindow>) {
        let show = self.show.as_raw_handle() as usize;
        let stop = self.stop.as_raw_handle() as usize;
        self.thread = Some(std::thread::spawn(move || {
            let handles = [HANDLE(stop as *mut _), HANDLE(show as *mut _)];
            loop {
                // SAFETY: ManagerInstance owns both handles and joins this thread
                // before closing them. The wait blocks without polling while idle.
                match unsafe { WaitForMultipleObjects(&handles, false, INFINITE) } {
                    result if result == WAIT_OBJECT_0 => break,
                    result if result.0 == WAIT_OBJECT_0.0 + 1 => {
                        let _ = window.upgrade_in_event_loop(|window| crate::ui::show(&window));
                    }
                    _ => break,
                }
            }
        }));
    }
}

impl Drop for ManagerInstance {
    fn drop(&mut self) {
        // SAFETY: The stop handle is valid until this destructor returns.
        let _ = unsafe { SetEvent(HANDLE(self.stop.as_raw_handle())) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
