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

A 15-sample empty compact-mode spot check of the account build used one app process and approximately 4.86 MiB private memory (22.93 MiB total Working Set), with 0% median CPU after startup. Benchmark mode disables account loading, so this checks interface regression only, not authenticated playback or refresh memory.

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
