# Front end

The workspace window loads one HTML page, `ui/workspace.html`, and a set of plain JavaScript files. There is no bundler. All files are compiled into the binary with `include_str!` and served by the `workspace://` scheme.

## Pages

`AppNav.navigate(page)` in `ui/prefs.js` shows one section and hides the rest. Valid page keys: `agents`, `sessions`, `overview`, `notes`, `cc`, `review`, `usage`, `settings`. An unknown key is ignored. `agents` is the simulated demo and falls back to `sessions` unless demo mode is on.

| Page key | Section id | Sidebar button | Script |
|---|---|---|---|
| agents (demo only) | `deck` | `show-agents` | `workspace.js` |
| sessions | `live` | `show-live` | `live.js` |
| overview | `overview` | `show-overview` | `overview.js` |
| notes | `notes` | `show-notes` | `notes.js` (also the Ctrl+K page switcher, `#qs-dialog`) |
| cc (Claude config) | `cc` | `show-cc` | `claudecfg.js` |
| flow | `flow` | `show-flow` | `flow.js` |
| git | `git` | `show-git` | `git.js` |
| (Claude config tabs) Settings, Skills & agents | in `cc` | `show-cc` | `claudecfg.js` |
| (Settings tabs) General, Data, Notifications, About | in `settings` | `show-settings` | `tabs.js` (shows or hides blocks only) |
| (Settings cards) webhook, Claude hook, Codex notify, Claude plan limits, desktop notifications | in `settings` | `show-settings` | `webhook.js` |
| (Settings card) backup | in `settings` | `show-settings` | `backup.js` |
| review (Pre-PR review) | `review` | `show-review` | `review.js` |
| usage | `usage` | `show-usage` | `usage.js` |
| settings | `settings` | `show-settings` | `settings.js` (bundle export and import buttons are wired in `notes.js`) |

The sidebar has a ninth button, Documents (`open-docs`), which is not a page: it calls `workspace_open_docs` and opens the separate read-only viewer window. Sidebar groups: WORKSPACE (Agents when demo is on, Overview, Sessions, Documents), TOOLS (Notes, Claude config, Pre-PR review, Token usage), APP (Settings). The sidebar collapses to icons; the choice is kept in the browser's `localStorage` under one key and works without it.

Page details added later: `away.js` is the Overview "I'm stepping away" card (`away_status`: toggle, parked questions, hook and instruction text). `queues.js` draws the Flow page's hidden-until-used Queues card from `queues_view` (running, waiting in order, reserved, over-estimate marker; refresh every 5 s while visible). `board.js` draws the Flow page's project board from `kanban_projects` and `kanban_board` (refresh every 5 s while visible, stale marker after an hour). `projwrite.js` is the consent dialog (`window.ProjWrite.open`) used by the Onboarding card and the Handoff note; it calls `project_file_preview` then `project_file_write` (two-step confirm, optional backup). `roles.js` (loaded right after `prefs.js`) holds the built-in role onboarding documents and one-line role prompts as fixed text; it fills the "Onboarding templates" card on the Notes page and adds the role prompts to the quick-prompt library in `settings.js`. `issuehelper.js` builds the "Report a problem or suggest something" card in Settings, About (copy buttons and a prefilled link to the GitHub issue forms; it sends nothing). The Settings tab groups in `tabs.js` must list every card of a tab, or the card shows on all tabs. `resumestate.js` (loaded after `overview.js`, on the Overview page) joins `live_activity` with `claude_memory_list` to show whether each session's notes files look current, and builds a handoff note from `session_changes` and the file list (copy, or save to the Notes shelf "Handoffs" with `notes_save`); it adds no command. `overview.js` also shows the "Needs you" signals (`attention_list`, per-line Hide kept in `localStorage`) and the plan-limit line (`claude_limits_status`); `flow.js` adds a "Recent activity" table (`timeline_list`); `live.js` adds the Resume panel for interrupted sessions (it calls the same `live_start` as the Start button, with the same gating) and the "Changes in this project" section (`session_changes`, refreshed every 10 s while open); `settings.js` holds the quick-prompt editor (up to 24) and the built-in prompt library.

## Shared helpers (all in `ui/prefs.js`, loaded first)

- `AppIPC.invoke(command, args)`: wraps the Tauri internal invoke.
- `AppPrefs`: `snapshot`, `current`, `save`, `reset`, `reload`, `patch`, `remember`, `ready`. Writes go through a queue with an optimistic revision number.
- `AppNav.navigate(page)`: described above.
- Events on `window`: `agentview:navigate` (each page script starts or stops its polling), `agentview:preferences`, `agentview:theme` (the terminal re-themes).

## Themes and styles

Stylesheets load in this order (`calm.css` is last and holds the Calm look: palette, sans-serif type, one card style, spacing scale): `workspace.css` (layout), `vendor/xterm.css`, `settings.css` (Cyberdream Light variables and settings panels), `identity.css` (brand, sidebar, console layout, the Molokai-style dark palette and the two PaperColor palettes). `identity.css` is last and overrides the others.

Setting values: `dark` (Calm Dark, the default), `light` (Calm Light), `papercolor-light`, `papercolor-dark`, `system` (follows the operating system and picks `light` or `dark` only). `appearance()` in `prefs.js` sets `data-theme` and `data-density` on the root element and swaps the sidebar mark between the dark and light SVG.

## How assets are embedded and served

- `agent_app_workspace::asset(path)` (in `workspace/src/lib.rs`) is an exact-match table of 18 routes: the page, three stylesheets, the page scripts, the vendored terminal library files and the two logo SVGs. Anything else, including paths with `..`, a query or a double slash, returns nothing (a test asserts this).
- To add a script: create `ui/<name>.js`, add a `<script defer src="/<name>.js">` tag to `workspace.html`, and add a route with `include_str!` in `asset()`.
- `src-tauri/src/main.rs` registers the `workspace` scheme. It answers only `GET` to host `localhost` with no port and no query, and sets these headers: a content-security policy (`default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src ipc: http://ipc.localhost; img-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'`), `Cache-Control: no-store`, `nosniff`, `Referrer-Policy: no-referrer` and `Cross-Origin-Resource-Policy: same-origin`.
- Test-only routes (`/__workspace-test.js`, `/__workspace-report` and others) exist only with the `workspace-e2e` feature.
- A second scheme, `viewer://`, serves the read-only document viewer through `core::router::handle`.

## Notes on safety in the UI

Rendered Markdown comes from `core::markdown::render` (escape first, links shown as text, no HTML, no images). Page scripts set `textContent` for user data and use `innerHTML` only for that rendered output.

`board.js` also offers "Save board as Markdown…": it builds a snapshot and opens the project-write dialog with the fixed name `BOARD.md` (preview, confirm, optional backup).

Documents page: `docs.js` (nav item `show-docs`, page `docs`). Folders and files shared with the app are added by typing a path, a review step shows the fully resolved path (`docs_check`), and a second click saves it (`docs_add` with that exact path). The list is stored in the app data folder (`docs-shared.json`, 0600, atomic writes, change log `docs-share.log`), merged with an optional `allowlist.toml`, and re-checked on every call. Git, Pre-PR review, Claude memory and the older viewer window all read this merged list.
