# Find your way around Tide

After [installing](INSTALLATION.md) and [connecting your account](BROWSER_COMPANION_BETA.md),
open Tide from the Windows Start menu.

## Play your first song

Search for a song or paste a YouTube music link into the top bar. Select a song
to play it. Or open **Library**, choose a playlist, then use **Play** or **Shuffle**.
YouTube playback requires a verified account connection. Local audio works without
one: choose **Settings → Open audio file**.

The centered player buttons handle previous, pause/play and next. The bottom
sliders control position and volume. Windows media controls are supported;
physical keyboard/Fn behavior while a game has focus still needs user validation.

## Gaming controls (0.1.1 and later)

In **Settings → Gaming controls**, Windows media keys can be switched off if
another app should receive them. Laptop Fn behavior depends on its hardware and
Windows settings; Tide does not remap Fn itself.

Optional global shortcuts control **Play / pause**, **Next song** and
**Previous song** while Tide is in the background. Enter a Ctrl or Alt combination
(for example `Ctrl+Alt+P`) and choose **Apply shortcuts**. Shift can also be used.
Letters, digits, Space and F1–F24 are supported, except Windows-reserved F12.
Blank fields disable individual shortcuts; clear all three and apply to disable
all custom shortcuts. No global shortcuts are registered by default. If Windows
or another app already owns a combination, Tide reports the conflict and keeps
your previous shortcuts. Choose combinations unused by your games; exclusive
keyboard handling in some games or elevated apps still needs hardware testing.

Tide uses Windows' shared default audio output so games can use the same device.
Normal output changes are handled by its WASAPI backend. If output initialization
fails and its bounded retries stop, returning a default multimedia output triggers
one retry of the same song, retaining position and pause intent. Stop, a different
song, natural completion and unrelated network failures are not resumed by this
path. No device identifiers are stored. This needs physical Bluetooth/USB/HDMI
switching checks before claiming compatibility with every device.

## Home and Library

- **Home:** recommendations from your YouTube Music account. Preview 2
  restores the leading rows and their original order, with limited refresh
  variety near those recommendations. Preview 1 rotates more aggressively;
  small upstream pools can repeat in either version.
- **Library:** your playlists and Liked Songs. Selecting a playlist opens its
  tracks; **Play** or **Shuffle** starts it.
- **My Supermix:** always pinned beside Liked Songs in Library. Opening it finds
  your current account's mix and loads its songs, independent of Home's visible
  recommendations. If YouTube does not offer the mix, retry later; its absence
  does not mean your account needs reconnecting.
- **Search:** stays separate from the queue until you select or queue a song.

Tide reads your existing library. It does not copy it to another service, edit
your playlists or download your whole collection. Listening does not currently
send playback-history updates to train YouTube recommendations.

## Queue and recommendations

Choose **Queue** beside the player to see upcoming tracks. It opens beside the
content in wide windows and over the content in narrow windows.

| Action | How |
| --- | --- |
| Play a song after the current one | Its **… → Play next** menu |
| Save a song for later in the queue | **… → Add to queue** |
| Reorder upcoming songs | Drag a row's handle within its manual or autoplay section |
| Remove an upcoming song | Its **… → Remove from queue** menu |
| Start related music from one song | **… → Start song radio**; this replaces upcoming tracks |
| Keep music going when the queue runs low | Enable **Autoplay** in Queue |

The current song stays fixed while reordering. Turning Autoplay off removes
upcoming automatic suggestions while keeping the current song and your manual
choices. Recommendations need a connected account and an available provider.

Tide prepares one upcoming stream address to reduce transition delays. Network
and extraction delays can still cause gaps; playback is not guaranteed gapless.
The best accessible audio stream is requested without re-encoding, but available
quality depends on the song, account and extractor.

## Appearance and account

- **Sun/moon:** change light/dark theme without changing window size.
- **Settings → Layout:** choose Normal or Compact sizing.
- **Mini:** reduce the player; its expand control returns to the full window.
- **Account:** check the connected identity, get connection help or disconnect.
  Manual imports are under Advanced connection options.
- **Settings → Updates:** check for a stable release or change startup checks.
  Preview updates still need the newer installer.

[Troubleshooting](TROUBLESHOOTING.md) · [Privacy](../PRIVACY.md)
