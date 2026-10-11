# UI and UX review

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

Library entries open playlist details rather than starting playback. Details show tracks and one Play/Shuffle action group. Search results remain separate from the queue; selecting a search or Home song starts a fresh song seed. Playlist playback follows the selected playlist and then recommendations. Song menus expose Play next, Add to queue and explicit replacement with song radio. Queue menus move existing upcoming tracks rather than duplicating them; current-song repeat actions deliberately create a repeat. Reordering stays within the manual/autoplay groups. Manual choices are inserted before automatic suggestions; disabling autoplay removes upcoming suggestions while retaining the current song and manual choices. Played queue history is omitted from the drawer.

Account state and appearance options appear only in the Account panel; Import/Disconnect/Help are under collapsed Advanced. Failed requests show one actionable banner, with Account settings for connection errors and Dismiss otherwise. Settings scroll at small window heights. Playlist/search metadata is bounded and transient; thumbnail fetching prioritizes the active browsing screen to prevent cache churn across hidden screens.
