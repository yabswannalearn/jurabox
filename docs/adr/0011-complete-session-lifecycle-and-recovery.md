# Complete session lifecycle and recovery

Accepted for implementation on 2026-10-24, following authorization to complete the pilot roadmap.

The session host remains the lifecycle authority. App launches and recovery checks run on bounded workers. Normal shutdown advances independent dependency branches through polling; prerequisites stay alive when a dependent cannot stop. Long operations expose progress without occupying the management UI or waiting for unrelated operations. Mutation requests are acknowledged separately from their completion. CLI exercises may wait through a synchronous convenience wrapper; the production UI polls progress.

Inactivity follows workspace activation, not keyboard idle time. Locked/unknown Windows session state, missing desktop association, long observation gaps such as sleep, and manager disconnection reset countdowns. Returning to a project restarts only services marked automatically stopped. An automatic shutdown already underway finishes before reactivation. Keep running belongs to the original session and survives verified reconnection; manual stops remain manual. Saved setup changes still require deliberate relaunch.

Cold recovery verifies each project independently. One uncertain project cannot invalidate unrelated verified projects. The UI exposes retry and explicit abandonment of management; abandonment archives evidence and leaves existing work untouched. A malformed disconnected host record can also be archived explicitly after obtaining the exclusive host lock. A live host prevents that action. No operation infers workload ownership from a process name, PID, or endpoint alone.

Compose records retain engine identity, a random ownership capability, exact observed container identities/creation values, creation intent, and the resolved ownership-labelled setup before creation. Recovery reconstructs that frozen setup and verifies the same local engine and containers. Missing, inconsistent, legacy, and unsupported evidence remains a project-level recovery problem. New containers are never silently created during recovery.

Workspace records include the Windows logon time. A verified change in logon identity establishes that original user-session commands cannot survive that logoff/reboot; missing evidence preserves uncertainty. Docker retains its independent engine/container verification. This mechanism requires an actual reboot/logoff pilot in addition to fixtures.

Command capture retains a bounded last-output file, coalesced to approximately one write per second while output arrives and flushed at capture completion. A recovered command can show that saved tail but cannot reconnect its live stdout pipe. Verified ended generations are retired and eligible for seven-day cleanup. Unknown/unsealed generations are retained. Marked temporary Compose snapshots are eligible for cleanup after their original process creation identity is gone and a one-day grace period has passed.

The private IPC protocol identifies its build. A manager refuses to mutate sessions belonging to a different live build. Portable updates require the manager, host, and keeper to exit, and the update script refuses to overwrite a running installation. Runtime ownership records stay separate from portable configuration.
