# Jurabox accepted design decisions

Prepared and accepted: 2026-10-17 (Asia/Shanghai).

Status: all recommendations Q01-Q28 accepted by the user with “yes for all.” The questions and their accepted decisions are retained as the decision record.

The consolidated specification is [PLAN.md](PLAN.md). Terminology is in [CONTEXT.md](../CONTEXT.md). Windows 11 is the first-release target; macOS and Linux are planned later.

Acceptance covers the product plan. Technical candidates still require validation, including the final UI toolkit and Windows desktop library. Development was subsequently authorized on 2026-10-18; see [implementation progress](PROGRESS.md).

## Q01 - First audience

Is the first usable release primarily for your own machine or for public distribution?

Accepted decision: a personal pilot first, then broader distribution after validating app compatibility and recovery. This is a delivery sequence, not a restriction on the long-term product.

## Q02 - UI approach

Should the interface use a native-rendered Rust UI or a web UI hosted in a desktop app?

Accepted decision: a native-rendered Rust UI, with Slint as a candidate rather than a final selection. Validate startup, idle resource use, Windows integration, and distribution requirements before choosing the toolkit. Keep the agreed central project-management window.

## Q03 - Application architecture

Should Jurabox begin as one Rust application or several services written in different languages?

Accepted decision: one Rust application with separate modules for project rules, session/service management, and OS integration. Implement only the Windows integration initially. Introduce a helper process or another language only when a concrete library or isolation need justifies it.

## Q04 - Adding apps

How should users add an app to a project?

Accepted decision: a searchable installed-app picker, a Browse fallback, and advanced launch arguments. Scripts and commands remain available for custom setups.

## Q05 - Required app compatibility

Which installed applications must work in the first pilot?

Accepted decision: start with the tools originally named: Zed, ChatGPT, Command Prompt/PowerShell or Windows Terminal, Chrome/Edge, and Docker Desktop. Use the user's installed browser and terminal choices in project setup and integration validation; other apps remain possible but unverified.

## Q06 - Browsers and profiles

Should Jurabox open ordinary browser windows or embed its own browser, and are separate browser profiles needed immediately?

Accepted decision: use an installed browser, with one project window and configured URLs, initially sharing the user's existing browser login/profile. Dedicated per-project profiles can be added later.

## Q07 - Existing app windows

May Jurabox automatically claim any existing window from an app, or should reuse be explicit?

Accepted decision: create a new project window when the app supports it. Reuse existing or shared windows only through an explicit project setting or assignment. Preserve the already-agreed follow-active-project behavior for configured shared apps.

## Q08 - Service types and startup actions

Should a long-running command, a Docker Compose workload, and a one-off startup script be represented differently?

Accepted decision: distinguish command services, Docker Compose services, and one-off startup actions. A script completing successfully is different from a long-running service exiting. Users still configure these explicitly; there is no automatic service discovery.

## Q09 - Normal shutdown

How should users configure normal service shutdown, and how long should Jurabox wait?

Accepted decision: use a normal stop request where supported, allow a custom stop command, and provide Compose-specific stop behavior. Start with a configurable 10-second shutdown waiting period. The agreed failure policy still applies: leave the service running and offer manual Force stop. The Docker integration must explicitly preserve that policy, since ordinary Docker stop behavior escalates to forceful termination after its grace period; exact integration mechanics require validation.

## Q10 - Dependency scope and readiness defaults

Should dependencies cross projects, and what readiness defaults should the first version use?

Accepted decision: dependencies stay within a project initially; reject self-dependencies and cycles. Default to basic process-running readiness, allow the agreed port or HTTP alternatives, and use successful HTTP responses for health checks. The already-agreed 60-second configurable readiness timeout remains unchanged.

## Q11 - Failures after startup

If a service crashes after becoming ready, should Jurabox automatically restart it and its dependents?

Accepted decision: show the failure and offer manual Retry. Leave other running tools available and show dependency problems. Defer automatic restart loops, cascading restarts, and continuous application-health monitoring.

## Q12 - Shared background services

Should the first version manage one background service shared by several projects?

Accepted decision: manage project-specific services initially. A manually started shared database or other external workload remains unmanaged and is never auto-stopped by Jurabox. Explicit shared-service ownership across projects can come later; shared app windows are already in scope.

## Q13 - Port conflicts

What should happen when a configured service cannot start because another workload is using its port?

Accepted decision: show the conflict and leave the other workload alone. Let the user change the saved configuration and deliberately relaunch; do not silently change ports or terminate another process.

## Q14 - Service output and interactive terminals

Should service output appear inside Jurabox, and should services support interactive terminal input?

Accepted decision: show recent service output and status in Jurabox, with limits on retained output. Use ordinary terminal app entries for interactive work initially; defer an embedded interactive terminal.

## Q15 - Default inactivity timeout

What should the default auto-stop timeout be, and may a project's saved setup disable it?

Accepted decision: 15 minutes, configurable per project, with an Off option. Saved policy changes follow the agreed next-launch rule; Keep running remains the immediate temporary override.

## Q16 - Timer edge cases

Should locking Windows or putting the computer to sleep consume the inactivity timeout, and what happens after Keep running is disabled?

Accepted decision: pause countdowns while locked or asleep and give inactive projects a fresh countdown on return. Turning Keep running off also starts a fresh countdown for an inactive project; active projects remain protected from inactivity-based auto-stop.

## Q17 - Returning during shutdown

What happens if you return to a project while its services are already stopping?

Accepted decision: finish the in-progress normal shutdown, then restart successfully stopped services if the project is still active. Reuse services that remain running and respect the existing shutdown-failure policy.

## Q18 - Manual closures and stops

Should switching back reopen an app you closed manually or restart a service you explicitly stopped?

Accepted decision: respect manual actions. Automatically restart services that Jurabox auto-stopped. A manually stopped service waits for an explicit Start or relaunch; a manually closed app waits for an explicit reopen or new launch.

## Q19 - Applying a saved setup through relaunch

How should a deliberate relaunch handle existing app windows, changed service settings, and a failure to stop an old service?

Accepted decision: preserve editor/browser windows, reuse still-configured windows, and launch newly configured tools. Leave removed app windows open for the user to close. Stop old managed services using their original session's shutdown actions before starting the new setup. If an old service refuses to stop, report the failure and pause the relaunch for explicit resolution.

## Q20 - Manual desktop changes

What happens if you rename, reorder, or delete a project desktop through Win+Tab?

Accepted decision: respect renaming and reordering and retain project association independently of desktop position. If a desktop disappears, report it, pause any inactivity decision that depends on that lost association, and recreate or reassociate the workspace when the user next opens it. Keep its apps and services intact.

## Q21 - Unavailable desktop integration

Should Jurabox remain useful if a Windows update or unsupported build breaks its desktop integration?

Accepted decision: keep configuration and explicit app/service launching available on the current desktop. Clearly report that desktop management is unavailable and pause desktop-driven inactivity auto-stop until reliable workspace tracking is restored.

## Q22 - Crash recovery

When Jurabox reopens after an unexpected crash, should it restore work automatically?

Accepted decision: reconnect only to verified surviving workloads, restore available session state, and report anything missing or uncertain. Do not launch duplicate or missing workloads silently; let the user start or retry those. Apply the already-agreed fresh-countdown and surviving-session Keep running rules where recovery succeeds.

## Q23 - Closing and deleting projects

How should a user end a project session or remove its saved setup?

Accepted decision: Close project stops its managed services normally and ends that session while preserving app windows and project files. Stop failures require explicit resolution. Delete project removes the saved entry after its session has ended, preserving files, app windows, and container data. Leave desktop cleanup manual initially.

## Q24 - Settings storage and sharing

Should settings stay on the computer or require an account and cloud synchronization?

Accepted decision: local settings with manual export/import, no account or cloud sync initially. Imported setups must allow app paths and folders to be remapped on another machine; live ownership/session records do not become portable configurations.

## Q25 - Service environment variables

Should a service support environment-variable overrides in the first version?

Accepted decision: inherit the user's environment and allow optional per-service overrides. Keep credentials in the user's existing environment or local configuration; defer a built-in secrets manager and exclude credential values from portable exports by default.

## Q26 - Notifications

Should normal switching and auto-stop actions generate desktop notifications?

Accepted decision: show ordinary progress and countdowns in the app/tray. Use notifications for failures or required user action, with a mute preference, instead of notifying for every successful transition.

## Q27 - What fast means

Which performance goals should drive the first version?

Accepted decision: prioritize immediate UI feedback, quick switching between already-open projects, and low idle CPU use. Treat roughly 100 ms for warm UI feedback as a provisional measurement target, not a measured result. Measure external app/service startup separately and choose a RAM budget after establishing a baseline. Start with status and logs; resource-history graphs can wait.

## Q28 - Build sequence

Should development begin with the entire feature set at once or proceed through reviewable milestones?

Accepted decision: first validate Windows desktop control and shared-window movement on the development machine; then build project configuration and launching; then service control/readiness; then auto-stop, reconnection, and recovery. Existing accepted features remain planned. This describes the future build order and does not start implementation.

## References for technical candidates

- [Slint Windows backend and rendering options](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backend_winit/).
- [Tauri process model](https://v2.tauri.app/concept/process-model/).
- [Windows virtual desktop integration candidate](https://raw.githubusercontent.com/Ciantic/VirtualDesktopAccessor/rust/README.md).
- [Docker Compose stop](https://docs.docker.com/reference/cli/docker/compose/stop/).
- [Docker container stop and its automatic escalation behavior](https://docs.docker.com/reference/cli/docker/container/stop/).
