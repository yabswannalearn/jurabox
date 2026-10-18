---
status: accepted
---

# Retain live sessions in a background Rust process

Command jobs and their output pipes currently belong to the UI process. Saving a PID would neither preserve the output reader nor establish ownership of surviving children after the UI exits. Compose ownership also includes a pinned engine, creation identities, an owner token, and a frozen configuration. Rebuilding those objects from names would weaken the existing ownership rules.

Run the same Rust executable in a hidden session-host mode, without initializing Slint. It owns the existing workspace/service modules and retains their original handles and in-memory state. The manager connects through a local Windows named pipe. This is one local helper, with no separate installation, language runtime, network listener, or distributed service framework. It supplies the lifecycle isolation permitted by ADR 2.

The host has one exclusive file handle per settings file. Its separate versioned JSON journal records its PID, creation time, executable, Windows session, random pipe generation, random authentication token, and open project setups. Write session intent atomically and flush it before launching anything. Keep saved definitions separate from runtime records. Private diagnostics use private journals.

The pipe has one first-instance-protected server, rejects remote clients, and grants access only to the object owner and SYSTEM. The manager verifies the kernel-reported server PID against the saved creation identity before sending data. The host checks the client's executable and Windows session, then requires the journal token. Clients use anonymous impersonation quality of service. Messages have explicit bounded framing. Before a quit disconnect, the pipe thread waits for the client to consume its final response using [Windows pipe-buffer flushing](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers); disconnecting can otherwise discard unread data. A failed response is never automatically replayed as a mutation.

Reopening the manager restores the same sessions, original setup, app assignments, desktop GUIDs, manual-stop state, service handles, Compose controller state, and bounded output tails by reconnecting to the surviving host. It does not launch a replacement workload. Exclude both host and manager windows from the assignment chooser. Expire picker snapshots when the connection ends.

**Quit and leave services running** disconnects the manager. Command pipe readers and Compose log capture remain alive with bounded tails. The host pauses workspace/readiness scheduling and periodic Docker inspection while disconnected and cancels queued starts, which then require explicit Start. Reconnection refreshes container state and reconciles shared windows with the active project. An already dispatched command or Docker operation can finish. Existing processes are not suspended. **Stop services and quit** uses the original shutdown setup, keeps the manager open on failure, and ends the host after successful cleanup and delivery of its response. An empty host exits when the manager disconnects.

This recovers from a manager exit or crash while the host survives. It does not recover from loss of the host itself, logoff, or reboot. If an open-session record no longer matches a live host, preserve it and block launches; do not adopt or terminate possible survivors. Safe cold recovery, an in-app resolution flow for lost records, and recovery of independently surviving containers remain follow-up work. The UI remains available for saved configuration in that situation. Do not delete the journal merely to bypass an ownership conflict.

[ADR 8](0008-recover-original-command-jobs.md) extends this boundary with write-ahead capabilities for command-only sessions after host failure. Legacy and lost Compose records still fail closed. Warm reconnection in this ADR continues to preserve output and the original in-memory objects.

Output is retained in bounded host memory, rather than written as unlimited log files. It is lost if the host dies. A running host also keeps the executable in use; updates must arrange clean host shutdown before replacement. Full update handling, startup integration, Keep running, inactivity policies, and release performance are separate work.

References: [Windows pipe security](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights), [server identity](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getnamedpipeserverprocessid), [client identity](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getnamedpipeclientprocessid), [creation times](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes), and [exclusive file sharing](https://doc.rust-lang.org/std/os/windows/fs/trait.OpenOptionsExt.html#tymethod.share_mode).
