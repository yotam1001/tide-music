# Changelog

## Unreleased

Tide is a Windows preview. A stable release is pending the checks in
[release readiness](docs/RELEASE_READINESS.md).

- Native audio playback with playlist queues, drag reordering, shuffle, and
  recommendation continuation.
- Full, compact, and mini interfaces with separate appearance settings.
- Locally encrypted account imports and persistence of updated playback cookies.
- Prepared next-song handoffs and retries for temporary content failures.
- Windows media controls and asynchronous file selection that keeps playback
  and queue processing running.
- Account help opens in the Windows default browser; automatic cross-browser
  session transfer remains pending.
- Application shutdown explicitly joins the playback worker before returning.
- Window fitting for changes in display scale and player mode.
- Automated Windows preview packaging with checksums and dependency notices.
- Theme-matched song menus, a visible Account entry, a direct theme switch,
  and removal of the routine Playing status strip.
- Published-stable update checks and verified install/restart, with optional
  startup checking. Preview releases still need manual installer updates.
- Smaller development compiler output and guarded local builds; build caches
  and toolchains are rejected by the installer package gate.
- User-first installation, Chrome connection, everyday-use and troubleshooting
  guides, with contributor and release instructions kept separately.

Known limitations and the scope of completed tests are documented in
[validation](docs/VALIDATION.md). Session longevity, game-focused physical media
keys, and remaining release gates must be verified before a stable release.
