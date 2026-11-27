# Data stores

Everything the app writes sits in its own data directory (`$XDG_DATA_HOME/agent-app`, else `~/.local/share/agent-app`). It is created with mode 0700 and must be absolute, a real directory, owned by you and closed to group and others, or storage refuses to start. Debug builds honour `AGENT_APP_DATA_DIR` and `AGENT_APP_CONFIG`; release builds ignore them.

```mermaid
flowchart LR
  subgraph D["data directory"]
    W[("workspace.sqlite3")]
    SD["sessions/"]
    S[("sessions.sqlite3")]
    L["manager.lock"]
    E["exports/"]
    SG["signals/"]
    FL["flag files: desktop-notify, limit-alert-pct"]
  end
  SD --> S
  SG --> SF["claude-notify.jsonl, codex-notify.jsonl, claude-limits.json (written by the person's own hooks and status line; the app only reads)"]
  SD --> L
  E --> B["agentview-bundle-*.json"]
  E --> G["agentview-diagnostics-*.json"]
  A["allowlist.toml (config dir)"] -. "read only" .-> core["core::policy"]
  CJ["~/.claude.json"] -. "read only, redacted" .-> view["claude_config_view"]
  PR["Claude / Codex session records"] -. "read only, counters" .-> usage["usage refresh"]
```

## workspace.sqlite3 (owner: `workspace::Store`)

One connection behind a mutex. Opened without following symlinks, with a 5 s busy timeout and foreign keys on. Open runs in an immediate transaction and refuses a newer schema (`user_version` above 2). The journal mode is not changed from SQLite's default.

| Table | Purpose | Limits and notes |
|---|---|---|
| `agents` | simulated roster (demo only; 14 seeded agents) | hierarchy validated on open |
| `messages` | simulated conversations | body 8000 bytes, pages of 100, unique `request_id` for idempotent sends |
| `app_settings` | preferences as one JSON row | revision-checked saves, 256 KiB cap |
| `app_settings_recovery` | copy of a bad settings row before an explicit reset | |
| `manual_reviews` | the user's own PASS (manual review) label per file | bound to the content hash; stale when the file changes; not an approval |
| `notes` | Notes library pages | 2000 pages, 256 KiB each, stale saves refused |
| `note_revisions` | earlier text of a page | last 20 per page |

## sessions/sessions.sqlite3 (owner: `sessions::SessionManager`)

- `manager.lock` takes an exclusive file lock for the life of the manager so only one app instance owns the store.
- A second connection (`UsageView`, used by `usage_snapshot` and `usage_refresh`) has no lock and runs no migration, and it has its own mutex so a slow usage refresh never blocks terminal reads. It only writes `usage_readings`.
- Migrations are tracked by `user_version` (now 3); later columns (`remote_control`, `exit_info`, `exit_at`) are added by checking `pragma_table_info`. Open refuses a newer schema. On open, `running` and `launching` become `interrupted` and `bootstrapping` becomes `bootstrap_unknown`; nothing is relaunched and no input is replayed.

| Table | Purpose |
|---|---|
| `sessions` | up to 64 registered sessions: label, folder, executable identity, provider, provider session id, status, Remote Control flag, exit status and time |
| `usage_sources` | binds a Codex session to its record file |
| `usage_readings` | numeric counters per session, with source identity and a status such as `ok`, `partial`, `identity_mismatch`, `counter_reset` or `too_large` |

## Files the app writes

Only under `exports/`, with mode 0600 and create-new:

- `agentview-bundle-<seconds>-<pid>.json`: preferences and notes, with a SHA-256 integrity value (up to 24 MiB). Note text is exported as stored, without redaction, so do not keep secrets in notes or quick prompts.
- `agentview-diagnostics-<nanos>-<pid>.json`: a small projected report (up to 16 KiB).

The only browser-side storage is the sidebar-collapse flag in `localStorage`.

## Signals folder and flag files

`signals/` (mode 0700, owned by you) holds files that the person's own tools write and the app only reads (symlinks refused, size capped, fields validated, text redacted): `claude-notify.jsonl` (Claude Notification hook), `codex-notify.jsonl` (Codex `notify`, turn finished) and `claude-limits.json` (the status line's 5-hour and 7-day plan usage, 4 KB cap). The app creates the folder; the snippets in Settings tell the person what to add to their own Claude or Codex settings, and the app never edits those. Two small flag files in the data directory hold app choices: `desktop-notify` (desktop notifications on) and `limit-alert-pct` (plan-limit alert percentage).

## Away mode

`away` (flag, content = start time), `away-instruction.txt`, `away-seen` (time) in the data folder, written by the app (mode 0600); `decisions/pending.jsonl` (folder mode 0700) holds the questions agents park. `away.rs` reads the decisions and writes only its own three files.

## Queues

`queues/<name>.jsonl` (folder mode 0700) holds the event lines a scheduler or `scripts/adapters/lockdir-queue.sh` writes; `queues.rs` folds the last 256 KB (newest line per id wins) and the Flow page draws it (`ui/queues.js`). Read-only for the app.

## Project boards

`kanban/<project>.jsonl` (folder mode 0700) holds the event lines agents append for a project board; `kanban.rs` folds the last 256 KB (newest line per card id wins) and the Flow page draws it (`ui/board.js`). Read-only for the app.

## Tasks log and token counts

`signals/claude-tasks.jsonl` (folder mode 0700) holds one line per UserPromptSubmit (`k:start`, with the first prompt line, at most 200 characters) and Stop (`k:end`) hook call: `t`, `s` (session), `c` (cwd), `tp` (Claude's `transcript_path`). It only grows; the app never writes to it (delete it to clear the history; Settings shows its size). `tasks.rs` folds the last 256 KB, and for finished tasks reads the Claude transcript named by `tp`: only if it is an absolute `.jsonl` path inside `<Claude root>/projects/<project>/`, no symlink on the way, regular file, 32 MB read cap. From each line only `timestamp`, `sessionId`, `message.id` and the four numeric `message.usage` fields are deserialised, never message content; the numbers of lines inside the task's start..end seconds are summed (cache reads apart). Neither the transcript path nor any transcript text is sent to the page or logged. Claude Code's workflow run records (`projects/<slug>/<session>/workflows/wf_*.json`) are an observed, not specified, format and may change; unusable ones are counted as skipped.

## Mailbox

`mailbox/` (mode 0700) holds `messages.jsonl` (one JSON line per message, appended by agents) and `<file>.<name>.pos` cursor files written by each recipient's own receive hook. The app only reads `messages.jsonl` (`mailbox.rs`); messages are data, never approvals.

## Backups and project writes

`backups/` (mode 0700) holds copies of project files the person chose to back up before the app replaced them through the consent dialog (`projwrite.rs`), plus `manifest.jsonl`. The app never deletes them. It writes into a project folder only for the three fixed file names, only into a registered session's folder, never through a link, only after a preview and an explicit confirm.

## Read-only sources

- **Allow-list** (`allowlist.toml` in the config directory): must be a plain file owned by you, not group or other writable, up to 1 MB. Keys: `allow`, `deny_names`, `gate_file`. It is read once at startup, so edits need a restart. If it fails to load the app falls back to an empty allow-list and shows a banner.
- **Claude Code state file** (`~/.claude.json`): shown as structure only (folders, trust flags, counts, MCP server names with a command name or host). Never returns environment values, arguments, headers, tokens, account details or history text, and never writes.
- **Provider session records**: read for numeric usage and, for Claude, the name of the latest tool. Files must be owned by you, not group or other writable and not hard-linked; the path is derived from the stored session id only; size and per-refresh budgets apply.

## In-memory state (lost on exit)

- Terminal output: a ring buffer of 256 KiB per running session (at most 30 running, about 7.5 MiB of buffers in all). Reads return up to 64 KiB and report `truncated` if the offset fell out of the buffer; writes are at most 16 KiB. Terminal content is never saved.
- The "Needs you" list (30 lines, 6 hours, `notify.rs`), the session timeline (300 events, 24 hours, `notify.rs`), the time each session was started in this app run (`live.rs`, for "Changes in this project"), and the hourly send counters for webhooks and desktop notifications.
- `LiveState` (the session manager and the usage connection), `WorkspaceState` (the store) and the parsed allow-list.

## Path policy and redaction (`core::policy`)

A path is accepted only if it passes these steps in order: no NUL byte; canonicalised (symlinks followed); valid UTF-8; not under `/proc`, `/sys` or `/dev`; no component matches the deny list (33 built-in patterns plus `*.iso` and any `deny_names`; case-insensitive; config can only add); inside an allow root or equal to an allowed file. Reads open beneath the allow root with `openat2` (no symlinks, no magic links), re-check device and inode, and refuse hard-linked files.

`redact()` replaces obvious token shapes (private-key headers, `ghp_`-style, `github_pat_`, `sk-`, `AKIA`, `xox*-`, `Bearer …`, and quoted `key/secret/password/token` values of 12 or more characters) with `[redacted]`. It is best effort; the deny list is the main control. It is applied to demo messages on write and read, and to rendered views (review, notes preview, viewer pages). It is not applied to stored notes or bundles.
