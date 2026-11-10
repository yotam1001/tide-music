# Tide → Grove feature audit

Reviewed 2026-10-22 against Tide 0.3.0, `origin/main` at `567cb97`, especially
`docs/USING_TIDE.md` and the native application. The older Tide checkout on this
branch is not the parity reference. **Full parity is not complete.**

“Implemented” means a reachable native control with connected application logic.
Offline callbacks do not establish that a Spotify grant, endpoint, audio device
or second Premium account works in production.

| Tide feature | Grove status | Access / remaining difference |
| --- | --- | --- |
| Search | Implemented | Search music names or paste a Spotify link; track results in batches of 10; stale responses cannot replace newer searches. |
| Direct music links | Implemented | Search/Now playing accept Spotify track, album, playlist and episode links. Queue → Add a song link resolves a track's name and adds it to Grove's queue. |
| Home / refresh | Provider alternative | Recent top tracks, falling back to saved tracks; not YouTube's recommendation feed. |
| My Supermix | No direct equivalent | Home is the available discovery entry point, not the same YouTube mix. |
| Playlists / Liked Songs | Implemented | Library previews, pagination, Play and Shuffle for loaded songs. Spotify grant/app restrictions can limit playlist contents. |
| Artist browsing | Provider alternative | Song menu/current artist open up to 50 tracks from three recent releases, with Play, Shuffle and Back. First credited artist only; artist choice remains open. |
| Artwork | Implemented | Public Spotify CDN only; no OAuth on downloads; bounded worker, timeout, size/decode limits, 48-entry cache. Visible models retain their decoded images until changed. |
| Song menus | Implemented | Play, Play next, Add to queue, Like/Unlike, View artist, Hide and playlist editing. Remove appears only for owned playlists and asks for confirmation. |
| Likes / dislikes | Provider alternative | Spotify Save/Unsave including current player. Hide is a Grove preference, not a Spotify dislike. |
| Playlist editing | Different semantics | Choose an owned playlist or paste its link. Ownership is checked before writes. Removal explicitly affects all matching copies, not Tide's exact duplicate entry. No automatic retry of unconfirmed writes. |
| Listening history | Provider alternative | Opt-in session history: up to 100 songs after 30 seconds of actual playback; paused/loading time, seek jumps and long event-loop stalls do not count. Sign-out clears it. No fabricated Spotify history write. |
| Editable queue | Partial | Grove queue supports Next, Queue, up/down, remove and clear. Entries remain until starts are confirmed. Drag handles, drawer/overlay and manual/autoplay sections remain open. |
| Spotify Connect upcoming | Implemented, separate | Read-only provider queue shown when populated. Link additions enter Grove's editable queue. A Grove play request creates a new playback context; preserving a prior Connect context is not promised. |
| Play / pause / previous / next / seek / volume | Implemented | Native controls, availability and jam-guest guards, persisted volume. Real Spotify audio remains unverified in this audit. |
| Shuffle / repeat | Implemented | Spotify context controls plus selection Shuffle for loaded Grove rows. Partial option events preserve other options. |
| Autoplay | Partial alternative | Optional continuation cycles through loaded Home rotation. No dynamic refill, stream prefetch or recommendation sections yet. |
| Loading / retry / quality | Partial | Loading, play/pause, errors and reconnect feedback. Source codec/bitrate/sample-rate and Tide-style output recovery remain open; quality is not guessed. |
| Light / dark | Implemented | Settings; persisted independently from window size. |
| Normal / Compact / Mini / restore | Implemented | Settings layout and mini/restore; previous expanded size is retained. |
| Windows media controls | Implemented; hardware check open | Shared native session, metadata/timeline and guarded callbacks; Settings can disable it. Review modes register no system session. |
| Custom global shortcuts | Missing | Grove does not register configurable Ctrl/Alt combinations. |
| Output-device recovery | Missing | Grove-specific bounded resume after Bluetooth/USB/HDMI output loss still needs backend work and physical testing. |
| Local audio files | Implemented | Settings → Open audio file uses Tide's owned mpv backend without a Spotify login. Native play/pause, seek, volume, retry, mini and return use the same player. Spotify pause must be confirmed before switching; local files cannot enter jams. Requires the shared mpv runtime. |
| Jams on own devices | Nearby rooms implemented | Real TCP host/guest review, ownership, host control, drift correction, stale samples and confirmed consumption. Each account streams independently; two-account audio validation remains open. |
| Internet Jams | Missing | Current transport accepts loopback/private addresses. Tide's YouTube-specific relay cannot be presented as Spotify-compatible. |
| Tide ↔ Grove jams / matching | Experimental nearby implementation | Either app hosts; the other service searches and asks the listener to confirm a recording or paste its link, then follows host timing. Session-only choices; no shared audio or accounts. Audible cross-service validation remains open. See [cross-service jams](../CROSS_SERVICE_JAMS.md). |
| Installer / updates | Private candidate | Grove has its own per-user installer, portable package, checksums and rollback/uninstall checks. Manual updates only; Spotify verification holds the beta from release. See [beta status](BETA_STATUS.md). |
| Branding | Partial | Approved log/record SVG is embedded. The live X banner still predates the final logo and was not replaced during this audit. |

Home is the default screen. Account has one primary connection action; client IDs,
callback details and recovery controls are collapsed under Connection options.
Settings uses labelled switches. Save and Repeat in the player use accessible
icons with green active states. Shared jams automatically choose a private local
address when available; Network options exposes the manual override. Song names
are resolved when adding shared links, and stale responses cannot add to a room
the listener has left.

## Verification

The native review modes never restore Spotify credentials, contact Spotify,
launch consent, write preferences or register Windows media controls.

```powershell
& ./scripts/cargo-local.ps1 -CargoArgs @('test','--package','grove-music','--locked','--offline')
& ./scripts/cargo-local.ps1 -CargoArgs @('clippy','--package','grove-music','--all-targets','--locked','--offline','--','-D','warnings')
& ./scripts/cargo-local.ps1 -CargoArgs @('build','--package','grove-music','--locked','--offline')
./target/debug/grove-music.exe --review-features "$env:TEMP/grove-feature-parity-review"
./target/debug/grove-music.exe --review-ui "$env:TEMP/grove-jam-parity-review"
./target/debug/grove-music.exe --smoke-ui "$env:TEMP/grove-parity-smoke"
./target/debug/grove-music.exe --review-local-audio "$env:TEMP/grove-local-audio-review"
```

Feature review uses actual callbacks with synthetic responses: sign-in guard,
overlapping searches, pagination, song menus, reorder/remove, hide/reset,
confirmed starts, stale EOF, timeout preservation, collection entry shapes,
playlist validation, theme, compact layout, artist/back, history consent and
accounting, and sign-out cleanup. Jam review uses a real second loopback peer,
including server-enforced permissions and disconnects.

Local-audio review is an explicit separate native probe: it writes an owned
silent WAV to the chosen output folder, runs the real mpv backend muted and
checks duration/position, pause, seek, resume, volume, mini and return without
Spotify credentials or a file-picker dialog. The other review modes never start
an audio backend. The actual Windows file chooser still needs manual validation.

Live checks remain: fresh consent with added scopes; Spotify search/library and
playlist writes; native audio/seek; two-account jam audio; media keys in games;
and physical output-device switching. Browse up to 2,000 rows; Play/Shuffle use
up to 500 loaded songs; the editable Grove queue holds up to 500 tracks.

## Spotify references

- [February 2026 migration guide](https://developer.spotify.com/documentation/web-api/tutorials/february-2026-migration-guide): omitted profile fields, library endpoints, search limits and playlist restrictions.
- [Artist albums](https://developer.spotify.com/documentation/web-api/reference/get-an-artists-albums) and [album tracks](https://developer.spotify.com/documentation/web-api/reference/get-an-albums-tracks): the available artist-browsing route.
- [July 2026 changes](https://developer.spotify.com/documentation/web-api/references/changes/july-2026): account-level quota sharing; another client ID does not create independent quota.

Brand connection started with the shared X banner on `@YotamMe`. This establishes
the sibling identity, not cross-provider account sharing or track matching.
