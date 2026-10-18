# Jurabox

A Windows project workspace manager in development. Windows 11 is the first target; macOS and Linux are planned later.

## Desktop compatibility phase

This first executable validates native virtual desktop access before the management UI is built.

Requires Rust with the MSVC toolchain, Visual Studio C++ Build Tools and a Windows SDK, and Windows 11. The pinned winvd library targets Windows 11 24H2 build 26100.2605 or newer; exact builds still need validation.

```powershell
cargo run -- status
cargo run -- desktop-check --exercise
```

The default command only lists desktops. The explicit exercise creates a temporary desktop and an owned test window, verifies movement and switch notifications, then restores the starting desktop and removes its temporary resources. Existing app windows are not selected for testing.

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The desktop exercise passed on Windows 11 25H2 build 26200.9711. See the [validation record](docs/PROGRESS.md) for scope and remaining compatibility work.

- [Project plan](docs/PLAN.md)
- [Accepted decisions](docs/DESIGN-QUESTIONS.md)
- [Domain glossary](CONTEXT.md)
- [Windows desktop architecture](docs/adr/0001-use-windows-virtual-desktops.md)
- [Rust application architecture](docs/adr/0002-rust-application-with-platform-modules.md)
