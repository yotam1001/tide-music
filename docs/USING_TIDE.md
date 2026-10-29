# Find your way around Tide

After [installing](INSTALLATION.md) and [connecting your account](BROWSER_COMPANION_BETA.md),
open Tide from the Windows Start menu.

## Play your first song

Search for a song or paste a YouTube music link into the top bar. Select a song
to play it. Or open **Library**, choose a playlist, then use **Play** or **Shuffle**.
YouTube playback requires a verified account connection. Local audio works without
one: choose **Settings → Advanced → Open audio file**.

The centered player buttons handle previous, pause/play and next. The bottom
sliders control position and volume. Windows media controls are supported;
physical keyboard/Fn behavior while a game has focus still needs user validation.

## Gaming controls (0.1.1 and later)

In **Settings → Advanced**, Windows media keys can be switched off if
another app should receive them. Laptop Fn behavior depends on its hardware and
Windows settings; Tide does not remap Fn itself.

Optional global shortcuts control **Play / pause**, **Next song** and
**Previous song** while Tide is in the background. Open **Settings → Keyboard shortcuts**,
select an action and press the combination; it saves automatically. Use Ctrl or Alt
with a letter, digit, Space or F1–F24 (except Windows-reserved F12). Shift is optional.
Esc cancels; × clears an individual shortcut. Existing custom shortcuts are
suspended only while the focused recorder is waiting, and restored on cancellation
or focus loss. No global keyboard hook or typing log is used.

No custom shortcuts are registered by default. If Windows or another app already
owns a combination, Tide reports the conflict and keeps
your previous shortcuts. Choose combinations unused by your games; exclusive
keyboard handling in some games or elevated apps still needs hardware testing.

**Settings → Audio** lets you choose Windows default or a specific output.
Play a song once to discover outputs. Windows default follows system output
changes; a selected device stays selected until you change it. An unavailable
saved output is labeled clearly instead of silently replacing your preference.
Output choices and normalization survive restarts and app updates and apply only
to your own player, including in a Jam. Physical Bluetooth/USB/HDMI switching
still needs hardware validation.

**Even volume**, off by default, smooths levels with a native FFmpeg dynaudnorm
filter. It processes streamed audio in small windows, without downloading or
analyzing entire songs. It changes dynamics and adds a small processing delay;
turn it off to keep the original sound. This is real-time dynamic normalization,
not a whole-track loudness measurement.

A song that still fails after bounded recovery offers **Retry song** and **Skip**.
Output failures direct you to Audio settings. Retry restores position and pause
intent; Skip uses your normal queue/autoplay. Dismiss hides the notice without
changing your saved login. Playback errors do not by themselves request a reconnect.

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
  Small covers load on demand using the existing bounded thumbnail cache.
- **Search this list:** filters playlist/artist tracks by title, artist or album.
  All words must match; playback order and duplicate entries are preserved.
- **Search queue:** use the magnifying glass in Queue. Clear the search before
  dragging to reorder; filtering never changes your queue.

Tide reads your existing library without downloading your whole collection.
The thumbs beside the player like or dislike the current song on YouTube Music;
select the active thumb again to clear its rating.

Use a song's **… → Add to playlist…** menu to choose one of your own playlists.
In an editable playlist, **… → Remove from playlist** removes that exact entry,
including when a song appears twice. Removing a playlist entry leaves your
playback queue alone. Use **Refresh** to reload the playlist from YouTube Music.
If an update cannot be confirmed, refresh before trying again.

**Settings → Listening history** is off by default. Enable it
to send new, actual listening to your selected YouTube Music account after enough
playback. Seeking, waiting, paused time and automated tests do not count. YouTube
decides how that history affects recommendations; changes may take time. Turning
it off stops new submissions and does not remove history already sent.

**Jams**, below **Library**, opens hosting and joining controls. In a Jam, likes,
playlist edits, history consent and volume apply to your own account/player;
the host controls shared playback.

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
- **Settings → Window size:** choose Normal or Compact sizing.
- **Mini:** reduce the player; its expand control returns to the full window.
- **Account:** check the connected identity, get connection help or disconnect.
  Manual imports are under Advanced connection options.
- **Settings:** check for a stable release. Startup checks are under More options.
  Preview updates still need the newer installer.

[Troubleshooting](TROUBLESHOOTING.md) · [Privacy](../PRIVACY.md)

## Artist pages (0.1.2 and later)

Select an artist name under a song in the player, queue, Home, search or a song
list. Tide uses YouTube Music artist IDs; songs with multiple credited artists
show a choice. Older saved songs are resolved from their video ID when needed.
If YouTube does not return an artist, Tide explains that in the page.

Artist pages show up to 50 top songs in YouTube Music's order. Use **Play**,
**Shuffle**, or the song menu to queue a track. **Back** returns to the prior
list or search. Browsing does not change the playing song. Artist pages need a
connected account and do not follow artists or change your library.

The small line under the playing artist distinguishes loading, buffering,
pausing and automatic retries. Otherwise it shows the audio format reported by
mpv when available, for example **Opus · ~160 kbps · 48 kHz**. The bitrate is an
approximate packet measurement that can vary during a song; it does not certify
Premium membership or a fixed source quality. Missing values are omitted.
