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

Reopening Jurabox reconnects to its background session process, which keeps the original apps, services, and recent output. If that process also crashes, command-only sessions can recover their original command jobs, verified window assignments, and desktops from private records and a small command keeper. Verified stopped services stay stopped until you choose Start/Retry. Once commands are verified stopped and their windows/desktops are gone, projects stay closed until you choose Open. Docker crash recovery, sudden reboot recovery, and live inactivity timers remain later work. The saved inactivity setting does not yet stop processes.

For an open project, **Workspace windows** shows the apps from its session setup. Use **Assign open window** or **Change window** to search accessible windows by title or executable path, then choose **Use window**. This also supports apps whose launcher and visible window belong to different executables. Assignment takes effect immediately and leaves launch settings unchanged. **Unassign** leaves the app open. Windows privately assigned to another entry are unavailable; enable Shared app in both saved setups and relaunch to link one window across sessions.

Under **Services and startup actions**, add a foreground command service or a finite startup action. Choose an executable and arguments, and set a working folder or use the project's folder. Commands run without an interactive terminal, inherit your environment, and support `{folder}` in arguments. Run scripts through an explicitly configured shell. A completed startup action is successful work; an exited long-running service is reported as a failure. Use the Docker Compose entry for detached container workloads.

Choose **+ Docker Compose**, select `docker.exe` and your Compose file, and set a working folder. Leave the context blank to use the current local Docker context, or enter its name. Docker Desktop must be running with its Linux engine. Jurabox creates a private stack, checks container ownership, starts its services in dependency order, and shows recent output. Later Start reuses its verified containers and session snapshot. Editing the original Compose file takes effect on deliberate Relaunch.

For Compose, **Stop** sends the configured normal container signal and waits without forcing a kill. A timeout leaves containers running; **Force stop** is explicit. Stop keeps containers. Close session, Relaunch, and Stop services and quit remove their verified stopped containers while keeping volumes, bind-mounted files, images, and networks. Files saved only inside a container's writable layer are removed when its container is removed. Containers from an older or unverified session cause a conflict and remain unmanaged; recovery is still planned.

This initial Compose support handles up to 32 services with one container each, including healthy/completed prerequisites. Build custom images first and specify their image name; missing images can be pulled. Explicit container names, profiles, restart policies, lifecycle hooks, and multiple replicas are reported as unsupported before creation. Remote Docker engines are not supported yet. The [Compose decision record](docs/adr/0006-manage-owned-compose-containers.md) explains these boundaries.

Choose **Process running**, a **TCP IP address and port**, or an **HTTP(S) health URL** for service startup readiness. HTTP requires a 2xx response and uses a direct GET without redirects, authentication, or cookies. The default readiness deadline is 60 seconds after the command starts. A timeout shows **Not ready** and leaves the workload running; **Retry** checks that same instance. Readiness is a startup gate, not continuous health monitoring. An endpoint already responding before launch reports a conflict without starting a command. This is a best-effort check, not proof of endpoint ownership.

Select other services/actions under **Wait for prerequisites**. Dependents wait until services are ready and finite actions finish successfully; waiting does not consume their own readiness deadline. Invalid or cyclic dependencies are rejected on Save. Failed prerequisites require explicit Retry. A later prerequisite failure is reported on running dependents without stopping them. Normal shutdown stops dependents first, keeps prerequisites alive if a dependent cannot stop, and still stops independent workloads. A single prerequisite's Stop is blocked while dependent workloads are live; explicit Force stop remains available.

**Session services** shows status and recent output, retaining 64 KiB per command stream. **Start/Retry** uses the session setup and avoids duplicating a live workload, including child processes that survive their launcher. **Stop** runs the configured normal-stop command with a default 10-second deadline. A missing/failed stop command or timeout leaves the workload managed and running. **Force stop** explicitly terminates that workload and its owned children. Close and relaunch pause on stop failures; saving a different stop command does not change the current session's instructions.

Choose **Stop services and quit** in File or the tray menu to shut down managed workloads, clean up owned stopped Compose containers, and exit. A shutdown or cleanup failure keeps Jurabox open. **Quit and leave services running** closes the manager while a small Rust background process retains service ownership and recent output. Reopening Jurabox reconnects to the same sessions and their original setup. Closing the window still hides it to the tray.

While the manager is closed, running commands continue and output tails remain bounded. Workspace/readiness scheduling and periodic Docker inspection pause, and queued starts require explicit Start after reopening. Already dispatched operations can finish. Command-only sessions can recover after host failure; lost Docker/mixed sessions and legacy records still block replacement launches. An in-app resolution flow for uncertain records remains unfinished. Output does not survive loss of the host. Use Stop services and quit before replacing the executable during development or an update.

The runtime journal sits beside the settings file as `projects.session-host.json`. It contains runtime credentials and session commands; keep it local and exclude it from configuration exports. A failed connection still lets you edit saved configuration and quit the manager. Settings versions 1–4 migrate to version 5 in memory and are rewritten only on Save. Existing readiness/dependency settings are preserved.

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
cargo run -- compose-check --exercise
cargo run -- session-check --exercise
cargo run -- recovery-check --exercise
```

Unit tests do not open windows or switch desktops. The live UI check uses temporary settings, exercises project/app/link forms and save/error/tray flows, and writes screenshots to `target/ui-check/`. The workspace exercise launches separate Jurabox test processes, verifies assignment, shared movement, session semantics, desktop deletion/recreation, and partial failure. It restores the original desktops and closes only its test windows. The live checks require an unlocked interactive Windows session.

To include an installed Chrome/Edge browser in the workspace exercise, set `JURABOX_CHECK_BROWSER` to its full executable path for that command. The check serves two temporary loopback HTTP pages, verifies the new browser window and reuse, and closes that window during cleanup. It uses the existing browser profile. This optional check passed with Edge 155.0.4283.45 on the development host.

The assignment exercise opens a temporary project and private test-app copy, checks the real picker callbacks, search keyboard focus, assignment/relaunch/unassign, cancellation during discovery, and closed-window refresh. It restores the original desktops and closes its own fixture. The workspace exercise also checks assignment and private/shared conflict rules. Set `JURABOX_CHECK_CONSOLE=1` to include a real CMD/terminal-host window; that fixture command exits by itself after 20 seconds and only a newly created, uniquely titled window can be selected by the exercise.

The service exercises use private temporary settings/folders and finite diagnostic commands, with no process-name or port-based selection. They check startup/exit semantics, output limits, normal-stop failures, duplicate prevention, explicit owned-job Force stop, TCP/HTTP readiness, dependency waits, stale probe cancellation, and reverse shutdown. The UI exercise also checks readiness/dependency forms, cycle and removal errors, Waiting/Not ready states, and Retry, creates one uniquely named project desktop, and restores the original desktops. Its fixtures exit after at most 60 seconds if interrupted; successful checks clean them up immediately. Service checks require an interactive Windows 11 host.

The Compose exercise needs the local Linux Docker Desktop engine running. It uses `postgres:16-alpine` for finite shell fixtures, pulling the image if missing, and creates private project/workload names. It checks actual container ownership, same-container Stop/Start and Retry, source-file snapshot behavior, healthy/completed prerequisites, output, startup failure, unverified-container refusal, and non-escalating Stop/explicit Force stop. It cleans up only its private containers, networks, and volumes. The normal UI check also verifies Compose form persistence without starting Docker.

The session exercise uses real separate background hosts and private journals. It checks leave-running, abrupt connection loss, original setup/output retention, duplicate avoidance, manual stops, cancellation of queued starts, clean exit, and refusal of a lost host identity. It also runs the real manager UI through the production pipe connection. The older UI exercises retain a direct in-process actor for isolated feature checks. Set `JURABOX_CHECK_DOCKER=1` for the session exercise to include same-container reconnection and Stop/Start after editing the original Compose source. This optional fixture requires the local `desktop-linux` context and `postgres:16-alpine`, allocates no Docker networks or volumes, and removes its private container on exit.

The recovery exercise deliberately ends only its own private background host using its creation handle. It checks surviving command children, original shutdown settings, manual stops, completed actions, missing commands, readiness Retry, scoped Force stop, and absence of duplicate launches. It also models the loss of all session resources without rebooting Windows. This does not replace a real logoff/reboot test.

Crash recovery records live beside the host journal in `projects.session-host.sessions/`. They contain original commands, window/job capabilities, and completion seals; keep them private and out of configuration exports. One query-only helper per launching host keeps command jobs discoverable until they finish, even if the session host crashes. It never stops workloads. Command output capture cannot reconnect after the host dies: that recovered instance's output is unavailable until it is stopped and explicitly started again. Missing jobs without completion evidence, damaged/future records, legacy open-session records, and lost Docker-session records are preserved and block replacement launches. This also protects against a lost keeper or sudden reboot with unfinished records. Old runtime records are retained conservatively; automatic cleanup and an in-app resolution flow remain planned.

## Design documents

- [Project plan](docs/PLAN.md)
- [Accepted design decisions](docs/DESIGN-QUESTIONS.md)
- [Domain glossary](CONTEXT.md)
- [Windows desktop architecture](docs/adr/0001-use-windows-virtual-desktops.md)
- [Rust application architecture](docs/adr/0002-rust-application-with-platform-modules.md)
- [App launching and session snapshots](docs/adr/0003-launch-apps-with-session-snapshots.md)
- [Owned foreground command workloads](docs/adr/0004-own-foreground-command-workloads.md)
- [Startup readiness and dependencies](docs/adr/0005-coordinate-startup-readiness-and-dependencies.md)
- [Owned Docker Compose containers](docs/adr/0006-manage-owned-compose-containers.md)
- [Background sessions and reconnection](docs/adr/0007-retain-sessions-in-a-background-rust-process.md)
- [Command recovery after host failure](docs/adr/0008-recover-original-command-jobs.md)
- [Implementation progress and validation](docs/PROGRESS.md)
