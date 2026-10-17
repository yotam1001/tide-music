# Tide Music — Windows preview

Maintainer draft: publish only with a reviewed matching candidate and completed
[preview checks](BETA_RELEASE.md). Remove this paragraph from the published notes.

Tide is a native Windows music player built for listening while gaming. Play
YouTube Music, use your existing playlists and recommendations, and switch to a
mini player when you want it out of the way.

## Start here

1. Download **TideSetup.exe** from this release and choose **Install Tide**.
2. For YouTube Music, get **tide-browser-chromium.zip** from the same release and
   follow **CHROME_SETUP.md**, or the [online connection guide](https://github.com/yotam1001/tide-music/blob/main/docs/BROWSER_COMPANION_BETA.md).
3. Open Tide from Start. Connect your account, then choose a song or playlist.
   Local files work without an account through **Settings → Open audio file**.

You need Windows x64 and internet access. No Rust, system Python or API key is
required. GitHub's Source code archives are for contributors, not normal setup.

## In this preview

- Native audio playback, search, playlists, Liked Songs and recommendations.
- Queue dragging, playlist shuffle, song radio and recommendation Autoplay.
- Dark/light themes, compact sizing and a mini player.
- Windows media controls and checksum-verified stable-update checks.
- Per-user installation, Start-menu discovery, and Windows Settings removal.

## Known limits

- This is an unsigned preview with unofficial YouTube integration.
- Chrome connection uses a manually loaded companion. There is no Chrome Web
  Store installation or official Google OAuth login flow.
- Other browser connection paths are not recommended yet. YouTube can expire a
  session or change stream availability; a minimum session lifetime is not promised.
- Audio quality and memory use vary. There is no universal RAM cap, gapless
  playback promise or guarantee of every Premium format.
- The updater skips prereleases. Install newer previews with their installer;
  update unpacked extension files separately.
- Experimental Jams are not part of this release.

[Troubleshooting](https://github.com/yotam1001/tide-music/blob/main/docs/TROUBLESHOOTING.md) ·
[Privacy](https://github.com/yotam1001/tide-music/blob/main/PRIVACY.md) ·
[Report a bug](https://github.com/yotam1001/tide-music/issues/new)

Include the app/Windows version and exact error. Never include cookies, request
headers, session files or signed media URLs.
