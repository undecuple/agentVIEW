# Contributing

Small, focused changes are welcome. Open an issue first for anything large.

- Run the checks: `python3 -I -m unittest discover -s tests` and `cargo test -p agent-app-core` (see README).
- Read-only is a rule, not a preference. A change that writes outside the app's own cache, shows a credential value,
  widens the allow-list defaults or grants an approval will not be accepted.
- Use synthetic data only in tests and screenshots. Do not attach game files, keys, tokens or personal paths.
- Commit messages are plain: a subject line and an optional short body.
- Enable the hook once after cloning: `git config core.hooksPath .githooks`.
