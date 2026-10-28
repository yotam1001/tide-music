# Contributor automation

- Use `scripts/cargo-local.ps1` for local Rust build, test and Clippy commands.
  Respect its disk-budget failures; do not bypass them with direct Cargo or a
  different output directory. Never delete generated files automatically.
- Prefer hosted CI for full workspace and installer validation. Local dev/test
  profiles deliberately disable incremental snapshots and limit debug symbols.
- Keep development output, local toolchains, credentials and sessions out of
  commits and installer payloads. Run `scripts/check_public_files.py` on the
  staged content before publishing.
- Preserve unrelated working-tree changes. Report validation limits honestly.

- Keep Tide and Jams free for users. Preserve a self-hosted relay path; do not
  introduce subscriptions or commit to paid hosting without explicit approval.
- Jams remain opt-in and separate from the stable installer until explicitly
  approved after synchronized playback and security validation.
