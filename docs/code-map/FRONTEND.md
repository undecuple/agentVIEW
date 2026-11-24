# Front end

The workspace window loads one HTML page, `ui/workspace.html`, and a set of plain JavaScript files. There is no bundler. All files are compiled into the binary with `include_str!` and served by the `workspace://` scheme.

## Pages

`AppNav.navigate(page)` in `ui/prefs.js` shows one section and hides the rest. Valid page keys: `agents`, `sessions`, `overview`, `notes`, `cc`, `review`, `usage`, `settings`. An unknown key is ignored. `agents` is the simulated demo and falls back to `sessions` unless demo mode is on.

| Page key | Section id | Sidebar button | Script |
|---|---|---|---|
| agents (demo only) | `deck` | `show-agents` | `workspace.js` |
| sessions | `live` | `show-live` | `live.js` |
| overview | `overview` | `show-overview` | `overview.js` |
| notes | `notes` | `show-notes` | `notes.js` |
| cc (Claude config) | `cc` | `show-cc` | `claudecfg.js` |
| flow | `flow` | `show-flow` | `flow.js` |
| git | `git` | `show-git` | `git.js` |
| (Claude config tabs) Settings, Skills & agents | in `cc` | `show-cc` | `claudecfg.js` |
| (Settings tabs) General, Data, Notifications, About | in `settings` | `show-settings` | `tabs.js` (shows or hides blocks only) |
| (Settings card) webhook | in `settings` | `show-settings` | `webhook.js` |
| (Settings card) backup | in `settings` | `show-settings` | `backup.js` |
| review (Pre-PR review) | `review` | `show-review` | `review.js` |
| usage | `usage` | `show-usage` | `usage.js` |
| settings | `settings` | `show-settings` | `settings.js` (bundle export and import buttons are wired in `notes.js`) |

The sidebar has a ninth button, Documents (`open-docs`), which is not a page: it calls `workspace_open_docs` and opens the separate read-only viewer window. Sidebar groups: WORKSPACE (Agents when demo is on, Overview, Sessions, Documents), TOOLS (Notes, Claude config, Pre-PR review, Token usage), APP (Settings). The sidebar collapses to icons; the choice is kept in the browser's `localStorage` under one key and works without it.

## Shared helpers (all in `ui/prefs.js`, loaded first)

- `AppIPC.invoke(command, args)`: wraps the Tauri internal invoke.
- `AppPrefs`: `snapshot`, `current`, `save`, `reset`, `reload`, `patch`, `remember`, `ready`. Writes go through a queue with an optimistic revision number.
- `AppNav.navigate(page)`: described above.
- Events on `window`: `agentview:navigate` (each page script starts or stops its polling), `agentview:preferences`, `agentview:theme` (the terminal re-themes).

## Themes and styles

Stylesheets load in this order: `workspace.css` (layout), `vendor/xterm.css`, `settings.css` (Cyberdream Light variables and settings panels), `identity.css` (brand, sidebar, console layout, the Molokai-style dark palette and the two PaperColor palettes). `identity.css` is last and overrides the others.

Setting values: `dark` (Molokai Dark, the default), `light` (Cyberdream Light), `papercolor-light`, `papercolor-dark`, `system` (follows the operating system and picks `light` or `dark` only). `appearance()` in `prefs.js` sets `data-theme` and `data-density` on the root element and swaps the sidebar mark between the dark and light SVG.

## How assets are embedded and served

- `agent_app_workspace::asset(path)` (in `workspace/src/lib.rs`) is an exact-match table of 18 routes: the page, three stylesheets, the page scripts, the vendored terminal library files and the two logo SVGs. Anything else, including paths with `..`, a query or a double slash, returns nothing (a test asserts this).
- To add a script: create `ui/<name>.js`, add a `<script defer src="/<name>.js">` tag to `workspace.html`, and add a route with `include_str!` in `asset()`.
- `src-tauri/src/main.rs` registers the `workspace` scheme. It answers only `GET` to host `localhost` with no port and no query, and sets these headers: a content-security policy (`default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src ipc: http://ipc.localhost; img-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'`), `Cache-Control: no-store`, `nosniff`, `Referrer-Policy: no-referrer` and `Cross-Origin-Resource-Policy: same-origin`.
- Test-only routes (`/__workspace-test.js`, `/__workspace-report` and others) exist only with the `workspace-e2e` feature.
- A second scheme, `viewer://`, serves the read-only document viewer through `core::router::handle`.

## Notes on safety in the UI

Rendered Markdown comes from `core::markdown::render` (escape first, links shown as text, no HTML, no images). Page scripts set `textContent` for user data and use `innerHTML` only for that rendered output.
