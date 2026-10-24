# App icons and efficient refresh

Accepted for implementation on 2026-10-24 after the user requested app logos and optimization.

Show authentic Windows icons beside installed-app names, configured project apps, browser choices, and workspace entries. Read icons using the Windows Shell image factory, including packaged app IDs, on a dedicated worker. Keep pixels off the UI thread until converting them to Slint images, release native bitmap/DC resources, cache results by target, and show a generic app glyph when metadata is unavailable. Icons are display metadata and are not stored in project definitions.

Use a virtualized app chooser so only visible rows are instantiated and request icons. Filtering remains an in-memory catalog operation. Runtime lists keep their models and update changed rows, preserving focus and avoiding rebuilding app rows for every output change.

Production polling sends lightweight metadata and bounded append/trim patches for visible-project logs. Quiet polls can return an unchanged reply. Full snapshots remain available for diagnostics. Output patches respect Unicode boundaries, bounded history rotation, reconnect/selection reset, and a per-response payload budget. No mutation is replayed after connection loss.

Stable Compose monitoring uses a longer reconciliation interval; stopping retains the short interval and lifecycle actions still verify current engine/container ownership. Compiler flags and renderer changes remain measurement decisions. The performance exercise records initialization/idle observations, host CPU/memory, polling latency, and actual delta-response bytes against equivalent full-snapshot bytes, clearly identifying debug versus release builds.

Settings schema 8 adds per-service environment overrides and preferences for sign-in startup and failure notification muting. Older settings migrate in memory until Save. Imports create new project/app/workload IDs and remap dependency IDs and optional path prefixes. Exports exclude all runtime ownership data. Free-form arguments, links, HTTP endpoints and environment values are omitted by default; including them requires an explicit choice because arbitrary credentials cannot be classified reliably.
