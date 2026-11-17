# Tide on macOS (preview)

Tide for Mac is available as a preview. See [MACOS_PORT.md](MACOS_PORT.md) for
platform details.

You need macOS 14 Sonoma or later on Apple Silicon or Intel. The disk image
bundles its playback tools (mpv and yt-dlp), so Homebrew, Rust and Python are
not needed.

## Install and open

1. Open **Apple menu → About This Mac**. If you see **Chip** with an Apple M
   series chip, download [Tide.dmg](https://github.com/yotam1001/tide-music/releases/download/v0.4.28-mac-preview/Tide.dmg).
   If **Processor** shows Intel, download [Tide-Intel.dmg](https://github.com/yotam1001/tide-music/releases/download/v0.4.28-mac-preview/Tide-Intel.dmg).
2. Open it and drag **Tide** onto **Applications**.
3. Open Tide from Applications or Spotlight.

The preview is ad-hoc signed and has no Apple notarization. If Gatekeeper
warns that Apple cannot verify the developer, check that you downloaded Tide
from the official GitHub release. After the first launch attempt, go to
**System Settings → Privacy & Security → Open Anyway** and follow the prompt.
See [Apple’s first-launch instructions](https://support.apple.com/en-us/102445).

## Sign in

Open the profile icon → **Sign in with Google**. A separate sign-in window
opens. Sign in, choose the profile you use for YouTube Music, open Library,
then select **Connect this profile**. The window closes and its browsing data
is discarded. A Zen profile's `cookies.sqlite` also works.

Your session is encrypted in
`~/Library/Application Support/TideMusic/account`, and its key is kept in your
login Keychain. macOS may ask once whether Tide can use that Keychain item.

## Listen together

On the Mac 0.4.28 preview and Tide for Windows, start a song and choose
**Jams → Start Jam**. Share the invite with a friend and have them choose
**Jams → Join Jam**. Each listener connects their own YouTube Music account.
Use the default shared relay or enter your own relay in Advanced settings.

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
- **Trackpad swipes.** Swipe two fingers right on the player bar or Mini player
  to skip to the next song, or left to go back. macOS keeps four-finger swipes
  for switching Spaces, so the swipe works inside Tide's window. Control Center,
  media keys and AirPods work from anywhere.
- **Global shortcuts** use ⌘, ⌥ or ⌃ combinations and need no Accessibility
  permission.
- **Song-change banners.** Turn on **Settings → Advanced → Notify when the song
  changes**.
- **Pinned Mini player** floats over every Space and over full-screen apps.
- **Appearance:** **Match system appearance** follows Light and Dark mode, and
  Reduce motion turns off interface animation.

## Updates

Tide checks for new stable releases on launch. You can download them from the
app or use the Mac download links above. Quit Tide, open the new disk image,
and drag Tide into Applications to replace the old copy. Your settings and
session are kept.

## Uninstall

Quit Tide and move **Tide** from Applications to the Bin. To also remove
settings and the saved session, delete `~/Library/Application Support/TideMusic`
and `~/Library/Caches/TideMusic`. Then, in Keychain Access, delete the
`com.tidemusic.Tide.session` item.
