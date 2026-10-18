---
status: accepted
---

# Launch app windows with an explicit session setup

An open project retains a copy of its saved setup. Switch does not launch apps or apply later configuration edits. Deliberate relaunch uses the latest saved setup, reuses unchanged verified windows, and leaves removed or replaced windows open. Closing a session releases its associations without terminating apps or deleting desktops. Persistence of these associations across Jurabox restarts belongs to the recovery milestone.

Use a single workspace worker in the Rust process. Desktop notifications and UI requests are serialized there; slow startup and bounded window discovery stay off the UI thread. An idle worker waits for events rather than scanning windows continuously.

For ordinary app entries, launch an explicit executable with configured arguments and an optional project working folder. The initial browser integration uses a new Chrome/Edge window and configured HTTP(S) links with the existing browser profile. Interactive console entries use Windows `CreateProcessW` with fresh console handles; inheriting or redirecting the tray process's standard streams would not provide the intended terminal interaction. See [Microsoft's process creation flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags).

Normalize the executable's path separators before constructing a console command line, while preserving the user's raw argument text. A native-process reproduction on the development host showed that CMD misparses its own executable name when it contains a forward slash, even though Windows successfully starts that executable. The live console exercise deliberately supplies a mixed-separator path to cover this case.

Window attribution checks the full executable path and newly appearing application windows, retaining the window handle, process ID, thread ID, and process creation time. Shared reuse is explicitly enabled in the saved app entry and requires one matching existing window. Multiple candidates or no attributable window produce a visible error; ordinary entries do not take over a pre-existing window. These checks reduce accidental assignment and process-ID reuse errors; they do not establish service ownership or prove general compatibility with brokered, elevated, packaged, or multi-window applications. The five-second discovery deadline is a bounded observation, not proof that an app failed to launch.

An app that delegates to another executable, or creates no unique window, can remain running without assignment. Report this outcome and preserve successful apps. Do not automatically retry unknown launches. Switching alone never opens another instance. Explicit relaunch can request another window, so the user is told to check an unassigned app before relaunching. App-specific launch adapters remain follow-up compatibility work.

Manual window assignment is available for each app in the live session, including a configured browser window. The chooser lists accessible application windows and requires an explicit selection. This permits a terminal or packaged app's host executable to differ from its configured launcher. Chooser tokens are scoped to a short-lived worker snapshot and revalidated before movement; they are never saved in project configuration. Unassign releases the association and leaves the app open. Shared assignment follows the actual active project; a window can be linked across sessions only when both entries are shared. Automatic shared reuse enforces the same private-window conflict rule.

Launch discovery snapshots all existing top-level handles, including hidden windows, before requesting a new app window. An existing hidden window becoming visible is not evidence of a new window. Caption text is display metadata and can change without invalidating a verified window identity. The chooser filters Windows desktop/taskbar surfaces and Jurabox's own process. This uses [Windows window enumeration](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumwindows) and [caption queries](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowtextw); it is not a guarantee that every protected or packaged window can be enumerated.

Explorer may register a new application window after it first becomes enumerable. Retain the discovered window identity before attempting movement and allow up to two seconds for initial desktop assignment. A movement failure retains the association for later explicit switching instead of losing track of the launched app. Warm switching has no startup polling delay.

Project settings use schema version 2 for app/browser entries. Version 1 loads with empty launch entries and migrates in memory; the file changes only on a deliberate Save. Reject unknown fields and unsupported versions to avoid silently deleting configuration.
