# Tide on macOS (preview)

Tide for Mac is available as a preview. See [MACOS_PORT.md](MACOS_PORT.md) for
platform details.

You need macOS 14 Sonoma or later on Apple Silicon or Intel. The disk image
bundles its playback tools (mpv and yt-dlp), so Homebrew, Rust and Python are
not needed.

## Install with one Terminal command (recommended)

Open **Terminal** and paste:

```sh
curl -fsSL https://raw.githubusercontent.com/yotam1001/tide-music/main/scripts/install-tide-macos.sh | sh
```

It downloads the disk image for your processor, checks it against the SHA-256
published with the release, verifies Tide's signature, copies Tide into
Applications and opens it. Because the download does not come from a browser,
macOS does not block the first launch. Run the same command again to reinstall.
After that, Tide updates itself from **Settings → About**.

## Install from the disk image

1. Open **Apple menu → About This Mac**. If you see **Chip** with an Apple M
   series chip, download [Tide.dmg](https://github.com/yotam1001/tide-music/releases/latest/download/Tide.dmg).
   If **Processor** shows Intel, download [Tide-Intel.dmg](https://github.com/yotam1001/tide-music/releases/latest/download/Tide-Intel.dmg).
2. Open it and drag **Tide** onto **Applications**.
3. Open Tide from Applications or Spotlight.

### First launch

The preview is ad-hoc signed. If Gatekeeper asks you to approve the developer,
check that the download came from Tide’s official GitHub release.

1. On macOS 14, try **Control-click Tide → Open** in Finder and confirm **Open**
   if offered.
2. On macOS 15 or later, or if Finder gives you no **Open** button, try opening
   Tide once. Then go to **System Settings → Privacy & Security**, scroll down
   to the Tide notice, and select **Open Anyway**. Confirm when prompted.

If macOS previously said Tide was damaged, download a fresh disk image from
the links above and replace Tide in Applications.

[Apple’s first-launch instructions](https://support.apple.com/en-us/102445)

## Sign in

Open the profile icon → **Sign in with Google**. A separate sign-in window
opens. Sign in, choose the profile you use for YouTube Music, open Library,
then select **Connect this profile** (or press Return). Family Link accounts
work too. Already signed in to YouTube Music in Zen? Choose **Zen browser →
Connect** in the same sheet to use that session in one click. The window closes and its browsing data
is discarded. A Zen profile's `cookies.sqlite` also works.

Your session is encrypted in
`~/Library/Application Support/TideMusic/account`, and its key is kept in your
login Keychain. macOS may ask once whether Tide can use that Keychain item.

## Listen together

Choose **Jams** below **Library** to host or join a Jam, the same as on
Windows. See the [Jam guide](../experiments/jam/README.md) for relay self-hosting.

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

With automatic checks enabled, Tide looks for a newer stable release with a
disk image for your processor. Choose **Update**: Tide downloads the image,
checks it against the release's SHA-256, verifies the new Tide's signature,
bundle ID and version, and stages it while your music keeps playing. Choose
**Restart to update** and Tide quits, replaces itself and reopens. The update
is not quarantined, so macOS does not ask you to approve Tide again, and its
stable signing identity keeps Keychain access without a password prompt.
Your settings and session are kept.

If Tide runs from the disk image or from a folder you cannot write to,
**Download Mac update** opens the download in your browser instead.

## Uninstall

Quit Tide and move **Tide** from Applications to the Bin. To also remove
settings and the saved session, delete `~/Library/Application Support/TideMusic`
and `~/Library/Caches/TideMusic`. Then, in Keychain Access, delete the
`com.tidemusic.Tide.session` item.
