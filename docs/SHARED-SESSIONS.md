> Evidence figures and design notes below are snapshots from when each step was made. Current command count is 40, and the look has changed since; see the [code map](code-map/README.md).

# Shared Claude Code and Codex sessions

Status: implemented candidate; authenticated native-TUI turns and conversation
recovery remain unverified. Reuses the prior [Claude milestone](LIVE-SESSIONS.md),
its independent PTY/lifecycle review and [Niri checks](NIRI.md). No model/effort,
global instruction, login, credential-store or sandbox configuration is changed.

## One Sessions desk

Choose Claude Code or Codex, inspect the installed CLI, register an absolute
project directory and start explicitly. The same roster shows both providers;
its filter only hides rows and never retargets the selected terminal. Every
input, resize and stop uses the app session ID plus launch generation. The
selected provider/coordinator is visible beside quick prompts.

Two app-owned sessions may run at once, in any provider combination. Existing
Claude launch records migrate without changing their IDs, project paths or saved
recipes. Provider-native IDs are stored separately from app IDs; a provider's
UUID cannot accidentally select another provider's terminal. Restoring metadata
never starts a CLI or replays input.

CLI settings, login, model choice and approvals stay native. The app checks
executable identity/version/recipe and refuses known provider-specific API/cloud
environment overrides without reading/logging their values. Native configuration
may still select other billing or settings; this is not a credential vault or an
OS isolation boundary. Current-user processes retain their usual host access.

## Codex identity and recovery

Installed Codex 0.165.0 has native resume but no new-session UUID flag. The adapter
uses a dedicated, app-owned `app-server --listen stdio://` helper only after Start:

1. Save that bootstrap was attempted before requesting a new thread.
2. Initialize the protocol and request an empty persistent thread in the approved
   working directory, with no model turn or permission overrides.
3. Validate the correlated response's canonical UUID, project and persistence
   metadata. Notifications and terminal text cannot establish an identity.
4. Set the thread's native display name to the coordinator label. In the isolated
   0.165.0 probe, thread/start alone did not survive restart; naming materialized
   the empty thread. This is a measured version-specific observation.
5. Stop that helper and verify the exact ID/project with a fresh helper process.
6. Save the verified provider ID, then open `codex resume UUID --no-daemon` in the
   embedded PTY. This keeps the app from adopting the user's shared daemon.

The protocol is based on the installed generated schema and the official
[Codex App Server guide](https://learn.chatgpt.com/docs/app-server). Every helper
has bounded input/output, timeouts, cancellation and owned-group cleanup. Failed
or interrupted bootstrap stays `bootstrap_unknown` when no trustworthy ID was
saved. It cannot silently create a replacement or pick the latest thread. Failed
resume never falls back to a different conversation.

## Coordinator quick prompts

Review shared inbox, Summarize status and Prepare handoff paste a single-line
prompt into the selected running coordinator. Custom prompt text is editable
but not saved in this slice. No Enter is added; inspect/edit it in the native
prompt before submitting. Confirm the CLI is at its normal prompt first.
Control characters are refused. Presets do not grant permissions or broadcast
to another coordinator, and filtering providers does not change their target.

## Lifecycle

Long startup/probe work runs away from the UI thread. Closing the workspace
signals cancellation; if an operation owns the session lock, close waits
asynchronously, stops owned processes, then retries native closure. Inspection
has bounded deadlines and may take several seconds to finish. Close failures
remain visible rather than abandoning a live child silently.

Normal stop/close terminates owned process groups; crash handling covers the
provider leader. Deliberately detached descendants and external Zellij/native
sessions remain outside that guarantee. Resuming restores a saved conversation,
not a process checkpoint or an assurance an interrupted tool completed.

## Verification scope

Backend coverage includes legacy Claude behavior, v1→v2 migration, provider
identity persistence, exact-ID mixed-provider resume, forged notification/response
IDs, malformed/mismatched metadata, unexpected approval requests, uncertain
bootstrap, cancellation and child cleanup. A distinct refuter independently
checks the backend and a separate refuter checks Tauri/UI integration.

GUI fixtures exercise Claude and Codex side by side, input across selection/filter
changes, paste without Enter, correct provider IDs/arguments, no bootstrap replay,
restart recovery and denial of all 13 workspace/session commands in Documents.
A native-close test stalls the Codex helper, requests actual window closure and
checks both the helper and another provider process exit while uncertain identity
is retained. Synthetic fixture recovery is not authenticated provider recovery.

The dark charcoal/pink design retains the functional hierarchy, Sessions and Usage
views, provider controls and quick prompts. It supersedes the light operator-ledger
draft following user feedback; unknown telemetry stays Unavailable. Captures contain only synthetic data.
SQLite is implemented storage; [DuckDB analytics and project database tools](DATABASES-DESIGN.md)
remain a design/backlog item. Resource monitoring, the vault and custom window
chrome also remain separate planned work.


## Candidate evidence

- Full pinned foundation run: 74 Python tests, 50 Rust tests (23 core, 19 session,
  8 workspace; one session test is a helper process), viewer policy and all demo,
  mixed-provider, close-cancellation and 640-pixel GUI checks passed. Existing
  HTTP fixture ResourceWarnings remain.
- Independent backend refuter ran all 19 tests plus a public-API adversarial probe;
  notification IDs cannot supply a provider identity, and uncertain bootstrap
  remains blocked after reopen. Root integration refutation found no remaining
  material objection after the actual native-close test.
- Niri 26.04 native fixture matrix passed 55%/full columns and 100/125/150/200%
  scaling. Actual child PTYs reported 67x22, 102x22, 80x13, 101x17 and 59x10 for
  the tested viewports. Terminal geometry/visibility is asserted after each case.
- Native Codex CLI 0.165.0 status reported ChatGPT login readiness without
  exposing account data. That is not an authenticated TUI turn/recovery test.
  No Claude login flow, model turn or paid API fallback was run here.

Drafts: [hierarchy](agentview-preview.png), [Sessions and quick prompts](agentview-live-preview.png),
[token usage](agentview-usage-preview.png). They are actual private-display
captures with synthetic data, not generated images or authenticated conversations.
Rendering checks used software rendering. Physical input/IME, mixed-monitor
transitions, hardware performance and FUSE remain unverified. Draft delivery receipts and detailed local logs are
retained in the shared handoff; a service receipt does not establish user reading.

## Design preference follow-up

Restored the preceding charcoal/pink workspace at the user’s request. The light
override and its asset route were removed; provider selection, Codex resume,
quick prompts and all session logic are retained. New screenshots above show
the restored design. Independent static refutation found no lost controls or IDs.

Targeted GUI checks passed for mixed-provider first start and restart at normal
and 640-pixel widths, plus hierarchy/usage and persistent demo conversation checks.
These use isolated synthetic CLIs; prior authenticated-recovery limits still apply.
Backend logic is unchanged; the full backend suite was not repeated for this style edit.

## Usage follow-up

The [local usage milestone](USAGE.md) adds numeric SQLite measurements and explicit
source refresh to the shared session identities. Claude output and total remain
unavailable; token measurements do not establish subscription limits or billing.
Authenticated native-TUI checks remain a separate deferred gate.
