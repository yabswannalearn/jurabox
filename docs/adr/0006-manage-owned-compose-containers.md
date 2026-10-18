# Manage Compose containers by verified identity

Date: 2026-10-18

Status: Accepted for the local Docker Desktop slice

## Context

A Docker client process can exit while its containers keep running. Windows Job Objects establish ownership of the client, but do not establish ownership of daemon-managed containers. Docker's ordinary stop command also escalates to a forced kill after its grace period, conflicting with Jurabox's explicit Force stop rule.

## Decision

Add a typed Docker Compose workload alongside command services and startup actions. Its saved setup contains the Docker executable, Compose file, optional local context, working folder, readiness/dependencies, and shutdown deadline. Settings schema 5 migrates versions 1–4 in memory until Save.

Use the installed Docker CLI and Compose plugin. Each workload has a separate Rust worker so CLI requests do not block the UI or other workspace operations. The first context lookup selects a local named-pipe endpoint; all later calls use that pinned endpoint, with inherited Docker endpoint/context and Compose control overrides removed from the child environment. Verify the engine ID on subsequent discovery. Initially require a Linux Docker Desktop engine. Remote/TLS endpoints need a separate validation pass.

Derive a stable private Compose project name from the Jurabox project/workload IDs. Resolve the Compose file through `compose config`, preserving its canonical escaping. Store a temporary snapshot under the user's local Jurabox data directory; resolved environment values can be present there, so it is not an export artifact. Keep the snapshot for the session, and remove it when the worker ends. Stopping/starting an existing session does not reread the source file. Existing named volumes keep stable names across deliberate relaunches. Runtime snapshots are not yet durable recovery records.

Before creating anything, reject existing containers in the namespace or with the same Jurabox project/workload labels. Create containers without starting them, using `compose create --no-build --no-recreate --pull missing`. Inject a random session owner token and full project/workload identity into container labels. Discover exact full container IDs and creation identities, verify the labels and expected service set, then start only those IDs. A matching name, image, port, or successful client exit never establishes ownership. Unknown containers, including leftovers from a prior manager run, produce a conflict and remain unmanaged. Incomplete/uncertain operations remain tracked and prevent duplicates.

Start independent roots before dependent services. Honor `service_started`, `service_healthy`, and `service_completed_successfully` inside the file. Jurabox prerequisites can wait for the complete Compose workload. Internal dependency waits share the configured startup budget after creation; network readiness uses the same start time. Pull/create work has a separate bounded client wait. A timed-out client remains tracked rather than being silently abandoned. Explicit Force stop can cancel that owned client before checking its containers again.

Normal Stop rechecks identities and sends each container's configured stop signal, defaulting to SIGTERM, through `container kill --signal`. It never uses the default kill signal or an escalating `stop` command. Reject SIGKILL and numeric signal 9 for normal shutdown, including equivalent numeric spellings. Stop dependents first; retain prerequisites if a dependent stays alive, while independent shutdown may continue. A timeout leaves surviving containers managed and running. Explicit Force stop sends SIGKILL only after current ownership verification. If the engine is unavailable or identities are uncertain, retain management state and report the problem.

Stop retains containers for a later Start. Close, deliberate Relaunch, and Stop services and quit remove verified stopped containers without force or volume removal. Their writable container layers are removed; named volumes, bind-mounted files, images, and networks are retained. No prune, broad name match, orphan removal, or Compose down is used. Ordinary Quit stays guarded while containers or uncertain operations remain owned; users can close the session or use Stop services and quit.

Container output is captured by owned read-only `docker logs` clients, with bounded tails and an aggregate display limit. Cancelling these clients during cleanup does not stop containers. Status checks run outside the workspace actor, normally every two seconds while active, with faster polling during shutdown. Readiness remains a startup gate. Container exit and identity loss are reported without automatic replacement or cascading shutdown of other Jurabox workloads.

## Initial boundaries

Support 1–32 services with one container each. Custom images must be built first and have an image name; missing images can be pulled. Reject explicit container names, profiles, restart policies, lifecycle hooks, optional/restarting dependencies, and multiple replicas before creating containers. These features require additional ownership and shutdown validation. Existing Compose configuration can still use environment/files and named/external resources; Jurabox does not manage the lifecycle of external resources.

Recovery after a manager crash remains unfinished. Labels identify conflicts but do not authorize adopting an older session. Durable ownership records, verified reconnection, cleanup of abandoned snapshots, environment override forms, inactivity policies, richer startup progress, and distribution/performance pilots remain follow-up work. No broad Compose compatibility claim is made by this slice.

## References

- [Compose config](https://docs.docker.com/reference/cli/docker/compose/config/)
- [Compose create](https://docs.docker.com/reference/cli/docker/compose/create/)
- [Container signals](https://docs.docker.com/reference/cli/docker/container/kill/)
- [Docker Engine API](https://docs.docker.com/reference/api/engine/version/v1.51/)
- [Owned command workloads](0004-own-foreground-command-workloads.md)
