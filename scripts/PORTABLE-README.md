# Jurabox Windows pilot

Requires an interactive Windows 11 desktop. The pinned virtual-desktop integration targets Windows 11 24H2 build 26100.2605 or newer; app and Windows-build compatibility must be checked on the intended machine.

Extract the folder and run jurabox.exe. Settings stay in %LOCALAPPDATA%\Jurabox. Closing the window hides it to the tray. Sign-in startup is optional in File > Preferences & portability.

Before updating, choose **Stop services and quit** and resolve any failures. All Jurabox manager, host, and keeper processes from that installation must exit. Extract the new package separately, then use update.ps1 -InstallDirectory <existing folder>. The updater refuses to replace a running installation and does not stop unknown processes or modify settings.

Compose and ordinary commands can modify project data according to their configured instructions. Normal Stop never authorizes Force stop. Configuration exports omit free-form values by default and exclude private runtime ownership records.

Jurabox uses Slint. Slint attribution and licensing information is available in Help > About Jurabox. This package is an unsigned personal pilot, not a signed installer or a general Windows/app compatibility certification.
