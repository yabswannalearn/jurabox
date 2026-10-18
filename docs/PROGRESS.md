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

Add app/link configuration and launching, then Open/Switch workspace actions backed by the verified desktop integration. The stored inactivity policy is configuration only at this point; service control, real inactivity countdowns, and recovery remain later milestones. Keep Slint release startup, keyboard navigation, multiple display scales, and idle resource use under validation.
