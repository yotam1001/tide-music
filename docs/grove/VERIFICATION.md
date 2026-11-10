# Grove verification

## Current native UI and feature review — 2026-10-22

- Grove's 16 unit tests and the shared jam crate's 15 tests pass. Strict Clippy
  passes for all targets of these two packages. Tide's 40 unit tests, four native
  integration checks and strict Clippy passed in the preceding parity audit;
  they were not rerun for the latest Grove-only login changes.
- All 18 feature-review stages pass, including metadata-backed queue additions,
  rejecting song lookups from a previous jam, confirmed Unlike removal,
  stale requests, playlist validation, appearance, history controls and explicit
  cross-service recording choices, stale-song responses and room isolation.
- Tide's offline native pairing review passes for selected results, pasted
  YouTube links, invalid sources and room isolation. Desktop and narrow matching
  screens were inspected.
- All 16 shared-jam callback stages and eight muted local-audio stages pass.
- Native snapshots were inspected at desktop, compact and 700 × 540 sizes,
  including Account, jams, playlist editing and dark/light Settings.
- The final Grove build, workspace check and 19-view native smoke run pass,
  including the new signed-out Home at desktop and minimum window sizes.
- Save and Repeat use labelled icons with green active states. Home opens first;
  Account and Settings stay at the bottom of the sidebar. Duplicate queue
  sections are removed. Developer connection and network controls are collapsed.
  Settings uses keyboard-accessible green switches.

These checks use fixtures, loopback peers and a muted local file. Live consent
saved a library approval, but read-only API checks encountered repeated HTTP 429.
The test account has no Premium. Live API writes, audible Spotify playback, hardware media keys and
two-account and cross-service audible synchronization still need live verification. The
[parity audit](TIDE_PARITY.md) records remaining Tide features.

The [beta status](BETA_STATUS.md) records packaging checks and release holds.

## Historical first-iteration verification

For the current feature audit, added native review modes, provider differences
and remaining work, see [Tide parity](TIDE_PARITY.md). The sections below retain
the earlier iterations and their historical screenshots/test counts.

Checked on Windows on 2026-10-22. This is a native Slint app, so browser/Playwright
inspection does not apply. Screenshots come directly from the running window's
`take_snapshot()` API and were visually inspected with `view_image`.

## Automated checks

- Workspace Rust tests passed, including the existing Tide tests. Existing
  opt-in native media/process probes remained ignored by their test definitions.
- The original six Grove tests covered the connection and playback foundation:
  share-link validation; playback state/queue handling; callback method, path,
  state and duplicate-code rejection; a real loopback HTTP exchange that rejects
  a bad callback before accepting the correct one; and cancellation releasing
  the callback port for reuse.
- Grove Clippy passed for all targets with `-D warnings`.
- The final native build and eleven-view smoke run passed.
- The launcher parses successfully. New Rust/launcher sources passed the local
  credential/private-path scan. `git diff --check` passed.

## Visual comparison

Reference: [concept.png](concept.png), generated with the built-in Image Gen tool
using [concept-prompt.txt](concept-prompt.txt). The reference is 1536 × 1024.
The concept and latest [same-size native capture](screenshots/native-reference.png)
were both opened with `view_image` in the final visual pass.

| Point inspected | Reference / implementation evidence | Resolution |
| --- | --- | --- |
| Palette | Pine background, darker rail, mint accents, pale foreground and muted green secondary text | Exact implementation tokens retained; generated grain replaced with flat native surfaces |
| Layout | Slim navigation, open listening workspace, link entry, note tile, queue and persistent bottom player | Structure retained; fixed large-window vertical stretching and aligned the context label with Now playing |
| Typography | Segoe-style sans serif with strong title and section hierarchy | Explicit Segoe UI sizes throughout; native logical-pixel sizing remains constant as the window grows |
| Controls/icons | Leaf identity, transport marks, queue/link symbols, mint action | Fixed icon vertical centering; inactive navigation is flat; central play control is circular; native sliders and line edits remain toolkit widgets |
| Spacing | Roomy main column, lightly separated sections, restrained rounded corners | Checked at 1200 × 800 and 1536 × 1024; larger windows add whitespace without stretching individual text rows |
| Responsive layout | Desktop rail/player with an optional mini view | Inspected 760 × 640, minimum 700 × 540, and mini 600 × 104; Expand restores the previous 760 × 640 size |
| Content/states | Disconnected listening state and upcoming queue | Queue and library examples are explicitly labelled synthetic; real playback controls remain disabled without approval |

The reference's visual direction was faithfully checked against the native
implementation. It is an adaptation to native widgets, not a claim of
pixel-identical reproduction of the raster concept. Intentional differences are
flat surfaces, toolkit slider/input styling, scalable logical-pixel typography,
and the user-requested Library screen and two-stage account connection workflow.
The reference's decorative Spotify badges are omitted; the player identifies
Spotify Connect with text. No unresolved clipping or overlapping controls was
seen in the inspected views.

Above-the-fold copy check: Listen retains Grove, Your listening space, the
Spotify-link entry, Now playing, empty playback/queue wording, Connection and
Mini. The added Library navigation and revised Connection copy implement the
explicit request for Spotifast-style sign-in. No marketing labels or extra
product claims were added.

## Captured views

- [Listen](screenshots/listen.png), [Connection](screenshots/connection.png),
  [Queue](screenshots/queue.png): 1200 × 800.
- [Library](screenshots/library.png) and [Liked Songs](screenshots/liked-songs.png):
  1200 × 800, using synthetic metadata.
- [Compact](screenshots/compact.png) and [restored](screenshots/restored.png):
  760 × 640. The native mini/expand callbacks were exercised during the run.
- [Mini](screenshots/mini.png): 600 × 104.
- [Minimum Listen](screenshots/minimum.png) and
  [narrow Connection](screenshots/connection-narrow.png): 700 × 540. Longer
  content scrolls while transport remains visible.
- [Reference-size capture](screenshots/native-reference.png): 1536 × 1024.

## What still requires live verification

No real Spotify account was authorized during these checks. The tests do not
establish that Spotify will accept the public application's grant for this user,
that actual library responses match the offline assumptions, that Windows
Credential Manager accepts and restores this user's real approvals, or that
signed-in audio is audible and continuous. Those checks require the user's
browser approval and a Premium account for local playback.

The implemented flow to exercise next is: Connection → Sign in with Spotify →
approve library access → Library → open a collection → Connection → Enable
playback → approve the same account → play a track → pause/seek/skip → restart
and restore saved approval → sign out. Callback validation/cancellation and the
native layout/mini workflow were verified independently; the full signed-in
path remains unverified.

Temporary references and duplicate QA captures remain under ignored `.local`,
and the unused Soloist download remains under ignored `.tools`. Automatic
approval review rejected their deletion with "blocked by policy". Grove does
not execute or depend on that download; preserved screenshots are under this
document's `screenshots/` directory.

## Shared jams iteration

The shared jams work adds a nearby room transport, shared queue, and playback
following through each participant's own Spotify engine. See
[shared jams](SHARED_JAMS.md) for setup and the exact prototype limits.

- Workspace regression tests passed. Grove now has 17 passing tests, covering
  host ordering, duplicate-add handling, guest ownership, queue/member bounds,
  invitation validation, oversized-message rejection, actual TCP exchanges with
  two guests, invalid tokens, host disconnect, drift/pause correction, stale
  updates, bounded load attempts, and a valid first playback request ID of zero.
- Grove Clippy passed with all targets and `-D warnings`.
- Native smoke coverage grew to 15 views. Added
  [room setup](screenshots/jam-setup.png), [host](screenshots/jam-host.png),
  [guest following](screenshots/jam-guest.png), and
  [minimum-size guest](screenshots/jam-narrow.png) captures. All room metadata in
  these captures is synthetic; no real room invitation or Spotify session is used.
- Visual inspection found the additional navigation crowded the bottom player at
  700 × 540. Narrow sidebar spacing and header height were reduced; the repeated
  capture confirms the full transport remains visible while room content scrolls.
- Source scan found no embedded account tokens or private keys; `git diff --check`
  passed. No persistent external service or firewall rule was configured.

The TCP tests establish message delivery and role behavior on loopback. They do
not establish LAN firewall reachability or audible sync between separate Spotify
Premium accounts. Those live checks remain outstanding. Internet relay, official
Spotify Jam interoperability and automatic shared-queue advancement are not
implemented.
