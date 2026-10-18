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
