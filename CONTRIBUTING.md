# Contributing

Small, focused changes are welcome. Open an issue first for anything large.

- Run `nix develop --command bash scripts/check-foundation.sh` for the foundation checks and GUI previews (see README).
- Document access stays read-only. A change that writes outside the app's own data/cache, shows a credential value,
  widens the allow-list defaults or grants an approval will not be accepted.
- Use synthetic data only in tests and screenshots. Do not attach game files, keys, tokens or personal paths.
- Commit messages are plain: a subject line and an optional short body.
- Enable the hook once after cloning: `git config core.hooksPath .githooks`.
