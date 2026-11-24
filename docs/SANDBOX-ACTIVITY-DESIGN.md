# Sandbox and activity management proposal

Status: requested design/backlog, not implemented.
The release-workflow work does not change Bubblewrap profiles or host permissions.
Live session adapters are a prerequisite for reliable runtime observations.

## Sandbox inspector, then an editor

Add a Sandbox view linked from each intake agent, coordinator and subagent.
Show the managed session's working directory, provider permission mode,
Bubblewrap version/profile revision when applicable, read-only/read-write binds,
network policy and the source/freshness of those observations. Mark unavailable
information unknown rather than infer it from a failed command.

Start with read-only inspection and named per-project presets. A later editor
should show a diff before changing filesystem or network permissions, save a
versioned backup, require explicit user approval and offer a synthetic test run.
Edits affect newly launched app-owned sessions; changing a file must not claim
to have reconfigured an already running sandbox. No blanket “disable sandbox”
button, privileged host changes or automatic retry with broader access.

[Bubblewrap's own documentation](https://github.com/containers/bubblewrap)
describes a low-level construction tool; the invoking application chooses the
policy. Design agentVIEW around the profile actually used by its session adapter.

## Blocked activity

Provide a searchable timeline and a blocked-session/application list for sessions
managed by agentVIEW. For each denied action, record:

- Agent, intake/coordinator ancestry, project, session and job IDs.
- Attempted action category and redacted command/path/network target.
- Decision source: provider approval, Bubblewrap setup, kernel/permission failure,
  app path policy, user rejection, or unknown.
- Reason, timestamp, profile revision, approval/request ID and available evidence.
- Safe next step: review the profile, request a scoped change, retry after a
  reviewed correction, or leave blocked. Never treat a timeout as approval.

Keep “application could not start” distinct from “one action was denied.”
Bubblewrap is not a system-wide registry of blocked apps; app-owned launchers
and provider adapters must supply observable events. A global host monitor is
outside this proposal and would need a separate scope and authorization.

## Database decision

Keep the existing local SQLite database as the initial source of truth for
conversations, jobs, activity and approvals. Add migrations, stable event IDs,
indexed per-session timelines, bounded queries and configurable retention.
Record decisions as new events instead of rewriting their history. Redact and
bound event payloads; don't store credentials or full terminal dumps by default.

Put storage behind a small app-owned interface so a future PostgreSQL backend
can preserve the same transaction and claim semantics. Do not add two active
database implementations before a server/multi-PC deployment needs them.
[SQLite's guidance](https://www.sqlite.org/whentouse.html) distinguishes local
application storage from shared client/server workloads. A remote phone/browser
should talk to the PC service; it should not open the SQLite file over NetBird.

Acceptance for a later PostgreSQL backend includes migration/backup/restore,
atomic job claims, idempotent message delivery, revision-pinned findings and
equivalent access checks across both stores. A “compatibility” toggle without
those checks is not sufficient.

## Other suggested priorities

1. **Pause/stop all:** stop new work promptly, preserve queues and show exactly
   which app-owned processes are paused, stopping or still active.
2. **Approval inbox:** show the command/diff, affected paths, requester, reason and
   current permission scope before a person decides. Preserve provider controls.
3. **Project/worktree ownership:** show branch and working directory, surface
   conflicting owners, and prevent accidental concurrent edits through job locks.
4. **Recovery and export:** transactional backups, schema-version checks, replay
   of interrupted work, and a redaction preview before exporting activity.
5. **Measured budgets:** separate token receipts, subscription telemetry and API
   spend; label stale/unknown figures and configurable pause thresholds clearly.

## Acceptance for the first sandbox slice

Use synthetic sessions to prove denial reasons retain their source, stale
profiles are labeled, out-of-scope writes cannot widen permissions, parent and
project filters work, private payloads are redacted, and restart preserves the
timeline. Then validate with one real app-owned adapter before claiming live
Bubblewrap management or visibility into blocked applications.
