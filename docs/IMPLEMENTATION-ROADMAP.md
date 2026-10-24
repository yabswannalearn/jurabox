# Pilot completion roadmap

Authorized on 2026-10-24: complete the improvement roadmap, add authentic installed-app icons, and optimize the application.

- [x] App icons in the installed-app chooser, saved app cards, browser choice, and workspace list; background extraction and caching.
- [x] Responsive lifecycle operations, dependency-aware incremental shutdown, progress, and return-during-stop behavior.
- [x] Lightweight status/log transport, incremental UI models, and reduced Docker polling cost.
- [x] Inactivity countdowns, Keep running, lock/sleep handling, and automatic reactivation that preserves manual stops.
- [x] Project-scoped uncertain-session recovery UI, verified Compose/mixed-session recovery, and reboot/logoff handling.
- [x] Changes pending and clear active/open/attention state.
- [x] Per-service environment overrides, portable settings import/export/remapping, optional sign-in startup, and failure notifications/mute.
- [x] Bounded durable output, conservative runtime cleanup, and helper/update coordination.
- [x] Windows CI configuration, packaging, release measurements and updated documentation.
- [ ] Complete the interactive compatibility/accessibility/scaling/reboot pilot listed in `PROGRESS.md`.
- [ ] Reduce the measured 2–3 second initial Open acknowledgement for ten command services; native command creation/record initialization still run during that initial host operation.

Validation must distinguish automated and fixture checks from actual reboot/logoff, installed-app, and assistive-technology pilot evidence. Existing workload ownership and explicit Force stop rules remain applicable throughout.

## Current implementation notes

The working tree now includes code for icons/virtualized app selection, asynchronous app/recovery workers and shutdown, delta status/log updates, inactivity and Keep running, project-scoped recovery and disconnected-record archival, Compose ownership records and frozen setup, verified logon change handling, preferences/portability/environment overrides, bounded saved output, conservative retention cleanup, binary/protocol update checks, packaging/CI, and performance exercises.

Implementation and fixture validation are recorded in `PROGRESS.md`, including 45 unit tests, native Windows/UI/Compose exercises, mixed cold recovery, a live one-minute auto-stop exercise, release metrics and the portable ZIP/checksum. Checked rows describe implemented behavior with fixture evidence. Actual reboot/logoff, physical lock/sleep, broad Windows-build/app coverage and assistive-technology certification remain interactive pilot gates. The first measured cold-open bottleneck is tracked separately above; steady-state transport and polling improved, while initial command creation still warrants another pass.
