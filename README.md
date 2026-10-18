# Jurabox

Jurabox is a Windows app in development for organizing each project's apps, browser links, startup actions, and background services into a workspace. The first release targets Windows 11 and native virtual desktops, with a central management UI and system tray controls.

## Run the project manager

Requires Windows 11, Rust 1.92 or newer with the MSVC toolchain, and Visual Studio C++ Build Tools with a Windows SDK.

```powershell
cargo run
```

The Slint management window supports creating, editing, and deleting saved projects, an optional project folder, and a saved inactivity policy. Closing the window hides it to the system tray. Click the tray icon or choose Show Jurabox to reopen it; use Quit Jurabox to exit. Launching Jurabox again activates the existing instance.

Settings are stored in `%LOCALAPPDATA%\Jurabox\projects.json`. Save operations run on a worker thread and replace the settings file through a temporary file in the same directory. Invalid or unsupported files are reported rather than overwritten. Deleting a saved project leaves its folder and files intact.

This development slice manages saved configuration. App/link launching, Open/Switch workspace controls, service management, and live inactivity timers are still being built. Selecting a project only edits its setup. The saved inactivity setting does not yet stop processes.

To start with only a tray icon:

```powershell
cargo run -- --tray
```

Help > About Jurabox includes Slint attribution. Slint 1.18.1 is used for this initial UI slice; broader toolkit and release performance validation is ongoing.

## Windows desktop compatibility

The pinned `winvd` integration targets Windows 11 24H2 build 26100.2605 or newer; individual builds still need validation.

List desktops without changing them:

```powershell
cargo run -- status
```

Run the desktop exercise from an unlocked, interactive Windows session:

```powershell
cargo run -- desktop-check --exercise
```

The exercise creates and names a temporary desktop, opens a Jurabox-owned test window, moves that same window between desktops, verifies desktop switch notifications, and returns to the starting desktop. It then closes its test window and removes only its own temporary desktop. It does not select an editor, browser, or another existing app for testing. If cleanup fails, the error is reported for manual resolution.

The desktop exercise uses a separate Win32 diagnostic window. A passing check establishes compatibility for that Windows build and the owned test window; it does not prove that every external app can be moved or that future Windows updates will remain compatible.

The basic exercise passed on Windows 11 25H2 build 26200.9711 on 2026-10-18. See [the validation record](docs/PROGRESS.md) for the checked behavior and remaining work.

## Development checks

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -- ui-check
```

Unit tests do not open windows or switch desktops. The live UI check uses temporary settings, exercises project save/error/tray flows, and writes screenshots to `target/ui-check/`. The live checks require an unlocked interactive Windows session; the desktop exercise runs separately.

## Design documents

- [Project plan](docs/PLAN.md)
- [Accepted design decisions](docs/DESIGN-QUESTIONS.md)
- [Domain glossary](CONTEXT.md)
- [Windows desktop architecture](docs/adr/0001-use-windows-virtual-desktops.md)
- [Rust application architecture](docs/adr/0002-rust-application-with-platform-modules.md)
- [Implementation progress and validation](docs/PROGRESS.md)
