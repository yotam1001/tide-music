# Lightweight native music app: implementation plan

Prepared 10 October 2026; status updated 13 October. Native playback, account
library, recommendations and artwork are implemented; release validation is in
progress. The design choices below retain the original investigation history.

## Implemented first milestone

The workspace now uses Slint for the native interface, an isolated mpv process for audio, yt-dlp/Deno for YouTube extraction and search, and Windows SMTC for media controls. Local and public YouTube playback passed muted integration tests. All three layouts share one queue and playback engine. Settings use a bounded JSON queue rather than SQLite at this stage. No YTerMusic source was copied.

Read-only account/library synchronization, automatic recommendation refill and
bounded thumbnail fetching are now implemented. Full, compact and mini layouts
share playback; theme and size controls are independent. Fifty production
Mini/Expand cycles retained the native window and player during local playback.
Remaining validation includes session longevity, actual audio-device changes,
monitor DPI migration and gaming impact. Tray behavior remains roadmap work.
See `RELEASE_READINESS.md` and `VALIDATION.md` for current evidence; the original
investigation below is not a claim that every proposed feature is complete.

## Goal and scope

### Deferred roadmap: synchronized listening rooms

Plan a Jam-style feature for friends listening together while gaming, after
playback reliability and convenient browser login are established. An isolated
[feasibility lab](../experiments/jam/README.md) now exercises synchronized native
playback, bounded room capabilities and an opt-in private HTTPS relay transport.
It is not included in Tide or its installer. Remote use requires an operator's
own configured TLS proxy and creation key; no hosted Tide service is provided.
The opt-in native developer build now includes host/join/leave and guest control
ownership; normal builds do not link its transport. Multi-device/live-service and
resource validation remain outstanding; no production release date is committed.

Synchronized playback on each participant's device is a required part of the
feature. A shared queue with audio playing only on the host does not meet its
purpose and will not count as the completed feature.

The initial design should use private invite-only rooms, a shared queue and
host-controlled pause, skip and ordering. Each participant streams independently
with their own account/session. A coordination service exchanges track IDs,
queue state and timing; it must not receive account credentials or relay audio.
Joining late, preloading, buffering, clock drift and reconnects require explicit
recovery behavior and multi-device tests. Resource use must stay bounded and be
measured alongside normal playback. Public rooms, chat and mobile clients are
outside the proposed first version.

Build a Windows-first native Rust desktop music player that leaves resources available for games. The primary experience is reliable music playback with media keys while the main window is minimized. Search, queue management, personal playlists and a compact player follow. Windows x64 is the initial target; other operating systems are later work.

This plan is for a personal prototype first. YouTube playback and account integration are unofficial dependencies, so they must pass a feasibility milestone before the project promises them as reliable features. Local files and permitted test streams let us validate the native player independently.

## Decisions

| Component | Proposed choice | Decision gate |
| --- | --- | --- |
| Application | Rust, stable toolchain where supported | Pin versions at implementation start |
| Interface | Slint, native window with event-driven updates | Check idle rendering, accessibility and GPU use on Windows |
| Audio | Evaluate YTerMusic's rodio player first | Verify codecs, buffering, cancellation, seeking and streaming |
| Alternative audio | libmpv behind the same playback interface | Use only if rodio misses essential behavior |
| YouTube Music catalog | Evaluate YTerMusic's ytpapi2 crate | Verify current read-only endpoints and auth behavior |
| Alternative integration | Isolated provider adapter; assess yt-dlp if needed | Count any helper runtime in total resource usage |
| Persistence | SQLite for metadata/history; small settings file | No stored expiring playback URLs as durable library entries |
| Windows integration | Media transport controls, tray, optional registered hotkeys | No game injection or elevated permissions |

Prefer a single process and reusable Rust modules, but do not choose them solely for aesthetics: an unreliable decoder costs more than a measured helper process. Keep UI, catalog and playback replaceable.

## YTerMusic evaluation

The supplied Reddit post links to [ccgauche/ytermusic](https://github.com/ccgauche/ytermusic). Its workspace separates `player`, `ytpapi2`, `database`, `download-manager`, `common-structs` and the terminal frontend. The [player manifest](https://github.com/ccgauche/ytermusic/blob/master/crates/player/Cargo.toml) uses rodio; the [API manifest](https://github.com/ccgauche/ytermusic/blob/master/crates/ytpapi2/Cargo.toml) uses reqwest, serde and tokio. These are promising candidates for reuse behind our own interfaces, rather than rebuilding everything or retaining the terminal UI.

GitHub API inspection on this date found the repository unarchived, latest commit `0663348365ebf5a5ebb354ecc6544b142c5c478b` dated 13 May 2026, titled `fix: reduce idle CPU and thread usage`. This establishes recent changes, not verified current playback compatibility. Recheck and pin an exact revision before reuse.

The README reports about 20 MB RAM for its terminal app. Treat that as an unverified upstream claim. The [player source](https://github.com/ccgauche/ytermusic/blob/master/crates/player/src/player.rs) opens a filesystem path and feeds a decoder to rodio; it is cached-file playback, not a network streaming implementation. The default [download manager](https://github.com/ccgauche/ytermusic/blob/master/crates/download-manager/src/task.rs) invokes yt-dlp. Consequently it can avoid libmpv but does not eliminate extractor sidecars. Its API module handles catalog requests, not playable stream resolution. Start without automatic offline music downloads.

An optional all-Rust extractor exists, but [PR 139](https://github.com/ccgauche/ytermusic/pull/139) documents replacing it as the default after 403 failures. Do not select it simply to claim an all-Rust dependency tree. Evaluate a bounded streaming adapter separately; do not equate this repository's local-file decoder with a complete streaming engine.

The repository [license](https://github.com/ccgauche/ytermusic/blob/master/LICENCE) is Apache-2.0. Preserve required attribution, license and modification notices for reused code, and inventory dependency licenses separately. Slint also has its own [licensing options](https://slint.dev/get-started), including an attribution-bearing royalty-free route for desktop applications.

## Playback feasibility milestone

1. Inspect pinned YTerMusic source: provider requests, authentication, format selection, audio decoder, buffering, file cache and thread lifecycle.
2. Build a minimal Windows test harness. Establish local-file and permitted-stream playback first.
3. Test current public YouTube Music metadata and playback behavior where available, with errors recorded and no unsupported access-control workarounds.
4. Exercise play/pause, seek, next/previous, rapid cancellation, track completion, repeated restarts, broken URLs and temporary network loss.
5. Measure startup, sustained playback, track-change peaks and idle after stop. Determine whether the engine requires full-file caching or can support bounded streaming.
6. Record what works, what requires account access, and what fails. Keep catalog access separate from playback access.

Exit: a documented supported prototype path, working controls, bounded buffers and a resource baseline. If pure Rust playback fails, assess libmpv through the same contract. A catalog-only success is not a playback success. If neither route is dependable, report the specific failure before expanding the UI.

YouTube's [developer policies](https://developers.google.com/youtube/terms/developer-policies) restrict audio separation and background playback through API services. An unofficial client is not an official integration, and an open-source code license does not authorize access to the service. This is a material product constraint, not a claim that the source cannot be studied or a blanket legal conclusion. Public distribution needs a separate assessment. Do not design around ad suppression, DRM bypass or access-control evasion.

yt-dlp is a fallback candidate, not a committed dependency: its [current documentation](https://github.com/yt-dlp/yt-dlp) describes external JavaScript runtime requirements for full YouTube support. Any extractor, runtime and player helper must be included in benchmarks and packaging.

## Architecture and contracts

Use a small Cargo workspace:

```text
crates/app/           startup, composition, settings
crates/core/          track models, queue, commands, state machine
crates/provider/      search, library, playlists, stream resolution
crates/playback/      decoder/engine, buffering, audio events
crates/storage/       metadata/history/cache persistence
crates/windows/       media controls, tray, registered hotkeys
ui/                  Slint views and design tokens
docs/concepts/        generated visual references
tools/bench/          repeatable resource collection
```

Define contracts before parallel implementation. Core owns the active queue and selection. Provider returns stable track identifiers and metadata; resolving a track returns a short-lived playback source. Playback receives commands and emits events: loading, playing, paused, buffering, ended and error, plus position and duration. Each request has a generation ID so a cancelled older request cannot replace the current track.

One owner serializes playback commands. Network and decoder work never block the UI thread. Use bounded channels, cancel abandoned searches and resolutions, debounce search, paginate lists, and bound retries. Stop shuts down the current source and clears buffers; pause retains only what the defined resume behavior requires.

## Features and order

**First usable build:** play a selected supported track, progress/seek/volume, a queue, next/previous, shuffle/repeat, clear playback errors, settings persistence and keyboard navigation.

**Gaming build:** minimize to tray, Windows media keys and now-playing integration, compact player, optional configurable hotkeys, clean exit and audio-device change handling. Always-on-top is optional and user-controlled. No automatic startup registration.

**Library build:** read-only search, playlist browsing and pagination once current provider behavior is verified. Account connection is optional, explicit and local. No automatic playlist edits or likes in the first version.

**Later:** account write operations, recommendations, gapless playback and additional platforms only after the baseline is stable. Offline music downloads, equalizers, visualizers and game overlays are outside the initial scope.

## Visual direction

The user selected `concepts/concept-05-burgundy-library.png` as the leading full-mode direction: charcoal/burgundy, album grid, song list and playing-details panel. Earlier concepts remain references. The companion lightweight candidate is `concepts/concept-07-lightweight-palette.png`, using the user's earthy artist palette in a new compact player design, rather than the large warm/serif library shown in concept 06. Generated concepts are static references, not working screens or proof of playback/account support.

Full and lightweight mode share one playback engine, queue, provider and settings. Instantiate only the active primary view. On entering lightweight mode, release the full library view, its large artwork references and unused textures; do not just hide the window while retaining all resources. Keep a compact text-first queue, one small current-track cover and transport controls. Collapsing this player removes the visible queue. Provide a reversible Full view/expand control without interrupting playback.

Palette changes are inexpensive theme values. Decoded images, GPU surfaces, renderer caches and extra windows are the meaningful view costs. Keep decoded artwork sized to its actual display size and set a smaller artwork cache budget in lightweight mode (initial target 2 MB versus up to 16 MB in full mode). Purging app references may not immediately reduce all allocator/driver memory, so verify mode-switch savings by measurement. Test at least 50 switches for retained textures, duplicate windows and interrupted audio. Do not load another rendering backend or audio engine for the second mode.

Use static artwork, restrained borders, readable text, keyboard focus indicators, accessible labels and scalable layout for Windows display scaling. Render native/vector controls rather than using mockup screenshots as the UI. Avoid decorative animation; update progress at most once per second when visible, and stop cosmetic updates when hidden.

## Performance budget and validation

These are initial engineering targets, not measured promises:

| Scenario | Target aggregate private memory |
| --- | --- |
| UI settled and paused | Under 100 MB |
| Normal playback, hidden | Under 150 MB |
| Startup/track-change transient peak | Under 250 MB |

Aim for average CPU below 0.2% of one logical core when paused/hidden and below 2% of one logical core during ordinary audio playback. If a target fails, report the measured result and tune the responsible component; do not silently revise the budget. The 20 MB upstream terminal claim is not our acceptance threshold.

Allocate initially: 16 MB compressed playback buffer, 16 MB decoded artwork memory, 50 MB artwork disk cache, paginated metadata and one optional bounded next-track preload. Decoder/GPU overhead still counts toward the totals. Avoid whole-library artwork decoding and unbounded history in memory.

Use release builds on a recorded Windows machine. Record CPU, RAM, GPU, Windows version, display scaling and power mode. Collect 1-second samples of process-tree Private Bytes, Working Set - Private and CPU time. Also inspect GPU activity; compare dedicated/shared GPU use without treating it as directly interchangeable with private bytes. Include children and any terminal process needed for an upstream reference run.

Run settled idle, 10-minute playback, paused/hidden, 30 track changes and a 60-minute soak. Compare median, p95 and peaks to browser YouTube Music on the same machine and tracks. Repeat a fixed game benchmark at least three times with and without playback; assess FPS and frame-time variation against baseline noise. A lower app RAM number alone does not prove smoother gaming.

## Agent ownership and milestones

Two research agents have already completed independent playback/integration and native architecture investigations. The playback researcher is also assessing the supplied YTerMusic reference. They have not implemented the application.

For implementation, use at most two workers plus the parent integrator, and only parallelize after shared contracts exist:

| Stage | Owner | Deliverable and acceptance |
| --- | --- | --- |
| 0: source/playback spike | Playback worker; parent verifies | Windows harness, pinned source/license inventory, actual playback and failure report |
| 1: contracts and skeleton | Parent | Compiling workspace, command/events, mock provider, UI-only benchmark |
| 2A: playback/provider | Playback worker | Engine adapter, cancellation, resolution and read-only provider behavior |
| 2B: UI | UI worker | Chosen main/compact layouts driven by mock state, input and scaling checks |
| 3: integration/storage | Parent | Real queue/UI wiring, bounded persistence, meaningful behavioral tests |
| 4: Windows controls | Worker when independent | Media keys, tray, device changes, shutdown; no duplicate handler ownership |
| 5: performance/release | Parent with bounded benchmark task | Profile report, portable Windows build and dependency notices |

Workers own separate directories and return changed files, contract impact, checks and remaining issues. They do not alter each other's modules, install global dependencies, publish externally or use account secrets. Parent owns integration, scope, shared dependency choices and verification. Prefer one worker when the next task is tightly coupled.

## Acceptance checks

- Queue transitions are correct after next/previous, shuffle/repeat and track completion.
- Rapid skips/searches cannot play or display stale results.
- Seek and pause work without blocking the UI; stop/exit leave no orphan helpers.
- Network loss, unavailable tracks and expired authentication produce actionable errors with bounded retries.
- Audio-device removal/reconnection behaves predictably.
- Media keys work with the game focused, without consuming unrelated game keys.
- Both layouts work at 100%, 150% and 200% display scaling and with keyboard navigation.
- Artwork and stream memory stay bounded; long sessions and repeated track changes do not cause persistent growth.
- Full/lightweight/collapsed switches preserve track, progress, queue and volume, instantiate only the active primary view, and avoid memory growth over 50 switches. Record actual memory and GPU differences rather than assuming compact size guarantees savings.
- A fresh-machine portable launch includes needed dependencies and third-party notices.

Run targeted unit tests for queue/cancellation/state transitions, integration checks against controlled permitted media, and manual Windows media/device tests. Keep live service smoke tests separate from deterministic tests.

## Privacy and delivery

Store optional account session credentials in Windows-protected storage, separate from metadata. Never paste credentials into chat, commit them, log them or scan the user's browser automatically. Provide disconnect/delete-credentials and redact URLs, headers and tokens in diagnostics. Use bounded logs and no analytics by default.

Deliver a portable local executable, setup/readme, known integration limitations, reproducible measurements and license notices. Public source publication is authorized. Installer and automatic updates remain later decisions.
