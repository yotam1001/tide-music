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

## Home and Library

- **Home:** recommendations from your YouTube Music account. Refresh rotates the
  selection, though a small upstream recommendation pool can still repeat.
- **Library:** your playlists and Liked Songs. Selecting a playlist opens its
  tracks; **Play** or **Shuffle** starts it.
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
