# Tide on macOS (in development)

The macOS build is being ported and has not been released yet. This page
describes how it will install and what it adds on a Mac. Platform details and
progress live in [MACOS_PORT.md](MACOS_PORT.md).

You need macOS 14 Sonoma or later on Apple Silicon or Intel. The disk image
bundles its playback tools (mpv and yt-dlp), so Homebrew, Rust and Python are
not needed.

## Install and open

1. Download **Tide.dmg** from the latest release.
2. Open it and drag **Tide** onto **Applications**.
3. Open Tide from Applications or Spotlight.

Until Tide has an Apple Developer ID, the app is ad-hoc signed and Gatekeeper
blocks the first launch. To allow it once, Control-click **Tide** in
Applications, choose **Open**, then **Open** again. On macOS 15 and later, open
**System Settings → Privacy & Security** and choose **Open Anyway** for Tide.
Only do this for a disk image downloaded from this repository's releases.

## Sign in

Open the profile icon → **Connect**. The WebView2 sign-in window is Windows-only.
On a Mac, connect with the browser companion or a Zen profile's
`cookies.sqlite` (Tide opens `~/Library/Application Support/zen/Profiles`). A
native Mac sign-in window is planned.

Your session is encrypted in
`~/Library/Application Support/TideMusic/account`, and its key is kept in your
login Keychain. macOS may ask once whether Tide can use that Keychain item.

## What's different on a Mac

- **Control Center and media keys.** The playing song, artwork and progress
  appear in Control Center and on the lock screen. The keyboard media keys and
  AirPods controls play, pause and skip.
- **Menu bar.** Turn on **Settings → Advanced → Show Tide in the menu bar** to
  control playback from the menu bar.
- **Dock menu.** Control-click Tide's Dock icon for Play/Pause, Next, Previous,
  Like and the Mini player.
- **Closing the window keeps the music playing.** Click the Dock icon to bring
  Tide back, and press ⌘Q to quit.
- **Keyboard:** ⌘, Settings · ⌘F Search · Space Play/Pause · ⌘→ / ⌘← Next/Previous ·
  ⌘↑ / ⌘↓ Volume · ⌘L Like · ⌘⇧M Mini player · ⌘K Search · ⌘W Hide
- **Global shortcuts** use ⌘, ⌥ or ⌃ combinations and need no Accessibility
  permission.
- **Song-change banners.** Turn on **Settings → Advanced → Notify when the song
  changes**.
- **Pinned Mini player** floats over every Space and over full-screen apps.
- **Appearance:** **Match system appearance** follows Light and Dark mode, and
  Reduce motion turns off interface animation.

## Updates

Tide checks for new releases on launch. On a Mac, **Download Tide.dmg** opens
the new disk image in your browser. Drag Tide into Applications again to replace
the old copy. Your settings and session are kept.

## Uninstall

Quit Tide and move **Tide** from Applications to the Bin. To also remove
settings and the saved session, delete `~/Library/Application Support/TideMusic`
and `~/Library/Caches/TideMusic`. Then, in Keychain Access, delete the
`com.tidemusic.Tide.session` item.
