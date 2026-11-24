# agent-app spec

A local desktop workspace with a read-only viewer for notes, gate status, diffs and images. MIT licence. Product name: agentVIEW; internal packages and configuration retain the `agent-app` name.

## Foundation workspace (2026-11-21)
The default window presents two simulated provider intake agents (Claude and Codex), project coordinators reporting to their intake, and worker/refuter subagents reporting to a coordinator. Hierarchy view shows that chain; project view groups coordinators from both providers. Provider, project and availability filters retain parent context.

The workspace stores its own demo roster, availability and conversations in a private local SQLite database. Messages are persisted before synthetic delivery; busy/paused recipients remain queued. Delivery and one simulated reply commit atomically, with idempotent send requests and bounded history pages (100 rows per query, at most 1000 recent rows displayed). Restart preserves history and availability. The simulated roster launches or attaches to nothing and Zellij sessions are untouched. (Real sessions are a separate page; see Live sessions below.)

Workspace commands are allowed only in the workspace window at its exact origin. They cannot choose arbitrary paths, senders, processes or hierarchy assignments. The separate document window has no workspace capabilities. Token usage is grouped by agent/provider but marked unavailable until real adapters supply telemetry; no tokenizer estimate substitutes for measurement. Jobs and instruction editing remain later milestones.

## Rules the app keeps
- It never grants an approval. An approvals view shows what a person must decide and where they decide it.
- Credentials: the app shows names and where a secret lives, never the value, in the UI, logs or repo. In the viewer this is enforced by deny rules on file names plus a best-effort redaction of obvious token shapes in displayed text. Redaction is not a guarantee, so secrets must not sit in allow-listed files.
- Read-only path allow-list. Paths are resolved with realpath (symlinks followed) before the deny rules, then checked against explicitly configured files or directories. An allowed directory includes its descendants subject to deny rules. Keys, credentials and build folders are denied. Hardlinks are refused by both viewers. A diff view never renders a denied file.
- It runs Claude Code or Codex only as the user's own explicitly started sessions with their existing permission modes (the Sessions page). The simulated roster runs nothing.
- No server beyond localhost. Files or SQLite, no database server until a need is shown.
- Synthetic data only in tests and screenshots.

## First milestone: read-only viewer
Views: shared docs, gate status (from a gate log file), recent file changes with diffs, images.
Data sources: files under an explicit allow-list in a config file. Document browsing never writes these files. The separate workspace writes only its own local application database.
Done when: it starts, lists the allow-listed docs, shows a diff of a synthetic file, and a test proves an excluded path is refused.

## Pages added after the first milestone
Sessions (real CLI terminals, Remote Control toggle, stop-all, exit status), Overview (observable data only: no sub-agent or network-bandwidth data), Token usage, Settings (themes, preferences, bundle), Pre-PR review (manual labels, not approvals), Notes and the Claude config view (below). See docs/code-map/README.md.

## Later, not started
Approval views, sandbox settings, subagent activity, agent-to-agent messaging, jobs.

## Image support
The viewer shows images from allow-listed paths. Same realpath, deny and allow-list policy as text; raster types only (png, jpg, gif, webp) checked by file signature, no SVG; served as images with `nosniff` and a CSP that allows only same-origin images; no image from a denied folder. `/img` serves files up to 5 MB, 16384 px a side and 16 megapixels, after a signature, extension and header-size check. Files over the cap are refused, never cut. No decoding, and no metadata (EXIF, text chunks) is read or shown. A doc page shows images referenced by relative markdown links only when they pass the same path policy; http, absolute, `~`, `data:` and denied-folder references are listed as not shown.

## Viewer limits
/view is capped at 1 MB and /gate at 16 MB, each with a visible "Truncated" notice. /diff reads 200 KB per file and prints at most 2000 lines. The gate list is a best-effort text parse and says so on the page; an empty list does not mean nothing is pending. Files are opened without following symlinks and checked against the path that passed the policy.

## Gate log order
The gate parse assumes the log lists rows newest first. A row is closed only by a row above it. If the file is ordered the other way, approved items show as pending.

## Running the viewer (stdlib only, Python 3.11+)
`cp allowlist.example.toml allowlist.toml`, then `python3 viewer/server.py --gate-file <path-to-gate-log>` and open http://127.0.0.1:8765/. Tests: `python3 -I -m unittest discover -s tests`.
After cloning run `git config core.hooksPath .githooks`. The hook can be skipped, so `.github/workflows/ci.yml` runs the same scan (`scripts/scan-files.sh`) on every commit, plus an identity and attribution-line check and the tests.

## Notes library and bundles
The workspace has a Notes page: Markdown pages grouped on shelves, kept in the app's own database (not in repositories or shared documents, so the read-only viewer rule is unchanged). Editor with toolbar, split live preview using the same safe renderer (no HTML, links as text, no images or scripts), search, save history (last 20 versions per page), stale-save refusal, delete with a second confirming click. Limits: 256 KB a page, 2000 pages.

Export and import bundle (Settings): one JSON file holding preferences and the Notes library with a SHA-256 integrity check and a format number. Not included: sessions, terminal content, conversations, usage records, review labels, allow-lists, paths, credentials. Quick-prompt text is part of the preferences and is exported as typed, so do not keep secrets in quick prompts. Bundles are written to and read from the app's private exports folder only (mode 0700, files 0600); the interface passes a listed file name, never a path. Import is check-then-import: pages are only added (a title clash is renamed with " (imported)"), nothing is overwritten or deleted, and preferences are replaced only when the box is ticked, through the normal validated save.

## Git view
A read-only page that shows, for each git repository that contains an allow-listed path: branch, ahead/behind counts against the upstream already known locally, changed and new files, the last 15 commits, and whether those commits use the repository's own configured e-mail. It runs only `git status --porcelain=v2 --branch -z --untracked-files=normal`, `git log -n15` with a fixed format, and `git config --local --get user.name` / `user.email`, each with a fixed argument list, a 5 second limit, 1 MB of output, no pager, no fsmonitor, no hooks path and a cleared environment. The page passes a repository path only to pick one from the listed set. File names matching the deny rules are counted but not listed; commit subjects pass the token redaction. There is no add, commit, fetch, pull, push, checkout, stash or config change, so the view cannot move a repository or contact a remote.

## Webhook notifications
A card in Settings stores up to five named webhook destinations. Each destination chooses its events: a running session with no terminal output for 10 minutes ("quiet"), a session that stopped with an error after running in this app session ("error"), or one that finished with exit 0 ("finished"). Defaults are quiet and error. The message names the agent and says it "may be waiting"; the app does not see approvals and never claims one is pending. A background check runs every 30 seconds, reads only the same activity data as the Overview, never blocks terminal actions, and sends each condition once (the quiet condition re-arms after output resumes). At most 20 messages an hour per destination.

Each address is treated as a secret: it is accepted only as `https://` with a public-looking host name (no user info, no IPv6 or private, loopback or link-local addresses, no local names), is kept only in the app's private database, is never returned to the page (only its host name), is not part of export bundles or diagnostics, and is passed to `curl` on standard input, never on a command line. The only subprocess is `curl -q --silent --show-error --proto =https --proto-redir =https --max-redirs 0 --max-time 10 ... --data-binary <message> -K -` with a cleared environment (PATH, locale, certificate and proxy variables only). Names that resolve to a private address are not detected. This is the app's first outbound network use; it is off until an address is saved and notifications are ticked, and the settings page is the only way to change it. Editing a destination keeps its saved address unless a new one is pasted. An earlier single address is migrated as "Default".

## Claude hook signal
The only reliable way to know that Claude Code is waiting for a person is Claude Code itself, through its Notification hook (fields `session_id`, `cwd`, `message`, `notification_type`). The Notifications tab in Settings shows a hook snippet that the person pastes into their own `~/.claude/settings.json`; the app never edits Claude's settings. The hook appends one small JSON line (time, session id, folder, notification type, message) to `claude-notify.jsonl` in the app's private `signals` folder (mode 0700). The app only reads that file: symlinks refused, complete lines only, lines older than the app start ignored, fields validated, text stripped of control characters and passed through token redaction. A permission, elicitation or agent-input notification becomes a "waiting for your decision" message and an idle prompt becomes "waiting for your input", sent to destinations that chose the "Claude is waiting" event, once a minute per session and type, naming the registered session for that folder or the folder name (never the path). A signal is data from Claude Code: it never answers a prompt, never approves anything and is not an approval state.

## Claude settings and inventory
The Claude config page has two more read-only tabs. Settings shows permission rules (allow, deny, ask; token shapes redacted), the default permission mode, hooks (event, matcher, type and the program's file name only), environment variable names (never values), the model, enabled plugin names, and which helper commands exist (never their text), from `settings.json` and `settings.local.json` in the Claude config folder. Skills & agents lists skills, commands and agents with name and description (and an agent's model and tools) taken from the first 4 KB of each file; bodies are never returned. Symlinked entries are followed only to plain files whose path passes the deny names. Nothing is written or run.

## Claude memory and session history
Two more read-only tabs on the Claude config page. Memory lists and shows Markdown files from fixed places only: `CLAUDE.md` in the Claude config folder, `projects/<slug>/memory/*.md`, and `CLAUDE.md` / `AGENTS.md` in allow-listed folders. The page passes a key that the app re-resolves against its own listing (no path is ever accepted); each file is resolved with realpath and must pass the deny names, is capped at 64 KB, has control characters stripped and token shapes redacted. History lists past Claude sessions (project folder name, session id, size, modification time) with a copy-able `claude --resume <id>` line; it reads only directory metadata and never opens a conversation file. Nothing is written or run.

## Time per project
The Overview shows, for each project, how long its sessions ran and how long they were active (terminal output within the last 2 minutes), for today, the last 7 days and all time. The 30-second background check adds the elapsed time to daily totals (UTC days) in the app's own database; a project with several sessions counts once. It only counts while the app is open and measures terminal activity, not time at the keyboard. Totals older than 400 days are dropped, are not part of export bundles, and cannot be edited from the page.

## Layout
The workspace uses a floating rounded sidebar, separate summary tiles with an icon, rounded cards, pill tabs on Settings (General, Data, Notifications, About; they only show or hide blocks) and a "By state" bar list on the Overview. The Overview flow tile draws a small sparkline of the terminal data rate seen since the page was opened. These are presentation only and add no data or commands.

## Polish
Presentation and keyboard only, no new data or commands. Disabled buttons stay readable (dashed outline, muted text) and Sessions shows a one-line reason when Start is disabled. The empty terminal area says what to do. Session tiles lead with a status dot and show the provider as a small chip. Buttons, selects and text inputs share one minimum height and radius, and keyboard focus is always visible. Alt+1 to Alt+9 open the nth sidebar page and Esc closes the open section or leaves the field; neither works inside a terminal, where those keys belong to the program. Below 1000 px wide the sidebar folds to icons unless the person has chosen to expand or collapse it. Settings, About shows the version and build id (the short commit).

## Flow view
A read-only page drawing you, the providers and the registered sessions as a graph, with one status dot per session: running, quiet for 10 minutes, stopped with an error, or not running. It uses only the activity data of the Overview (no new command). It shows no approval state, no message edges and no sub-agents, because the app cannot observe them.

## Backup with rclone
A card in Settings copies the app's own exports folder (the bundles) to a remote already configured in rclone, into `agentview-backup/exports`. The app runs only `rclone listremotes` and `rclone copy <exports folder> <remote>:agentview-backup/exports --immutable --max-depth 1` (with `--dry-run` for the preview), with a cleared environment apart from PATH, HOME and XDG_CONFIG_HOME, and a time and output limit. `copy` only adds files, so nothing is deleted or overwritten at the destination. The remote must be one that `listremotes` printed; only names are shown, never rclone's settings or raw output. A real copy is accepted only after a dry run for the same remote in the same app session (ten minutes, one copy per dry run) and a second confirming click. Setting up remotes stays in rclone itself.

## Claude config view
A read-only page for `~/.claude.json`: project folders with trust flag, allowed-tool count, history count and MCP server names (kind and command or host name only), plus global MCP servers. Environment values, arguments, headers, tokens, account details and history text are never returned to the page. A server URL is reduced to its host name (no path, query or user info) and a command to its first word's file name. The file is never written, because Claude Code rewrites it while running. Backup, diff and edits are deliberately not offered.
