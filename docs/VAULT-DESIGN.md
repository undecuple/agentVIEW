# Credentials and access

Status: requested product design, not implemented. The app has no
credential vault, access broker or live provider authentication. This document
does not import keys, inspect existing secrets or grant agents access.

## Credentials view

Provide one place to register SSH identities, API tokens and references to
existing provider/Git credential stores. Show a friendly name, credential type,
provider, account, SSH public fingerprint where applicable, storage source,
expiry, last use and effective access. Secret values stay masked and are omitted
from conversations, onboarding documents, logs, metrics and ordinary exports.

Use an explicitly configured OS credential service or encrypted vault for new
secrets; keep only references and grant metadata in SQLite/PostgreSQL. A missing
or locked backend must not fall back to plaintext. Prefer existing provider login
flows for Claude/Codex rather than importing their internal authentication files.

## Who can use a credential

Show an access matrix and hierarchy overlay: credential → permitted projects →
intake/coordinator → approved subagents/sessions. Separate these permissions:

- View metadata and request access.
- Use a credential for specified operations, accounts and targets.
- Delegate an existing grant within its approved scope.
- Manage grants, rotate/delete credentials or explicitly reveal/export a secret.

Only the user manages or expands grants by default. A coordinator's access does
not automatically pass to its workers. Offer explicit, previewed project/role
grants with optional descendant access, expiry and per-use approval. Show the
exact effective recipients; adding, moving or restarting an agent must not
silently broaden a grant. Agents cannot approve their own requests or obtain
credentials by changing their displayed name, role or onboarding instructions.

Default to a deny-by-default coordinator allow-list per vault entry. For example,
the user can allow a deployment coordinator to use one deployment credential
while denying both intake agents and every other coordinator. Subagent access is
a separate explicit grant, even when its coordinator is allowed.

Bind grants to trusted adapter identities and app-owned session/process records,
not identity claims supplied in chat. Check current grants at operation time;
queued jobs do not retain access that has since been revoked.

## Use without copying secrets into chat

Prefer brokered operations: the agent requests an authenticated action and the
broker checks the grant, performs the action and returns a redacted result. For
SSH, design around dedicated, restricted identity agents or equivalent isolation
instead of handing every child the user's full SSH agent socket. Target and
operation restrictions must be enforceable at the broker/remote account boundary;
a label in the UI is not an SSH restriction.

Where a tool requires a raw token, identify that mode explicitly. Use isolated,
short-lived scoped credentials where available, expose them only to the approved
process, and avoid command-line arguments or durable config files. Once a process
has received a raw secret, local grant revocation cannot make it forget that
value. Show whether upstream revocation/rotation and process termination are
needed. Do not promise protection from arbitrary same-user host processes or
external CLI sessions without an enforced OS isolation boundary.

The vault must not become an arbitrary authenticated command runner. Validate
typed operations, destinations and approved working directories independently
of prompts; retain normal provider, sandbox and publication approvals.

## SOPS-backed storage

Support SOPS as an optional encrypted-file backend, starting with user-approved
age identities and encrypted files. SOPS also supports PGP and cloud KMS options;
those need separately configured adapters rather than automatic discovery or
cloud access. See the official [SOPS documentation](https://getsops.io/docs/).

Keep the decryption identity available only to the vault broker's enforced
isolation boundary. Do not distribute the age private identity or mount readable
plaintext secret files into coordinator/worker environments. SOPS protects
stored files; the app broker enforces coordinator/session use grants. Possession
of a decryption identity outside that boundary can bypass the app's allow-list.
Do not describe an app checkbox as protection against such access.

Use reviewed file paths, installed executables and argument arrays. Decrypt only
for an authorized operation, avoid persistent plaintext copies and never return
an entire decrypted document when only one approved entry is needed. SOPS file
structure/metadata may remain visible; keep sensitive names out of public files.
An encrypted file must not be committed or synchronized merely because it is
encrypted. This feature does not alter existing sops-nix or system configuration.

Treat recipient changes, re-encryption and key rotation as explicit reviewed
operations. Removing a recipient cannot recall old ciphertext or a previously
exposed secret. Show when replacing the actual upstream credential is required.
Acceptance adds synthetic encrypted-file round trips, wrong/missing-key failure,
unauthorized-coordinator refusal, no worker access to decryption identities or
plaintext, and isolation across vault entries and projects. No real identity or
credential is imported during design or fixture tests.

## Activity and recovery

Record requester/project/session, credential reference, operation category,
target, grant decision and timestamp without recording secret material. Provide
pending-access requests, last-use history, expiry reminders, revoke-one/revoke-all
and rotation flows. Distinguish revoking an app grant from revoking the credential
at its provider. A locked vault or failed broker denies the operation with a clear
status; it must not retry through another account or less restricted credential.

Web/mobile clients may request approved operations from the PC broker. They do
not download secrets or gain credential access merely by joining NetBird. Vault
backups, reveal/export and remote grant management need distinct user-approved
flows and must preserve existing access boundaries.

## Acceptance before live use

Use synthetic credentials to prove cross-project and cross-agent refusal,
non-inheritance and explicit delegation, forged identity refusal, expiry,
revocation of queued work, locked-backend refusal, secret-free logs/exports and
absence of broad inherited SSH sockets. Verify isolation against an unauthorized
worker, not only a UI checkbox. Test raw-token exposure and revocation limits
explicitly, then verify one narrow real adapter before claiming enforcement.
