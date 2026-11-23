# Session adapters, observability and tool health

Status: adapter contract and planned observability/tool-health work. The local
Claude/Codex PTY adapters are tracked in [shared session evidence](SHARED-SESSIONS.md);
authenticated native-TUI recovery is not yet verified. Local source-labeled token measurements are tracked in [usage evidence](USAGE.md).
The Prometheus exporter, updates and scheduled checks remain unimplemented.

## Claude Code sessions

The first live adapter should wrap the installed CLI rather than reproduce its
terminal interface or bypass its controls. Record the project directory, app
agent/session ID, provider session ID, launch profile, capability/version snapshot
and observed availability. Keep login with the installed tool; store no credential
values. Launch fixed executables with argument arrays and a reviewed working
directory, not a generated shell command.

Provide distinct actions for New session, Resume this session, Continue latest
in this project, Fork and Attach to a running provider-managed session. Automation
should use exact stored IDs, not whichever conversation happens to be newest.
Feature-detect installed CLI capabilities. The observed local Claude Code 2.1.316
supports resume/continue, session IDs, fork and background-agent attachment; its
presence is not evidence that this app already implements those features.

An embedded PTY preserves the interactive experience and permission prompts.
Where supported, provider-managed background sessions plus PTY attachment can
keep work alive independently of the app window. Structured event adapters can
then feed the activity/job UI. Test each mode separately: JSON/print mode is not
assumed to behave like an interactive attached terminal.

The [CLI reference](https://code.claude.com/docs/en/cli-reference) and
[session guide](https://code.claude.com/docs/en/sessions) are the authoritative
references to recheck during implementation. Resume is conversation recovery,
not process checkpointing; do not assume interrupted tools rerun or all launch
options survive. Compare the approved recipe, settings and permissions before
resuming instead of silently broadening them.

Existing foreground Zellij processes are not automatically adopted. Offer a
reviewed import/handoff using project and session identity. Use native attach
only when that session type/version supports it. Serialize app-managed writers
per provider session; external clients may need an explicit handoff. Do not
claim an app lease controls every terminal on the machine.

Acceptance: two project sessions stay separate; restart resumes the correct ID;
closing the UI has a documented stop/detach behavior; permissions remain intact;
live-attach and transcript-resume are distinguished. After a crash between prompt
delivery and acknowledgement, reconcile provider state or show “delivery unknown”
rather than blindly resend a possibly destructive turn. No live test is run here.

## Prometheus and Grafana

Plan an optional, read-only metrics exporter with explicit enablement, a loopback
default bind, configurable access controls and no public exposure by default.
Remote scraping over NetBird still requires an approved endpoint and access
policy. Do not install or reconfigure a user's Prometheus/Grafana stack silently.

Initial metrics and dashboards:

- Managed sessions by provider/role/state; adapter health and reconnects.
- Queue depth, delivery latency, job duration/failures and approval wait time.
- Blocked actions by bounded decision source/category; never raw error strings.
- Provider-reported token counters, telemetry availability and freshness. Unknown
  usage is absent/explicitly unavailable, not a fabricated zero or an estimate
  presented as measurement.
- App-owned process/resource and database health where measured.

Use bounded labels such as provider, role and configured project ID. Exclude
prompts, credentials, full commands, paths and unbounded request/session IDs.
Follow [Prometheus metric guidance](https://prometheus.io/docs/practices/naming/).
Provide version-controlled dashboard/data-source examples compatible with
[Grafana provisioning](https://grafana.com/docs/grafana/latest/administration/provisioning/).
Keep the activity store as the source of detailed audit events; metrics summarize
it rather than replace it. Dashboards must not become an approval bypass.

Acceptance: the exporter is off by default, sample metrics pass promtool checks,
labels do not leak private data or grow without bound, unavailable usage is clear,
and dashboards work against a disposable test stack before live integration.

## Tool Health & Updates

Add update reminders for Claude Code, Codex and other registered tools. Inventory
the installed version, executable path/install source, last check and available
version from an approved official source. Support manual checks and user-chosen
reminder cadence, with snooze/dismiss and an honest offline/unknown state.

Detect Nix-managed binaries and route updates through the user's declarative
system/package configuration. Do not run a self-updater against the Nix store.
Other install methods need their own reviewed update path and explicit approval;
no automatic upgrade, new schedule, global config edit or paid-API fallback is
enabled here. An available update is not itself proof of compatibility.

Before an approved update, identify active sessions, retain resume/handoff data,
and wait for a safe boundary. Afterward check CLI capabilities and flag changed
session/permission behavior. Preserve rollback information where the install
method supports it. Update checks should not expose credentials or collect
conversation content. Reminder delivery uses user-configured channels only.
