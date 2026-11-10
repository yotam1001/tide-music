# Grove — native Spotify edition

The [private beta guide](BETA.md) covers installation and sign-in. The
[beta status](BETA_STATUS.md) records the Spotify blockers and release gates;
the installable candidate is not yet a verified Spotify release.

The current [Tide parity audit](TIDE_PARITY.md) lists implemented features,
Spotify-specific alternatives and remaining gaps. Full parity is not complete.
Experimental [Tide + Grove nearby jams](../CROSS_SERVICE_JAMS.md) now share host
timing across services after the listener confirms a matching recording.
Search, Home rotation, paginated library browsing, song menus, owned playlist
edits, a Grove queue, artwork, persistent appearance/volume preferences, optional
session history and Windows media controls now have reachable native controls.
Settings → Open audio file also plays local audio without a Spotify login using
the shared mpv runtime; Return to Spotify stops the local file. Files cannot be
played into shared jams. If Spotify is playing, Grove waits for pause confirmation
before starting the file.

The approved log/record mark is embedded from `assets/grove/logo.svg`; the
accepted presentation is `docs/brand/grove-options/b-log-disc-v2.png`.
Older PNG concepts are historical assets, not the active application mark.

Grove is a separate Rust/Slint sibling of Tide, with charcoal surfaces and fresh
green accents. It uses the same connection architecture as Spotifast: a Web API grant
for browsing and an independent streaming grant for in-process librespot playback.
Tide's existing YouTube player, account and settings remain independent.

## Run and sign in

```powershell
powershell -NoProfile -File scripts/start-grove.ps1
```

Open **Account → Connect Spotify**. The Spotify approval opens in Zen
when installed, with the system browser as fallback. Password entry stays in
the browser. Grove loads your verified account, playlists and Liked Songs.

Choose **Enable playback** for the separate streaming approval. Approve the
**same Spotify account**. Grove then becomes a native Spotify Connect device and
plays locally through Windows audio. Playback requires Spotify Premium. There
is no Soloist API key, external playback daemon, or browser runtime in the app.

Previously saved library access is restored on normal startup. **Enable playback**
tries a saved playback approval before opening the browser. **Connection options**
contains sign-in recovery and playback reset controls.
The playback engine currently needs manual reconnection after a dropped session.

## Current features

- Home, Search, Library, Now playing, Queue, Shared jams, Settings and Account,
  plus mini/expand. Search accepts music names and Spotify links.
- Play Spotify track, album, playlist and episode share links or URIs.
- Paginated library and Liked Songs, artist releases, artwork, song menus,
  Like/Unlike, local hiding and owned-playlist editing.
- Native play/pause, previous/next, seek and volume. Title, artist and progress
  follow librespot player events rather than simulated playback.
- Editable Grove queue with Play next, reorder, remove and clear; a separate
  read-only Spotify upcoming queue appears when populated.
- Light/dark and compact layouts, optional session history and media keys,
  local audio files, and continuation from loaded Home songs.
- Cancellable PKCE browser sign-in, independent library/playback approvals,
  account matching, protected credentials, and sign-out.
- Nearby shared rooms with invitations, participants, a host-controlled shared
  queue, and playback following on each listener's own device. See the
  [shared jams prototype](SHARED_JAMS.md) for setup, limits and verification gaps.

This remains an early implementation. The [parity audit](TIDE_PARITY.md) records
remaining work, including internet jams, output recovery and configurable global
shortcuts. The private installer candidate is available, but reliable Spotify
API access and audible playback still need live verification.

## Application identity and credentials

Library access defaults to the public application identity used by spotify-player
and ncspot, as Spotifast does. The consent screen may show that shared app's name.
Its rate limits are shared with other users; Grove respects Spotify's Retry-After
cooldown and offers manual retry. No borrowed client secret is used.

Optionally enter your own **Spotify app client ID** under Account → Connection options before signing
in, with `http://127.0.0.1:8989/login` registered as its redirect. Development-mode
accounts and endpoint coverage remain subject to Spotify's current limits. The
streaming approval uses librespot's standard playback client identity and the
separate `http://127.0.0.1:8898/login` callback. Neither flow requests a password
inside Grove.

Reusable approvals are stored as separate `GroveMusic` entries, `library-v1` and
`playback-v1`, in Windows Credential Manager. Access tokens stay in memory. The
library entry contains the refresh grant and verified account ID; the playback
entry contains librespot's reusable credential. The engine gets no plaintext
credential cache and no persistent audio cache in this version. Storage failures
are reported; working in-memory access is not falsely presented as saved.

Both callback listeners bind to IPv4 loopback **before** opening the browser.
The flow uses PKCE S256, random state, exact callback-path validation, bounded
request reads, a five-minute approval deadline, and cancellation. Wrong-state or
unrelated callbacks cannot consume the pending approval. Both account IDs must
match before playback credentials are saved. Cancelled or superseded tasks cannot
save late results; playback events carry a session generation so old events do
not replace a newer connection.

**Disconnect** stops playback, cancels pending work, clears visible account data and
deletes Grove's two protected entries. A storage deletion failure is reported as
incomplete sign-out and must be retried. Removing local credentials is separate
from revoking Spotify application access on Spotify's account page.
When the library is signed out, **Remove saved sign-in** remains available under
Account → Connection options so failed credential removal can be retried.

## Design and verification

Visual thesis: neutral charcoal surfaces, near-black navigation and player,
crisp white typography, and selective fresh-green accents. The lighter main
canvas separates content from chrome without tinting all surfaces green.

The screen follows the revised [Image Gen concept](concept-v2.png): navigation, listening
workspace, link entry, queue and persistent transport. The library and two-stage
Connection screen extend the same system for the requested Spotifast approach.
All interface text, icons and controls are native Slint, not a screenshot.

Tokens live in `GroveTheme` in `ui/grove.slint`: background `#141617`, rail and
player `#0b0d0e`, surface `#1d2022`, accent `#35df82`, text `#f5f7f8`, muted
`#a1a8ae`. Segoe UI uses 32/24/16/14/12 px typography. Navigation is left aligned
with a green selection marker; disabled actions use neutral gray. The record
empty state and sliders are native vector shapes. Sliders retain Slint's input,
keyboard and accessibility behavior under the custom treatment.
Motion is limited to control state changes, player updates and mini/expand.

```powershell
& ./scripts/cargo-local.ps1 -CargoArgs @('test','--package','grove-music','--locked')
& ./scripts/cargo-local.ps1 -CargoArgs @('clippy','--package','grove-music','--all-targets','--locked','--','-D','warnings')
powershell -NoProfile -File scripts/cargo-local.ps1 build --package grove-music --locked
./target/debug/grove-music.exe --smoke-ui .local/grove/screens
./target/debug/grove-music.exe --review-ui "$env:TEMP/grove-ui-review"
./target/debug/grove-music.exe --review-features "$env:TEMP/grove-feature-parity-review"
./target/debug/grove-music.exe --review-local-audio "$env:TEMP/grove-local-audio-review"
```

Smoke mode never reads saved credentials, opens a sign-in browser, or connects to
Spotify. It captures synthetic native states, narrow layout, mini and restore.
Native Slint snapshots replace browser QA because this executable has no web UI.
The optional `--review-ui` mode exercises native callbacks against a second
loopback room worker: validation, repeated activation, hosting, joining, queue
changes, guest permissions, leaving and host disconnect. It never restores or
changes Spotify credentials and exits after completing the checks. Its captures
contain temporary room invitations that expire when the review finishes.
`--review-features` runs 18 offline callback stages for browsing, queueing,
appearance, history and cleanup. `--review-local-audio` uses an owned silent WAV
and the real mpv backend at zero volume to exercise local transport, seeking,
mini/restore and switching, without Spotify access or preference writes.
See [verification notes](VERIFICATION.md) for the inspected dimensions, comparison
points, tests and remaining live-verification limits. The lockfile pins librespot
0.8.0 and its compatible vergen 9.0.6 build dependency.

## References

- [Spotifast's documented two-grant approach](https://spotifast.rocks/how-it-connects/)
- [librespot](https://github.com/librespot-org/librespot)
- [librespot's native Connect example](https://github.com/librespot-org/librespot/blob/v0.8.0/examples/play_connect.rs)
- [Spotify PKCE](https://developer.spotify.com/documentation/web-api/tutorials/code-pkce-flow)

Librespot is an unofficial client and can require updates as Spotify changes.
Grove uses the upstream library and does not vendor Spotifast's application code.
