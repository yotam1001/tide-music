# Changelog

## Unreleased

## 0.1.3 — 2026-10-19

- Reduce transient RAM allocations when loading account results, generating
  album thumbnails and replacing playlists, preserving queue rollback and art quality.

## 0.1.2 — 2026-10-19

- Open artist names from the player, queue, search and Home to browse YouTube
  Music top songs, with Play, Shuffle, queue actions and Back navigation.
- Show loading, buffering and bounded retry feedback in the player, along with
  the observed audio codec, approximate bitrate and sample rate when available.
- Preserve specific audio-output recovery guidance and position/pause intent
  when retrying playback.
- Check muted local transitions, skips, process-tree memory and observed helper
  cleanup in the hosted Windows installer test.

## 0.1.1 — 2026-10-18

- Add opt-in Windows global shortcuts for play/pause, next and previous in
  Settings, with conflict detection, transactional changes and no keyboard hooks.
- Allow Windows media controls to be disabled independently of custom shortcuts.
- Follow the shared Windows default audio output and retry the current song after
  an audio-output failure when a default output returns, preserving position and
  pause intent. Hardware/game-focus checks remain pending.

- Publish matching app and installer version 0.1.1 on the stable update channel.

## 0.1.0 — 2026-10-17

- Publish the preview 2 feature set on the stable update channel at the
  maintainer's request, allowing installed previews to test Install and restart.
- Match the Cargo, app, installer and GitHub release version as 0.1.0.
- Keep the existing updater validation, audio engine and account handling.
  Outstanding device and long-term checks remain in the release-readiness notes.

## 0.1.0-preview.2 — 2026-10-17

- Pin My Supermix beside Liked Songs in Library, resolving the connected
  account's mix on demand even when it is absent from Home's displayed cards.
- Restore the leading YouTube Music Home rows and their original order, instead
  of balancing unrelated shelves and forcing equal song/playlist exposure.
- Keep refresh variety close to the leading recommendations, with a bounded
  repeat penalty rather than exhausting every unseen suggestion first.

## 0.1.0-preview.1 — 2026-10-17

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
