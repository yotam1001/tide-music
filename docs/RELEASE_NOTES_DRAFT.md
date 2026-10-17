# Tide Music 0.1.0

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

In preview 1 or preview 2, open **Settings → Check for updates**, then choose
**Install and restart**. Tide verifies the installer download, stops playback,
updates the installed app and reopens it. Your account and preferences are kept.
Portable/development copies should use **TideSetup.exe** manually instead.

This is the preview 2 feature set with matching stable app, installer and release
versions, published at the maintainer's request to test public in-app updating.
It includes the Home relevance fix and the pinned My Supermix Library shortcut.
No updater policy, audio engine or account behavior changes in this promotion.
The existing Chrome companion remains compatible and does not need replacing.

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
