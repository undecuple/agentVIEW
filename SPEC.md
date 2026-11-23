# agent-app spec

A local desktop workspace with a read-only viewer for notes, gate status, diffs and images. MIT licence. Product name: agentVIEW; internal packages and configuration retain the `agent-app` name.

## Foundation workspace (2026-11-21)
The default window presents two simulated provider intake agents (Claude and Codex), project coordinators reporting to their intake, and worker/refuter subagents reporting to a coordinator. Hierarchy view shows that chain; project view groups coordinators from both providers. Provider, project and availability filters retain parent context.

The workspace stores its own demo roster, availability and conversations in a private local SQLite database. Messages are persisted before synthetic delivery; busy/paused recipients remain queued. Delivery and one simulated reply commit atomically, with idempotent send requests and bounded history pages (100 rows per query, at most 1000 recent rows displayed). Restart preserves history and availability. No real Claude Code/Codex process is launched or attached to, and Zellij sessions are untouched.

Workspace commands are allowed only in the workspace window at its exact origin. They cannot choose arbitrary paths, senders, processes or hierarchy assignments. The separate document window has no workspace capabilities. Token usage is grouped by agent/provider but marked unavailable until real adapters supply telemetry; no tokenizer estimate substitutes for measurement. Live multi-session wrappers, jobs and instruction editing remain later milestones.

## Rules the app keeps
- It never grants an approval. An approvals view shows what a person must decide and where they decide it.
- Credentials: the app shows names and where a secret lives, never the value, in the UI, logs or repo. In the viewer this is enforced by deny rules on file names plus a best-effort redaction of obvious token shapes in displayed text. Redaction is not a guarantee, so secrets must not sit in allow-listed files.
- Read-only path allow-list. Paths are resolved with realpath (symlinks followed) before the deny rules, then checked against explicitly configured files or directories. An allowed directory includes its descendants subject to deny rules. Keys, credentials and build folders are denied. Hardlinks are refused by both viewers. A diff view never renders a denied file.
- It does not run Claude Code or Codex. A later version could do so only as the user's own sessions with existing permission modes.
- No server beyond localhost. Files or SQLite, no database server until a need is shown.
- Synthetic data only in tests and screenshots.

## First milestone: read-only viewer
Views: shared docs, gate status (from a gate log file), recent file changes with diffs, images.
Data sources: files under an explicit allow-list in a config file. Document browsing never writes these files. The separate workspace writes only its own local application database.
Done when: it starts, lists the allow-listed docs, shows a diff of a synthetic file, and a test proves an excluded path is refused.

## Later, not started
Pending PR and approval views, sandbox settings, subagent activity, session driving.

## Image support
The viewer shows images from allow-listed paths. Same realpath, deny and allow-list policy as text; raster types only (png, jpg, gif, webp) checked by file signature, no SVG; served as images with `nosniff` and a CSP that allows only same-origin images; no image from a denied folder. `/img` serves files up to 5 MB, 16384 px a side and 16 megapixels, after a signature, extension and header-size check. Files over the cap are refused, never cut. No decoding, and no metadata (EXIF, text chunks) is read or shown. A doc page shows images referenced by relative markdown links only when they pass the same path policy; http, absolute, `~`, `data:` and denied-folder references are listed as not shown.

## Viewer limits
/view is capped at 1 MB and /gate at 16 MB, each with a visible "Truncated" notice. /diff reads 200 KB per file and prints at most 2000 lines. The gate list is a best-effort text parse and says so on the page; an empty list does not mean nothing is pending. Files are opened without following symlinks and checked against the path that passed the policy.

## Gate log order
The gate parse assumes the log lists rows newest first. A row is closed only by a row above it. If the file is ordered the other way, approved items show as pending.

## Running the viewer (stdlib only, Python 3.11+)
`cp allowlist.example.toml allowlist.toml`, then `python3 viewer/server.py --gate-file <path-to-gate-log>` and open http://127.0.0.1:8765/. Tests: `python3 -I -m unittest discover -s tests`.
After cloning run `git config core.hooksPath .githooks`. The hook can be skipped, so `.github/workflows/ci.yml` runs the same scan (`scripts/scan-files.sh`) on every commit, plus an identity and attribution-line check and the tests.
