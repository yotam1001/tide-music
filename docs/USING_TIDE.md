# Find your way around Tide

After [installing](INSTALLATION.md) and [connecting your account](BROWSER_COMPANION_BETA.md),
open Tide from the Windows Start menu.

## Play your first song

Search for a song or paste a YouTube music link into the top bar. Select a song
to play it. Or open **Library**, choose a playlist, then use **Play** or **Shuffle**.
YouTube playback requires a verified account connection.

The centered player buttons handle previous, pause/play and next. The bottom
sliders control position and volume. Windows media controls are supported;
physical keyboard/Fn behavior while a game has focus still needs user validation.

## Desktop navigation (0.4.13)

The navigation rail provides **Home**, **Library**, and **Jams**. Taller windows
also show **Liked** and **Supermix**. Open saved playlists from Library; use the
single top-right profile icon to open Account. Home offers **Resume** and **Lyrics**
for the current song, with an up-next preview when space allows. These reuse the
existing queue and artwork.

The player keeps **Like**, a labeled **Lyrics** microphone button and **Queue**
visible. **More (…)** contains **Dislike** and **Open mini player**. Mini mode keeps
its direct rating, pin and expand controls. The queue remains a vertical list with
drag-to-reorder controls.

Use **Ctrl+K** to focus search and **Ctrl+L** to open or close lyrics. **Esc** closes
lyrics. While recording a custom keyboard shortcut, these combinations are recorded
instead of triggering navigation.

Windows taskbar previews offer previous, play/pause and next. The Tide tray icon
opens the player; its context menu also offers transport, **Minimize to tray** and
**Quit Tide**. Closing the main window still exits Tide normally. Turning off Windows
media controls also removes the tray and taskbar controls. Jam transport permissions
are the same whether a command comes from the player or Windows.
Windows media controls also reuse the current cover from Tide's thumbnail cache.
This is a small memory-only image; it does not add another artwork download or
save a cover file to disk.

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

- **Home:** a featured selection, compact song cards and cover shelves grouped
  under your YouTube Music recommendation sections. The layout preserves the provider's order; it does not change
  which music is recommended. Select a song to play it; its **…** menu offers
  queue, radio and playlist actions. Playlist tiles open their tracks.
- **Library:** switch between **Playlists**, **Albums**, **Artists**, and **History**.
  My Supermix and Liked Songs stay pinned in Playlists. Use the view icon beside
  the heading to switch collections between covers and a compact list; this choice
  saves automatically. History remains in listening order, including repeats.
  Short windows use lists automatically. Selecting a playlist or album opens its
  tracks; **Play** or **Shuffle** starts it. Select an artist to open their page.
  Refresh reloads the selected tab; each new section shows up to 200 items.

Playlists without an image use a small cover bundled with Tide. Source artwork
replaces it as it loads; no additional download or per-playlist image file is
needed for the fallback. The main browsing area keeps a readable width on large
windows while adapting when you resize Tide. Preference switches support mouse,
keyboard and screen-reader input.
- **My Supermix:** always pinned beside Liked Songs in Library. Opening it finds
  your current account's mix and loads its songs, independent of Home's visible
  recommendations. If YouTube does not offer the mix, retry later; its absence
  does not mean your account needs reconnecting.
- **Search:** stays separate from the queue until you select or queue a song.
  Small covers load on demand using the existing bounded thumbnail cache.
  Use **Songs**, **Albums**, **Artists**, and **Playlists** to change category.
  Signed-in Music search returns up to 40 results; the unconnected songs-only
  fallback returns up to 20. Opening a collection shows its tracks or artist page.
- **Search this list:** filters playlist/artist tracks by title, artist or album.
  All words must match; playback order and duplicate entries are preserved.
- **Search queue:** use the magnifying glass in Queue. Clear the search before
  dragging to reorder; filtering never changes your queue.

Both cover grids adjust to the available width. Only thumbnails near the
visible rows are requested, with at most 40 image references per browsing
model and the bounded 48-entry, 160-pixel thumbnail cache. Scrolling releases
offscreen references; no artwork disk cache or embedded browser is added.

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
- **Window size:** drag the window edges; the layout adapts automatically.
- **Mini:** reduce the player; its expand control returns to the full window.
- **Account:** check the connected identity, get connection help or disconnect.
  Manual imports are under Advanced connection options.
- **Settings:** check for a stable release. Startup checks are under Advanced.
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

### Synced lyrics

Use the lyrics icon beside Queue to open the full-page lyrics view. Lyrics are fetched for the current song only while the full-page view is visible. Tide asks YouTube Music and can match LRCLIB by title, artist and duration to find karaoke timing or fill in missing timed lyrics. That separate lookup sends only public song metadata, never your Google cookies or session headers. Source attribution appears below the lyrics. Lyrics follow actual playback. Lines with word timing highlight word by word when they fit on one line. Other timed lines highlight as a whole. Click a timed line to seek. Guests in a Jam follow their own playback timing and cannot seek the room through the lyrics panel. Use Following to pause/resume auto-scroll and Back or Escape to return to browsing.

Some songs provide plain lyrics or none. Retry is local to lyrics and does not disconnect your account. Leaving the lyrics view or switching songs discards stale lyrics. One bounded read may finish to preserve refreshed session cookies; lyrics are not saved to disk. Missing playlist covers use four small bundled illustrations chosen consistently by playlist ID.
