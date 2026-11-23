# App preferences and colour themes

Status: implementation candidate; integration checks pending. Reuses
[Claude architecture checkpoints](DESIGN-tauri.md), [session boundaries](SHARED-SESSIONS.md)
and [usage source contracts](USAGE.md). The previous preview had fixed interface
and terminal values and no app Settings page.

## Scope and persistence

Settings cover app appearance, startup page/preferred provider, remembered
hierarchy/session filters, terminal font/blink/bounded in-memory scrollback,
usage defaults/optional visible-page refresh, and editable/reorderable quick
presets. Defaults never start, resume, authenticate or configure a provider.
Native provider models, permissions, login and global instruction files stay
outside this preference schema.

Preferences are versioned JSON in the app-owned workspace SQLite store. Saves
use revisions: a stale form cannot overwrite a newer save. Invalid values and
terminal control/separator characters are refused. Missing preferences use
factory defaults without a write. Damaged supported preferences require explicit
Restore app defaults and retain their original bounded record in private storage.
Newer formats, oversized records and invalid metadata remain read-only. Duplicate
JSON keys cannot conceal a newer format. Recovery never resets project/session
or conversation records.

Appearance and terminal changes apply after Save. Startup page/provider defaults
take effect predictably without a CLI launch. Usage refresh is manual by default;
opt-in intervals are 60/300/900 seconds and run only while Usage is visible and
the window is focused. There is no background system schedule. Preset buttons
always paste without Enter; adding a preset does not grant tool permissions.
Preset text is saved locally and excluded from diagnostics.

## Palette and artwork

Both themes adapt the official dark and light palettes in
[Cyberdream](https://github.com/scottmckendry/cyberdream.nvim/blob/eafadf5712e447bb813f8f7eab8012d1391fdba2/lua/cyberdream/colors.lua).
Primary accent is cyan #66d9ef with pink text on charcoal #1b1d1e in Dark (identity.css) and blue #0057d1 in Light, with matching cyan/green/
purple/status colors. Muted text and surface shades are adjusted for application
readability. System mode follows the OS color-scheme preference. The established
layout and original lettering/icon geometry are retained.

Deterministic icon/banner/divider/social generators emit matching dark and light
assets. README pictures select the matching banner/divider automatically.
Cyberdream attribution is retained in assets/cyberdream-LICENSE.txt and included
with packaged licenses. Artwork derives from checked-in deterministic geometry, with no runtime downloads.

## About and diagnostics

About displays app/platform/storage information and conventional numeric CLI
versions saved with registered sessions. It does not probe executables or read
credentials. Diagnostic export projects a bounded JSON report of app version,
preference health, appearance/terminal settings and safe numeric version strings.
It excludes prompts, conversations, session IDs, filesystem paths, account details
and credential values. Export writes a new private file beneath app-owned exports;
web content supplies no destination path. The report preview is shown before sharing.

## Acceptance

Backend validation/recovery/CAS and duplicate-key controls have a distinct refuter.
Root integration has a separate terminal refuter. GUI coverage must include restart,
preference application, stale drafts, quick-paste safety, light/dark rendering,
visible-only scheduling, diagnostic privacy and Documents isolation. Narrow/Niri
captures and exact-head AppImage/source packages follow final review.

## Themes (2026-11-24)
Dark is now a Molokai-inspired palette with soft pink text (`#f08cb0` on `#1b1d1e`); Light stays Cyberdream Light. PaperColor Light and PaperColor Dark are additional choices, inspired by the vim colour scheme PaperColor (MIT). Only colour values are used as inspiration; no code or assets were copied from these schemes. The setting values are `dark`, `light`, `system`, `papercolor-light` and `papercolor-dark`.
