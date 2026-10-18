# Jurabox

Jurabox is a Windows app in development for organizing each project's apps, browser links, startup actions, and background services into a workspace. The first release targets Windows 11 and native virtual desktops, with a central management UI and system tray controls.

## Run the project manager

Requires Windows 11, Rust 1.92 or newer with the MSVC toolchain, and Visual Studio C++ Build Tools with a Windows SDK.

```powershell
cargo run
```

The Slint management window supports saved projects, an optional folder, app entries, browser links, and a saved inactivity policy. Closing the window hides it to the system tray. Click the tray icon or choose Show Jurabox to reopen it; use Quit Jurabox to exit. Launching Jurabox again activates the existing instance.

Settings are stored in `%LOCALAPPDATA%\Jurabox\projects.json`. Save operations run on a worker thread and replace the settings file through a temporary file in the same directory. Invalid or unsupported files are reported rather than overwritten. Deleting a saved project leaves its folder and files intact.

Selecting a project edits its setup. Save it, then choose **Open project** to create a Windows desktop and launch its apps. **Switch to project** returns to an existing session without reopening closed apps. **Relaunch saved setup** applies later saved changes and reuses unchanged verified app windows. **Close session** leaves app windows and the desktop open; end the session before deleting the saved entry. Desktop cleanup is manual.

Use **Find installed app** to search Windows App Paths and common tools, or **Custom app** with Browse for another `.exe`. Optional arguments use Windows quoting; `"{folder}"` passes the project folder as one argument. Enable the console option for interactive CMD/PowerShell entries. Scripts can be launched through an explicitly configured shell; these GUI/terminal entries have no managed service shutdown behavior.

The browser option opens configured HTTP(S) links in a new Chrome or Edge window using the existing profile. **Shared app** explicitly permits reuse of one matching existing window and moves it with the active project. Multiple matching windows are reported as ambiguous. Otherwise, only a unique new window matching the executable is assigned. If an app delegates to another executable, takes longer than five seconds, or reuses a previous window, it may launch without assignment: check it before explicitly relaunching. Packaged apps, terminal hosts, and application-specific adapters still need pilot validation.

Live sessions currently last only while Jurabox runs. Reopening Jurabox restores saved settings, but does not reconnect to surviving app windows or workloads yet. Command services, startup actions, startup readiness, and dependencies are available in this development slice; Compose, live inactivity timers, and recovery remain later work. The saved inactivity setting does not yet stop processes.

For an open project, **Workspace windows** shows the apps from its session setup. Use **Assign open window** or **Change window** to search accessible windows by title or executable path, then choose **Use window**. This also supports apps whose launcher and visible window belong to different executables. Assignment takes effect immediately and leaves launch settings unchanged. **Unassign** leaves the app open. Windows privately assigned to another entry are unavailable; enable Shared app in both saved setups and relaunch to link one window across sessions.

Under **Services and startup actions**, add a foreground command service or a finite startup action. Choose an executable and arguments, and set a working folder or use the project's folder. Commands run without an interactive terminal, inherit your environment, and support `{folder}` in arguments. Run scripts through an explicitly configured shell. A completed startup action is successful work; an exited long-running service is reported as a failure. Detached container workloads need the later Compose integration.

Choose **Process running**, a **TCP IP address and port**, or an **HTTP(S) health URL** for service startup readiness. HTTP requires a 2xx response and uses a direct GET without redirects, authentication, or cookies. The default readiness deadline is 60 seconds after the command starts. A timeout shows **Not ready** and leaves the workload running; **Retry** checks that same instance. Readiness is a startup gate, not continuous health monitoring. An endpoint already responding before launch reports a conflict without starting a command. This is a best-effort check, not proof of endpoint ownership.

Select other services/actions under **Wait for prerequisites**. Dependents wait until services are ready and finite actions finish successfully; waiting does not consume their own readiness deadline. Invalid or cyclic dependencies are rejected on Save. Failed prerequisites require explicit Retry. A later prerequisite failure is reported on running dependents without stopping them. Normal shutdown stops dependents first, keeps prerequisites alive if a dependent cannot stop, and still stops independent workloads. A single prerequisite's Stop is blocked while dependent workloads are live; explicit Force stop remains available.

**Session services** shows status and recent output, retaining 64 KiB per command stream. **Start/Retry** uses the session setup and avoids duplicating a live workload, including child processes that survive their launcher. **Stop** runs the configured normal-stop command with a default 10-second deadline. A missing/failed stop command or timeout leaves the workload managed and running. **Force stop** explicitly terminates that workload and its owned children. Close and relaunch pause on stop failures; saving a different stop command does not change the current session's instructions.

Choose **Stop services and quit** in File or the tray menu to shut down managed workloads and exit. Any stop failure keeps Jurabox open. Ordinary Quit is guarded while jobs are live in this slice; a leave-running exit needs the later durable-output/reconnection work. Closing the window still hides it to the tray. Output is in memory and is not preserved after quitting. Settings versions 1–3 migrate to version 4 in memory and are rewritten only on Save. Existing workloads keep process-running readiness with no dependencies.

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
cargo run -- workspace-check --exercise
cargo run -- assignment-check --exercise
cargo run -- service-check --exercise
cargo run -- service-ui-check --exercise
```

Unit tests do not open windows or switch desktops. The live UI check uses temporary settings, exercises project/app/link forms and save/error/tray flows, and writes screenshots to `target/ui-check/`. The workspace exercise launches separate Jurabox test processes, verifies assignment, shared movement, session semantics, desktop deletion/recreation, and partial failure. It restores the original desktops and closes only its test windows. The live checks require an unlocked interactive Windows session.

To include an installed Chrome/Edge browser in the workspace exercise, set `JURABOX_CHECK_BROWSER` to its full executable path for that command. The check serves two temporary loopback HTTP pages, verifies the new browser window and reuse, and closes that window during cleanup. It uses the existing browser profile. This optional check passed with Edge 155.0.4283.45 on the development host.

The assignment exercise opens a temporary project and private test-app copy, checks the real picker callbacks, search keyboard focus, assignment/relaunch/unassign, cancellation during discovery, and closed-window refresh. It restores the original desktops and closes its own fixture. The workspace exercise also checks assignment and private/shared conflict rules. Set `JURABOX_CHECK_CONSOLE=1` to include a real CMD/terminal-host window; that fixture command exits by itself after 20 seconds and only a newly created, uniquely titled window can be selected by the exercise.

The service exercises use private temporary settings/folders and finite diagnostic commands, with no process-name or port-based selection. They check startup/exit semantics, output limits, normal-stop failures, duplicate prevention, explicit owned-job Force stop, TCP/HTTP readiness, dependency waits, stale probe cancellation, and reverse shutdown. The UI exercise also checks readiness/dependency forms, cycle and removal errors, Waiting/Not ready states, and Retry, creates one uniquely named project desktop, and restores the original desktops. Its fixtures exit after at most 60 seconds if interrupted; successful checks clean them up immediately. Service checks require an interactive Windows 11 host.

## Design documents

- [Project plan](docs/PLAN.md)
- [Accepted design decisions](docs/DESIGN-QUESTIONS.md)
- [Domain glossary](CONTEXT.md)
- [Windows desktop architecture](docs/adr/0001-use-windows-virtual-desktops.md)
- [Rust application architecture](docs/adr/0002-rust-application-with-platform-modules.md)
- [App launching and session snapshots](docs/adr/0003-launch-apps-with-session-snapshots.md)
- [Owned foreground command workloads](docs/adr/0004-own-foreground-command-workloads.md)
- [Startup readiness and dependencies](docs/adr/0005-coordinate-startup-readiness-and-dependencies.md)
- [Implementation progress and validation](docs/PROGRESS.md)
