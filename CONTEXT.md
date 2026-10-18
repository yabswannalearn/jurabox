# Jurabox

Jurabox organizes the tools and background workloads used for different projects. This glossary defines the language used when discussing those working environments.

## Language

**Project**:
A named, saved collection of tools, resources, and actions associated with a body of work. A **Project** may optionally be associated with a local folder.
_Avoid_: Container, desktop, workspace when referring to the saved collection.

**Workspace**:
The live working environment associated with one **Project**, including its application windows. A **Project** can remain configured while its **Workspace** is inactive.
_Avoid_: Project when referring specifically to the live working environment.

**Project session**:
A period of work begun by launching a **Project** with a particular **Session setup**. It continues across workspace switches, service auto-stop/reactivation, and verified reconnection to surviving work with that same setup.
_Avoid_: Launch when referring only to switching back to an existing workspace.

**Saved setup**:
The currently saved configuration of a **Project**, used for its next launch or deliberate **Relaunch**.
_Avoid_: Session setup when referring to changes saved for a future launch.

**Session setup**:
The configuration used to begin a **Project session**, retained for that session even when the **Saved setup** changes.
_Avoid_: Latest settings when referring to an already-running session.

**Selected project**:
The **Project** currently displayed for configuration in the Jurabox window. It may differ from the **Active project**.
_Avoid_: Active project when referring only to the configuration selection.

**Active project**:
The **Project** whose **Workspace** is currently active.
_Avoid_: Selected project when referring to the workspace currently in use.

**Project app**:
A user-facing application associated with a **Project**, such as an editor, browser, or terminal window.

**Shared app**:
A user-facing application instance explicitly assigned for reuse across multiple **Workspaces**.
_Avoid_: Duplicate app when referring to the same running instance.

**Window assignment**:
An association between an open application's window and a **Project app** within a **Project session**. Assigning a window does not make its process a **Managed service**.
_Avoid_: Service ownership when referring only to an app window's workspace association.

**Project service**:
A background workload supporting a **Project**, such as a development server or a database.
_Avoid_: App when referring specifically to a background workload.

**Startup action**:
A finite instruction performed when a **Project** starts, such as a setup script. Successful completion finishes the action rather than indicating that a long-running service has exited.
_Avoid_: Project service when referring to work expected to finish during startup.

**Managed service**:
A **Project service** started by Jurabox for that project and included in its verified lifecycle ownership, including surviving services it reconnects to after reopening.
_Avoid_: Any running process associated with the project.

**Unmanaged service**:
A **Project service** outside Jurabox's verified lifecycle ownership, including manually started services and services whose ownership cannot be established.
_Avoid_: Managed service merely because it supports the same project.

**Ready service**:
A **Managed service** whose configured startup readiness condition has been satisfied.
_Avoid_: Running when only the existence of a process has been established and the selected readiness condition requires more.

**Project inactivity**:
A period during which a **Workspace** is not active while Jurabox is managing its **Project session**. This describes absence from that workspace, rather than absence of keyboard or mouse input.
_Avoid_: Computer idle time.

**Auto-stop**:
The automatic shutdown of eligible **Managed services** after a configured period of **Project inactivity**.
_Avoid_: Hibernate, suspend, kill project, close workspace.

**Keep running**:
A temporary exemption from **Auto-stop** for one **Project session**, lasting until it is turned off or that session ends.
_Avoid_: Permanent auto-stop setting, computer sleep prevention.

**Reactivation**:
Returning to a **Workspace** whose **Managed services** were stopped automatically, within the existing **Project session**.
_Avoid_: Resume when it implies restoring the exact execution state of an interrupted workload.

**Reconnection**:
Resuming lifecycle management of a surviving **Managed service** after Jurabox reopens.
_Avoid_: Adoption of an unknown service, relaunch.

**Session host**:
The background Jurabox process that retains open **Project sessions**, original ownership handles, and recent output while the manager is closed.
_Avoid_: Separate installed server, saved project definition.

**Relaunch**:
Starting a new **Project session** using the project's latest **Saved setup**.
_Avoid_: Reactivation, switching projects.

**Close project**:
Ending a **Project session** while retaining its **Saved setup** and project files.
_Avoid_: Delete project.

**Delete project**:
Removing a **Project** and its **Saved setup** from Jurabox after its session has ended, while retaining the user's files.
_Avoid_: Deleting the project folder.

## Example dialogue

**Developer:** Project A's workspace is active, but I select Project B in Jurabox to edit its settings. Does that activate Project B?

**User:** No. Project B is the selected project; Project A remains the active project until I explicitly open or switch workspaces.

**Developer:** I switched from Project A's workspace to Project B. Is Project A closed?

**User:** No. Its project apps stay open. Its eligible managed services can auto-stop after the configured period of project inactivity.

**Developer:** What happens when I reactivate Project A?

**User:** Services Jurabox auto-stopped start again with the current project session's setup. A shared app can follow the active project, using the same running instance.

**Developer:** I saved a different startup command while that session was open. Will reactivation use it?

**User:** No. Those saved changes take effect when I deliberately relaunch the project.

**Developer:** I explicitly stopped a service. Will switching back start it again?

**User:** No. Reactivation restarts services Jurabox auto-stopped. My manual stop stays in effect until I explicitly start the service or relaunch the project.

**Developer:** My setup script completed successfully. Is that a service failure?

**User:** No. It is a startup action, so successful completion is expected. A project service is expected to keep running.

**Developer:** Its development server was already running before Jurabox opened the project. Does auto-stop affect it?

**User:** No. That server is an unmanaged service and stays running.

**Developer:** What if Jurabox started the server, then I quit Jurabox while leaving it running?

**User:** Jurabox can reconnect to that surviving managed service when it reopens, if it can verify ownership, and restores Keep running if it was enabled for the surviving session. Manually started or unknown services remain unmanaged.

## Flagged ambiguities

- **Project versus workspace:** A project is the saved setup; a workspace is its live working environment.
- **Selected versus active project:** Selecting a project for configuration does not activate its workspace or start its services.
- **Project inactivity versus computer inactivity:** Reading on the active workspace does not make that project inactive.
- **Auto-stop versus closing everything:** Auto-stop concerns eligible managed services. Editors and browsers remain open in the initial scope.
- **Service ownership:** Jurabox manages services it starts for a project and can verify as its own. Manually started and unknown services remain unmanaged and are left running.
- **Reactivation versus relaunch:** Reactivation continues the existing project session and its setup. A deliberate relaunch applies the latest saved setup in a new session.
- **Saved versus session setup:** Saving changes updates the configuration for a future launch. It does not replace the setup of an existing session, including its shutdown instructions.
- **Startup action versus service:** An action finishes; a service continues running. A successful action exit is not a service crash.
- **Manual stop versus auto-stop:** Returning to a workspace reactivates automatically stopped services while preserving the user's explicit stop decisions.
- **Close versus delete:** Closing ends a live session; deleting removes its saved project entry. Neither means deleting project files.
- **Running versus ready:** A running process is sufficient only for the basic process-running readiness choice. Other configured readiness conditions must also be satisfied before the service is ready.
- **Keep running versus saved settings:** Keep running is a live session exemption from auto-stop and is restored when its surviving session is recovered. It is separate from the saved project setup used for future launches; a new session starts with the exemption off.
- **Service ownership after exit:** Reconnection restores management of verified surviving services Jurabox originally started. A service being already running does not by itself establish or rule out ownership.
