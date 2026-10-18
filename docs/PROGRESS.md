# Implementation progress

## Milestone 1: Windows desktop compatibility

Implementation began on 2026-10-18 after the user asked to start building. The first slice was the Rust diagnostic executable; the initial management UI follows in milestone 2 below.

The executable provides a read-only desktop listing and an explicitly requested desktop exercise. The exercise uses desktop identities, a desktop it creates itself, and a dedicated window owned by its own process. It checks window movement, switching, notifications, and cleanup.

### Validation completed on 2026-10-18

Host: Windows 11 25H2, build 26200.9711. Toolchain: Rust 1.99.1, Cargo 1.99.1, x86_64 MSVC. Desktop library: winvd 0.0.49, pinned in Cargo.toml and Cargo.lock.

| Check | Result |
| --- | --- |
| `cargo fmt --check` | Passed |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed |
| `cargo test --locked` | Passed: 3 command-safety tests |
| `cargo run -- status` | Passed: current desktop and both existing desktops read successfully |
| `cargo run -- desktop-check --exercise` | Passed: all live checks below |

The live exercise verified:

1. Creation and renaming of a new desktop with a distinct identity.
2. Delivery of desktop notifications after asynchronous listener startup.
3. Movement of the owned test window to the temporary desktop.
4. Switching to that desktop and receipt of its switch event.
5. Movement of the same running window back to the starting desktop.
6. Return to the starting desktop and receipt of its switch event.
7. Closure of the test window and removal of the created desktop.
8. Restoration of the original ordered desktop identities and active desktop.

The desktop count was two before and after the exercise. No existing user app was selected or moved by the tool. These results establish basic compatibility on this host, not support for every Windows build or app. Error cleanup is implemented but has not been validated with injected failures or an Explorer crash.

Debug builds can print winvd's COM-initialization retry and listener messages. Both executed commands exited successfully; the final check result is reported separately from those upstream diagnostics.

### Scope still to validate

- Movement and reuse behavior for the actual installed editor, ChatGPT, browser, and terminal apps, including elevated or pinned windows.
- The Slint management UI, system tray behavior, display scaling, startup speed, and idle resource use.
- Project configuration, launching, managed services, inactivity, and recovery in the later milestones described in [PLAN.md](PLAN.md).

Real external-app movement and reuse checks remain part of milestone 1's remaining compatibility work.

## Milestone 2: Management window and saved projects

Started on 2026-10-18 after the user asked to continue. This slice implements:

- A Slint 1.18.1 management window, project sidebar, and project settings form.
- Creating, renaming, and deleting saved projects with stable UUID identities, an optional folder, and a saved inactivity timeout or Off policy.
- Local versioned JSON settings in `%LOCALAPPDATA%\Jurabox\projects.json`, with temporary-file replacement and write work outside the UI thread.
- Protection for unsaved drafts during project selection, new project creation, and quitting; explicit discard and delete confirmation actions.
- Native system tray Show, New project, and Quit actions; closing the window hides it.
- A Windows instance guard and activation event to reuse the existing management process.
- Help > About Jurabox with the required Slint attribution widget.
- A live `ui-check` command that operates only on its own temporary settings and project files and captures the app's rendered snapshots.

### UI direction

Use a slate project rail and a light settings surface, with a teal stacked-window mark. Bahnschrift identifies the application and headings; Segoe UI is used for controls and supporting text. Standard Slint widgets retain keyboard and accessibility behavior, with a Fluent light style. The UI exposes saved configuration only in this slice; it does not display fake running workspaces or service states.

### Validation completed on 2026-10-18

| Check | Result |
| --- | --- |
| `cargo fmt --check` | Passed |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed |
| `cargo test --locked` | Passed: 9 tests, including 6 project/storage tests |
| `cargo run --locked -- ui-check` | Passed with temporary settings and project files |
| Normal and compact snapshots | Inspected at 1040 × 740 and 820 × 650; corrected an oversized status area |
| About snapshot | Inspected; Slint attribution widget is visible |
| Two-process activation | Passed: a second normal launch exited with code 0 and restored the original tray process's responsive Jurabox window |

The live UI check verifies keyboard text entry and dirty-state updates, creation, rename with stable identity, reload from disk, a folderless project with inactivity Off, unsaved-edit protection, discard, and metadata deletion without removing user files. It also induces a settings write failure inside its own temporary directory and verifies that the saved UI state and unsaved draft remain intact.

The close-to-tray check dispatches the framework close event and verifies that the event loop continues. A Windows click notification sent to this process's own native Slint tray window restores the manager. Show and Quit callback paths and the instance guard/activation event are also exercised. This is programmatic validation; manual interaction with the native popup menus, full keyboard traversal, and screen-reader behavior remain to be checked.

The generated content snapshots are in `target/ui-check/` and can be reproduced with `cargo run -- ui-check`. The check's sample projects and files are temporary and do not become the user's saved projects.

### Initial resource observation

A separate debug build was started with `--tray` and no project launched. Its initialization log reported 277 ms, which excludes first-window rendering. During one 73.922-second idle sample, the process CPU-time counter did not increase; the ending working set was 91.37 MiB and private memory was 86.85 MiB. These are observations from this development machine, not release benchmarks, a RAM budget, or evidence for the warm UI feedback target.

After the checks, the normal management window was opened for the user. The second launch reused the original process; a duplicate manager was not left running.

### Remaining work

The next app/link and workspace slice is recorded below. The stored inactivity policy is configuration only at this point; service control, real inactivity countdowns, and recovery remain later milestones. Keep Slint release startup, keyboard navigation, multiple display scales, and idle resource use under validation.

## Milestone 2: App launching and project workspaces

Implemented on 2026-10-18 after the user asked to commit each completed phase and continue. The desktop compatibility phase is commit `f882202`; the initial management UI/tray phase is commit `6a4478e`.

### Implemented scope

- App entries with stable IDs, executable paths, Windows arguments, optional console creation, and explicit shared reuse.
- A searchable cached Windows App Paths catalog with common tool locations and native Browse fallback; this is not a complete inventory of packaged or portable apps.
- An optional Chrome/Edge project window with HTTP(S) links and the user's existing browser profile.
- Optional folder browsing and `{folder}` argument expansion; launching uses the project folder as the working directory when set.
- Separate Open and Switch behavior, a copied session setup, deliberate relaunch with unchanged-window reuse, and Close session without closing apps or removing desktops. Deletion is blocked until the session ends.
- Native desktop GUID associations, native-switch event handling, shared-window movement, and recreation of a deleted desktop while preserving attributable app windows.
- A serialized workspace worker, bounded window discovery during startup, and event-driven idle behavior. Partial failures preserve successful apps and retain a session for explicit resolution.
- Settings schema 2 with in-memory version 1 migration; the original file is left untouched until Save.

The [app/session ADR](adr/0003-launch-apps-with-session-snapshots.md) records attribution rules and limitations. Matching uses a full executable path and a unique new application window, or an explicitly permitted unique existing shared window. Process identity includes creation time. These app associations never authorize service termination.

### Validation

The final unit run passed all 12 tests, including native argument parsing, app/browser persistence and validation, and version 1 migration without rewriting the original file until Save.

The live workspace exercise passed on Windows 11 25H2 build 26200.9711, including a real Edge 155.0.4283.45 window with two temporary loopback HTTP pages. It verified:

1. External child-process app launch and exact-executable window assignment.
2. Continued availability of the successful app when another configured executable is missing.
3. Shared-window movement across two project desktops and active project identity.
4. Switch without duplicated apps or application of newly saved configuration.
5. Deliberate relaunch applying saved changes while preserving unchanged app/browser windows, including a renamed app label.
6. Respect for manually closed apps on Switch and reopening on explicit relaunch.
7. External desktop switching and recreation after desktop deletion while preserving the live app.
8. Close session leaving app windows open, followed by fixture-only cleanup.
9. Both real browser URLs loading, browser window assignment, unchanged-window reuse, and closure of the test browser window.
10. Restoration of the original ordered desktop identities and active desktop, with test child windows and the private copied executable removed.

The exercise's private executable copy prevents its shared-app matcher from selecting a pre-existing Jurabox management window. New-window movement waits briefly for Explorer registration and retains the app identity on failure. The default exercise uses only the test app; the real-browser path is explicitly enabled with `JURABOX_CHECK_BROWSER`.

The final `cargo run --locked -- ui-check` passed with temporary settings. In addition to the original save/error/tray checks, it verified app and browser-link persistence, unrelated edits preserving app entries, and selection without launching a session. Normal and compact screenshots were inspected, including scrolled app/browser fields and a clearly labeled session-controls layout fixture. The review corrected a collapsed multiline URL editor and stacked session actions to fit the compact sidebar. Snapshots are reproducible in `target/ui-check/`.

`cargo fmt --check` and `cargo clippy --locked --all-targets -- -D warnings` passed. Native file-picker interaction, complete keyboard traversal, screen-reader behavior, and release performance remain pilot checks.

### Remaining work

Runtime session/desktop/window associations are not yet persisted. After Jurabox restarts, only saved projects are restored; users must not assume surviving windows are reconnected. Manual window selection, brokered/packaged/terminal-host adapters, actual editor/ChatGPT/Docker compatibility, multi-monitor and scaling checks, and broader failure recovery remain open. A launch with no attributable window is reported and never automatically retried; explicit relaunch may request another window.

The manual-assignment follow-up is recorded below. App compatibility and later service/lifecycle work remain part of the roadmap. No app in this slice is automatically stopped.

## Milestone 2: Manual window assignment

The user asked to publish the completed phases and continue on 2026-10-18. Foundation commit `f882202` was pushed to `main`. The management UI is [PR #1](https://github.com/yabswannalearn/jurabox/pull/1); the launcher is [PR #2](https://github.com/yabswannalearn/jurabox/pull/2), initially stacked on the UI branch so each diff contained one phase. They and the assignment follow-up [PR #3](https://github.com/yabswannalearn/jurabox/pull/3) were subsequently merged at the user's request, as recorded below.

### Implemented scope

- A live-session Workspace windows panel, distinct from saved app configuration.
- A searchable chooser of accessible application windows across desktops, with captions, actual executable paths, and existing assignment labels. Windows desktop/taskbar surfaces and the manager's own process are excluded.
- Explicit assignment of a host window whose executable differs from the configured launcher, without changing launch settings or starting another app.
- Session-scoped opaque chooser tokens with stale-list and process/window identity checks. A closed choice produces an actionable Refresh error. Cancelled or superseded discovery responses cannot reopen the chooser.
- Unassign without closing or moving the window. Unchanged relaunch reuses manually assigned windows.
- Private-window conflict protection for both manual assignment and automatic shared reuse. Cross-session linking requires both entries to be shared; shared assignment follows the actual active project.
- Hidden-window identities included in the pre-launch snapshot, so an existing window becoming visible is not mistaken for a new one. Caption changes do not invalidate window identity.
- Keyboard focus goes to chooser search, and background form controls are disabled while it is open. The sidebar constrains long project names.
- A separate `assignment-check --exercise` for the actual worker/UI callbacks and private external fixtures. The workspace exercise additionally covers assignment and sharing rules, with an optional real CMD/terminal-host test.
- Native console executable-path normalization. A minimal `CreateProcessW` reproduction isolated CMD misparsing a mixed-separator executable name; argument text remains unchanged. The optional live console fixture retains this path form as a regression case.

### Validation

`cargo fmt --check`, `cargo build --locked`, `cargo test --locked` (12 tests), and `cargo clippy --locked --all-targets -- -D warnings` passed.

The live `assignment-check --exercise` passed: picker discovery across desktops, keyboard isolation, manual host assignment, unchanged relaunch, unassign leaving the app open, cancel during discovery, closed-window rejection, refresh, and restoration of the original desktops. Compact picker, assigned-window, and expired-choice snapshots were inspected in `target/ui-check/`; long sidebar names no longer introduce horizontal scrolling.

The workspace exercise passed with both `JURABOX_CHECK_BROWSER` (Edge) and `JURABOX_CHECK_CONSOLE=1`. It covered opaque-token rejection, caption changes, hidden-window snapshots, manual assignment to a different executable, private/manual/automatic sharing conflicts, linking shared entries, active-project movement, unchanged relaunch, unassign, and dead-window rejection. The real CMD fixture started from a mixed-separator executable path and its terminal-host window was explicitly assigned, moved, reused, and unassigned. Only a newly created window with the fixture's unique title was eligible. Its window and the browser fixture were closed during cleanup.

The existing `ui-check` passed project/app/link persistence, unsaved-edit and failed-save protection, deletion preserving files, instance activation, and close-to-tray/Show behavior. Compact editor and browser-field snapshots were inspected. Final `status` showed the same original two desktop GUIDs with the original first desktop active. These are development-host checks, not general application compatibility certification.

### Remaining work

Read-only application inventory found Zed, ChatGPT, Docker Desktop, and Windows Terminal installed on the development host. Installation alone does not establish compatibility. Complete their launch/reuse pilot, including packaged apps, terminal-host behavior, native path pickers, keyboard traversal, scaling, and elevated-window limitations. Runtime persistence and reconnection remain unimplemented.

Next service phase: typed commands/startup actions and Compose workloads with ownership tracking, bounded output, readiness/dependencies, graceful Stop, Retry, and explicit Force stop. Inactivity timers, Keep running, lock/sleep handling, verified reactivation, and recovery follow reliable service ownership.

## Milestone 3: Command services and startup actions

On 2026-10-18 the user requested merging PRs #1, #2, and #3 and starting the next phase. All three passed their GitHub checks and were merged in dependency order with merge commits, preserving the individual phase commits. The resulting main commit is `2af6af4`. This slice starts from that main on `codex/command-services`.

### Implemented scope

- Typed command-service and finite startup-action configuration, explicit working locations, optional normal-stop commands, and per-entry shutdown deadlines.
- Settings schema 3 with in-memory version 1/2 migration until Save.
- Native creation-time Windows job assignment with an inherited-handle allowlist. Ownership uses retained creation handles, including children that survive their launcher. No process-name, port, or PID-based adoption.
- Combined stdout/stderr capture with a 64 KiB tail per start/stop command, fixed-size reads, omitted-output markers, and a final-drain latch before returning to event-only idle behavior.
- Session status/output and manual Start/Retry, normal Stop, and explicit Force stop. Startup action success is distinct from an unexpectedly exited command service.
- Close and deliberate relaunch use the old session's shutdown setup and retain that session on stop failure. Saving does not alter live commands; switching does not reopen a manually stopped service.
- Stop services and quit; failures keep Jurabox open. Ordinary Quit is guarded while managed jobs are live because output capture is currently attached to the manager process.
- Separate core service and real UI exercises with private temporary configuration and finite diagnostic workloads.

The [workload ADR](adr/0004-own-foreground-command-workloads.md) records ownership, output limits, shutdown behavior, and the temporary delivery boundary around leaving workloads running after exit.

### Validation

`cargo fmt --check`, `cargo build --locked`, `cargo test --locked` (15 tests), and `cargo clippy --locked --all-targets -- -D warnings` passed on the Windows 11 development host. Unit coverage includes version 1/2 migration without rewriting on load, typed workload persistence, explicit working-location/stop-command validation, and bounded output across chunks without newlines.

The core `service-check --exercise` passed partial startup, finite-action completion versus unexpected service exit, manual Retry without restart loops, prevention of duplicate live jobs, explicit working folders, normal Stop/Start, and missing/failed/timed-out stop retention. It also passed surviving children after launcher exit, scoped process-tree Force stop while another job remained live, a still-running timed-out stop command, large Unicode output with no line breaks, and final output drainage before returning to event-only idle behavior.

Real CMD and Windows PowerShell startup actions passed raw shell argument handling and combined stdout/stderr capture. Their native launches deliberately used mixed-separator executable paths; the temporary working folders included spaces and Unicode characters.

The real `service-ui-check --exercise` passed form persistence, startup completion/output, manual Stop/Start, Switch respecting manual stops, saved/session setup separation, old stop instructions during failed relaunch, Close and quit retaining a failed workload, explicit Force stop, successful relaunch, and normal stop-and-quit. The development slice's Quit guard was exercised. Compact service-editor, live-output, and stop-failure snapshots were inspected; the log tail is positioned at the top of its output area and long lines wrap within it.

Existing `ui-check`, `assignment-check --exercise`, and `workspace-check --exercise` passed after the service integration. The workspace exercise included real Edge browser links and the real CMD/terminal-host fixture. These checks restored the original ordered desktop GUIDs and original active desktop, and closed their owned windows/workloads. No release-performance or broad installed-application compatibility claim follows from these tests.

### Remaining work

This is the first slice of milestone 3. Compose-specific ownership and non-escalating shutdown, TCP/HTTP readiness and deadlines, dependency coordination, environment overrides, and richer progress/cancellation remain. Durable output, the accepted leave-running exit, persistence, and verified reconnection require further lifecycle work. Inactivity auto-stop is not active, and existing saved inactivity values remain configuration only.

The ordinary Quit guard is temporary; it does not remove the accepted leave-running exit from the product plan. No crash-survival or reconnection guarantee is made by this slice. Full editor/ChatGPT/Docker compatibility, elevation, scaling, keyboard/accessibility, and release performance pilots remain open.

## Milestone 3: Startup readiness and dependency coordination

On 2026-10-18 the user deferred manual testing until the product is further along and authorized PR/merge followed by continued development. Command-services [PR #4](https://github.com/yabswannalearn/jurabox/pull/4) was merged as `ee63ffa`, preserving phase commit `4b45231`. This follow-up starts from that main on `codex/service-readiness`.

### Implemented scope

- Process-running, explicit TCP IP/port, and HTTP(S) 2xx startup gates with a configurable default 60-second deadline starting after the owned command launches.
- Background network probes, a single pending check per workload, bounded probe stages, and attempt identities that discard stale completion after cancellation or Retry.
- Pre-launch endpoint conflict checks that report existing responses without creating a command or adopting/terminating the existing workload. This is best effort and cannot prove response ownership.
- Separate Waiting, Checking endpoint, Starting, and Not ready states. Timeouts retain live workloads; Retry checks the same job. Successful startup ends probing.
- Project-local prerequisites by stable workload UUID, cycle/self/duplicate/missing-reference rejection, and queued dependents that wait for service readiness or successful completion of finite actions. Failed prerequisites require explicit Retry; running dependents retain their processes and report transitive dependency problems.
- Reverse dependency normal shutdown, preservation of prerequisites when a dependent refuses to stop, continued independent shutdown, and a single-entry Stop guard for prerequisites with live dependents. Force stop remains explicit and scoped to the selected owned job.
- Readiness/dependency configuration and runtime controls in the manager, with guarded removal of referenced prerequisites. Settings schema 4 migrates versions 1–3 in memory until Save.

The [coordination ADR](adr/0005-coordinate-startup-readiness-and-dependencies.md) records network behavior, conflict-check limitations, scheduling, cancellation, and shutdown semantics.

### Validation

`cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked` (18 tests), and `cargo build --locked` passed on the Windows 11 development host. New unit coverage checks endpoint/timeout validation, graph validation with stable IDs across renames, schema 3 migration without rewriting on load, and schema 4 readiness/dependency round trips.

The core service exercise passed delayed TCP readiness, unordered dependency declarations, waiting without consuming a dependent's deadline, cancellation of queued starts, single-prerequisite Stop protection, and reverse-order shutdown. HTTP checks passed 503/302/204 responses, preserved query strings, redirect refusal, timeout retention, Retry using the original process, finite-action prerequisites, conflicts without command creation/adoption, and transitive prerequisite-loss reporting without cascading stops. It also passed failed dependent shutdown retaining prerequisites while independent work stops, explicit Retry after a failed action, and stale preflight cancellation without blocking the workspace actor. Existing command/output/ownership checks passed again.

The real service UI exercise passed readiness and dependency persistence, cycle rejection preserving the draft, referenced-prerequisite removal protection, Waiting and Not ready states, and live-instance Retry. The first run exposed a one-second deadline/poll-ordering bug: a completed response could be discarded before it was read. Results now carry completion timestamps and the first probe begins immediately. The full exercise then passed with the one-second timeout, along with its existing old-session shutdown, save separation, manual stop/switch, relaunch/Close/quit failure retention, explicit Force stop, and successful stop-and-quit checks. Its failure cleanup and successful rerun both restored the original desktops and ended only owned fixture workloads.

Compact HTTP editor, Waiting, Not ready, running/output, and stop-failure snapshots were inspected. Existing `ui-check` and `assignment-check --exercise` passed again. The workspace exercise passed with real Edge browser links and the CMD/terminal-host fixture, including unchanged reuse, manual assignment/sharing, desktop movement and recreation, partial launch failure, and owned-window cleanup. Final status confirmed the original ordered two desktop GUIDs with the original first desktop active. These checks do not establish broad application compatibility or release performance.

### Remaining work

Compose-specific ownership and non-escalating shutdown, environment overrides, and richer progress/cancellation remain in milestone 3. Inactivity timers, Keep running, return-during-stop handling, durable output, leave-running quit, session persistence, and verified reconnection remain later lifecycle work. Existing saved inactivity values still do not stop workloads. Full application compatibility and release-performance validation remain open.

## Milestone 3: Local Docker Compose workloads

The user requested continuing after the readiness/dependency phase. [PR #5](https://github.com/yabswannalearn/jurabox/pull/5) was merged as `1a89d3e`, preserving phase commit `753ebb5`. This slice starts from that main on `codex/compose-services`.

### Implemented scope

- A typed Compose workload and UI fields for Docker executable, Compose file, local context, working folder, readiness, prerequisites, and normal shutdown deadline.
- Settings schema 5 with in-memory version 1–4 migration until Save. Existing command/action readiness and dependency settings are preserved.
- A dedicated worker per opened Compose workload. Its pinned local named-pipe endpoint and engine ID remain separate from the Docker CLI client jobs and container ownership.
- A stable project/workload namespace, random owner-token labels, full container IDs and creation identities, and verification before lifecycle control. Existing/unverified namespaces are rejected without adoption. Incomplete CLI operations remain tracked and prevent replacement attempts.
- Canonical Compose session snapshots, image pull on missing, create without start/recreate/build, then start of verified container IDs. Subsequent Start reuses the containers and frozen setup. Named volumes have stable names across sessions.
- Compose started/healthy/completed prerequisites, finite initialization containers, and integration with Jurabox workload dependencies. Independent roots start first; normal shutdown works in reverse order and retains prerequisites of live dependents.
- Non-escalating normal container signals with explicit Force stop, bounded combined logs, container-exit reporting, and kept ownership on uncertainty. SIGKILL and equivalent numeric signal 9 are forbidden for normal shutdown.
- Stop retaining containers; Close/Relaunch/Stop-and-quit removing verified stopped containers while preserving volumes, bind mounts, images, and networks. Ordinary Quit also guards kept Compose containers.
- A finite private Docker exercise and extended UI form/persistence checks. Diagnostic cleanup uses only its newly created private namespace and never Docker prune or process/image/name matching of existing user workloads.

The [Compose ownership ADR](adr/0006-manage-owned-compose-containers.md) records lifecycle, ownership, snapshot, context, and supported-file decisions. The first slice supports a local Linux Docker Desktop engine, up to 32 single-container services, and prebuilt images; explicit container names, profiles, restart policies, lifecycle hooks, optional/restarting dependencies, and replicas require later validation.

### Validation

The development host has Docker Desktop 4.43.2, Engine 28.3.2, and Compose 2.38.2-desktop.1. Its engine was stopped initially and was started for the exercise; pre-existing user containers were inventoried separately and were not test targets. `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked` (23 tests), and `cargo build --locked` passed. New unit coverage includes schema 4 migration/Compose round trips, shell-free Windows argument quoting, supported-file validation and dependency ordering, owner/creation identity rejection, and normal-stop rejection of SIGKILL and equivalent numeric spellings.

The first live Docker attempt exposed double escaping of literal dollars in canonical Compose output, which broke a shell command. The snapshot now preserves Compose's existing escaping. The same attempt exposed a startup-exit status that could stay on Starting; exited containers now report failure. Both regressions passed the final real-engine rerun: literal command/environment values remained intact, and a command exiting with code 7 reported Failed while explicit Retry reused its container.

`compose-check --exercise` passed actual creation, exact-container ownership, duplicate avoidance, bounded output, normal Stop/Start retaining the same IDs, frozen setup after source edits, and rejection of another unverified session. It passed Compose healthy/completed prerequisites, a Jurabox action waiting for the stack, refusal to Force stop an unverified namespace member while leaving the existing container running, normal shutdown, timeout retention, and explicit scoped Force stop. The private fixtures have a 60-second fail-safe. Their database-image volume path uses tmpfs to avoid allocating anonymous volumes; cleanup removes only fixture containers and verified private networks/named volumes. The final run left the Docker volume inventory unchanged. Original user container IDs remained present with their prior running/stopped states.

The extended `ui-check` passed real Compose form add/save/reload/remove without launching a workspace. Its compact editor snapshot was inspected. Existing core service/readiness and service UI exercises passed again, including real CMD/PowerShell output, owned child jobs, readiness Retry/dependencies, old-session shutdown rules, and failed Close/relaunch/quit retention. Assignment and workspace exercises passed with real Edge browser links and the CMD/terminal-host fixture. They restored the original two desktop GUIDs and original active desktop and closed their owned windows. These checks do not establish general Compose/app compatibility or release performance.

### Remaining work

Durable session/ownership records, verified reconnection, a leave-running exit, cleanup of abandoned runtime snapshots, environment override forms, inactivity timers, Keep running, and richer progress/cancellation remain. A manager restart does not reconnect containers; unknown leftovers are reported as conflicts. No automatic inactivity stop is enabled in this slice. Remote contexts, broader Compose compatibility, full application/elevation/scaling/accessibility pilots, and release-performance measurements remain open.

## Lifecycle: Background sessions and manager reconnection

The user requested continuing after the Compose phase. [PR #6](https://github.com/yabswannalearn/jurabox/pull/6) was merged as `e192424`, preserving phase commit `f34f8db`. This slice starts from that main on `codex/session-host`.

### Implemented scope

- A hidden same-executable Rust session host, without Slint initialization, retaining the existing workspace/service objects and their original ownership handles independently of the manager process.
- A private local named pipe with owner/SYSTEM access, remote-client rejection, first-instance protection, server creation-identity verification, client installation/session checks, a random journal token, and bounded message framing.
- An exclusive kernel file handle per settings file, a separate versioned atomic host journal, and session intent flushed before launching work. Saved project settings remain independent.
- Reconnection to the same live host: original project setup, desktop GUIDs, window associations, command jobs, Compose controllers, manual-stop state, and bounded output tails survive manager exit/disconnection. No process/container adoption or replacement launch is involved.
- Explicit Quit and leave services running in the manager and tray. Background output capture remains bounded; workspace/readiness scheduling and periodic Docker inspection pause when disconnected, and queued starts require explicit Start after reconnect. Reconnection refreshes container state and reconciles shared windows. Already dispatched commands/Docker operations can finish.
- Stop services and quit across the pipe, with original shutdown instructions, failure retention, explicit Force stop, successful response delivery, and clean host exit. Empty hosts exit when the manager disconnects.
- Lost/mismatched/malformed/future host records are preserved and block new launches. Connection failure still permits configuration editing and manager exit; deletion is blocked until sessions can be resolved. Both host and manager windows are excluded from assignment choices; picker snapshots expire on disconnect.
- Dedicated multiprocess/real-UI exercises using private journals and finite commands, with an optional private Docker fixture. Existing direct-actor UI checks remain available for isolated feature regression tests.

The [session-host ADR](adr/0007-retain-sessions-in-a-background-rust-process.md) records why one Rust helper is justified and separates manager reconnection from cold recovery.

### Validation

`cargo fmt --check`, `cargo build --locked --offline`, `cargo test --locked --offline` (25 tests), and `cargo clippy --locked --offline --all-targets -- -D warnings` passed on the Windows 11 development host. Added unit coverage rejects oversized/truncated frames and preserves malformed/unsupported runtime journals.

The real `session-check --exercise` passed leave-running and abrupt connection loss, unchanged host identity/generation, original service/output/setup retention, duplicate prevention, manually stopped state, cancellation of queued dependent starts, normal stop, rapid repeated quit/reconnect, clean host shutdown, and preserved lost-host refusal. Negative cases reject a live PID with a different creation time and a client with the wrong token. The exercise's real manager UI reconnects through the production pipe, retains logs, keeps the window open after failed stop-and-quit, performs scoped Force stop/Start, and exits while leaving its service alive. Reconnection and stop-failure snapshots were inspected. Final quit responses are drained before the pipe disconnects; normal session removal joins Compose workers for private-snapshot cleanup.

With `JURABOX_CHECK_DOCKER=1`, the exercise passed real Compose reconnection, exact container identity retention, remote normal Stop/Start, and frozen setup after the original source was edited to invalid content. Docker Desktop was stopped initially and started for validation. The finite fixture uses network mode none and tmpfs at the image's anonymous volume location; it allocates no networks or volumes. Its container was removed, the Docker volume inventory was unchanged, and the pre-existing user containers retained their identities and running/stopped states.

Existing core command/readiness checks and service UI checks passed, including raw CMD/PowerShell output, owned children, dependency coordination, one-second readiness Retry, manual stops, old-session shutdown, and failed Close/relaunch/quit retention. The settings/tray UI check passed. Assignment and workspace exercises passed with real Edge links and a CMD/terminal-host fixture, restoring the original ordered desktops and active desktop and closing their owned windows. These checks do not establish general app/Compose compatibility or release performance.

### Remaining work

This is manager recovery while the original session host survives. Loss of the host itself, logoff/reboot, cold recovery of independently surviving containers/commands, and an in-app resolution flow for uncertain records remain unfinished. Such records currently block launches rather than silently replacing work. Output is bounded host memory and is lost with the host. Abandoned snapshot cleanup and executable-update coordination also remain.

Inactivity timers, Keep running, lock/sleep handling, return-during-stop behavior, environment override forms, and richer cancellation/progress remain. No automatic inactivity stop is enabled. Full application/elevation/scaling/accessibility pilots and release-performance measurements remain open.

## Milestone 4: Command recovery after session-host failure

Implemented on 2026-10-18. The [command recovery ADR](adr/0008-recover-original-command-jobs.md) extends the previous warm-reconnection boundary.

### Implemented scope

- Separate bounded atomic workspace/service records, fresh session generations, original setup snapshots, and command/stop-command capabilities flushed before process creation.
- Private owner/SYSTEM Windows jobs with random global names, atomic creation-time assignment, rejection of existing objects on new launch, and recovery through original capabilities rather than process names or PIDs.
- One extra query-only Rust command keeper per launching host. It confirms its retained job handle before command creation, handles creation interrupted by owner death, seals verified quiescence before releasing job names, and exits after its detached jobs finish. It never starts or stops workloads. An empty attached keeper waits without polling.
- Exclusive-lock replacement of a lost host, preserving the original desktop/window incarnations and original session setup. Recovery launches no apps, services, dependents, or stop commands.
- Manual stops and recorded successful actions remain intact. Unrecorded action results are unknown; queued starts become stopped; network readiness requires explicit Retry of the same recovered job. Surviving children remain controlled even if their launcher has exited.
- Interrupted normal-stop commands remain owned and cannot be duplicated by Retry. Explicit Force stop covers the original workload and stop-command jobs.
- Ended jobs require completion evidence. Missing jobs without a seal, missing/damaged/future records, legacy open-session journals, and lost Compose/mixed sessions fail closed without overwriting ownership evidence. Bootstrap errors reach the manager instead of being hidden behind a generic connection timeout.
- Clear unavailable-output text for recovered command instances. A new Start after that instance stops captures output again. Once commands are verified quiescent and original windows/desktops are gone, the old session closes for the next deliberate Open.

### Validation

The first actual host-crash test failed with surviving children after the last host job handle closed. A separate native Windows probe reproduced the loss of the job's namespace entry while the workload remained alive. The implementation now retains query handles in the separate keeper; a saved name alone is never treated as proof of termination.

`recovery-check --exercise` passed an actual creation-handle-scoped host kill, child-only survival, original desktop and external window assignment, original shutdown instructions after saved setup changes, manual stops, completed actions, ended jobs, cancelled queued dependents, network readiness Retry without replacement, recovered normal Stop, explicit Force/Start, and no duplicate starts. A surviving stop command remained owned and was not launched again. Future workspace records were preserved. A second host loss after verified command shutdown and removal of only the fixture's window/desktop left the project closed. This models resource loss after verified quiescence; it does not establish sudden reboot/logoff recovery.

`cargo test --locked --offline` passed all 30 tests, including native job-name collision rejection, refusal of a missing unsealed job, ownership-write failure preventing process creation, and preservation of missing/future/damaged/oversized records.

`cargo fmt --check`, the final `cargo build --locked --offline`, and `cargo clippy --locked --offline --all-targets -- -D warnings` passed. Both recovery and session/UI exercises passed again with the final executable. Final inspection confirmed the original two ordered desktops and active desktop, with no Jurabox fixture processes left running. No Docker resources were created or changed by this phase's checks.

The existing multiprocess `session-check --exercise` and its production manager UI passed warm reconnection, retained output, failure retention, remote Force/Start, leave-running exit, and legacy lost-record refusal. `service-check --exercise` passed the existing command, child ownership, bounded-output, readiness, dependency, stop-failure, and retry cases. Fixtures used private settings and finite workloads.

### Remaining work

Docker/mixed-session crash recovery needs durable engine/container creation evidence and frozen setup. Sudden reboot/logoff can lose both a keeper and an unsealed job; those records remain blocked until reliable boot/session evidence or an explicit resolution flow is implemented. No actual reboot/logoff test was performed. Automatic orphan-record/snapshot cleanup, durable bounded output, helper/update coordination, and release resource measurements remain open.

Inactivity timers, Keep running, lock/sleep handling, return-during-stop, environment override forms, and broader app/elevation/scaling pilots remain. Stored inactivity settings still do not stop workloads.

## App setup usability: choose installed apps by name

Implemented on 2026-10-18 after the local pilot exposed an awkward path-based app form and a rejected quoted Zed Start Menu shortcut. The [app selection ADR](adr/0009-select-installed-apps-by-name.md) records the updated behavior.

- **+ Add app** opens a searchable, scrollable chooser using Windows display names. Selected apps fill their launch details automatically. App cards show names and the explicit sharing option; technical fields stay under collapsed **Advanced**.
- Discovery reads the Windows AppsFolder on a worker thread, with App Paths and known-tool fallbacks. The management thread filters a cached snapshot. This host returned 322 entries, including 61 Windows application IDs. ChatGPT appeared as one packaged choice.
- Ordinary Start Menu shortcuts and quoted Copy as path text resolve to an executable and their original arguments without executing the shortcut. CMD/PowerShell selection automatically enables a fresh console.
- Packaged targets use Windows activation and their stable application IDs. Attribution queries application IDs while retaining existing window-incarnation checks. Existing shared/private rules and manual window assignment remain applicable.
- Project browsers are chosen as Google Chrome or Microsoft Edge, with duplicate shortcut variants removed. Optional path configuration is under Advanced.
- Settings schema 6 reads versions 1–5 without rewriting until Save. Existing app, service, Compose, readiness, and dependency settings are preserved.

The live `ui-check` passed discovery, keyboard search focus, case-insensitive search, empty results, selection, automatic console setup, browser filtering, Windows-app selection/save/reload, cancellation without edits, and the existing project/save/error/tray flows. Screenshots at 1040×740 and 820×650 were inspected, including the ChatGPT search and browser chooser. Catalog inspection and UI selection did not launch the user's installed apps.

`cargo test --locked --offline` passed all 34 tests, including quoted shortcut resolution/argument preservation, packaged-target round trips and invalid-target rejection, old settings migration without a write, missing-app activation errors, and preserving an existing multithreaded COM apartment during activation.

`cargo fmt --all --check`, `git diff --check`, and `cargo clippy --locked --offline --all-targets -- -D warnings` passed.

`workspace-check --exercise` passed launch attribution, partial failures, shared movement, unchanged relaunch, manual assignment, private/shared conflicts, and restoration of the original desktops. `session-check --exercise` passed warm reconnection, retained logs, the real manager UI, and remote Force/Start/quit flows. Package-specific launch/reuse behavior, elevated windows, and broader scaling/accessibility pilots remain to validate.

## Zed project selection in Advanced

Implemented on 2026-10-18. Per the user's placement request, **Advanced → Project to open** appears for Zed and offers its normal launch, the Jurabox project's folder, or a separately chosen folder. Existing app entries keep normal launch behavior until changed. See the [Zed project ADR](adr/0010-choose-a-zed-project-in-advanced.md).

Folder selection uses an optional typed setting in schema 7, validates project-folder references and absolute custom locations, and checks folder availability before launching. Versions 1–6 migrate without rewriting until Save. Folder mode uses a private editor window, prevents conflicting shared/console/manual-argument settings, and remains part of the original session setup until deliberate relaunch.

Inspection of the installed Windows `Zed.exe --help` and `bin/Zed.exe --help` established that only the bundled CLI exposes `-n`. The adapter launches that CLI with separate new-window/folder arguments and attributes the resulting GUI executable. It reports missing launcher files instead of sending CLI-only flags to the GUI executable.

`cargo test --locked --offline` passed all 38 tests, including the launcher/GUI pair, folder names with spaces, unavailable folders, project/custom folder round trips, old settings preservation, and conflicting launch-mode rejection. Actual interactive Zed launch behavior remains a pilot check; the read-only help inspection does not establish end-to-end window compatibility.

The live `ui-check` passed Zed detection, unchanged default behavior, the Advanced-only form, both folder choices/save/reload, missing-project-folder validation, discard restoration, and normal-launch opt-out. Inspected screenshots at 1040×740 and 820×650 show the selected folder and native picker button. The existing app/browser/project/tray/save-error checks also passed. `workspace-check --exercise` passed existing launch/assignment/sharing/relaunch semantics and restored the original desktops. Formatting and Clippy with warnings denied passed; the Windows executable was built locally.

The packaged Windows GUI-subsystem executable passed the full UI check and the multiprocess session/UI check. One earlier packaged UI run could not register its Windows tray icon; the complete repeat passed. No application code changed between those two runs.
