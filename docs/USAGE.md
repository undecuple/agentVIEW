# Local token measurements

Status: implemented (the page is Token usage in the sidebar; the look has since changed to the current themes); authenticated
provider counter verification remains deferred. Source map:
[Claude architecture checkpoints](DESIGN-tauri.md), [session lifecycle](SHARED-SESSIONS.md),
and [observability plan](INTEGRATIONS.md). The preceding preview deliberately
showed all telemetry as Unavailable; that remains the fallback for absent data.

## Scope

The Usage view covers registered app-managed Claude Code and Codex sessions.
Coordinator labels come from those live records, not the simulated hierarchy.
Opening the view reads saved SQLite measurements. **Refresh local usage** reads
matching native session records without launching a CLI or sending a model turn.
Provider home paths come from the native process environment; web content cannot
choose an arbitrary source path. No global settings or login files are changed.

Only usage metadata is retained. Prompts, answers, tool arguments, terminal text,
credentials and account details are not copied into the measurement store or UI.
Source records may contain conversation text; parsing discards it. This is local
reported usage, not an independently verified billing receipt.

## Counters and periods

Input includes reported cache reads and cache writes; cache columns are subsets.
Total is input plus output. Claude's raw ordinary input excludes both cache
categories, whereas Codex's cached input is already a subset of its input.
Unsupported or missing counters remain Unavailable rather than a measured zero.
Provider cards show reported subtotals with session coverage; they do not claim
complete machine-wide usage. Separate subagent transcripts and unrelated sessions
are outside this milestone.

Session totals, provider subtotals and daily UTC measurements can be filtered by
provider and all recorded time/today/last 7/last 30 days. Days without a measurement
are not filled with zero. Source state, last check and last measurement appear
per session. Historical measurements remain distinguishable from current checks.

Subscription limits, actual API spend and estimated monetary cost remain separate
and unavailable. Tokens do not establish a subscription percentage, and this view
does not read credentials or query billing endpoints.

## Sources and verification

The Claude adapter reads an exact project/session JSONL file. Its supported
counter subset is ordinary input, cache reads and cache writes, deduplicated by
message identity. A message belongs to the UTC day of its first numeric record.
Later coherent revisions update its counts; conflicting cache distributions are
rejected. Ignored lower/equal replays cannot freshen the retained measurement time. **Claude output and total remain Unavailable**: a reliable final
output contract for this internal transcript format has not been established.
The [SDK cost guide](https://platform.claude.com/docs/en/agent-sdk/cost-tracking)
warns that per-step output values can be placeholders; this is not proof that
every on-disk transcript has that behavior, so no broader claim is made.

New Codex sessions bind the exact rollout path returned by native bootstrap.
Older app records without that binding remain source_not_bound; refresh does not
search all sessions or bootstrap another thread. Cumulative token snapshots are
reconciled rather than summed, with timestamped differences attributed to UTC
days. Cached input is a subset; reasoning tokens are not added again to output.

These are versioned internal formats, not guaranteed provider APIs. Malformed,
changed, missing, ambiguous or unsupported sources must fail visibly and retain
historical values without inventing fresh usage. Custom Claude project directory
names and unsupported path encodings are not discovered by a global scan.

Read bounds are 32 MiB per source, 64 MiB per refresh, 1 MiB per JSONL line,
100,000 records per source and 4,096 daily buckets. Reaching a bound preserves
saved readings with a visible source status; it does not silently truncate totals.
Each per-session counter is capped at 10^12 so aggregates across the maximum
64 managed sessions remain exact JavaScript integers. Refresh checks cancellation
between chunks. Linux `openat2` refuses symlinks/escapes; no unsafe fallback is used.
Source replacement, shrinkage, counter decreases and identity mismatch retain
historical readings rather than silently rebasing a conversation.

Native `/clear`, `/new` or forks that create a different conversation inside the
terminal are not automatically adopted: accounting follows the saved native ID.

Official references consulted for this milestone:

- [Claude session storage](https://code.claude.com/docs/en/sessions#where-transcripts-are-stored)
- [Claude token semantics](https://code.claude.com/docs/en/monitoring-usage#token-counter)
- [Codex App Server](https://learn.chatgpt.com/docs/app-server) and installed generated protocol schema

Acceptance includes duplicate and streaming revisions, restart, missing sources,
invalid identity/numbers, daily attribution and independent backend/integration
refutation. Synthetic GUI fixtures must never be presented as authenticated
provider turns. Authenticated native-TUI recovery remains a separate deferred gate.

## Candidate evidence

- Evidence snapshot at the time (current counts are in code-map/BUILD-TEST.md): pinned foundation runner passed 74 Python tests, 23 core Rust tests, 28 session
  tests and 8 workspace tests, viewer policy, Documents isolation from all 15
  commands, native-close cancellation, hierarchy/usage persistence and mixed
  provider session GUI checks at normal and 640-pixel widths.
- After the final freshness correction, all 10 current usage regressions passed
  (19 unchanged session tests filtered). This is an incremental check, not a
  second full-suite run. The normal binary was rebuilt by the foundation runner;
  the final candidate was then rebuilt for Niri checks.
- Independent backend refutation ran eight earlier usage tests and a separate
  public-API probe for identity, duplicate/partial records, missing/truncated
  sources, reopening, counter resets, cache coherence and numeric-only persistence.
  It inspected the final timestamp/partial-status changes; root ran those tests.
- Independent integration review challenged aggregate coverage and stale display
  claims. Provider breakdowns and daily cells now show counter-specific coverage,
  and real GUI assertions verify retained values/status/time after a source goes
  missing, partial Claude counters, UTC filters and refresh deduplication.

Actual private-display previews: [usage overview](agentview-usage-preview.png) and
[daily breakdown](agentview-usage-daily-preview.png). **All displayed counts are
synthetic fixture data**, not measurements from an authenticated conversation.
No personal transcripts, credentials, login changes, model turns or billing calls
were used. Internal format compatibility is conditional on the documented shapes;
authenticated native-TUI recovery and real counter reconciliation remain unverified.

The final native session fixture passed Niri 26.04 at 55%/100% columns and
100/125/150/200% scaling, with real child PTY dimensions observed after resize.
Usage tables separately passed the 640-pixel no-page-overflow assertion. This
is software-rendered Linux evidence; physical IME/clipboard, monitor transitions
and hardware performance remain outside the checks.

### Empty-record correction

The final bounded follow-up ignores records with no supported numeric counters
before assigning a message day or cumulative baseline. Empty/unsupported-only
Claude records cannot poison otherwise known totals; empty Codex snapshots cannot
consume the first numeric daily measurement. Reported zero counters remain valid
measurements. Both provider regression cases reproduced failure before the fix
and passed afterward, including independent focused refutation. The current
usage suite now has 12 tests (19 unchanged session tests filtered); root reran
that focused suite and rebuilt the normal app before packaging the final revision.
Prior full foundation, GUI captures and Niri checks remain the preceding evidence;
UI and non-usage lifecycle code did not change in this correction.
