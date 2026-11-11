# Tide runtime state map

This inventory covers the checked-in desktop player, playback worker, provider,
browser companion, native login helper, installer and Jam relay. It describes
control state, rather than treating every preference as a separate machine.
Several machines run concurrently. A paused player may be checking for updates,
loading a playlist and hosting a Jam at the same time.

The source files in the tables own the behavior. State names below describe
observable phases unless an enum is explicitly named. This document is an
inventory and change guide, rather than a second runtime implementation.

## Boundaries that every change must preserve

- Playback generation identifies a selected play, even when the same song repeats.
  Account identity identifies the selected Music profile and session revision.
  Async results must belong to the current generation or account before adoption.
- A preview, search, refresh or failed request does not grant permission to replace
  playback. Starting a mix, choosing a track, and joining/leaving a Jam are explicit
  ownership transitions.
- A terminal result releases its busy state. Canceling also invalidates late
  callbacks. A loading flag must not outlive the operation it describes.
- Transient errors preserve verified credentials and usable results. A rejected
  session is a separate state from a network timeout.
- Update artifacts remain verified across a failed launch. Canceling a restart
  must allow retry without downloading again.
- Guest membership grants queue requests and synchronized listening. Host control,
  account cookies and local file access remain separate authorities.
- User preferences survive application replacement. Source code, build caches
  and credentials are outside the installer payload.

## Player and browsing

| Machine | States and transitions | Owner and identity |
| --- | --- | --- |
| Queue and transport | Restored/paused, load requested, playing, paused, seeking, ended, stopped, failed; selection increments generation | `crates/app/src/main.rs`, `crates/core/src/lib.rs`; queue index and playback generation |
| mpv worker | No process, launching, IPC connected, load awaiting start, active file, disconnected, shutting down | `crates/playback/src/lib.rs`; session, generation, supersede epoch and entry ID |
| Playback recovery | Loading, progressing, intentional seek, stalled, retry scheduled, retries exhausted; stable progress resets the retry allowance | `crates/playback/src/recovery.rs`; original source and generation |
| Next-track preparation | Empty, requested, extracting, ready, expired, retry delayed, superseded, closed | `crates/playback/src/prefetch.rs`; source, freshness, cancellation token and cookie revision |
| Output recovery | No pending recovery, output returned, waiting for backend, resume once, expired/superseded | `crates/app/src/output_recovery.rs`; playback generation |
| Autoplay refill | `Idle → Fetching → Waiting → Fetching`, or `Filled` / `Exhausted`; at most three attempts. Refills ask for the account's personalized Up next, seeded by the last song the listener chose and excluding queued, played and session-autoplayed songs; explicit Start radio stays song radio | `crates/app/src/radio.rs`, `request_radio` in `main.rs`; explicit `Phase` and generation |
| Library request | Idle, fetching a typed action, response, retryable failure, rejected session, canceled | `LibraryAction` and library worker in `main.rs`; request, playback seed and account |
| Crossfade | `Idle → Preloading → Mixing → Idle`; cancels on pause, seek, skip, queue change, error, Jam or a short song; the outgoing song ending completes a mix | `crates/app/src/crossfade.rs`; standby engine generation and equal-power gains |
| Radio preview | Opening, fetching, ready, empty/failed, closed; explicit Play starts a queue | `LibraryAction::Preview` in `main.rs`; radio seed and detail view |
| Home recommendations | Initial load, usable results, refreshing, refreshed, failed with previous results retained. Each shelf has a slot budget; refresh keeps its leading third and cycles the rest through least-recently-shown cards; familiar shelves (Listen again) sit last | `discovery.rs`, `browse_layout.rs` and `main.rs`; provider order and account |
| Library sections | Playlists/albums/artists/history/explore, category selected, fetching, ready/empty/failed | `library_browse.rs`; section, category and account |
| Search | Idle, query/category selected, fetching, ready/empty, retryable failure, canceled/superseded | `search_results.rs` and search completion handling in `main.rs`; search generation |
| Search suggestions | Debouncing, fetching, ready, dismissed, canceled/superseded | `search_results.rs`; query, selected account and cancellation flag |
| Artist navigation | Resolving credited identities, choosing an artist, fetching, displaying, opening a release, returning | `artists.rs`; canonical artist ID and saved origin |
| Navigation and panels | Home/library/search/detail plus account/settings/lyrics/Jams/queue overlays; back and forward restore saved context | `ui/main.slint` and callbacks in `main.rs`; active view and saved origin |
| Queue editing | Pointer idle, pressed, dragging, hovering a target, releasing, canceled; filtering and playback invalidate a drag | `QueueItem` / `QueueDrawer` in `ui/main.slint`; source index and queue model |
| Feedback and playlist writes | Idle, account-bound mutation pending, confirmed, failed, superseded; picker loading/open/closed | `music_feedback.rs`; account identity, video and detail revision |
| Listening history credit | Unobserved, tracking decoded progress, qualified, submitting, credited; reset on consent/account/play changes | `history_feedback.rs`, `music_feedback.rs`; account and playback generation |
| Lyrics | Hidden, requesting, plain/synced/word timed, unavailable, retry deferred, superseded; follow/manual scroll are independent | `lyrics.rs`, `lyrics_effects.rs`, `ui/main.slint`; song, generation and account |
| Artwork | Missing, queued, fetching, decoded, rejected, evicted, invalidated | `artwork.rs`, `artwork_view.rs`; public URL, size and cache generation |
| System artwork | No published cover, preparing a bounded cover, published, superseded, cleaned up | `system_artwork.rs`; current artwork identity |

## Accounts and integrations

| Machine | States and transitions | Owner and identity |
| --- | --- | --- |
| Account adoption | Disconnected, saved session detected, candidate staged, verified, retry delayed, rejected, explicit retry | `connection.rs` and adoption callbacks in `main.rs`; encrypted session revision and profile |
| Cookie synchronization | Leased snapshot, playback rotation observed, merge pending, persisted, conflict/retry, session replaced | `cookie_updates.rs`, `provider/src/lib.rs`, `main.rs`; lease, account and cookie revisions |
| Native sign-in | Idle, owned helper running, user verification, saved/canceled/rejected/timeout, helper cleanup | `native_login.rs`, `crates/windows/src/login.rs`; owned process and temporary profile |
| Browser capture | Idle, armed, captured, sending, imported/error/expired/canceled | `browser-companion/background.js`; selected tab, request ID and capture generation |
| Browser native messaging | Starting host, framed request, bounded response, success/error/disconnected | browser companion and `tide-session-host`; approved extension origin and request |
| Discord presence | Disabled/invalid ID, starting, connected, retrying, retiring, stopped | `discord.rs`; application ID and latest activity revision |
| Shortcut recording | Idle, recording a chosen action, modifiers only, valid chord, conflict, commit/cancel | `shortcut_capture.rs`, `crates/windows/src/shortcuts.rs`; selected binding |
| Window lifecycle | Visible, minimized, hidden to tray, mini/pinned, restoring, quitting | `main.rs`, `crates/windows/src/mini.rs`, `taskbar.rs`; native window lifetime |
| Settings persistence | Primary valid, backup recovery, migration, temporary write, validated replacement, failure preserves previous file | `settings_store.rs`, `settings_location.rs`; version-independent user data |

## Updates, setup and Jams

| Machine | States and transitions | Owner and identity |
| --- | --- | --- |
| Updater operation | `Idle`, `Checking`, `Downloading`, `Preparing`; completion/cancel/failure returns operation ownership to Idle | `updater.rs::Task`; exactly one channel, transfer or handoff |
| Update artifact | Unknown, newer release available, partial download, verified/cached installer, invalidated | `updater.rs`, `publisher.rs`; signed version, digest, platform and validity window |
| Restart handoff | Process started, preflight pending, installer readiness confirmed, player exit allowed, canceled/failed/timed out | `updater/handoff.rs`, `installer/TideSetup.cs`; exact parent process and private channel |
| Installation transaction | Payload verified, ownership checked, runtimes prepared, files staged, registration committed, old release cleaned, rollback/failure | `installer/TideSetup.cs`, `scripts/install-common.ps1`; product and release ownership |
| Jam participation | Inactive, starting host/guest, connected, reconnecting, terminal, leaving, retiring worker | `jam.rs`, `jam_session.rs`; role, owned session and restored local queue |
| Jam synchronization | Waiting for timeline, loading track, correcting pause/position, following, stale/failed | `experiments/jam/src/sync.rs`; play ID, revision, local generation and timeline age |
| Jam room admission | Created/open, locked, full, invite rotated, member removed, idle/expired/closed | `experiments/jam/src/relay.rs`; separate host, invite and membership secrets |
| Jam requests | Guest enqueue/skip requested, bounded pending request, host acknowledged/rejected, duplicate/stale/rate limited | relay protocol and `jam.rs`; member, request ID and current play ID |
| Short Jam code | Allocated, join attempted, accepted/locked/full/limited, rotated, expired with room | relay, client and `jam_session.rs`; guest admission only |
| Guest title lookup | Unresolved, one bounded request, resolved/unavailable, session abandoned | `jam.rs`; validated video source and current guest session |

## Additional nested lifecycles

The workflows above contain smaller machines. These matter when changing resource
ownership or retry behavior, even though they do not need separate UI screens.

| Machine | States and transitions | Owner |
| --- | --- | --- |
| Helper child process | Spawned, running, exited, canceled, process tree reaped | `crates/playback/src/owned_process.rs`; native-login and provider callers |
| Request cancellation | Issued, cancel requested, worker unwinds, late completion discarded | Worker-specific cancellation flags and generation checks |
| Cache failure entry | Absent, fetch attempted, failure retained, retry/eviction eligible | `crates/app/src/artwork.rs`; bounded thumbnail cache |
| Relay rate allowance | Available, consumed, limited, time window elapsed | `experiments/jam/src/relay.rs`; global and peer-specific counters |
| Signed update authorization | Unverified, verified for exact bytes, expired, invalidated | `crates/app/src/updater/publisher.rs`; signature and expiry |
| Installation ownership | Absent, owned/valid, stale owned record, foreign/conflicting | `scripts/install-common.ps1`; registry and release records |
| Browser helper registration | Missing, prepared, registered for exact origin, incompatible, unregistered | `scripts/setup-browser-companion.ps1`; current user and extension origin |

These are nested resource and authorization lifecycles, rather than seven extra
top-level application modes.

## Refactoring decisions

The updater operation now uses a single enum in place of three independent
optional operation owners. It cannot own a check, download and restart handoff
simultaneously. The verified installer is deliberately separate, because a ready
installer may survive a check or a failed restart.

Autoplay now names its phases instead of combining `complete`, `filled`,
attempt counts and optional deadlines. A queue edit may reopen a successful
refill. It must not reset the failure budget or bypass backoff.

Keep the other owners separate until their transitions need changing. Playback
intent is independent of backend observation. Jam role is independent of
connectivity. Combining these into one global enum would multiply states and
make unrelated workflows block one another.

## Working on a transition

1. Find the owning row above and inspect its producer, completion and cancellation.
2. Identify the account/generation/view token that makes an async result current.
3. Preserve late-result rejection and terminal-state cleanup.
4. Update existing transition tests for the behavior being changed. Add a focused
   regression when a missing transition caused the bug.
5. Check the normal UI entry point as well as the callback. Disabled widgets
   alone are not an authorization boundary.
6. Keep diagnostic phase names free of account cookies, invite codes and signed
   stream URLs. Prefer a named phase and generation over dumping full state.

Navigation/panel flags and the Jam role/connectivity lifecycle remain the largest
implicit clusters. Future work should consolidate their entry/exit transitions
before converting their data representation, so Back, restored playback and guest
permissions stay stable.
