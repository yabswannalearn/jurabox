mod app_windows;
pub mod apps;
mod fixture;
mod instance;
mod probe_window;
pub use fixture::OwnedProbe;
pub mod workspace;
pub use instance::ManagerInstance;

pub fn probe_window() -> anyhow::Result<()> {
    let window = probe_window::ProbeWindow::create()?;
    while window.is_alive() {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(())
}

pub fn workspace_check() -> anyhow::Result<()> {
    workspace::check()
}

pub fn activate_management_window(window: &slint::Window) -> anyhow::Result<()> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle();
    if let RawWindowHandle::Win32(handle) = handle.window_handle()?.as_raw() {
        let hwnd = windows::Win32::Foundation::HWND(handle.hwnd.get() as *mut _);
        // Only the management window's own handle is passed to this function.
        // A desktop integration failure must not prevent showing the configuration UI.
        if let Ok(desktop) = winvd::get_current_desktop() {
            let _ = winvd::move_window_to_desktop(desktop, &hwnd);
        }
        // SAFETY: The handle was obtained from this live Slint window. Windows
        // decides whether foreground activation is allowed; no focus rules are bypassed.
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{
                SW_RESTORE, SetForegroundWindow, ShowWindow,
            };
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }
    Ok(())
}

use anyhow::{Context, Result, bail, ensure};
use probe_window::ProbeWindow;
use std::{
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};
use windows::{Win32::Foundation::HWND, core::GUID};
use winvd::{Desktop, DesktopEvent};

const WAIT: Duration = Duration::from_secs(5);

pub fn status() -> Result<()> {
    let current =
        current_id().context("desktop integration unavailable: cannot read the current desktop")?;
    let desktops = vd(winvd::get_desktops()).context("cannot enumerate desktops")?;
    println!("Jurabox {} | winvd 0.0.49", env!("CARGO_PKG_VERSION"));
    println!(
        "Desktop integration available; {} desktop(s).",
        desktops.len()
    );
    for desktop in desktops {
        let id = vd(desktop.get_id())?;
        println!(
            "{} {} | {:?} | {}",
            if id == current { "*" } else { " " },
            vd(desktop.get_index())? + 1,
            id,
            vd(desktop.get_name())?
        );
    }
    Ok(())
}

// winvd::Error does not implement std::error::Error. Preserve its diagnostic
// variant at the boundary rather than leaking that limitation through the tool.
fn vd<T>(result: winvd::Result<T>) -> Result<T> {
    result.map_err(|error| anyhow::anyhow!("Windows desktop API: {error:?}"))
}

fn current_id() -> Result<GUID> {
    vd(vd(winvd::get_current_desktop())?.get_id())
}

fn window_desktop_id(hwnd: HWND) -> Result<GUID> {
    vd(vd(winvd::get_desktop_by_window(hwnd))?.get_id())
}

fn desktop_ids() -> Result<Vec<GUID>> {
    vd(winvd::get_desktops())?
        .into_iter()
        .map(|desktop| vd(desktop.get_id()))
        .collect()
}

fn wait_until(description: &str, mut condition: impl FnMut() -> Result<bool>) -> Result<()> {
    let deadline = Instant::now() + WAIT;
    loop {
        if condition()? {
            return Ok(());
        }
        ensure!(Instant::now() < deadline, "timed out: {description}");
        thread::sleep(Duration::from_millis(25));
    }
}

fn wait_for_change(events: &Receiver<DesktopEvent>, expected: GUID) -> Result<()> {
    let deadline = Instant::now() + WAIT;
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        let event = events
            .recv_timeout(remaining)
            .context("desktop switch notification was not received")?;
        if let DesktopEvent::DesktopChanged { new, .. } = event
            && vd(new.get_id())? == expected
        {
            return Ok(());
        }
    }
    bail!("timed out waiting for a desktop switch notification")
}

/// All mutations are limited to a desktop and window created by this exercise.
struct Exercise {
    original: Desktop,
    temporary: Option<Desktop>,
    window: Option<ProbeWindow>,
}

impl Exercise {
    fn cleanup(&mut self) -> Result<()> {
        // Close the owned window even when desktop integration has become unavailable.
        if let Some(mut window) = self.window.take() {
            window.close()?;
        }
        if let Some(temporary) = self.temporary {
            let original_id = vd(self.original.get_id())?;
            let temporary_id = vd(temporary.get_id())?;
            let ids = desktop_ids()?;
            if !ids.contains(&temporary_id) {
                self.temporary = None;
                return Ok(());
            }
            ensure!(
                ids.contains(&original_id),
                "starting desktop disappeared; temporary desktop retained for manual cleanup"
            );
            if current_id()? == temporary_id {
                vd(winvd::switch_desktop(self.original))?;
                wait_until("restore the starting desktop", || {
                    Ok(current_id()? == original_id)
                })?;
            }
            // Use the created desktop's identity, never an index or a user's desktop name.
            vd(winvd::remove_desktop(temporary, self.original))?;
            wait_until("remove the temporary desktop", || {
                Ok(!desktop_ids()?.contains(&temporary_id))
            })?;
            self.temporary = None;
        }
        Ok(())
    }
}

impl Drop for Exercise {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            eprintln!("Cleanup needs attention: {error:#}");
        }
    }
}

pub fn desktop_check() -> Result<()> {
    let baseline = desktop_ids().context("desktop integration unavailable")?;
    let original = vd(winvd::get_current_desktop())?;
    let original_id = vd(original.get_id())?;
    println!(
        "Starting desktop: {original_id:?}; {} desktop(s).",
        baseline.len()
    );
    println!("Exercising a temporary desktop and a Jurabox-owned window...");

    let (sender, events) = mpsc::channel::<DesktopEvent>();
    let _listener = vd(winvd::listen_desktop_events(sender))
        .context("cannot start desktop event notifications")?;
    let mut exercise = Exercise {
        original,
        temporary: None,
        window: None,
    };

    let result = (|| -> Result<()> {
        let temporary = vd(winvd::create_desktop()).context("cannot create a desktop")?;
        let temporary_id = vd(temporary.get_id())?;
        ensure!(
            !baseline.contains(&temporary_id),
            "create returned an existing desktop; refusing to modify it"
        );
        exercise.temporary = Some(temporary);
        println!("PASS create: {temporary_id:?}");

        // Listener startup is asynchronous. Observe a change on our own desktop before
        // relying on switch notifications; do not assume the returned handle is ready.
        let deadline = Instant::now() + WAIT;
        let mut attempt = 0;
        loop {
            attempt += 1;
            let name = format!(
                "Jurabox compatibility check {}-{attempt}",
                std::process::id()
            );
            vd(temporary.set_name(&name))?;
            if let Ok(DesktopEvent::DesktopNameChanged(desktop, _)) =
                events.recv_timeout(Duration::from_millis(150))
                && vd(desktop.get_id())? == temporary_id
            {
                ensure!(
                    vd(temporary.get_name())? == name,
                    "desktop rename did not persist"
                );
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "desktop event listener did not become ready"
            );
        }
        println!("PASS rename and desktop notifications");

        let window = ProbeWindow::create()?;
        let hwnd = window.handle();
        exercise.window = Some(window);
        wait_until("test window is visible to the shell", || {
            Ok(window_desktop_id(hwnd).is_ok_and(|id| id == original_id))
        })?;

        vd(winvd::move_window_to_desktop(temporary, &hwnd))
            .context("cannot move the owned test window")?;
        wait_until("move the test window", || {
            Ok(window_desktop_id(hwnd)? == temporary_id)
        })?;
        println!("PASS move owned window to temporary desktop");

        vd(winvd::switch_desktop(temporary)).context("cannot switch to the temporary desktop")?;
        wait_until("switch to the temporary desktop", || {
            Ok(current_id()? == temporary_id)
        })?;
        wait_for_change(&events, temporary_id)?;
        println!("PASS switch to temporary desktop and receive event");

        vd(winvd::move_window_to_desktop(original, &hwnd))?;
        wait_until("move the shared test window back", || {
            Ok(window_desktop_id(hwnd)? == original_id)
        })?;
        ensure!(
            exercise.window.as_ref().unwrap().is_alive(),
            "moving the window destroyed its running instance"
        );
        println!("PASS move same running window back to starting desktop");

        vd(winvd::switch_desktop(original))?;
        wait_until("return to the starting desktop", || {
            Ok(current_id()? == original_id)
        })?;
        wait_for_change(&events, original_id)?;
        println!("PASS return to starting desktop and receive event");
        Ok(())
    })();

    let cleanup = exercise.cleanup();
    if let Err(error) = cleanup {
        return Err(error).context(format!("cleanup incomplete; exercise result: {result:?}"));
    }
    println!("PASS close owned window and remove temporary desktop");
    result?;
    ensure!(
        desktop_ids()? == baseline,
        "desktop list changed during the check; other desktops were not modified"
    );
    ensure!(
        current_id()? == original_id,
        "active desktop changed during the check"
    );
    println!("PASS original desktop list and active desktop restored");
    println!("Desktop compatibility check passed.");
    Ok(())
}
