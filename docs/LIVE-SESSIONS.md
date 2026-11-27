> Evidence figures here are snapshots (current command count: 40). Added since: Remote Control toggle and session names for Claude sessions, stop-all, kept exit status with an interrupted/ended banner, base64 terminal reads, hidden-window-aware polling, and a separate usage connection. See the [code map](code-map/README.md).

# Live Claude session milestone

**Current follow-up:** [shared Claude/Codex sessions](SHARED-SESSIONS.md) supersedes
the Claude-only scope below. Prior checks remain historical evidence.

Implementation complete; authenticated provider recovery deferred. See DESIGN-tauri revisions 3–6, FOUNDATION and INTEGRATIONS. The bounded gap is live
interactive processes, not a redesign of the document viewer or credential vault.

Root owns Tauri/UI/integration. foundation_fixes owns the sessions crate, checked
by distinct foundation_refuter. Root integration is checked by terminal refuter.
Keep inherited model/effort settings. No live weekly Codex meter is available;
no usage percentage is inferred from task tokens.

This slice provides two explicit project sessions with native CLI login/permissions,
PTY input/output/resize, saved exact IDs and explicit resume after restart.
Closing the workspace stops app-owned processes. No input replay, background
adoption, auto-resume, new model selection or permission bypass is included.

Saved IDs alone do not establish a provider transcript exists. A failed resume
must remain visible rather than silently creating a different conversation.
Lifecycle state comes from owned process observations; native Claude displays
its own task/approval readiness. The app cannot infer permission status from a
quiet terminal. The credential vault and SOPS remain separate future work.

Acceptance evidence distinguishes synthetic PTY recovery tests, CLI capability
inspection, startup/auth UI checks and real conversation recovery. Synthetic controls alone
do not establish real provider transcript compatibility. No paid API fallback.

The user chose to finish the build and verify login later. Native auth status
reported no login; no account details were retained. Authenticated real Claude
turns and provider transcript recovery are deferred explicitly. This does not
block building/testing the adapter with synthetic subprocess fixtures.

Runtime policy comes from the user's current native Claude configuration.
Stored executable identity/version and launch recipe are checked; external
settings, hooks and permissions are not frozen by this milestone. Known API/cloud
billing environment overrides are refused, but the app does not certify the
billing method selected by external CLI configuration. Review Claude's native
login/account prompts. No model/effort flags or permission bypasses are injected.

## Keeping sessions running in the background

Setting "When I close the window" (Settings, Workspace) has two choices.

- **Stop my sessions and quit** (default): closing the window stops every session and
  exits, as always.
- **Keep running sessions in the background**: if at least one session is running,
  closing the window only hides it; the app process keeps the sessions (they are its
  child processes on its own terminals). With no running session, closing quits as
  usual, and a hidden app quits by itself within a few seconds of its last session
  ending, so no invisible idle process stays behind.

Bringing the window back: start agentVIEW again. The second launch finds the first
one through a private control socket (`control.sock` in the app data folder: folder
mode 0700, socket 0600, refused if the path is a symlink or not a socket of yours; the
user id of the other end is checked with `SO_PEERCRED` on both sides), asks it to show
its window and exits. This happens with either close setting: **in the default mode too,
a second launch is handed to the running instance** (its window is brought to the front)
instead of opening a second app. The socket decision runs under a file lock
(`control.lock`), so two launches at the same moment cannot both claim it.

How to run `--quit`: the program is `agent-app`; the AppImage passes its arguments on, so
`./agentVIEW-nixos-<version>-<revision>-x86_64.AppImage --quit` works, and so does
`agentview --quit` when you made the `agentview` launcher from [INSTALL](INSTALL.md)
(a `~/.local/bin/agentview` symlink to the AppImage, or the Nix wrapper script). From a
source build it is `target/release/agent-app --quit`. `--quit` asks the running instance
to stop all sessions and exit; it prints that, or "agentVIEW is not running" (exit 0), and
exits 1 only if the socket cannot be used. The socket accepts only the words `show` and
`quit` (one line, at most 14 characters), never runs or opens anything from a message
and uses no network. It serves a few connections at once (at most 4; further ones are
dropped), each limited to 2 seconds.

What happens when the socket is not usable:

- A socket left by a crashed instance (nobody answers, connection refused) is replaced.
- If the first instance owns the socket but does not answer (busy, hung or quitting), the
  second launch retries for a few seconds. If there is still no answer, it starts normally
  without background control and says so on stderr, unless the first instance still holds
  the sessions store: then it prints "does not answer ... holds the sessions store" on stderr
  and exits with status 1, because two apps must not share the session store. It never
  exits silently.
- Any other problem with the socket (wrong owner, not a socket, no private folder) only
  disables this feature and never blocks startup.
- Closing the window hides it only if the socket file is still the one this app created
  and its serving loop is alive (it stops after 5 failed accepts in a row). Otherwise
  closing stops the sessions and quits as in the default mode.

Terminating signals: SIGTERM, SIGHUP and SIGINT sent to a running app (hidden or not) take
the same path as the Quit button: stop all sessions, remove the socket, exit. A second
signal during that kills the app at once. If stopping the sessions fails the error is
written to stderr and the app exits anyway. Output of a session that ended while the window
was hidden is not kept: the idle watcher exits the app within a few seconds of the last
session ending.

The window is hidden, not destroyed, so what you see after "show" is exactly what you
left: the session list, the selected session and its terminal, with the output
buffer (256 KiB per session) intact; input keeps working. A "Stop all sessions and
quit" button on Sessions and on Settings ends everything whatever the setting says
(two clicks, the second confirms).

Honest limits: there is no tray icon or other indicator while hidden, so you return by
launching agentVIEW again (or find the process by name); sessions do not outlive the
app process, so logging out, a crash or a reboot still ends them; an open Documents
window is hidden and shown together with the workspace; the desktop-notification and
webhook monitors keep running while hidden.

Normal close/stop terminates the owned process group. Linux parent-death handling
covers the CLI leader on abrupt app termination; independently detached tool
processes are outside that guarantee. External Zellij sessions are not adopted.


## Verification and demo

- Pinned Nix foundation run: 74 Python tests, 43 Rust tests, normal desktop build,
  document policy GUI, default and 640-pixel demo restart, and live-terminal GUI
  restart all passed. Python HTTP fixtures emit existing ResourceWarnings.
- Live GUI uses a deterministic synthetic CLI in two private project directories.
  It proves controlling PTYs, distinct provider IDs, terminal input across tab
  switches, inert terminal markup, correct resume arguments, one restored fixture
  turn per project, no replay and all 13 commands denied from the document viewer.
- Separate backend refutation reproduced and corrected launcher-thread lifetime
  and post-spawn SQLite failure cleanup. Final 12 backend tests passed, including
  cross-process leases and abrupt manager death. One is a test-process helper.
- Root integration refutation corrected queued input routing, stale snapshot
  updates, live/demo labels, shutdown-error handling and generated release notes.
- Official installed CLI version/help was inspected: Claude Code 2.1.316 supports
  the required flags. Native auth status reported no login. No authenticated
  Claude turn or real provider transcript resume was tested; user deferred it.

[GUI demo capture](agentview-live-preview.png) shows the synthetic test CLI, not
an authenticated Claude conversation. Terminal output is interpreted by bundled
xterm.js 6.0.0 with first-party fit addon 0.11.0; fixed package integrity and asset
hashes are in ui/vendor/manifest.json, with MIT license texts under licenses/terminal.
No runtime CDN, terminal link opener, OSC clipboard access or HTML injection is
used. Inline styles are allowed for the terminal renderer; scripts remain local.

To use the real CLI: open Live Claude, inspect the installed executable/version,
register an absolute project directory, then start explicitly. Follow native
login/trust/permission prompts in the terminal. Stop before leaving work you
cannot interrupt. Resume after reopening uses the saved UUID, never --continue;
if Claude has no transcript for that ID, its native error is shown without a
fallback conversation. Process status does not establish approval readiness.

Run `nix develop --command bash scripts/check-foundation.sh` for the complete
synthetic acceptance checks. Normal and release builds exclude all fixture CLI
selection and GUI test endpoints; the release binary check rejects their markers.


## Resize and scaling follow-up

A direct native Wayland capture exposed excessive vertical stacking at 1.5 scale.
The live roster is now compact at narrow widths, launch identity is expandable,
and the project path stays visible with its full value available in details.
The focused terminal scrolls into view only on geometry changes; stop controls
remain available through page scrolling. Terminal height adapts to the viewport and fitted columns/rows
are sent to the owned PTY.

Targeted synthetic GUI checks cover 640-pixel restart/input and native Niri
55%/full columns at 100%, 125%, 150% and 200% scale, including actual child PTY
geometry and nonzero/fully-visible terminal assertions after every resize.
See [the Niri runner](NIRI.md#live-terminal-resizing). Authenticated Claude testing
is still deferred. This follow-up does not add Codex, a resource monitor or a
custom title bar; those requests remain separate wishlist items.
