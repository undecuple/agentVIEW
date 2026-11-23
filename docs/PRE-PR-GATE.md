# Pre-PR check

`scripts/pre-pr-check.py` is a read-only cleanup check any agent runs before it hands a change to a person for review.
It never pushes, posts, comments or edits anything. Stdlib Python 3.11+.

```
python3 scripts/pre-pr-check.py --repo PATH --base origin/main --title "core: what changed" --body draft.md
```

It checks the commits in `BASE..HEAD` and the draft body, prints a report (FAIL, WARN, PASS) and exits 1 on any FAIL.

| Checked | Result |
|---|---|
| Author and committer identity on every commit | FAIL |
| Attribution or session lines in commit messages | FAIL |
| Key, archive, database, log and game-data file types | FAIL |
| Token shapes, home-directory paths, other e-mail addresses, configured internal names in added lines | FAIL |
| Duplicate new-file headers | FAIL |
| Body footers, tokens, home paths, internal names, a required AI-help line (if configured) | FAIL |
| One commit, rebased on the base, at most 400 changed lines, whitespace, title shape, body headings in order (Why, Change, Tests, Evidence, Overlap) | WARN |

Not checked (listed in every report as "needs a human"): that the test fails before the change and passes after, that every
test command was run, overlap with open PRs, plain and true wording, and the approver's sign-off for this exact text.

Optional `<repo>/.pr-check.toml`: `identity_regex`, `forbidden_words`, `ai_line`, `max_changed_lines`.

Planned: a read-only "Pre-PR check" view in the app that runs this fixed script against an allow-listed repository and shows
the report. The app would not run anything else, and approval stays with a person.
