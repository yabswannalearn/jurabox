---
status: accepted
---

# Use Windows virtual desktops for the initial Windows release

Jurabox will use native Windows virtual desktops for project workspaces, initially prioritizing switching between projects. This keeps applications in their normal Windows environment and avoids the compatibility work involved in embedding arbitrary external application windows inside a custom container.

The trade-off is that Jurabox must integrate with Windows desktop management. Full automation may require internal Windows interfaces through libraries such as winvd, so support must be validated for the selected Windows builds. Saved project configuration remains a separate concept from the desktop used to display a running workspace.

The initial release targets Windows 11. macOS and Linux are planned for later versions, with their workspace integrations and exact behavior to be investigated separately.
