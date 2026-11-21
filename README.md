<div align="center">

<img src="ui/assets/tide-icon.png" alt="Tide logo" width="112">

# Tide Music

**YouTube Music, without the browser.**
A fast, native player that stays light on RAM, even while you game.

[![Download for Windows](https://img.shields.io/badge/Download-Windows-c2185b?style=for-the-badge&logo=windows&logoColor=white)](https://github.com/yotam1001/tide-music/releases/latest/download/TideSetup.exe)
[![macOS Apple Silicon](https://img.shields.io/badge/macOS-Apple%20Silicon-8e2449?style=for-the-badge&logo=apple&logoColor=white)](https://github.com/yotam1001/tide-music/releases/latest/download/Tide.dmg)
[![macOS Intel](https://img.shields.io/badge/macOS-Intel-8e2449?style=for-the-badge&logo=apple&logoColor=white)](https://github.com/yotam1001/tide-music/releases/latest/download/Tide-Intel.dmg)

[![Website](https://img.shields.io/badge/website-tide--music--links.netlify.app-1a1a2e?style=flat)](https://tide-music-links.netlify.app)
[![Latest release](https://img.shields.io/github/v/release/yotam1001/tide-music?label=latest&color=c2185b)](https://github.com/yotam1001/tide-music/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-8e2449)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-b7410e?logo=rust)](docs/DEVELOPMENT.md)

<br>

<img src="docs/screenshots/readme/home.png" alt="Tide's home screen with personalised mixes, quick picks and album art" width="860">

</div>

---

## Why Tide

- 🪶 **Featherweight.** Native audio playback, with no embedded browser left running in the background.
- 🎮 **Made for gaming.** A pinnable mini player keeps music one click away without stealing your RAM or your screen.
- 🎧 **Your real library.** Sign in with your YouTube Music account to get your playlists, Liked Songs, Supermix and recommendations.
- 🤝 **Jams.** Invite a friend, share one queue and listen together in sync.
- 🔒 **Private by design.** Your session is encrypted on your machine. No Tide account, no analytics and no crash uploader.

## A closer look

<table>
  <tr>
    <td width="50%"><img src="docs/screenshots/readme/lyrics.png" alt="Karaoke lyrics view"><br><sub><b>Karaoke lyrics</b>: words light up in time with the song</sub></td>
    <td width="50%"><img src="docs/screenshots/readme/queue.png" alt="Queue panel"><br><sub><b>Queue</b>: drag to reorder while Autoplay keeps the music going</sub></td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/readme/playlist.png" alt="Playlist view"><br><sub><b>Playlists</b>: play, shuffle, edit and search inside any playlist</sub></td>
    <td><img src="docs/screenshots/readme/search.png" alt="Search results"><br><sub><b>Search</b>: songs, albums, artists and playlists in one place</sub></td>
  </tr>
</table>

<div align="center">
  <img src="docs/screenshots/readme/mini.png" alt="Mini player" width="600"><br>
  <sub><b>Mini player</b>: pin it above your game and keep playing</sub>
</div>

## Features

| | |
|---|---|
| 🎵 **Play anything** | Songs, albums and playlists from YouTube Music, with native playback |
| 🎤 **Karaoke lyrics** | Word-by-word synced lyrics with smooth scrolling |
| 🏠 **Your YouTube Music** | Home recommendations, playlists, Liked Songs, saved albums and history |
| 📻 **Radio & Autoplay** | Start song radio or keep related music flowing automatically |
| 🧑‍🎤 **Artist pages** | Click an artist to play, shuffle or queue their top songs |
| 📋 **Smart queue** | Play next, drag to reorder, and search the queue or any playlist |
| 👍 **Like & save** | Like or dislike songs and edit your own playlists |
| 🔊 **Audio control** | Output device picker, bit-perfect exclusive mode, crossfade, equalizer, sleep timer and even volume |
| 🪟 **Mini player** | Compact one-row player that stays on top with adjustable opacity |
| ⌨️ **System integration** | Media keys, global shortcuts, trackpad swipes, macOS Control Center / menu bar and Discord status |
| 🔄 **In-app updates** | Updates download in the background; restart whenever you're ready |

## Get started

**Windows (x64)**

1. Download **[TideSetup.exe](https://github.com/yotam1001/tide-music/releases/latest/download/TideSetup.exe)** and install it. You don't need Rust, Python or an API key.
2. Click the profile icon → **Sign in with Google**, pick your Music profile, open Library, then click **Connect this profile**.
3. Search, open your Library or tap a Home recommendation. That's it.

If native sign-in isn't available, choose **Use browser companion instead**
([Chrome companion](https://github.com/yotam1001/tide-music/releases/latest/download/tide-browser-chromium.zip)).

**macOS (Sonoma 14 or later, preview)**

Download for [Apple Silicon](https://github.com/yotam1001/tide-music/releases/latest/download/Tide.dmg)
or [Intel](https://github.com/yotam1001/tide-music/releases/latest/download/Tide-Intel.dmg), then follow the
[Mac install guide](docs/INSTALLATION_MACOS.md).

📖 [Account setup](docs/ACCOUNT_SETUP.md) · [Installation](docs/INSTALLATION.md) · [Using Tide](docs/USING_TIDE.md)

## Listen together with Jams

1. Both of you install Tide and connect your own YouTube Music accounts.
2. Play a song, open **Jams**, then **Start Jam → Copy invite**.
3. Your friend pastes the invite into **Jams → Join Jam**.

Guests browse and press **+** to add songs, the host controls playback, and
everyone keeps their own volume. No browser needs to stay open. Jams use a free
shared relay by default, and **Advanced setup** lets you
[host your own](experiments/jam/README.md).

## Privacy

- Sign-in data is encrypted with Windows DPAPI or your Mac login Keychain.
- Tide has no account server, no analytics and no automatic crash uploads.
- Jams contact a relay only when you host or join one.
- Never share your cookies or session files with anyone.

[Full privacy details](PRIVACY.md) · [Security policy](SECURITY.md)

## Updates & help

On Windows, open **Settings → Check for updates**. Music keeps playing while the
update downloads, and you click **Restart to update** when you're ready. Your
accounts and preferences carry over. On Mac, replace Tide in Applications with
the new disk image ([details](docs/INSTALLATION_MACOS.md#updates)).

[Troubleshooting](docs/TROUBLESHOOTING.md) ·
[Report a bug](https://github.com/yotam1001/tide-music/issues/new) ·
[Remove Tide](docs/INSTALLATION.md#remove-tide)

## Contribute

Start from `main` and open your pull request against `main`. See the
[branch guide](CONTRIBUTING.md#which-branch-should-i-use) and the
[developer guide](docs/DEVELOPMENT.md). Maintainers can use the
[launch checklist](docs/BETA_RELEASE.md).

---

<div align="center">
<sub>
Tide is <a href="LICENSE">MIT licensed</a>. Dependencies keep their own licenses (<a href="THIRD_PARTY_NOTICES.md">third-party notices</a>).<br>
Tide is not affiliated with Google or YouTube.
</sub>
</div>
