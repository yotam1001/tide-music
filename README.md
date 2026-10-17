# Tide Music

A native Windows music player built to keep RAM use low while gaming.

[![Watch the Tide Music demo](docs/media/tide-promo-preview.gif)](docs/media/tide-promo.mp4)

[Watch the 48-second demo](docs/media/tide-promo.mp4)

The demo shows an earlier prototype; its memory comparison is one measured
example, not a fixed RAM guarantee.

Play YouTube Music, bring in your playlists and recommendations, and keep the
player out of the way with a mini mode. Tide uses native audio playback rather
than keeping an embedded browser running.

**Windows preview:** [Download TideSetup.exe](https://github.com/yotam1001/tide-music/releases/download/v0.1.0-preview.1/TideSetup.exe)
from [v0.1.0-preview.1](https://github.com/yotam1001/tide-music/releases/tag/v0.1.0-preview.1).
For YouTube Music, also download the matching
[Chrome companion](https://github.com/yotam1001/tide-music/releases/download/v0.1.0-preview.1/tide-browser-chromium.zip)
and follow the connection guide below. This is an unsigned prerelease.

![Tide's dark interface](docs/screenshots/full.png)

[Light theme](docs/screenshots/light.png) · [Queue](docs/screenshots/queue.png)

Screenshots use sample music metadata.

## Get started

You need **Windows x64** and internet access. Chrome is the supported preview
connection path for YouTube Music. The installer requires no Rust, Python or
API keys.

1. **Get the downloads.** On the Releases page, choose **TideSetup.exe** under
   **Assets**. For YouTube Music, also get **tide-browser-chromium.zip** from the
   same release. GitHub's **Source code** downloads are for developers.
2. **Install and connect.** Follow the [Chrome connection guide](docs/BROWSER_COMPANION_BETA.md):
   load the companion first, enter its ID in Tide setup, then import your selected
   Music tab. Sign in to your existing Google account in Chrome; Tide does not
   ask for your password. The companion is loaded manually, not from the Chrome
   Web Store. For local files only, just open the EXE and choose **Install Tide**.
3. **Play something.** Open Tide from Start. After connecting, search in the top
   bar, choose a playlist in **Library**, or pick a **Home** recommendation.
   To play a file without an account, use **Settings → Open audio file**.

Setup adds Tide to Start. Already installed? Rerunning setup can enable the
companion without uninstalling Tide. [Installation help](docs/INSTALLATION.md)

## What you can do

- Play songs, playlists and local audio with native playback.
- See your playlists, Liked Songs and personalized recommendations.
- Shuffle playlists, queue songs next, and drag upcoming tracks into order.
- Start song radio or enable Autoplay for related recommendations.
- Switch between dark/light themes, compact sizing and a mini player.
- Use Windows media controls and check for stable updates in Settings.

Theme is the sun/moon button in the header. **Account** is beside **Settings**.
**Queue** sits beside the player controls. [Learn the controls](docs/USING_TIDE.md)

## Before you try the preview

**YouTube integration is unofficial.** Service changes can affect playback and
sign-in. Chrome import has worked in a local test, but fresh-user onboarding and
long-term session reliability still need validation. Zen's native connection is
unresolved; Edge and Firefox are not recommended setup paths yet. Experimental
Jams are not included in the main app.

**Your existing account is used.** Tide reads your library; it does not move it
to another service or download your whole collection. Premium availability
depends on your account and the accessible streams. Tide does not provide
Premium, implement ad blocking, or guarantee every track or format.

**Your session stays local.** Saved sign-in data is encrypted for your Windows
user. There is no Tide account server, analytics or automatic crash uploader.
Never share cookies or session files. [Privacy details](PRIVACY.md)

**Memory use varies.** Include Tide and its temporary playback helpers when
comparing resource use. The [measurement notes](docs/media/memory-test.md) explain
the demo's figures. Installed Tide does not create Rust build caches.

## Updates and help

**Settings → Check for updates** finds published stable releases and offers
**Install and restart**. Startup checks can be disabled. The updater skips beta
and draft releases; newer previews need a manual installer update. Accounts and
preferences are kept during app updates. Update the unpacked companion separately
through its guide.

[Troubleshooting](docs/TROUBLESHOOTING.md) ·
[Report a bug](https://github.com/yotam1001/tide-music/issues/new) ·
[Remove Tide](docs/INSTALLATION.md#remove-tide)

Report security issues through [SECURITY.md](SECURITY.md), without publishing
private information.

## Contribute

See [Contributing](CONTRIBUTING.md) and the [developer guide](docs/DEVELOPMENT.md).
Release maintainers can use the [launch checklist](docs/BETA_RELEASE.md).

Tide's original source is [MIT licensed](LICENSE). Dependencies keep their own
licenses; see [third-party notices](THIRD_PARTY_NOTICES.md). Tide is not affiliated
with Google or YouTube.
