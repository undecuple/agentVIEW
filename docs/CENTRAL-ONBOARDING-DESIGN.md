# Central agent onboarding

Status: requested product requirement; the foundation preview does not yet edit
global instructions. This design adds no global instruction files or permissions.

## One shared source

Add a Central Onboarding view within Instructions & Skills. A user-selected,
approved document holds common global rules for both agent families. Keep common
rules there; retain provider-specific supplements and project-specific rules in
their own files, with their scope shown explicitly.

The app should manage lightweight pointers and explicit read instructions in the
appropriate `AGENTS.md` and `CLAUDE.md` files instead of copying the shared rules
into many documents. A Markdown link alone is not proof that a runtime loaded a
document: verify each adapter's effective instruction setup and show the read
status/revision as observed or unknown.

## Management flow

1. Register the canonical document path within approved access roots. Inventory
   existing pointers without overwriting another project's instructions.
2. Show source, scope, content hash, current revision, last edit and linked agents.
3. Edit through a diff preview, with explicit user confirmation for global writes.
   Save immutable prior revisions and support rollback as a new revision.
4. Check broken links, missing permissions, contradictory rules and duplicate
   copies. Present conflicts for review rather than silently choose a policy.
5. Generate or update small provider/project pointers only in specifically
   approved files. Preserve unrelated content and provider-specific settings.
6. Notify affected managed sessions of a revision change. Show which sessions
   loaded it, which need a new turn/restart, and which remain unverified.

Document changes are guidance, not permission grants. They must not override
host/provider controls, self-approve tools, widen paths, or execute embedded
instructions merely because the editor opened the document. Show effective
precedence where the adapter can establish it and label uncertainty honestly.

## Initial content structure

Offer headings for roles/hierarchy, approvals and authority, allowed work,
documents and handoffs, delegation/refutation, tests and evidence, Git/publication,
resource limits, usage/budgets, privacy, recovery and notifications. These are a
template, not preapproved rules: existing user-authored rules must be reconciled
and approved before becoming the central source.

## Acceptance

Use synthetic files to verify both pointer formats reference the same canonical
revision, edits preserve unrelated instructions, out-of-root/global writes are
refused without approval, stale revisions are visible, rollback preserves history,
and conflicts are surfaced. Then confirm real adapter access/read evidence before
claiming a live session has adopted the rules. Remote/mobile clients use the same
approved management flow through the PC service; they do not silently edit global
agent configuration.
