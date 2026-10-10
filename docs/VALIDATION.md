# Prototype validation

Validated on Windows 11 Pro 10.0.26200, Ryzen 5 7600X, approximately 15.1 GiB usable RAM and Radeon RX 9060 XT graphics. Release builds use Rust 1.99.0 for x86_64-pc-windows-gnu. Measurements below are one-machine observations, not product guarantees.

## Functional evidence

- Workspace queue, source validation, bounded IPC, process containment and Windows transport tests pass.
- Muted local WAV and public YouTube playback passed load, pause, seek, rapid replacement, stale-generation rejection, stop, EOF and shutdown smoke checks.
- Repeated explicit Pause commands remain paused.
- Windows media-session registration was tested with a real owned window: metadata/state readback, timeline updates and handler cleanup pass.
- Native full, compact and collapsed layouts were rendered and inspected, including long titles and empty queues. Illustrations are vector placeholders.
- The pinned runtime setup script completed successfully. Runtime binaries and toolchains are excluded from Git.

Physical keyboard/headset routing while a game has focus, audio-device removal, extended playback/soak tests, display scaling at 150%/200%, and game frame-time comparisons remain unverified. Account/library sync, real album artwork and system tray support are not implemented.

## Initial renderer experiment

The first GPU-rendered release failed the initial idle private-memory target. Twenty samples per layout, with a one-second delay between collections, an empty queue and no playback helper, yielded settled median private memory of approximately 255 MiB full, 213 MiB compact and 210 MiB collapsed after excluding the first five samples. This motivated selecting and measuring the software renderer. Keeping all layouts in the executable does not instantiate all of them simultaneously.

## Reproduce

## Software-rendered idle measurements

Twenty samples per empty-queue layout, excluding the first five samples:

| Layout | Median private MiB | Median total working-set MiB | Median CPU, one core |
| --- | ---: | ---: | ---: |
| Full | 7.44 | 24.78 | 6% |
| Compact | 4.86 | 22.37 | 0% |
| Collapsed | 3.73 | 20.99 | 0% |

These are short visible-window tests with an empty queue. Full-view CPU had a 32% p95 and requires further focus/redraw investigation; compact was 0% at p95, collapsed 4%. They do not establish minimized playback cost or long-session stability. Builds were active elsewhere on the machine during some measurements. No audio helper was running in these idle samples.

## Reproduce

Build with `cargo build --release -p tide-music --locked`, install playback runtimes using `scripts/setup-runtime.ps1`, and run:

```powershell
pwsh -File scripts/measure-process.ps1 -Executable target/release/tide-music.exe -Seconds 30 -AppArguments '--benchmark-mode','light'
python scripts/summarize-resources.py .local/resource-samples.json
cargo run -p tide-playback --example smoke -- path/to/seekable-audio.wav
```

Benchmark mode uses transient settings and does not change the saved queue or preferences. The measurement script starts its own process and closes it after sampling. It sums Private Bytes and total Working Set for that process tree, including active helpers. Total Working Set includes shared pages and is not Working Set - Private. CPU percentage uses one logical core as denominator. Very short-lived child processes between samples may be missed, and exited-process CPU totals are approximate. Samples include startup; settled comparisons should discard the first five samples. GPU memory is not included.

Live-service smoke tests are separate from deterministic CI because provider availability and extraction behavior can change.
