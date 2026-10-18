# Jurabox plan

Last updated: 2026-10-18 (Asia/Shanghai).

Repository: [yabswannalearn/jurabox](https://github.com/yabswannalearn/jurabox). The user selected this repository for the project on 2026-10-17.

Status: product decisions accepted. The user accepted all recommendations Q01-Q28 in the [design decision record](DESIGN-QUESTIONS.md) and authorized starting development on 2026-10-18. The basic desktop exercise and initial project-management/tray UI pass their checks on the development host. App/link launching and Open/Switch actions are next; real app compatibility and later service/lifecycle features remain in progress. See [implementation progress](PROGRESS.md) for executed checks and remaining work.

Terminology: [domain glossary](../CONTEXT.md). Architecture: [Windows desktops](adr/0001-use-windows-virtual-desktops.md) and [Rust application with separate platform integration](adr/0002-rust-application-with-platform-modules.md).

## Product and release scope

Jurabox is a desktop app that organizes a project's apps, browser pages, startup actions, and background services into a working environment.

- Deliver a personal pilot on Windows 11 first. Validate on the current Windows 11 25H2 development machine before widening distribution.
- macOS and Linux are planned for later versions. Their workspace integrations have not been selected or validated; identical native desktop capabilities are not assumed.
- Prioritize switching between projects. Simultaneous side-by-side project desktops are outside the first version.
- Keep the accepted feature set, delivered through reviewable milestones.

## Project setup and management UI

Provide one central Jurabox management window for all projects, accessible from the system tray. The UI direction is a project sidebar and a selected-project configuration/status panel.

- A project requires a name. Its local folder is optional.
- Selecting a project displays its configuration without opening its workspace, starting services, or changing desktops.
- A separate Open project or Switch to project action activates its workspace.
- Configure apps, browser links, startup actions, services, readiness, dependencies, and inactivity policy through this UI.
- Show service state and recent output, project countdowns, Keep running badges, failures, and actions needed.
- Show Changes pending when saved settings differ from the running session's setup.
- Add apps through a searchable installed-app picker, Browse fallback, and optional advanced launch arguments.
- Add services explicitly; automatic discovery from project files is deferred.
- A command requiring a working folder must have an explicit location or use the project's configured folder. A folderless project does not imply an arbitrary working directory.

## Apps, windows, and browsers

The first compatibility targets are Zed, ChatGPT, Command Prompt/PowerShell or Windows Terminal, Chrome/Edge, and Docker Desktop. Validate the actual installed browser and terminal choices during the pilot. Other apps remain possible but unverified.

- Open a new project window when the application supports it.
- Reuse an existing or shared window only through an explicit project setting or assignment.
- Configured shared app windows follow the active project where integration supports it.
- Moving a shared window preserves its current application state. Per-project conversation/account switching is not a general guarantee.
- Use installed external browsers, initially with one window per project and its configured links.
- Use the user's existing browser login/profile initially. Dedicated per-project profiles are deferred.
- Use ordinary terminal app entries for interactive work; an embedded interactive terminal is deferred.
- Respect manually closed apps. Switching back does not reopen them; an explicit reopen or new launch does.

## Project sessions and saved changes

A project session uses the setup with which it was launched. Keep that session setup separate from the project's latest saved setup.

- Saving configuration does not alter a running workspace.
- Reactivation after auto-stop uses the existing session's setup, including startup and shutdown actions.
- Reconnection to verified surviving work does not apply newly saved configuration or itself relaunch services.
- Retry and explicit service Start within a session use that session's setup.
- A deliberate project relaunch applies the latest saved setup to a new session.
- This preserves launch settings, not an application's in-memory execution state or a snapshot of external files.

### Deliberate relaunch

Preserve editor/browser windows, reuse still-configured windows, and launch newly configured tools. Leave removed app windows open for the user to close.

Stop old managed services using their original session's shutdown actions before starting services from the new setup. If an old service refuses to stop, report the failure and pause the relaunch for explicit resolution. Do not silently start a conflicting replacement.

## Service types and ownership

Distinguish three configured entries:

| Entry | Meaning |
| --- | --- |
| Command service | A long-running command managed for one project |
| Docker Compose service | A configured project container workload |
| Startup action | A finite script or command whose successful completion finishes that action |

A completed startup action is not a crashed long-running service. Completing a detached Docker launch command does not establish that its workload has stopped.

A service entry includes a name, working location, start configuration, normal shutdown configuration, readiness choice, and any project-local dependencies.

- Manage only workloads Jurabox started for that project and can verify as its own.
- Reconnect to verified surviving managed services after reopening.
- Leave manually started and unknown workloads unmanaged and untouched.
- Do not establish ownership from a matching process name, process ID, or listening port alone.
- Avoid duplicate service instances during launch, retry, reactivation, and reconnection.
- Manage project-specific services initially. A manually started shared database or other shared external workload stays unmanaged.
- Explicit service ownership shared across projects is deferred. This does not defer shared app windows.

## Startup coordination and failure behavior

Independent apps and services may start immediately. A service may wait for a prerequisite within the same project to become ready. Reject self-dependencies and dependency cycles; cross-project dependencies are deferred.

Users choose one startup readiness condition:

| Choice | Meaning |
| --- | --- |
| Process running | Basic default; does not prove the application is fully initialized |
| TCP port accepting connections | The configured endpoint accepts a connection; does not establish complete application health |
| HTTP health URL | The configured endpoint returns a successful response; use HTTP 2xx as the initial success rule |

- A service's readiness deadline starts with its own startup attempt, not while it is waiting for prerequisites.
- Default readiness timeout: 60 seconds, configurable per service.
- On timeout, show Not ready, stop that readiness attempt, leave a still-running service running, and keep dependent services waiting.
- Retry rechecks readiness for an existing running service rather than creating another instance.
- A failed prerequisite leaves its dependents waiting with an explanation. They may start when it becomes ready.
- If one service fails to start, keep successful apps and services available, show the error, and offer Retry for the affected service.
- If a service later crashes, report the failure and offer manual Retry. Keep other running tools available and report dependency problems.
- Defer automatic restart loops, cascading restarts, continuous application-health monitoring, and custom readiness scripts.
- On a port conflict, report the problem and leave the other workload alone. The user can edit the saved setup and relaunch; do not silently change ports or terminate another workload.
- Show recent service output with limits on retained output. Defer an embedded interactive terminal and long-term resource-history graphs.

## Normal shutdown and Force stop

Use a normal stop request where supported, an optional user-configured stop command, or appropriate Compose-specific behavior.

- Default normal shutdown waiting period: 10 seconds, configurable.
- A waiting deadline does not authorize forceful termination.
- If shutdown fails, leave the managed service running, report the failure, and offer an explicit Force stop button.
- Other services may complete their normal shutdown. Accurately report partial outcomes.
- Force stop remains an explicit user action scoped to the affected managed workload.
- Unmanaged services, editors, and browsers are not targets of inactivity-based shutdown.

The Docker integration must preserve this manual-force policy. Ordinary Docker stop behavior escalates after its grace period, so its integration needs an explicitly non-escalating approach and validation. See the [Docker stop documentation](https://docs.docker.com/reference/cli/docker/container/stop/).

## Inactivity and Keep running

| Setting | Accepted default/behavior |
| --- | --- |
| Inactivity auto-stop | 15 minutes, configurable per project, with an Off option |
| Saved inactivity changes | Apply on the next project launch |
| Keep running | Immediate override for the current project session |
| Readiness timeout | Separate 60-second setting |
| Normal shutdown waiting period | Separate 10-second setting |

- Switching away starts the inactive project's countdown. Returning before expiry cancels it.
- Reading or thinking on the active project does not make it inactive.
- On expiry, request normal shutdown of eligible managed services. Keep editors, browsers, shared app windows, and unmanaged workloads open.
- Keep running pauses auto-stop immediately and shows a badge. It lasts until switched off or the session ends.
- A recovered surviving session retains its previous Keep running state and badge.
- New sessions, including deliberate relaunches, start with Keep running off.
- Pause countdowns while Windows is locked or asleep. Give inactive projects fresh countdowns on return.
- Turning Keep running off starts a fresh countdown for an inactive project. Active projects remain protected from inactivity-based auto-stop.
- An auto-stopped project can still have open GUI apps; service state and workspace visibility are separate.

### Returning to a project

Restart services stopped by Jurabox's auto-stop using the existing session setup. Reuse running services and existing app windows.

Respect a user's manual service stop: that service stays stopped until explicit Start or a relaunch. Respect manually closed apps.

If the user returns during an inactivity-triggered shutdown, finish the in-progress normal shutdown, then restart successfully auto-stopped services if the project is still active. Reuse anything still running. This does not reverse a deliberate manual stop, Close project, or relaunch operation.

## Desktops and degraded operation

- Use one native Windows virtual desktop per open project.
- Keep saved project identity and configuration independent of temporary desktop ordering.
- Respect desktop renaming and reordering performed through Win+Tab.
- Native desktop switches update the active project; selecting a project in the configuration UI remains a separate action.
- If a desktop disappears, report it, preserve its workloads, and pause any inactivity decision that depends on its lost association.
- Recreate or reassociate the workspace when the user next opens that project.
- If desktop integration becomes unavailable, keep configuration and explicit launching on the current desktop available.
- Clearly report degraded desktop management and pause desktop-driven auto-stop until reliable tracking returns.
- Leave desktop cleanup manual in the first version.
- Windows desktops organize windows; projects still share the computer's files, accounts, ports, and runtimes.

## Closing a project and deleting its setup

Close project requests normal shutdown of its managed services and ends that session while preserving app windows, project files, and the saved setup. Stop failures require explicit resolution.

Delete project removes its saved entry only after its session has ended. Preserve project files, open app windows, and container data. Do not automatically remove desktops.

## Starting, quitting, and recovery

### Windows sign-in and reboot

Start with Windows is optional and off by default. When enabled, open Jurabox quietly in the system tray at sign-in, without launching project apps or services.

After a Windows reboot, show saved projects and wait for the user to choose what to open. A new project launch uses the latest saved setup with Keep running off.

### Quitting Jurabox

Closing the main window sends Jurabox to the tray and keeps management available.

Provide two explicit exit actions:

- Quit and leave services running.
- Stop managed services and quit.

Editors and browsers stay open in either case. Fully quitting ends monitoring, inactivity-based auto-stop, and automatic reactivation handling until Jurabox opens again.

If Stop managed services and quit cannot stop a service, keep Jurabox running and report the failure. The user can explicitly force-stop the affected service to finish quitting, or choose Quit and leave services running.

### Reconnection and crash recovery

When Jurabox reopens after leaving workloads running, or after an unexpected crash:

- Reconnect only to surviving workloads whose original ownership can be verified.
- Restore available session state and the original session setup.
- Give recovered inactive projects fresh inactivity countdowns. Time while Jurabox was closed does not count.
- Restore a surviving session's Keep running override and badge if it was enabled.
- Report missing or uncertain workloads without silently starting replacements.
- Offer explicit Start or Retry for missing workloads, and avoid duplicates.
- Leave manual and unverified workloads unmanaged.

Reconnection is distinct from starting every previous project after a reboot. Verification and persistence mechanisms require technical validation.

## Local settings, environment, and notifications

- Store settings locally. No account or cloud sync initially.
- Support manual export/import, with app/folder remapping on another machine.
- Export saved configuration, not live service ownership or session records.
- Inherit the user's environment and support optional per-service overrides.
- Keep credentials in existing environment/local configuration; defer a built-in secrets manager.
- Exclude credential values from portable exports by default. Export behavior must not assume arbitrary values can be reliably classified as credentials automatically.
- Show ordinary progress and countdowns inside the app/tray.
- Notify for failures or required action, with a mute preference. Do not notify for every successful switch or auto-stop.

## Architecture and technical candidates

Begin as one Rust application with separate modules for project rules, session/service management, UI, and OS integration. Implement only the Windows integration initially. Add a helper process or another language only for a demonstrated library or isolation need.

| Concern | Decision or candidate |
| --- | --- |
| Core and application architecture | Rust, one application with separate modules |
| Interface | Slint 1.18.1 for the initial native-rendered management UI; pilot and release validation ongoing |
| Documented Windows integration | Microsoft's windows crate is a candidate |
| Full virtual desktop integration | winvd 0.0.49 for the initial desktop slice; basic operations validated on build 26200.9711, real app compatibility pending |
| App and script launching | Rust process APIs are a candidate |
| Process tracking | Windows Job Objects where suitable; not the universal ownership mechanism |
| Settings/session persistence | Versioned local JSON for saved project settings; runtime ownership/session persistence and recovery remain to be designed |
| macOS and Linux | Later platform integrations; none implemented for the initial Windows release |

Project definitions and service/session rules remain separate from Windows-specific handles and desktop operations. The future platform targets justify this boundary without requiring a generic plugin system or distributed services.

### UI toolkit recommendation

Slint 1.18.1 is being used for the initial management UI slice, following the documentation assessment on 2026-10-17 and the user's request to continue development on 2026-10-18. The broader toolkit validation gate remains open; implemented behavior and executed checks are tracked in [PROGRESS.md](PROGRESS.md).

- Define the project sidebar, configuration forms, status panel, and dialogs in `.slint` files compiled into Rust components. Keep project/session rules and Windows operations in Rust modules. See the [Rust integration documentation](https://docs.slint.dev/latest/docs/rust/slint/).
- Its Winit backend supports Windows, macOS, and Linux, matching the UI portability direction. Platform-specific workspace integration remains separate. See the [backend documentation](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backend_winit/).
- Built-in system tray support fits Jurabox's background management workflow. See the [Slint 1.17 tray introduction](https://slint.dev/blog/slint-1.17-released).
- Slint offers a free desktop licensing option requiring attribution. Include the applicable attribution when distributing under that option. See the [official licensing overview](https://slint.dev/get-started#choose-a-license).

Validate startup and idle resource use, tray operation with the management window hidden, keyboard navigation, display scaling, and Windows integration before final selection. Keep slow app/service operations off the UI thread. Renderer choice and actual performance remain measurement questions.

## Performance goals and build sequence

Prioritize immediate UI feedback, fast switches between already-open projects, and low idle CPU use.

- Roughly 100 ms for warm UI feedback is an accepted provisional measurement target, not a measured result or universal guarantee.
- Measure external app/service startup separately from Jurabox's own responsiveness.
- Choose a RAM budget after establishing a baseline.
- Prefer Windows event notifications over frequent full-window scans.
- Keep retained service output bounded.
- Start with status and logs; resource-history graphs are deferred.

Planned development milestones:

1. Validate desktop creation/switching, shared-window movement, and library compatibility on the development machine.
2. Build project configuration, app/link launching, explicit window assignment, and basic workspace switching.
3. Build typed service control, output, readiness, dependencies, shutdown, and Retry/Force stop.
4. Add inactivity policies, Keep running, graceful reactivation, persistence, reconnection, and crash recovery.
5. Validate the complete personal pilot before broader distribution.

The milestone 1 desktop compatibility tool passes its basic exercise. Milestone 2 has started with project configuration and tray controls; app/link launching and Open/Switch actions remain next. Later service and lifecycle milestones remain planned.

## Technical validation still required

These are engineering checks, not undecided product preferences:

- Basic desktop control and movement of an owned test window passed on build 26200.9711. Extend validation to actual installed app windows, reuse behavior, and failure recovery before claiming general compatibility.
- Select the native UI toolkit after checking startup, idle resource use, integration, and distribution requirements.
- Validate graceful command/Compose shutdown without automatic force escalation.
- Design reliable workload identity and session persistence for reconnection, including detached container workloads.
- Validate dependency-aware startup/shutdown, partial failures, and return-during-stop behavior.
- Validate supported app launch/reuse behavior, multiple monitors/scaling, and elevated-window limitations.
- Saved project settings now use versioned local JSON. Validate upgrades and interrupted-write recovery, and select runtime/session persistence, output limits, and export handling consistent with the accepted rules.
- Measure responsiveness and idle resource use rather than assuming the language or toolkit establishes performance.

Development host inspected read-only on 2026-10-17: Microsoft Windows 11 Home Single Language, 25H2, build 26200.9711, 64-bit.

The inspected upstream winvd manifest reports version 0.0.49 and a minimum of Windows 11 24H2 build 26100.2605. Its project reports testing on Windows 11 25H2 build 26200.8117. Jurabox's desktop exercise subsequently passed on the development machine's build 26200.9711 on 2026-10-18; the narrower test scope and remaining app checks are recorded in [PROGRESS.md](PROGRESS.md).

## Decision record and references

All Q01-Q28 recommendations were accepted on 2026-10-17. The [accepted questionnaire](DESIGN-QUESTIONS.md) preserves their original questions and rationale; this plan is the consolidated behavior specification.

- [Windows virtual desktops](https://support.microsoft.com/en-us/windows/experience/configure-multiple-desktops-in-windows).
- [Documented virtual desktop API](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ivirtualdesktopmanager).
- [VirtualDesktopAccessor capabilities and requirements](https://raw.githubusercontent.com/Ciantic/VirtualDesktopAccessor/rust/README.md).
- [Windows Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects).
- [Windows process termination](https://learn.microsoft.com/en-us/windows/win32/procthread/terminating-a-process).
- [Docker Compose stop](https://docs.docker.com/reference/cli/docker/compose/stop/) and [start](https://docs.docker.com/reference/cli/docker/compose/start/).
- [Docker stop escalation behavior](https://docs.docker.com/reference/cli/docker/container/stop/).
