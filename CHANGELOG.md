# Changelog

## Unreleased

## 0.4.21 - 2026-11-11

- Add Bit-perfect output in Settings → Audio: exclusive device access with no
  processing and Tide's volume at 100%, so the decoded stream reaches your
  device or DAC unchanged.
- Add true crossfade (3–12 s) in Settings → Audio: the next song starts under
  the end of the current one, using a second short-lived player while they
  overlap.
- Autoplay after a playlist or Liked Songs blends that playlist's radio with the
  last song's Up next, like YouTube Music continuing a playlist.
- Home's recent-listening shelf mixes three recent songs, at most two songs per
  artist, and sits below YouTube Music's first two shelves.
- Home scrolls as one page; playlist headers collapse to a compact bar while
  you scroll.
- The sidebar shows your own playlists first, with covers, and leaves Blends,
  mixes and system lists to the Library page.
- A new update dialog, sharper Now Playing artwork and generated covers for
  songs and playlists without artwork.
- Smoother karaoke that glides through each word, breathing dots during
  instrumental breaks, and a lighter lyrics background.

- Add equalizer presets in Settings → Audio: Bass boost, Bass reducer, Treble
  boost, Vocal and Loudness. Boosting presets lower the level first to avoid
  distortion, and work together with Even volume.
- Add Fade between songs in Settings → Audio: each song eases in over 2.5 s and
  out over its last 4 s instead of cutting. Off by default.
- Add a sleep timer to Now Playing: 15, 30, 45 or 60 minutes, or the end of the
  current song.

## 0.4.19 - 2026-11-11

- Autoplay continues with YouTube Music's personalized Up next from the last song
  you picked, skipping songs already queued or played, instead of song radio.
- Home reads more shelves, rotates each one on refresh, moves Listen again below
  discovery shelves and adds a "Because you played" shelf from recent listening.
- Search opens on All, YouTube Music's mixed results by section, with a Top result.
- Playlists and albums get a numbered track table with an album column and round
  Play and Shuffle buttons.
- Sidebar with your playlists, a time-of-day greeting and quick tiles on Home,
  pill filters, sliding Settings and Account sheets and an Animations switch.
- Now Playing shows a live lyrics preview over a slowly drifting blurred cover.
- Like and Dislike sit beside the song title; larger Home and library tiles.
- Right-to-left titles stay inside the player bar; restore recent searches.

## 0.4.18 - 2026-11-11

- Give the interface depth: one theme palette, a raised content panel and a glow
  tinted by the current artwork, also behind the player.
- Show gradient art for songs without covers and level bars on the playing song.
- Lift tile covers and reveal a play badge on hover; right-click a song for its menu.
- Add a rounded search field with recent searches, large headers for Liked Songs
  and song radio, a Now Playing view from the player cover, and page fades.
- Remove the Open audio file action.

## 0.4.14 - 2026-11-05

- Refresh Home and playlist layouts with larger covers and more legible track rows.
- Improve discovery and playlist management.
- Add full-screen karaoke with word timing where available and improve timed lyric lookup.
- Seed autoplay from the last manually queued song and filter disliked radio tracks.

## 0.4.13 — 2026-11-04

- Redesign navigation and the player bar while retaining Tide's logo and colors.
- Add temporary native Google sign-in with explicit Music profile selection,
  encrypted session storage and browser-companion fallback.
- Browse saved albums, artists and listening history directly in Library.
- Search songs, albums, artists and playlists using YouTube Music results.
- Add taskbar preview transport, tray controls and memory-only Windows media artwork.
- Keep the queue vertical and move secondary footer actions into More.

## 0.3.0 — 2026-10-22

- Add Like/Dislike controls, including clearing a rating, for the selected account.
- Save songs to owned playlists and remove exact entries without changing the queue.
- Match the playlist chooser to Library rows, with viewport-loaded covers using
  the existing bounded memory cache.
- Add optional listening-history feedback based on actual playback, off by default.
- Move Jams below Library in the left navigation.
- Keep feedback requests bounded and account-bound; uncertain writes are not retried.

## 0.2.0 — 2026-10-22

- Include synchronized Jams in the normal Windows installer with guest queue requests.
- Add a shared free relay and Advanced setup for self-hosted relays.
- Clarify Jam controls and preserve local playback when the host ends a room.
- Validate the installed player, clean Windows install/update/removal, native UI
  and queue continuation before publishing the stable installer.

## 0.1.3 — 2026-10-19

- Match the setup executable's Windows file/product versions to the app release.
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
