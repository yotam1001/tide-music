# Prototype validation

## Final candidate and real Windows lifecycle (2026-10-15)

Candidate `f02691501a72aed0f07164999b36437ac1c4b567` passed all jobs in
[push CI](https://github.com/yotam1001/tide-music/actions/runs/38751976892) and
[PR CI](https://github.com/yotam1001/tide-music/actions/runs/38751986879).
The real lifecycle job used disposable hosted Windows runners, actual per-user
HKCU uninstall/native-host keys and Start-menu shortcuts, pinned real runtime
downloads, installed local-audio UI smoke checks, update and registered-command
removal. Both downloaded evidence artifacts report success. The PR artifact uses
GitHub's synthetic merge revision; the draft downloads use the push artifacts
for the exact candidate commit.

The downloaded setup passed embedded extraction/cleanup and its embedded ZIP
hash matches the separate candidate ZIP. ZIP payload, source/checksum/privacy
checks and both companion archives pass. Text source comparison normalizes
checkout line endings; icons and archive/extracted file bytes match exactly.
The setup SHA-256 is
`2896a85043fc5ce19e2a1352ff26344cb94451d4ada7da828e3c639b1ce37a04`;
the preview ZIP SHA-256 is
`29a293eadde97ffb62f41cfa3c5c34eaf4776a3d5b63241c61ab1c446100220a`.
Local script validation passed 43 tests with three capability skips. Index and
fresh-public-clone privacy audits passed without findings.

Real CMD regressions first reproduced a waiting launcher's directory lock and
failure after its child deleted the executing batch file. Both launchers now
leave the owned working directory and parse their complete result-handling tail
before setup/removal can delete them. Disposable self-deleting fixtures verify
success, exact failure exit codes and paths containing spaces and `!`. Launchers
close their own consoles; callers in an existing CMD shell use `cmd /c`.

An intermediate `a941445` PR check failed when its harmless PowerShell setup
fixture exceeded the test's 15-second startup deadline. That failure remains
recorded; both final runs passed without rerunning or relabeling the failed job.

This evidence does not establish interactive installer clicks, high-DPI setup,
fresh authenticated browser import through the graphical route, physical Fn keys
with game focus, or weekly session longevity. Chrome used a synthetic extension
ID; no account credentials were used by CI. The beta remains an unpublished draft
until the remaining checks and publication review.

## Fresh-runner failures and onboarding hardening (2026-10-15)

The first clean hosted Windows run for `01406d9` did not pass: its Windows
PowerShell child inherited the PowerShell 7 module path and failed at
`Get-FileHash` before installation. The failure artifact was downloaded and
inspected. The graphical setup child, advanced CMD wrappers and hosted probe
now select the Windows PowerShell system modules without changing the user's
persistent environment. The C# regression launches a harmless real script with
an intentionally incompatible inherited module path and verifies hash commands,
path quoting and exact Chrome arguments; the CMD regression also verifies
argument transfer and a nonzero child exit code survives the wrapper.

A separate temporary-directory probe reproduced inability to remove the current
working directory. The real uninstall entry now leaves both the process and
PowerShell working directories before removal, and the uninstall shortcut uses
a stable Windows folder. Entry regressions pass in PowerShell 7 and Windows
PowerShell 5.1. Hosted validation now invokes the actual registered uninstall
command from the owned install directory instead of invoking only the package
script. A new passing hosted result is required; the original failed run remains
failed evidence, not a release pass.

The primary account action now opens browser connection guidance; the legacy Zen
picker is under Advanced. Companion packages include the existing Tide PNG icon
resources at 16/32/48/128 with manifest/dimension validation, a prominent
session-cookie disclosure and privacy link. Privacy and store-preparation docs
describe actual processing and human publication steps. These changes do not
establish Chrome Web Store availability or a fresh authenticated browser import.

## Graphical Chrome connection setup (2026-10-15)

New setup source offers an unchecked-by-default Chrome companion option and
exact extension-ID field. C# tests cover opt-in control behavior, invalid IDs,
argument/path quoting, passing the normalized ID into the existing owned setup
core, and control bounds. A setup containing the previously validated matching
preview ZIP compiled and passed its actual embedded-payload verification. That
local bridge build is not a new player build or a distributable release asset.

The application Help URL now points to maintained main-branch account guidance.
Docs lead with companion connection instead of manual headers. Actual browser
handshake/import through the new graphical route and rendered high-DPI screens
remain unverified; do not infer them from argument/control tests. Preview capture
was obscured by another full-screen app; no user game/browser inputs were sent.

A separate clean hosted-Windows CI job is being added for real per-user registry,
shortcuts, private runtimes, native app, update and removal. Its guard refuses to
run on this development machine. Passing that job will be fresh Windows system
integration evidence, not evidence of GUI clicks, authentication or game-focused
physical keys.

## Actual Windows CI installer candidate (2026-10-15)

Downloaded the successful main-branch `1e403b7` CI artifacts. The actual
`TideSetup.exe --verify-payload` passed embedded extraction/checksum checks, and
the matching preview ZIP passed its payload/checksum/privacy gate.

The opt-in `verify-installed-preview.ps1` probe passed real pinned downloads,
installation, update to a fresh release, and removal. Both installed revisions
passed mpv/yt-dlp/Deno version probes, managed Python imports, and native UI
smoke assertions with silent local audio: browsing preservation, queue actions,
media Next/Previous routing, pointer drag, and long-drag/autoscroll. Update removed
the previous owned release; uninstall removed all owned installed files, test
shortcuts, and the fixture uninstall entry.

Isolation matters: the test used disposable files and actual `.lnk` files outside
the real Start menu, isolated application-data paths, and in-memory uninstall
registry I/O. It did not change the current user's installed Tide or account.
It did not test the setup GUI click path or Windows Settings on clean Windows.
Windows Sandbox is unavailable on the test machine. Authenticated streaming,
game-focused physical keys, and days-long session behavior were not retested by
this local-audio probe. See [beta gates](BETA_RELEASE.md).

## Restore repaint regression (2026-10-13)

Restoring the native window now invalidates the full scene through an identical
background layer. This handles Windows discarding software-rendered contents
while the reused buffer and window dimensions remain unchanged. There is no
continuous repaint, resize, color change or extra image buffer in this fix.

`cargo test -p tide-music --test restore_repaint --release --locked` passes:
the actual AppWindow repaints a deliberately cleared reused buffer in eight
restore cycles across dark and light themes, with exact pixel comparison and
unchanged dimensions. Formatting, strict app all-target Clippy and the release
build pass. The rebuilt GNU application also remained fully drawn through two
native title-bar minimize/restore cycles without resizing. This is focused
restore coverage, not a claim that every display-driver issue is resolved.

## Current build and sustained playback (2026-10-13)

Revision b019b14 passes both Windows CI jobs and a fresh public-clone privacy
audit. Its portable ZIP passes the payload allowlist, checksums and privacy scan.
A fresh extraction outside the checkout, runtime/helper setup and five-minute
muted local test passed fifteen natural transitions, pause/resume, skip and seek.
The seventeen decoded starts had a longest sampled delay of 501 ms. The harness
recorded exit code zero and confirmed observed helpers ended. Settled process-tree
working set was median 81.82 MiB, p95 81.96, peak 83.05; private bytes median
47.04 MiB, p95 47.16, peak 48.32. This excludes YouTube network extraction.

The preceding 8c86152 one-hour application reported success with fourteen natural
transitions, sixteen decoded starts, seventeen persisted native session updates,
recommendation refills and controls. Longest request delay, including cold start,
was 4006 ms. Settled working set was median 70.79 MiB, p95 77.74, peak 404.59;
private bytes median 53.30 MiB, p95 61.28, peak 345.28. A separate post-run
library check passed without another session import. Its supervisor nevertheless
recorded failure after application completion without preserving the reason;
that result is not rewritten as an overall pass, and final exit/cleanup for that
run is unproven. The measurement harness now retains the process handle and
records exit code and helper cleanup independently. The independent six-hour signed-in test on b019b14 completed its playback
assertions; the application reported PASS, while its supervisor failed after
completion. Separate retained-handle observations confirmed normal exit and the
end of observed helpers. See the six-hour observation below for that distinction.
Weekly session lifetime remains unproven.

The focused UI harness also passes its eleven control/focus/accessibility checks
at the new 650 by 420 minimum. Captured minimum and simulated 200% renders keep
the transport visible; scrolling reaches the lower Account settings. These are
synthetic checks, not physical display or gaming-performance verification.

## Native window sizing (2026-10-13)

An owned, muted production fixture reproduced a native DPI resize that grew the
window beyond a 1032-pixel monitor work area: 1119 pixels tall at 150% and 1479
at 200%, hiding the transport. Tide now fits its outer frame to the current
monitor work area after startup, mode, DPI or work-area changes. The responsive
minimum is 650 by 420 logical pixels (650 by 112 in Mini), and expanding restores
the previous size without forcing the former 660-pixel minimum height.
Minimized/maximized windows defer fitting until restored; fitting does not
activate or raise the window, or continuously override manual resizing.

The fixed fixture passed native WM_DPICHANGED messages at 150%, 200% and 100%,
including maximize, DPI change, restore and Mini/Expand at 200%. Outer height
remained within 1032 pixels; captured full-window renders showed the transport.
The same window and mpv process retained decoded progress, and normal close took
107 ms with the owned helper removed. This uses synthetic native messages on
one monitor, not physical monitor migration. Eighteen app tests and strict app
Clippy pass. A separate fifty-pair production Mini/Expand regression also passes.

## Production media controls and closing (2026-10-13)

File selection now uses rfd's asynchronous Windows picker and Slint's local
executor. Synchronous COM dialogs previously blocked Slint's queue timers and
media callbacks: revision 8c86152 reproduced an EOF handoff stall while an audio
picker stayed open. The fixed production fixture advanced to a second six-second
local track with the picker open, accepted Windows Pause/Resume, blocked duplicate
pickers, and passed cancel/reopen. Closing with the reopened picker still present
took 122 ms; the process and owned player ended. No private files were selected.
Only weak UI/state references cross the await, and account Cancel, Disconnect or
Switch invalidates a delayed account selection before import. A shared RAII guard
allows one picker and resets on completion, cancellation or spawn failure.
Eighteen app tests, strict app Clippy and formatting pass for this change; live
signed-in tests below predate this UI fix until a new revision is explicitly named.

Windows media registration now runs after the UI event loop starts, with bounded
retries while the native window becomes available. The previous startup attempt
could silently skip registration before Winit created that window.
An isolated, muted production app accepted Windows broker Play, Pause, Resume,
Next and Previous commands and advanced decoded local playback. Normal closing
during decoded playback and extractor loading took 119 ms and 112 ms respectively;
both app processes exited successfully and their owned helpers ended.
These checks used synthetic local settings, not the user's running window.
Physical Fn keys with a game focused remain unverified. All eighteen app tests,
strict app Clippy and formatting checks pass.

An additional isolated production fixture passed fifty Mini/Expand pairs during
muted local playback. Every resize retained the same native window, the mpv
process remained unchanged, and decoded position advanced beyond fourteen
seconds. App private memory was 7.797 MiB before and 7.805 MiB after the cycles;
this short two-point observation is not a general leak test. Normal closing
afterward took 118 ms and removed the owned helper. The test sent pointer
messages only to its owned synthetic window, without moving the user's cursor.

An isolated production fixture also reinitialized its own WASAPI output ten
times between automatic selection and an available explicit device. Decoded
progress continued in the same player; pausing through Tide's own button stayed
paused after reinitialization and resumed through that button. This does not
simulate hardware removal or prove device migration. Direct mpv pause changes
are not equivalent to accepted Tide pause intent. Position can rebase when an
output is reinitialized; the pause check requires stable position after settling.

The 8c86152 portable ZIP passed payload, checksum and privacy scans. A fresh
runtime and optional-helper installation outside the checkout then passed five
minutes of muted local playback: fifteen natural transitions, controls and
seventeen decoded starts, with longest sampled start delay 501 ms. Median
process-tree working set was 81.74 MiB, peak 82.98; median private bytes 47.04 MiB,
peak 48.32. This local test excludes authenticated extraction costs.

Opt-in listening verification accepts 300-21,600 seconds so an overnight run can
exercise continuous playback without hourly restarts. No regular app behavior
or test pass criteria change with that duration bound.

## Latest authenticated continuity check (2026-10-13)

Revision 5809a9c passed another muted 1,800-second signed-in run using the same
private-window session, without another import: seven natural transitions,
nine recommendation refills, pause/resume, skip and seek. Ten native session
updates were persisted. Preserving prepared audio through EOF reduced sampled
natural handoff delays to 250-750 ms; the initial cold load took 4.259 seconds.
After five startup samples, median working set was 69.77 MiB (p95 76.77,
peak 420.05); median private bytes were 52.23 MiB (p95 58.81, peak 360.29).
CPU median was 0% of one core, p95 4.424%. These are process-tree samples.

The subsequent app build also passed the targeted queue-depletion probe:
removing every upcoming row through the real callback, without restarting the
current song, allowed recommendations to refill and decoded playback to advance
at EOF. That test seeks to EOF; it is recovery evidence, not natural listening
duration. A one-hour follow-up on revision 8c86152 is pending.

Earlier authenticated check:

Revision 5e49e9b passed a muted 1,800-second signed-in listening run with
eight natural song transitions, ten recommendation refills, pause/resume,
skip and seek. Sixteen native cookie updates were persisted. A separate account
library check after the run passed without importing another session.
The session was imported from a private Zen login; regular Zen was allowed to
remain open. This does not establish weekly session longevity.

Natural transitions still took approximately 3.75-4.75 seconds to decoded audio;
the initial load took 5.266 seconds and the explicit skip took 0.749 seconds.
Timing uses 250 ms sampling. A subsequent source review found that the preparation
timer discards the ready next stream at EOF before the queue timer consumes it.
Preserving preparation through EOF is under validation; this run predates that fix.

After excluding five startup samples, process-tree median working set was
79.90 MiB (p95 86.76, peak 407.33), and median private bytes were 54.16 MiB
(p95 60.90, peak 341.25). Extraction causes transient peaks. Median CPU was
1.386% of one logical core, p95 5.837%. These are muted test observations,
not gaming frame-time measurements.

The same revision's extracted portable package passed five-minute local continuity
outside the checkout. Both Windows CI jobs and fresh public-clone privacy audits
passed. The historical sections below describe earlier builds and failures.

Subsequent source regressions cover queue depletion after a successful radio
refill: removing upcoming rows reopens that generation for recommendations while
exhausted failures retain their retry limit. Temporary Home, Library and playlist
errors now retry the saved content request instead of routing to account settings;
account verification and generation-scoped radio retries retain their own paths.
All eighteen app tests and strict app Clippy pass. The optional muted
`--smoke-autoplay-depletion` probe removes upcoming rows through the real app
callback, seeks to EOF and requires decoded playback in the next generation.
Its live result passed; this targeted seek test does not substitute for
natural listening coverage.

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
owned helpers end. The subsequent revision returns extractor cookie changes
through a bounded, zeroized queue. Scoped deltas merge against the original
account and cookie snapshot; a conflicting authentication cohort is rejected.
API saves also compare their original snapshot, and a Windows mutex serializes
protected session writes. Missing cookie rows do not erase stored credentials.
The combined revision passes 77 deterministic Rust tests and strict Clippy.
These checks do not prove a minimum session lifetime.

A fresh manual session import verified the intended profile and library with Zen
closed. An initial live listening run reached one natural transition and three
recommendation refills, but its supervising command disappeared without a final
result or resource report. It is not accepted as a sustained-run pass. The saved
account passed another library check afterward. The explicitly supervised
cookie-isolation run then failed after approximately five minutes, one natural
transition and three recommendation refills. A separate health probe confirmed
rejected authentication afterward. Cookie isolation alone was insufficient.

That failed run collected 288 process-tree samples: settled median working set
68.1 MiB, peak 354.7 MiB; settled median private bytes 50.6 MiB, peak 293.6 MiB.
Mean sampled CPU use was 3.83% of one logical core, including startup and retries.
These preliminary failed-run figures are not stable-session or gaming evidence.
The revision that persists native updates received a fresh verified session.
The user reported closing Zen before the test. Its detached 30-minute muted
authenticated listening test failed at approximately 12 minutes, after three
natural transitions and five recommendation refills. Five native session updates
were saved before the failure. A separate library health probe then confirmed
rejected authentication. A process inspection afterward found one Zen window
and 16 Zen processes, 14 started before the test. The user subsequently confirmed
closing only YouTube Music, with Zen still open. This does not prove browser
activity caused the failure; an isolated-session retest remains necessary.

The next revision fixes two code-backed cookie-handling defects. Direct profile
import and helper response updates exclude host-only `youtube.com` cookies from
the Music session and report conflicting scoped authentication values instead of
guessing. Native Netscape updates honor the include-subdomains flag as well.
API persistence now compares the authentication cohort and individual cookie
baselines, allowing a valid sign-in refresh after a concurrent non-authentication
update or cookie reordering. Newer conflicting authentication cohorts still
reject stale responses; newer non-authentication cookies and current request
metadata are preserved. Provider checks pass 14 tests and helper checks pass 42.
These defects are reproducible in synthetic tests; neither is yet proven to be
the cause of the live failure.

The listening probe now records request-to-decoded-progress delays with 250 ms
sampling and counts native updates not applied. That count includes stale or
invalid snapshots and already-stored changes, so it alone does not prove a lost
rotation. Real track metadata, signed URLs and credential values remain absent
from these diagnostics.

The timing probe passed a five-minute muted local run with 15 natural transitions,
17 decoded audio starts, acknowledged pause/resume, skip and seek. Its longest
sampled request-to-decoded-progress delay was 501 ms, with 250 ms sampling.
Observed helpers ended after successful exit. This validates the diagnostic
callback path and local handoffs; it does not measure YouTube extraction delays.

The failed run collected 683 process-tree samples: settled median working set
69.8 MiB, peak 347.9 MiB; settled median private bytes 53.1 MiB, peak 289.0 MiB.
Mean sampled CPU was 2.99% of one logical core, including startup and retries.
These remain preliminary failed-run figures, not stable-session gaming evidence.

The current revision also passed the focused native UI harness's 11 checks and
the actual Windows media broker integration checks. Play, pause, next and previous
reached an unfocused test window exactly once; dropping the media session disabled
it and removed callbacks. These checks use the application's own test window and
do not prove that a particular keyboard's physical Fn shortcuts work.

The populated application UI smoke run passed native pointer queue reordering
and long edge-autoscroll through recycled rows, preserving the playing track and
playback generation. Its light/dark home, queue and settings snapshots were
reviewed. Three production-mode app windows using isolated, empty account/settings
storage closed normally with exit code zero in 8-10 ms. This covers the normal
Slint and media-session startup/close path without a connected account; it does
not yet cover closing during an authenticated request or active playback.

Revision aa49e66 passed both Windows CI jobs and a privacy audit of a fresh public
clone. Its cookie-persistence portable ZIP passed the payload allowlist, checksum
and credential/private-path scans. Authenticated playback longevity remains a
release gate regardless of these passing checks.

# GitHub-built Windows preview follow-up

The e32f41f GitHub MSVC preview passed ZIP payload, checksum and privacy checks.
Binary import inspection found no Visual C++ runtime redistributable dependency.
Its fresh portable installation passed native media transport, DPI/mode fitting,
and playback/control/close checks with a file picker open.

The first five-minute local listening run reported 15 natural transitions and
exit code zero, but its supervisor rejected helper cleanup without recording
which process remained. That failure remains unresolved evidence. A diagnostic
replay with explicit process identity and live-state checks passed 15 natural
transitions, exit code zero, and no observed remaining helpers. The revised
measurement also rejected a deliberately surviving test child. This replay does
not establish the cause of the original failure or replace live service testing.

The subsequent shutdown change removes the playback handle from shared UI state
and joins its worker before returning from the application, even when callbacks
or an async picker retain that state. Eighteen app tests, formatting and strict
app Clippy passed. The rebuilt debug app passed automatic EOF and media controls
with a picker open, duplicate-picker suppression, cancellation/reopening, and
normal close with the picker still open; the owned app and helpers ended.
This shutdown-only change is not included in the already-running six-hour binary.

# Browser companion artifact validation

Windows CI run 38354710202 passed checks and preview packaging for branch revision
328c649, using pull-request merge revision b581e5b. Its actual downloaded preview
ZIP included the optional native receiver, setup script and companion guide and
passed the exact payload, checksum and privacy checks. ZIP SHA-256:
`e0c7e31dd62e4f03c9173a50812cba30baa5ab7cb89e29f97dea8be9df521ac4`.

Both downloaded Firefox and Chromium packages contained exactly their six
extension files. Their ZIP contents matched the loadable folders, and every
published payload/archive checksum passed. The CI-built receiver executed with
isolated fixture application-data storage and a synthetic verification helper:
native framing, encrypted save, rejection preserving the baseline and port
cancellation before save passed. No actual Google session, browser installation,
registry registration or profile access was involved.

Later source revisions serialize Disconnect with session writes and roll back
incomplete native-host registrations. Provider concurrency tests and eight
temporary-file/in-memory-registry installer checks pass locally; those changes
are not included in the b581e5b artifact above. Browser signing, real imports,
account identity selection and application adoption of an externally imported
session remain release requirements. The six-hour b019b14 playback test also
does not cover the companion or these later source changes.


### Six-hour playback observation (2026-10-13)

The b019b14 release binary reported PASS after 21,600 seconds of decoded audio
monitoring, 102 natural transitions and 104 decoded starts, including radio
refills, pause/resume, skip and seek. It recorded 119 persisted playback session
updates; the longest sampled request delay was 5,263 ms. This validates that
specific playback run, not weekly session longevity or later login changes.

The enclosing resource supervisor reported RuntimeException in its listening
phase after the application PASS. Its initial exit record contains exit code 0
but no completed cleanup report. Preserve this as an overall supervisor failure;
the exact exception was not retained. An independent observer, attached after
the run began, separately retained process handles and confirmed root exit code
0 and all 243 helpers it observed ended. That observer does not cover helpers
that ended before it attached. No clean overall supervisor PASS is claimed.


A separate post-run library check, built from 5677f50, subsequently returned
PASS using the saved session without another header import. This supports session
continuity beyond the six-hour playback run; it does not establish weekly
longevity. The original supervisor failure remains recorded.

The resource sampler now retains handles for observed process instances instead
of reopening PIDs during cleanup. Temporary-process regressions check both a
successful parent/child shutdown and a genuinely surviving helper, which must
fail. Argument paths with spaces are covered. Sampling races are counted, and
failure stage/type/line are recorded without copying process arguments or account
data. Helpers whose entire lifetime occurs between samples remain unobserved;
cleanup evidence applies only to retained instances.


### Packaged login adoption (2026-10-13)

GitHub run 38391311608 completed its checks and preview packaging successfully.
The downloaded artifact represents PR merge be8ad870 (branch source 5677f50).
Windows ZIP SHA-256:
`4a5d078aec090959fdc77d5420b5a9c220032d5605ea3ddd8b99faeff87b14a9`.
The actual ZIP passed its payload/checksum/privacy verifier. Its extracted
MSVC executables passed the isolated native connection regression, covering
same-index audio continuity, superseded identity changes, retained pause/seek
intent and disconnect. Both companion artifacts passed checksum and ZIP/folder
parity checks. Existing local playback tools were supplied for this test; it
does not prove a fresh-machine runtime installation.

Authorized real native-host registration exposed missing shared Mozilla registry
parents, which the installer now creates without rewriting existing parents.
The registration succeeded after that correction. Mocked registry regressions
exercise missing parents and rollback. The temporary extension has since been
loaded in Zen; authenticated browser import remains unverified.


The six-hour b019b14 process-tree samples (first five discarded) have working-set
median/p95 79.32/89.43 MiB and peak 457.54 MiB; private bytes median/p95
53.73/63.93 MiB and peak 411.18 MiB. CPU uses one logical core as its denominator:
median 0%, p95 4.41%, peak 417.836% during parallel helper work. These peaks
matter for gaming and are not represented by settled app-only memory.
For the 104 individual decoded-start records, sampled request-to-audio delay is
median 500 ms, p95 751 ms and maximum 5,263 ms, with 250 ms sampling. This combines
cold start, natural transitions and test controls, not a skip-only latency metric.


A subsequent five-minute signed-in check of the downloaded 0e0f674 package ended
with insufficient transition/control coverage (exit 1): its generic progress
records show one natural transition before the time limit, below the required
two. This is a failed test, not a reliability pass. It had decoded progress and
recommendation refills without a reported reconnect failure. The corrected
supervisor retained the nonzero exit and recorded failure stage `checking_exit`.
A longer check is needed to gather the required complete-song coverage.

### In-app connection continuity (2026-10-13)

The account-control completion and manual-file import paths now stage the same
guarded connection adoption used for external browser imports. They no longer
unconditionally replace the active audio engine or reread the displayed account
index separately from the adopted descriptor. The existing bounds on storage
retries and identity revalidation still apply.

The rebuilt native application passed the isolated connection regression with
an additional in-app account request while silent audio was advancing. The check
requires the same playback generation and an increasing position after both
external and in-app same-index reconnects. Superseded identity switches, retained
pause/seek intent and disconnect also passed. App tests (22), formatting and
strict workspace Clippy passed. These fixtures do not prove real browser import
or long-term session longevity. The already-running packaged listening test
predates this change.

Before that source change, the e9ac520 GitHub preview (PR merge 1d5dcc084eb32d5808a5f754689f1ddbcf29646d)
passed CI checks and packaging. Its downloaded Windows ZIP passed payload,
checksum and privacy verification and its executables passed the then-current
isolated adoption regression. ZIP SHA-256:
`6bf9bb5054c826dc78a8452c9aa416624c249a8e57f60cee4d78fb1ad2aed392`.

The periodic playback-cookie sync now also uses the exact saved-connection lease
refresh guard and defers updates during pending adoption. This closes a remaining
unguarded refresh path where another import could change profiles between cookie
persistence and lease refresh. The provider regression for stale playback leases
and superseded verified candidates passes, as do formatting and strict app
Clippy. Live longevity remains a separate verification requirement.

The 1,200-second signed-in check of the 0e0f674 package also ended with
insufficient coverage (exit 1). Its records show four natural transitions, six
decoded starts, six recommendation refills and seven persisted session updates,
without a reported account or playback failure. The required five natural
transitions were not reached before the time limit. Preserve it as a failed
check; do not lower the requirement or infer long-term reliability from the
absence of a reported error. Its supervisor snapshot predates the nonzero-exit
cleanup correction, so its initial `helpers_ended: false` is not a completed
cleanup evaluation. A longer run of the updated source is the next live check.

### Native transport diagnostics (2026-10-13)

The companion now provides an explicit, credential-free host probe and consumes
Firefox native-port errors as well as Chromium runtime errors. The 27 companion
tests and five native-host tests pass, including strict replies, unauthorized
senders, busy-capture preservation, bounded timeout, older-host detection and
non-disclosure of raw browser errors. Formatting and strict provider Clippy pass.

The registered local host returns a framed `host_ready` directly with Firefox
arguments. The actual temporary Zen companion reports a missing/unusable native
application, so browser transport and authenticated import remain failed/pending.
The probe did not alter the active listening test's account storage. Do not count
the direct-host or synthetic tests as a successful Zen handoff.

The executable adoption fixture also runs `check_host` before importing its
synthetic session and compares every file in its isolated APPDATA tree before
and after the check. It requires an exact framed success reply, zero host exit,
empty diagnostic output and no provider directory. This check and the subsequent
adoption/transport-intent fixture pass against the local release binaries.

Before these diagnostic changes, the e777f29 PR preview (merge revision
6e8f9a9c7ef95e0e43337db5a3c21468363ad914) passed both CI jobs. Its downloaded
MSVC ZIP passed payload, checksum and privacy checks and the isolated native
adoption regression against its actual app and host executables. ZIP SHA-256:
`eb29a7b3a86d5d2f0dee90c0383b2a36271d533879814b49e500015d7a0ae9b1`.

### Completed current one-hour listening and Windows package (2026-10-13)

The e777f29 GNU release app completed 3,600 seconds of authenticated listening:
17 natural transitions, 19 decoded starts, 18 recommendation refills and 19
persisted session updates. Radio and observed pause/resume, skip and seek passed.
The enclosing sampler exited zero and recorded root exit zero, all 156 observed
helpers ended and zero sampling races. Standard error was empty. This is a
completed passing run, unlike the historical supervisor failures. The provider
Library health example subsequently passed on the same saved session without a
fresh import. App, playback, provider library and lockfile source are unchanged
between e777f29 and 369f8a1; the intervening changes concern host diagnostics,
the fixture and documentation. This does not prove weekly session longevity.

After discarding the first five samples, 3,264 process-tree samples show working
set median/p95/peak 70.20/76.807/408.52 MiB and private bytes
52.54/58.89/348.70 MiB. CPU, with one logical core as denominator, is
1.395/5.663/108.29 percent. Percentiles use linear interpolation. The 19 sampled
decoded-start delays have median 500 ms, p95 and maximum 5,004 ms; cold startup
and controls are included, with 250 ms sampling. These are not skip-only latency
or gaming frame-time measurements.

Both 369f8a1 CI jobs passed. Its downloaded Windows MSVC app and host passed the
credential-free storage-untouched probe and isolated adoption/intent regression.
The ZIP passed payload/checksum/privacy gates; SHA-256:
`8f79ac787f879a11cd37d952eac8a834e98163f99ebe07ebb0a929b34bb4be33`.
Both downloaded companion packages passed exact folder/ZIP payload and checksum
checks and matched committed source after normalizing Windows checkout line
endings. The fresh public clone at 369f8a1 passed its privacy audit with zero
findings. The preceding 911a0f5 MSVC package passed the same native probes too.

Actual Zen testing still fails native lookup with that downloaded MSVC host as
well as the local GNU host; direct registered-host framing succeeds for both.
Authenticated browser import, signed distribution, physical media keys with
game focus, monitor migration and normal authenticated GUI close remain open.

### Normal authenticated GUI close (2026-10-13)

The actual downloaded 369f8a1 MSVC application was opened normally through
computer control, with no smoke flags or synthetic account. It loaded the saved
account's Home results. Volume was set to zero through the visible slider; its
percentage changed correctly. Clicking Play started authenticated playback:
the UI reported Playing with position advancing to 0:14 and a decoded duration.
Clicking the native title-bar X closed the window normally. A retained-process
observer recorded root exit zero and all nine helpers it observed ended.
The user's settings were restored byte-for-byte after exit, and the temporary
settings backup was removed. No account was reimported or disconnected.

This closes the normal authenticated GUI-close check above for this tested case;
it does not prove every close race, physical media-key mapping, game-focus
behavior or monitor migration. The existing isolated close/race regressions
remain separate evidence. Real browser import and distribution are still open.

### Progressive artwork and song changes (2026-10-13)

Artwork requests follow the scrolled viewport in Home, Library, playlist details,
search and Queue, replacing the first-12 and current-plus/minus-eight limits.
Offscreen model images are released, with two rows of overscan and a maximum
40-image viewport window. Requests run serially on a background worker; the
current song is promoted ahead of queued list thumbnails. Requests coalesce and
remain bounded, including one reserved current-song slot. The memory-only cache
still holds at most 48 thumbnails of 96 by 96 RGB pixels (about 1.27 MiB of raw
pixels, excluding decoding and renderer overhead); no disk artwork cache is added.

The song-change software-renderer regression keeps stale row artwork present,
clears the current cover while the next song loads, then displays the new cover.
It passes without retaining either previous cover. Tests also cover viewport
scrolling beyond the former cap, bounded requests and priority promotion, stale
account completions, and stable model identity during row updates. This evidence
does not measure live network image latency or total application memory.

The optimized local build passed its populated UI smoke with native pointer drag
and long-drag autoscroll, without starting playback. Twelve fixture screenshots
cover the rendered layouts and panels. The normal authenticated app was reopened,
and actual covers were observed farther down Home after scrolling past the old
loading cap. Account contents and screenshots are excluded from public artifacts.

### Per-user preview installation (2026-10-13)

The packaged setup launcher provisions an owned versioned release, private
playback/helper runtimes and Start menu shortcuts without administrator access.
Its isolated lifecycle tests cover first install, upgrade, removal, dependency
and shortcut/registration rollback, busy processes, conflicting registrations,
modified shortcuts, unowned files, checksums, traversal, reparse paths and locks.
PowerShell 5.1 cases and the actual ZIP payload allowlist integration pass. A real
shortcut roundtrip was confined to a temporary directory.

`python scripts/verify-managed-runtime.py --download` passed real provisioning in
a disposable fixture: isolated CPython 3.14.0 with six exact helper package
versions/imports, and mpv 0.41.0, yt-dlp 2026.08.19 and Deno 2.9.7 downloads,
pinned checksums and executable version checks. The fixture was removed. No
account, native-host registration or installed application was changed by this
check. Full fresh-machine installation/upgrade/removal and signed distribution
remain open; the launcher is a preview, not evidence of stable-release readiness.

### Player indicators, recommendation variety and queue text (2026-10-16)

Home Refresh selects 24 playable cards from up to 240 personalized candidates,
prioritizing unseen/recently unshown IDs with bounded memory-only history.
Small upstream pools can repeat; no listening history is written to create variety.

Real native pointer/keyboard input reproduced the progress thumb losing its clock
binding after seeking to maximum. Keeping externally owned values separate from
local input previews fixes the regression across 30 simulated track handoffs in
one fixed-size reused software buffer. This is not 30 naturally completed songs.
Volume text matches a full-render reference without resizing in that test. The
reported physical Windows volume-label freeze was not reproduced offscreen;
a repaint limited to the player bar is a mitigation pending actual-session review.

Rendered queue tests reproduced mixed-direction Hebrew/Japanese/emoji metadata
painting left across artwork and drag handles despite Text elision. Metadata now
has a bounded clipped column and queue delegates use the visible list width.
Long spaced names, 600-character unbroken names and mixed-direction strings keep
time/actions and artwork/handles unchanged at widths 650, 780 and 1140 in both
themes. Native remove controls remain reachable. Full names remain in accessibility
labels. Software images use synthetic metadata only; no user account or playback
is opened. These ordinary player fixes contain no experimental Jam code.

### Playback runtime downloads without a release API lookup (2026-10-16)

Hosted clean-install checks repeatedly encountered GitHub's unauthenticated API
rate limit while looking up release metadata. The failed evidence was retained;
retrying those jobs passed, but the API remained an avoidable setup dependency.
Setup now downloads the same exact mpv/yt-dlp/Deno release URLs and verifies the
same reviewed SHA-256 pins against the actual files before extraction. Versions,
checksums, overwrite refusal, path checks and bounded download timeout are unchanged.
No GitHub account/token or insecure fallback is added.

Isolated installer lifecycle/package tests pass. New regressions exercise the
production asset function with the metadata API unavailable, reject missing pins
and existing output before downloading, and reject a corrupted runtime by the
actual checksum check before tools are extracted. These cases cover PowerShell 7
and built-in Windows PowerShell 5.1. The real-download verifier also passes pinned
CPython/helper imports and all three playback-tool checksums and executable
versions in a disposable temporary fixture, which is removed afterward. It does
not install Tide, register a browser host, access an account or show an app window.
GitHub's download hosting and other upstream services still need to be reachable.
