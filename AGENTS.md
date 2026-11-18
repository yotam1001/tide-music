# Contributor automation

- Use `scripts/cargo-local.ps1` for local Rust build, test and Clippy commands.
  Respect its free-space check. Once local validation is complete and the
  build outputs are no longer needed, verify the target path is inside the
  current checkout before clearing that checkout's generated Cargo output.
- Prefer hosted CI for full workspace and installer validation. Local dev/test
  profiles deliberately disable incremental snapshots and limit debug symbols.
- Keep development output, local toolchains, credentials and sessions out of
  commits and installer payloads. Run `scripts/check_public_files.py` on the
  staged content before publishing.
- Preserve unrelated working-tree changes. Report validation limits honestly.

- Keep Tide and Jams free for users. Preserve a self-hosted relay path; do not
  introduce subscriptions or commit to paid hosting without explicit approval.
- Jams are a stable feature, approved by the owner on 2026-11-18. Windows and
  Mac release packages must include them; do not build stable packages with
  `--no-default-features` alone or turn the installed Jam checks into
  exclusion checks.
