# UI and UX review

## Settings, menus and visual hierarchy (2026-10-17)

The current review keeps Home, Library, the optional Queue drawer and the centered
playback bar. The existing Tide mark, icon files, burgundy dark accent and warm
light palette are retained at the user's request.

| Finding | Change |
| --- | --- |
| Appearance and window sizing were hidden behind Account | A direct sun/moon control and separate Settings entry stay in the header. Settings groups Layout, local playback and About; theme switching lives only in the header and Account has its own labeled header button |
| Account was hard to find and exposed numbered account slots before the user needed them | Account moves beside Settings; its panel leads with profile and connection state, with account-slot selection and manual import under Advanced |
| Song actions expanded a row into horizontal button bars | Song and queue actions use explicitly styled Slint popups with keyboard selection, Escape and outside-click dismissal, keeping list geometry fixed; their colors no longer depend on the OS context-menu style |
| Queue duration, title, menu and a separate remove button competed for width | Remove moves into the song menu. Drag handles stay visible; the playing row shows its artist without duplicating the Now playing heading |
| Most secondary actions looked like primary boxed controls | Secondary controls have a quieter treatment with hover/focus feedback; the main play button remains dominant |
| Navigation and mixed playlist/song rows lacked consistent spacing | A wider normal navigation rail, consistent list-row height and content gutters clarify the hierarchy; narrow windows keep a smaller rail |
| Routine Playing status consumed a separate strip above playback | Remove the status strip; actionable errors retain their existing banner |
| Home repeated Resume above the existing transport | The redundant action is removed; playback stays in the centered transport |
| Empty screens framed a large blank rectangle | Empty-state copy remains, with open space instead of an unnecessary card outline |

Theme and size continue to use independent existing callbacks. Connection errors
and connection CLI entry points now open Account rather than Settings. Panels
are mutually exclusive, close with Escape, scroll at short heights and leave
playback available. Browsing uses 72px rows, 48px covers and 16/13px song/artist
text; the queue retains its denser 66px rows. Artwork viewport selection reads
the same Slint layout dimensions rather than duplicating heights in Rust.
Existing metadata clipping, artwork scheduling/cache bounds, slider binding
fixes and restore invalidation remain in place.

Verification uses the real Slint components and software renderer, plus native
pointer/keyboard input, without browser rendering. The tests use synthetic
metadata and never load an account or start audio. These isolated checks do not
establish physical media-key routing, real-session reliability or user acceptance
of the redesign. See VALIDATION.md for completed results and remaining gates.

## Earlier review notes

The sections below record previous iterations; the settings/menu behavior above
supersedes their earlier control placement descriptions.

The earlier screen mixed account setup, library browsing and playback, showed oversized invented artwork above the queue, and coupled light theme to a smaller window. The new shell uses Home, Library and Queue as stable destinations. Search and transport controls remain available on every full/compact screen.

- Home shows provider recommendations and shortcuts to Liked Songs and playlists. Unsupported album cards cannot start playback and show a disabled action.
- Library keeps account selection and connection state together and gives each playlist Play and Shuffle actions.
- Queue shows track rows, current selection, upcoming count, remove, shuffle, clear, repeat, Autoplay and Start radio. The decorative cover banner is removed.
- Brightness and density are separate controls. Mini preserves the previous window dimensions and has Restore. Circular window sizing bindings were removed. Eight screenshot states exercise theme and mini restoration; this addresses identified sizing faults but does not establish that every reported disappearance is resolved.
- Volume updates its percentage immediately as the slider changes, independently of the media poll.
- Recommendations are requested near the queue end and appended without duplicates. EOF advances to queued tracks or waits for the pending radio response. Next at the end can request more recommendations. Stop, clear and current-track removal cancel pending automatic restart intent. A stale response for another playback generation is discarded.

## Artwork cost

The memory-only cache holds at most 48 thumbnails, each 96 by 96 RGB pixels: 1,327,104 bytes (about 1.27 MiB) of decoded cache pixels. Downloads and decoding add temporary buffers. Fetching runs on one worker, with a 1 MiB response limit, decoder dimensions bounded to one million source pixels, five-second timeout and no redirects or account cookies. Only approved HTTPS image CDNs are accepted. Current/nearby queue tracks and the first twelve library/recommendation cards are prioritized. Missing or uncached images use a small neutral tile rather than invented album covers. The cache clears on account changes.

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
