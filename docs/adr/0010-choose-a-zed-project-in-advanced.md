---
status: accepted
---

# Choose a Zed project in Advanced

After choosing Zed as a project app, the user wants to point Jurabox at the code project it should open. Keep this choice inside the app's **Advanced** settings, as requested, rather than adding another required step to app selection.

For a recognized Zed executable, show **Project to open** with three choices: **Open Zed normally**, **Use this Jurabox project's folder**, and **Choose another folder**. The last choice uses the native **Choose folder…** dialog. Display the selected location and explain that it takes effect on the next launch. The default is Open Zed normally, preserving existing behavior. This feature opens the code folder; it does not run a build, task, or development server.

Keep the selection as an optional typed launch setting. Settings schema 7 reads versions 1–6 without rewriting until Save. A project-folder reference follows the session's original project folder; a custom folder is independent and supports a folderless Jurabox project. Save rejects a missing project-folder reference, relative custom paths, and conflicting manual launch arguments. Check the folder still exists immediately before requesting launch.

An explicitly selected folder opens in its own Zed window. Choosing a folder turns off shared and console modes, with those controls disabled while the selection remains active. Reusing an arbitrary shared editor window would not establish which code folder it contains. Existing sessions keep their original setup until deliberate relaunch; changing the folder counts as a changed app launch.

Use Zed's bundled command-line launcher with `-n` and the selected folder as separate arguments. On this Windows installation the Start Menu points to the GUI `Zed.exe`, whose `--help` does not expose `-n`; the sibling `bin/Zed.exe` is the CLI and advertises `-n, --new`. Resolve this pair and attribute the window to the GUI executable, not the CLI. Missing launcher files report a repair/reinstall or normal-launch option instead of passing unsupported flags to the GUI.

Start with Zed, whose launch contract has been checked. Other IDE integrations can add explicit adapters later. Native window attribution still may require manual assignment for reused or brokered windows; this configuration does not confer service ownership.

Reference: [Zed CLI reference](https://zed.dev/docs/reference/cli). Both installed executables were inspected with their read-only `--help` options on 2026-10-18.
