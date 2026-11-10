# UI and UX review

The earlier screen mixed account setup, library browsing and playback, showed oversized invented artwork above the queue, and coupled light theme to a smaller window. The new shell uses Home, Library and Queue as stable destinations. Search and transport controls remain available on every full/compact screen.

- Home shows provider recommendations and shortcuts to Liked Songs and playlists. Unsupported album cards cannot start playback and show a disabled action.
- Library keeps account selection and connection state together and gives each playlist Play and Shuffle actions.
- Queue shows track rows, current selection, upcoming count, remove, shuffle, clear, repeat, Autoplay and Start radio. The decorative cover banner is removed.
- Brightness and density are separate controls. Mini preserves the previous window dimensions and has Restore. Circular window sizing bindings were removed. Eight screenshot states exercise theme and mini restoration; this addresses identified sizing faults but does not establish that every reported disappearance is resolved.
- Volume updates its percentage immediately as the slider changes, independently of the media poll.
- Recommendations are requested near the queue end and appended without duplicates. EOF advances to queued tracks or waits for the pending radio response. Next at the end can request more recommendations. Stop, clear and current-track removal cancel pending automatic restart intent. A stale response for another playback generation is discarded.

## Artwork cost

The memory-only cache holds at most 56 thumbnails, each 160 by 160 RGB pixels: 4,300,800 bytes (about 4.1 MiB) of decoded cache pixels. The helper chooses the smallest provider thumbnail of at least 160 px so cards and the playlist cover stay sharp. Downloads and decoding add temporary buffers. Fetching runs on one worker, with a 1 MiB response limit, decoder dimensions bounded to one million source pixels, five-second timeout and no redirects or account cookies. Only approved HTTPS image CDNs are accepted. Current/nearby queue tracks and the first twelve library/recommendation cards are prioritized. Missing or uncached images use a small neutral tile rather than invented album covers. The cache clears on account changes.

## Brand

A new wave/rhythm mark replaces the letter T. The generated source and PNG/ICO variants are in `ui/assets/`. Windows builds embed the ICO in the executable; the native window also sets its icon. The source-generation prompt is recorded alongside the assets.

## Remaining limits

YouTube access is unofficial and can change. Radio can return no usable recommendations; the app reports that outcome rather than silently claiming continuation. Queues are capped at 2,000 tracks. Albums and unsupported recommendation card types are not playable from Home yet. Long-session stability and subjective usability still need normal listening feedback.


## Simplified listening hierarchy

The previous revision still gave setup, appearance and playback too many permanent controls. The current shell has only Home and Library in the left navigation, one search field and Account menu in the header, and one persistent playback bar. The queue drawer is closed initially, docks beside content at wide widths and overlays content at narrow widths. Playing music does not navigate to Queue.

Library entries open playlist details rather than starting playback. Details show tracks and one Play/Shuffle action group. Search results remain separate from the queue; selecting a search or Home song starts a fresh song seed. Playlist playback follows the selected playlist and then recommendations. Song menus expose Play next, Add to queue and explicit replacement with song radio. Queue drag handles move existing upcoming tracks rather than duplicating them; current-song repeat actions deliberately create a repeat. Reordering stays within the manual/autoplay groups. Manual choices are inserted before automatic suggestions; disabling autoplay removes upcoming suggestions while retaining the current song and manual choices. Played queue history is omitted from the drawer.

Account state and appearance options appear only in the Account panel; Import/Disconnect/Help are under collapsed Advanced. Failed requests show one actionable banner, with Account settings for connection errors and Dismiss otherwise. Settings scroll at small window heights. Playlist/search metadata is bounded and transient; thumbnail fetching prioritizes the active browsing screen to prevent cache churn across hidden screens.

## Visual refinement

The current destinations and control placement stay intact. Home/Library use
small vector icons and a quiet tinted selection. Track and playlist lists use
thin inset separators instead of a filled card around every row; the current
song retains its accent. Larger headings, consistent text contrast, restrained
button outlines and matching dark/cream surfaces clarify the visual hierarchy.
Menu and close icons are drawn as vectors, and empty artwork uses a music note.
No new status panels, illustrations, background textures or animations are added.
The playback transport is centered independently of title length in full,
compact and mini layouts. Mini uses a small expand icon in place of the wide
Restore button; its accessible label describes returning to the full window.
Account and compact Queue backdrops are explicitly anchored to the content area's
left/top edges. Slint's default centering of their narrower rectangles previously
left an unintended undimmed strip and a visible rectangular boundary.


## Modern interface and explicit UI state (claude-fixes-ui)

### Implicit state machines made explicit

| Before | After |
| --- | --- |
| Four booleans (`home-open`, `library-open`, `detail-open`, `search-open`) with an implied priority order; Back depended on which flags happened to be left set. | `Page { home, library, playlist, search }` plus a `section` root. Back always returns to the section; Escape closes settings, then the queue, then the page. |
| `account-busy` meant "any provider request", so the Account panel said "Connecting…" and the whole library was disabled while radio recommendations loaded. | `provider-busy` (any request) and `account-connecting` (an account change) feed one derived `AccountState { disconnected, saved, verifying, connected }`, shown consistently in the top bar dot, Account sheet, Home and Library. |
| The banner's action came from `error-is-account`/`error-retryable` flags; `report_error` left a previous account flag set, so an unrelated playback error could offer "Account settings". | `ErrorAction { dismiss, retry, account }` is derived in one place; every new error resets the account flag, and every banner has a Dismiss control backed by `dismiss-error`. |
| Each song row owned a `menu-open` flag, so several inline menus could be open at once and rows changed height. | Song actions open in a popup menu: at most one exists and rows keep a fixed height. |
| Loading was only visible as the "Loading…" status string. | A `loading` property drives the player subtitle and accessible play-button label. |
| Clearing the queue was a single click. | It asks for confirmation inline; the confirmation expires after a few seconds. |

### Visual system

`ui/theme.slint` defines one palette per theme (`Theme` global) and the shared controls; `components.slint`, `queue.slint` and `player.slint` hold content, queue and player components; `main.slint` composes the window. Buttons use a `Hit` surface (standard Button for keyboard and accessibility, TouchArea for hover and pointer clicks), so every control now has hover and press feedback.

### Motion

All durations come from `Theme.fast`/`medium`/`slow`. They become zero when **Animations** is off in Account settings, a choice saved with the other preferences. Animations exist only while something changes: hover and press transitions, page enter (fade and rise), the Account sheet and narrow-window queue sliding in over a fading scrim, the toast and error banner, the segmented control and switches, card hover lift, and equalizer bars on the playing song, which stop when paused. Native pixel tests turn motion off to compare settled frames.

### Dynamic color

The app samples one representative color from the current cover and the open playlist's cover (`artwork_tint`), favoring colorful pixels and normalizing brightness. Page heroes and the player glow cross-fade to that tint.
