# Implementation progress

## Milestone 1: Windows desktop compatibility

Implementation began on 2026-10-18 after the user asked to start building. The first slice is the Rust diagnostic executable. The management UI and project/service features remain planned.

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


Next: validate the Slint management window and tray, then add project configuration and launching.
