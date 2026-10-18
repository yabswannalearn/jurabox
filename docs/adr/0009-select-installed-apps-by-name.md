---
status: accepted
---

# Select installed apps by name

The app setup screen should let a person choose Zed, ChatGPT, a terminal, or another installed app without knowing its executable path. The previous inline picker only showed six App Paths results and exposed technical fields for every entry. Browsing to a quoted Start Menu shortcut then failed executable validation.

Use a searchable, scrollable **Add app** chooser with Windows display names. Selecting a result adds its launch details automatically. Keep paths, arguments, console settings, and renaming under a collapsed **Advanced** section. Sharing remains an explicit choice. The browser chooser lists installed Chrome and Edge by name; links remain a separate project setting. Cancelling the chooser does not change the draft.

Read the Windows AppsFolder Shell namespace on a background thread and cache one snapshot for the management window. Read-only App Paths and known tools remain a fallback. Search filters the snapshot without launching PowerShell, running app scripts, scanning drives, or touching project files. Preserve shortcut arguments and automatically choose a fresh console for CMD and PowerShell. Browse accepts executable files and ordinary Start Menu shortcuts, including Explorer's quoted Copy as path text. Resolve shortcut metadata without ShellLink Resolve, network searching, or executing the shortcut. Shortcuts without an executable target direct the user back to Add app.

Packaged apps use their registered Application User Model ID with Windows' application activation manager. Do not persist a changing WindowsApps package installation path. Settings schema 6 adds an optional app ID; a packaged target has an empty executable and no console flag. Versions 1–5 migrate in memory, preserving existing app settings and writing only on Save. A previous Jurabox release rejects the newer settings rather than deleting fields. Quit the previous manager and session host before replacing the executable.

Classic apps retain full executable-path attribution. For packaged apps, query the process application ID while retaining the same verified window/process incarnation. Automatic attribution still requires a unique newly appearing window; shared reuse remains explicit. Brokered or reused windows may need **Assign open window**. Activation does not prove that a new window exists, permit automatic retries, or confer managed-service ownership. Package-specific behavior still requires pilot testing.

References: [Shell known-folder items](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shgetknownfolderitem), [shortcut target metadata](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ishelllinkw-getpath), [Windows app activation](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-iapplicationactivationmanager-activateapplication), and [process application IDs](https://learn.microsoft.com/en-us/windows/win32/api/appmodel/nf-appmodel-getapplicationusermodelid).
