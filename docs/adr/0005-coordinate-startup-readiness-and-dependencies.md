# Startup readiness and dependencies

Date: 2026-10-18

Status: Accepted for the command-workload slice

## Context

A started process does not establish that its server can accept requests. Project workloads can require another service or a successful finite startup action. The accepted plan requires safe timeout/retry behavior and dependency-aware shutdown while preserving the current session setup.

## Decision

Store readiness and prerequisite IDs on each workload in settings schema 4. Migrate versions 1–3 in memory without rewriting on load. Existing commands default to process-running readiness, a 60-second readiness timeout, and no prerequisites. Prerequisites refer to stable workload UUIDs within the same project. Reject self, duplicate, unknown, and cyclic edges; renaming a label does not change the relationship. The editor prevents removal of a referenced prerequisite until its references are unchecked.

Queue startup requests, then launch eligible entries as their prerequisites become ready. Independent entries continue after another entry fails. A finite startup action releases dependents only after successful completion and exit of its entire owned job. A service releases dependents after process-running, TCP-connect, or HTTP 2xx readiness. TCP initially accepts explicit IPv4/IPv6 socket addresses, without DNS. HTTP supports full HTTP(S) URLs, including query strings; TLS verification uses the system default. HTTP checks are direct GET requests without a proxy, redirects, cookies, or automatic authentication. Response bodies are ignored. Authenticated or proxy-only health endpoints require a later extension.

Network probes run on separate workers, with at most one pending probe per entry. TCP connection attempts have a 300 ms limit. Native WinHTTP has separate resolve/connect/send/receive timeouts (1000/500/500/500 ms); these are per-stage limits, not one hard total duration. The workspace actor never waits for a probe result. The first check starts immediately after command creation or Retry; further checks are at least 500 ms apart. Each result records its completion time, so a response received before the deadline is accepted even if the actor's next poll occurs after it. Attempt UUIDs discard results after cancellation or Retry. Readiness deadlines start after the owned command is created, excluding prerequisite waits and endpoint preflight.

Before creating a command with network readiness, check whether its configured endpoint already responds. An existing TCP connection or HTTP response of any status reports a conflict and leaves existing work alone. This best-effort check can race, and an unresponsive existing HTTP listener might not be detected; it is not proof that a later response belongs to the new process. It never grants ownership or termination rights. Process ownership remains the retained creation handles described in ADR 0004.

On readiness timeout, leave the owned process running and show Not ready. Retry rechecks that instance; it never starts a duplicate while any of its owned processes remain. Successful readiness ends probing. Later prerequisite failure is reported transitively on running dependents without cascading stops/restarts; new dependents stay queued. Retrying a failed prerequisite can release those queued entries. A manual Stop cancels a queued start and invalidates pending probes.

Normal project shutdown cancels queued starts, then stops entries in reverse dependency order. If a dependent remains live, keep its prerequisites alive and report the failure while independent entries may still stop. Normal single-entry Stop refuses to stop a prerequisite with live dependents. Explicit Force stop retains its existing scope and can stop a prerequisite; running dependents report the lost prerequisite. No automatic force escalation is introduced. Close/relaunch/quit retain their existing failure semantics and original session setup.

## Consequences and remaining work

Waiting, Checking endpoint, Starting, Not ready, Running, and terminal states are visible separately. The timeout is checked on the service poll cadence and can be reported slightly after its nominal deadline. Normal stop remains serialized on the background workspace worker; its existing per-entry shutdown deadlines can postpone another workspace operation. Rich progress/cancellation, Compose ownership, environment overrides, inactivity lifecycle, durable output, and reconnection remain follow-up work. This slice does not establish general application compatibility or release performance.

## References

- [WinHTTP timeouts](https://learn.microsoft.com/en-us/windows/win32/api/winhttp/nf-winhttp-winhttpsettimeouts)
- [WinHTTP response headers](https://learn.microsoft.com/en-us/windows/win32/api/winhttp/nf-winhttp-winhttpqueryheaders)
- [WinHTTP option flags](https://learn.microsoft.com/en-us/windows/win32/winhttp/option-flags)
- [Owned command workloads](0004-own-foreground-command-workloads.md)
