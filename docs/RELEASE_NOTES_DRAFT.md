# Tide Music 0.1.3

Maintainer draft: publish only with a reviewed matching candidate and completed
[preview checks](BETA_RELEASE.md). Remove this paragraph from the published notes.

Tide is a native Windows music player built to keep RAM use low while gaming. Play
YouTube Music, use your existing playlists and recommendations, and switch to a
mini player when you want it out of the way.

## Start here

1. For YouTube Music, download **tide-browser-chromium.zip** and follow
   **CHROME_SETUP.md** first, or the [online connection guide](https://github.com/yotam1001/tide-music/blob/main/docs/BROWSER_COMPANION_BETA.md).
   Load the companion so you can enter its extension ID during setup.
2. Download **TideSetup.exe** from this release and choose **Install Tide**.
   For local files only, you can skip the companion and start with this step.
3. Open Tide from Start. Connect your account, then choose a song or playlist.
   Local files work without an account through **Settings → Open audio file**.

You need Windows x64 and internet access. No Rust, system Python or API key is
required. GitHub's Source code archives are for contributors, not normal setup.

## Update from an installed preview

In Tide 0.1.2, 0.1.1, 0.1.0 or an installed preview, open **Settings → Check for updates**,
then choose **Install and restart**. Tide verifies the installer download, stops
playback, updates the installed app and reopens it. Your account and preferences
are kept. Portable/development copies should use **TideSetup.exe** manually.
The existing Chrome companion remains compatible and does not need replacing.

## New in 0.1.3

- Reduce temporary RAM allocations when loading account results and playlists.
- Resize album covers without copying the full cropped source image, preserving
  thumbnail pixels and channel precision.
- Replace playlists without cloning the previous queue; playback failures still
  restore the original queue.

## Recent updates included

- **Artist pages:** select an artist name in the player, queue, search or Home
  to see up to 50 top songs from YouTube Music. Play, shuffle or queue from the
  list. Multiple artists offer a choice; Back restores the previous view and
  queue drawer without changing playback.
- **Playback feedback:** loading, buffering, pause and automatic retries appear
  in the player. Specific audio-output errors retain useful recovery guidance;
  manual retry preserves position and pause intent.
- **Audio format:** see the observed codec, approximate bitrate and sample rate
  when mpv reports them. Measured bitrate varies and does not certify Premium
  membership or a fixed source quality.
- **Performance regression checks:** Windows CI exercises local song transitions,
  repeated skips, process-tree memory and observed helper cleanup.

Windows source/UI/package and actual install/update/remove checks passed for the
feature implementation. A live public artist lookup also passed without a saved
account. The final matching versioned candidate must pass CI before publication.
The synthetic three-second local-audio benchmark uses an explicit null audio
output: it does not measure YouTube startup latency, WASAPI device compatibility
or gaming impact. Physical keyboard/headset checks and long-term service behavior
remain separate; the stable channel does not mark those checks as passed.

## Included

- Native audio playback, search, playlists, Liked Songs and recommendations.
- Queue dragging, playlist shuffle, song radio and recommendation Autoplay.
- Dark/light themes, compact sizing and a mini player.
- Windows media controls and checksum-verified stable-update checks.
- Per-user installation, Start-menu discovery, and Windows Settings removal.

## Known limits

- This is an unsigned release with unofficial YouTube integration.
- Chrome connection uses a manually loaded companion. There is no Chrome Web
  Store installation or official Google OAuth login flow.
- Other browser connection paths are not recommended yet. YouTube can expire a
  session or change stream availability; a minimum session lifetime is not promised.
- Audio quality and memory use vary. There is no universal RAM cap, gapless
  playback promise or guarantee of every Premium format.
- The updater skips prereleases. Install newer previews with their installer;
  update unpacked extension files separately when they change.
- Experimental Jams are not part of this release.
- Physical media keys in focused games, fresh-user browser onboarding and
  long-term session continuity remain separate checks. The stable channel
  label does not claim that these outstanding checks have passed.

[Troubleshooting](https://github.com/yotam1001/tide-music/blob/main/docs/TROUBLESHOOTING.md) ·
[Privacy](https://github.com/yotam1001/tide-music/blob/main/PRIVACY.md) ·
[Report a bug](https://github.com/yotam1001/tide-music/issues/new)

Include the app/Windows version and exact error. Never include cookies, request
headers, session files or signed media URLs.
