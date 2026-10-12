# Prototype validation

Validated on Windows 11 Pro 10.0.26200, Ryzen 5 7600X, approximately 15.1 GiB usable RAM and Radeon RX 9060 XT graphics. Release builds use Rust 1.99.0 for x86_64-pc-windows-gnu. Measurements below are one-machine observations, not product guarantees.

## Functional evidence

- Workspace queue, source validation, bounded IPC, process containment and Windows transport tests pass.
- Muted local WAV and public YouTube playback passed load, pause, seek, rapid replacement, stale-generation rejection, stop, EOF and shutdown smoke checks.
- Repeated explicit Pause commands remain paused.
- Windows media-session registration was tested with a real owned window: metadata/state readback, timeline updates and handler cleanup pass.
- Native full, compact and collapsed layouts were rendered and inspected, including long titles and empty queues. Illustrations are vector placeholders.
- The pinned runtime setup script completed successfully. Runtime binaries and toolchains are excluded from Git.
- The local portable executable rendered all three empty layouts, then passed a separate live YouTube IPC readback check: duration 634.601 seconds, position advancing beyond one second, volume zero and pause false. Closing it removed the owned helpers.

Physical keyboard/headset routing while a game has focus, audio-device removal, extended playback/soak tests, display scaling at 150%/200%, and game frame-time comparisons remain unverified. Real album artwork and system tray support are not implemented.

## Account integration validation

The account branch adds encrypted session import, read-only playlists/Liked Songs/Home requests and a YouTube-scoped playback cookie lease. Deterministic tests cover bounded metadata, safe track URLs, rejection of unsupported actions, header filtering, DPAPI roundtrip/tamper rejection, encrypted storage, normal lease removal and owner-aware stale lease cleanup. Local playback remains available before account verification. Library UI screenshots use synthetic metadata and do not authenticate.

Actual public music playback was tested muted with the official NCS upload of Cartoon/Jéja's “On & On”: duration 207.981 seconds, seek/pause/replacement/stop/EOF checks passed, including process cleanup. This verifies music playback through the native engine, not Premium playback.

No real account session has been imported in development or CI. Personal playlists, recommendations and Premium ad-free behavior require the user's local connection and remain unverified. Account verification confirms access to the library endpoint, not subscription entitlement. Provider setup installs a transient Python helper; it is not resident during idle/playback. The earlier memory measurements below predate this integration and must not be read as measurements of live account refresh.

A 15-sample empty compact-mode spot check of the account build used one app process. Samples 6–15 had median private memory of 4.87 MiB, total Working Set of 22.98 MiB and 0% CPU. Benchmark mode disables account loading, so this checks interface regression only, not authenticated playback or refresh memory.

The muted public-music smoke also passed with an empty fixture cookie jar whose path contains spaces and a comma, exercising mpv option escaping through actual YouTube extraction. This is not an authenticated or Premium test.

Connect account reads the user-selected existing Zen cookie database through read-only SQLite and validates account access before import. Tests cover cookie scope, expiry/context filtering, input bounds and profile confinement alongside library behavior. Live user account access remains unverified until the user completes the connection.

## Initial renderer experiment

The first GPU-rendered release failed the initial idle private-memory target. Twenty samples per layout, with a one-second delay between collections, an empty queue and no playback helper, yielded settled median private memory of approximately 255 MiB full, 213 MiB compact and 210 MiB collapsed after excluding the first five samples. This motivated selecting and measuring the software renderer. Keeping all layouts in the executable does not instantiate all of them simultaneously.

## Software-rendered idle measurements

Twenty samples per empty-queue layout, excluding the first five samples:

| Layout | Median private MiB | Median total working-set MiB | Median CPU, one core |
| --- | ---: | ---: | ---: |
| Full | 7.44 | 24.78 | 6% |
| Compact | 4.86 | 22.37 | 0% |
| Collapsed | 3.73 | 20.99 | 0% |

These are short visible-window tests with an empty queue. Full-view CPU had a 32% p95 and requires further focus/redraw investigation; compact was 0% at p95, collapsed 4%. They do not establish minimized playback cost or long-session stability. Builds were active elsewhere on the machine during some measurements. No audio helper was running in these idle samples.

A separate 15-sample full-view repeat after builds completed, with no deliberate input, yielded 7.45 MiB median private memory and 0% median/p95 CPU across samples 6–15. The earlier CPU burst was not reproduced; focus, pointer activity and window redraws should be controlled in longer comparisons.

## Muted YouTube playback measurements

A 30-sample run in compact mode played the public Big Buck Bunny video as audio with volume forced to zero. Across samples 6–30, process-tree median private memory was 47.36 MiB, p95 47.97 MiB and peak 48.05 MiB; median total Working Set was 66.54 MiB. Startup included a sampled private-memory peak of 95.96 MiB. Settled samples included the app and mpv. Median CPU was 2% of one logical core, p95 8%. Extractor CPU accounting is approximate as described below. No mpv, yt-dlp or Deno processes remained after the app closed.

This short test does not establish 10-minute playback, minimized playback, a 60-minute soak or gaming frame-time impact. Search and playback helpers are contained in owned kill-on-close Windows jobs. Search starts suspended until containment is attached, and descendant termination precedes joining inherited output-pipe readers, including on normal launcher exit.

## Reproduce

Build with `cargo build --release -p tide-music --locked`, install playback runtimes using `scripts/setup-runtime.ps1`, and run:

```powershell
pwsh -File scripts/measure-process.ps1 -Executable target/release/tide-music.exe -Seconds 30 -AppArguments '--benchmark-mode','light'
python scripts/summarize-resources.py .local/resource-samples.json
cargo run -p tide-playback --example smoke -- path/to/seekable-audio.wav
```

Benchmark mode uses transient settings and does not change the saved queue or preferences. The measurement script starts its own process and closes it after sampling. It sums Private Bytes and total Working Set for that process tree, including active helpers. Total Working Set includes shared pages and is not Working Set - Private. CPU percentage uses one logical core as denominator. Very short-lived child processes between samples may be missed, and exited-process CPU totals are approximate. Samples include startup; settled comparisons should discard the first five samples. GPU memory is not included.

Live-service smoke tests are separate from deterministic CI because provider availability and extraction behavior can change.

Account switching validates the selected signed-in Google account before encrypted replacement. Tests cover unavailable slots, input bounds, dropping previous brand-channel selection, and cancellation preserving the previous encrypted session. Home parsing skips unsupported mixed content. Windows selected-profile tests cover canonical short-path aliases.

A local authenticated check verified the requested second Google account, its library, liked songs and Home response. A public YouTube audio extraction with selected-account headers and webpage/config skips succeeded and the extractor detected a Premium subscription. Credentials, private account identities and collection contents are not part of this repository.


## Navigation, queue and artwork revision

The revision passes 40 deterministic Rust tests (two separate manual/live tests remain ignored), 29 Python helper tests and strict Clippy. Eight native screenshot states cover Home, Library, Queue, light/dark, compact, mini and restore; theme switching asserts that the window dimensions stay unchanged, and volume changing asserts the displayed value updates immediately. A second populated fixture checks long titles and queue alignment. The release PE contains multi-size ICON/GROUP_ICON resources.

Short process-tree measurements of the new release, without account access:

| Scenario | Settled private MiB | Settled total working set MiB |
| --- | ---: | ---: |
| Full, empty queue | 7.66 | 28.33 |
| Compact, empty queue | 6.14 | 25.13 |
| Compact, muted public YouTube playback | 49.92 | 69.80 |

Idle samples lasted 12 seconds, excluding the first four samples. Playback lasted 20 seconds; the settled median uses the final eight samples and includes mpv. The highest observed startup/extraction working set was 136.29 MiB. These measurements exclude album artwork and signed-in library contents and do not prove long-session behavior or gaming frame-time impact. Decoded artwork cache pixels are capped at about 1.27 MiB, with extra temporary fetch/decode buffers.

A live song-radio read returned 25 usable recommendation tracks with thumbnail URLs. The opt-in muted `--smoke-autoplay` test then found that the saved session no longer passed account verification, and timed out before starting playback. Direct account/library reads confirmed the session needed renewal. End-to-end authenticated recommendation playback is therefore pending a fresh local connection; the radio response, queue operations and native playback checks are separate evidence, not a completed autoplay handoff test. No session values, account names or private library contents are included in these results.


## Simplified shell validation

The Home/Library revision passes 45 deterministic Rust tests (two separate manual/live tests stay ignored), strict Clippy, and the existing 29 Python helper tests. New tests cover manual queue insertion before autoplay, reordering without moving the current song or crossing origin groups, removing upcoming suggestions when autoplay is disabled, and loading older saved track metadata with the new origin field defaulting to manual.

The native UI smoke accepts an optional `--fixture-audio path/to/silent.wav` argument. With a silent local file it exercises real playback commands and asserts that search/playlist play preserves the browsing screen, choosing a search song starts a fresh queue, manual choices take priority, autoplay-off removes suggestions, and playlist shuffle/reordering/removal work. The test uses transient settings and never loads a saved account. Eleven screenshot states cover full/light Home, compact queue overlay, compact Library, playlist details, search, wide queue, Account settings, Library, mini and restore. Rendered review caught and corrected unsupported menu glyphs, misleading unavailable album navigation and unknown duration placeholders.

These checks establish local playback/control and layout behavior. They do not substitute for the still-pending authenticated live autoplay handoff or a long listening session.

A 12-second full-window idle repeat for the simplified release, excluding the first four samples, measured 7.52 MiB median private memory and 27.85 MiB total working set. No account or audio helper was active; this is a short idle measurement.

## Next-song preparation

The playback worker prepares one YouTube stream at a time, with bounded output,
timeout, cancellation and a short memory-only lifetime. Deterministic tests cover
request deduplication, matching consumption, expiry, Stop/Drop invalidation,
safe CDN/header validation, exclusive cookie-copy cleanup and one-time fallback
without restarting superseded playback. Both extraction paths use the same best
audio policy. The app requests preparation during the final minute of unpaused
playback and follows the current queue order, including repeat wrapping.

Two opt-in muted probes used public YouTube clips without an account session:

| Run | Cold replacement | Prepared replacement | Preparation in background | Drop cleanup |
| --- | ---: | ---: | ---: | ---: |
| 1 | 2.896 s | 0.304 s | 2.136 s | 0.093 s |
| 2 | 3.273 s | 0.302 s | 2.303 s | 0.094 s |

Replacement timings include fresh mpv startup and first decoded audio. These are
two local observations, not guaranteed latency or gapless playback. The second
probe's debug process family sampled a 127.6 MiB peak across 41 samples; all
tracked processes were gone after exit. This includes the native player and
transient extraction helpers, excludes signed-in sessions/artwork, and is not a
settled playback memory benchmark. Signed-in next-song preparation still needs
a separate live-service check; numeric account-header routing is covered by a
synthetic metadata test.

## Queue dragging

The revision passes 54 deterministic Rust tests and strict Clippy; two manual
tests remain ignored. Native pointer checks cover a short reorder and a longer
edge drag that scrolls the original row out of view. Both preserve the current
song and playback generation. Core tests reject invalid moves and moves across
manual/autoplay groups. Twelve native screenshot states include the insertion
marker, compact queue and restored player.

With the queue open and 2,000 synthetic tracks, an eight-second release sample
measured 9.48 MiB private memory and 28.94 MiB working set. Rows remain virtualized.
No account, artwork or audio helper was active; this is an idle queue measurement,
not the total cost of signed-in music playback.

## Media keys and visual refinement

Windows media-control integration was exercised on a real interactive desktop:
the system broker sent Pause, Play, Next and Previous to a hidden, unfocused
test window. Each action arrived exactly once and in order; the test left other
media sessions untouched. All six Windows tests, including lifecycle and broker
tests, passed with ignored interactive tests enabled. This validates Windows
broker delivery, not the keyboard firmware's Fn layer or exclusive fullscreen
game behavior. The interactive tests remain opt-in in CI.

The audio helper explicitly disables its own media-key input, leaving Tide as
the handler. Media Next/Previous use the same callbacks as the visible controls,
including Next's recommendation handling at queue end. A cached media-state
timer publishes the latest playback snapshot every 100 ms rather than a UI
snapshot from the previous one-second tick. The native silent-audio smoke also
checks media Next/Previous advance exactly once and preserve browsing.

The visual pass retains layout dimensions and navigation. Native screenshots
cover dark/light Home, Library, playlist details, search, both queue layouts,
insertion feedback, settings, mini and restore. Real pointer drag checks remain
part of that smoke test.

## Early skipping

The earlier final-minute preparation gate has been removed. A 250 ms scheduler
requests the next stream once current playback has loaded, including while
paused. Only one stream address is retained, for at most ten minutes and never
past thirty seconds before its signed expiry. Both playback paths retain the
same best-available-audio selection.

The opt-in muted app probe uses two public clips and invokes the actual Next
callback well before the first clip ends. A release run without an account
prepared the next stream 6.458 seconds after the initial load request and reached
decoded audio 0.545 seconds after Next. A debug run using the existing encrypted
session verified the account, prepared the next stream after 12.201 seconds and
reached decoded audio 0.328 seconds after Next. No private library data or
credentials are included in these measurements.

The final release, with retry handling and diagnostics removed, repeated the
signed-in check: preparation was ready after 14.106 seconds and Next reached
decoded audio in 0.355 seconds.

One preceding authenticated probe timed out before readiness; a subsequent
diagnostic probe accepted the stream and passed. Failed preparation now has a
twenty-second retry backoff. A fake-resolver test verifies failure, deduplication
during backoff, successful retry and subsequent cache reuse; another test covers
stale cancellation. Temporary diagnostics were removed. These are individual local observations, not a
latency guarantee. Skipping before preparation finishes, or selecting a different
song, can still require a network lookup. This check does not validate the
separate authenticated recommendation handoff described above.

The revision passes 59 deterministic Rust tests; three helper/interactive tests
remain opt-in. Formatting and strict Clippy also pass.

## Reliability and reconnect follow-up

The subsequent revision passes 67 deterministic Rust tests and 33 Python helper
tests, plus workspace formatting and strict Clippy. A native playback fault probe
passed process loss, truncated HTTP recovery, retained pause/seek intent, bounded
retry exhaustion, replacement and Stop cancellation. A focused native UI harness
passed 11 slider, accessibility, overlay, minimum-size and scaled-render checks.
Actual monitor DPI migration and the final hardware media-key regression remain
to be checked.

Reconnect now preserves the selected Google account instead of always importing
account index zero. Anonymous account-menu responses, temporary service errors,
unreadable profile databases and container-only sign-in have distinct safe
outcomes. Tests use synthetic cookies and account responses; they do not verify
a successful real reconnect. A read-only health probe currently reports rejected
authentication for the saved session. The sustained signed-in listening test
could not start, so repeated natural transitions and recommendation refills are
still an outstanding release gate.

A release build subsequently passed a muted five-minute local queue run with
15 natural transitions, acknowledged pause/resume, skip and seek. The completion
poll runs every 250 ms; volume persistence stays capped at once per second.
Process-tree samples excluding startup measured a median working set of 79.0 MiB
and peak 81.8 MiB, with median private bytes of 51.2 MiB and peak 53.5 MiB. The
first and last 30 settled working-set medians were 80.1 and 74.7 MiB respectively;
there was no increasing memory trend in this short run. All observed helpers
ended after successful app exit. CPU readings were zero throughout and are not
accepted as useful CPU evidence. This test used three queue entries referencing
one short local silence file, without accounts, thumbnails or extraction; it
does not prove long-session YouTube stability or gaming impact.

## Portable preview validation

An extracted preview was installed outside the source checkout using its included
runtime and provider setup scripts. The fresh Python environment installed the
pinned helper dependency graph, passed imports and reported no broken dependency
requirements. A muted five-minute local run passed 15 natural transitions,
pause/resume, skip and seek, and all observed helper processes ended afterward.
Median process-tree working set was 58.8 MiB, peak 89.8 MiB; median private bytes
were 51.4 MiB, peak 52.6 MiB. This remains a local-audio test.

The distributable build remaps compiler paths. The final ZIP passed exact payload
allowlist, SHA-256 and credential/private-path checks. Its scanner also rejected
synthetic API credentials and Google cookies injected into checksum-correct
copies of the executable and helper. Binary scanning treats compiled cookie-name
literals and SVG property names as static text rather than credential values.
The About Slint attribution was restored in Account settings and verified visible
after scrolling at simulated 200% scale.

A subsequent 25-second run of the remapped executable outside the checkout
confirmed startup and local playback. The resource script now preserves fractional
CPU time instead of choosing PowerShell's integer Math.Max overload. This short
sample reported mean CPU use of 1.59% of one logical core including startup,
with a 20.62% peak. These figures do not measure authenticated playback or gaming.

## Session continuity follow-up

The helper distinguishes a content HTTP 401/403 from an identity rejection by
checking the same authenticated account before requesting reconnect. Stale radio
errors and failed connection candidates cannot invalidate a newer verified
account. Successful requests persist validated, scoped cookie updates in DPAPI
storage and refresh the playback lease; reconnect verification also retains its
cookie updates. Python regression checks pass 37 tests.

Native extraction receives an exclusive private cookie snapshot for each load
and recovery. A synthetic regression verifies that a late extractor save cannot
overwrite the newer canonical playback lease, and that subsequent snapshots use
the newer lease. Cookie snapshots are capped at 64 KiB and removed after their
owned helpers end. Extractor cookie rotations are still not returned to encrypted
storage, so these checks do not prove a minimum session lifetime.

A fresh manual session import verified the intended profile and library with Zen
closed. An initial live listening run reached one natural transition and three
recommendation refills, but its supervising command disappeared without a final
result or resource report. It is not accepted as a sustained-run pass. The saved
account passed another library check afterward. A new run must use explicit
completion reporting and the executable containing the cookie-isolation change.
