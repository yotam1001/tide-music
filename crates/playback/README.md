# tide-playback

`tide-playback` implements the `tide-core` playback contract with a Windows
mpv sidecar controlled through JSON IPC over a named pipe. The worker is
created by `start()`, while `mpv.exe` is launched only when the first track is
loaded. Dropping `PlaybackHandle` requests a clean mpv exit, forces termination
if the helper does not exit promptly, reaps the process, and joins the worker.
An unnamed Windows Job Object with `KILL_ON_JOB_CLOSE` contains mpv before
any source is loaded and terminates its extraction/helper descendants on exit.
Playback fails clearly if Windows refuses to install that containment.

The backend accepts an existing local file or an `http`/`https` URL. YouTube
hosts require `yt-dlp.exe`; the engine selects audio formats, disables video,
uses a bounded 16 MiB forward demuxer buffer plus a 2 MiB backward buffer,
and passes playback sources as JSON
arguments. It does not read browser profiles. `start_with_account` accepts a
caller-owned private cookie lease and selected account index for signed-in
extraction. Child `PATH` includes the discovered runtime directory so
`yt-dlp.exe` can find an adjacent JavaScript runtime such as `deno.exe`.

`prepare_next(Some(source))` prepares one upcoming YouTube stream, with
deduplicated requests, a 45-second resolution timeout, a 2 MiB metadata output
limit and a memory-only lifetime capped at ten minutes and thirty seconds before signed URL expiry. Invalid or already-expiring addresses fall back to ordinary loading. `prepare_next(None)` cancels it.
The app schedules this once current playback is loaded, checking every 250 ms; Pause retains the candidate for early skipping. Queue
changes replace the request; Load consumes a matching fresh result and cancels
unfinished work. Failed resolution can retry after a twenty-second backoff,
without repeatedly launching helpers on every timer tick. Stop and Drop also
cancel and join preparation. Each resolver
owns a kill-on-close process job and, when signed in, an exclusive cookie copy
in the protected lease directory that is removed after process cleanup.

Preparation selects the same `bestaudio/best` policy as ordinary playback. It
accepts supported HTTPS YouTube media CDN addresses, preserves required safe
headers, and never persists signed stream URLs. An unavailable result uses
ordinary extraction; a rejected prepared load joins the bounded reconnect
policy and refreshes through the original source.
It avoids resolving the next song at EOF when preparation succeeds, but still
starts a fresh mpv session and is not guaranteed gapless playback.

An opt-in muted public-source probe compares cold and prepared startup:
`cargo run -p tide-playback --example transition -- <public-url> <next-public-url>`.
It uses no account session and checks cancellation when the handle is dropped.

The app-level early-skip check uses transient settings and invokes the same Next
callback as the player controls:
`cargo run -p tide-music -- --smoke-early-skip <public-url> <next-public-url>`.
Use `--smoke-early-skip-session` instead to verify the existing encrypted local
session before testing. Both probes stay muted and do not save their test queue.

Runtime discovery checks, in order, `TIDE_RUNTIME_DIR`, the app's adjacent
`tools` directory, `.tools/runtime` beside the app or in nearby project
ancestors, then `PATH`. `mpv.exe` is required at `start()`; `yt-dlp.exe` is
required only when loading a YouTube URL. The project-local runtime directory
is the intended development and portable packaging location.

The worker uses bounded command and IPC event queues, limits individual IPC
lines to 256 KiB, and bounds IPC writes to 500 ms. Each replacement load stops
the previous owned process tree and starts a fresh mpv session. This cancels
blocked extraction and keeps untagged mpv property events attributable to one
track. Session, generation, epoch, and playlist-entry checks suppress metadata
and completion events from superseded tracks, including repeated loads that
reuse a generation. Older generations are rejected. `Stop` exits mpv and
releases its buffers; the next `Load` starts a fresh helper process. Pause and
seek commands queued for an old track are discarded. Volume is an accepted
target shared by all sessions, and new mpv processes receive it before loading.

Unexpected IPC loss, source read errors, and early EOF of a known finite track
reconnect the same original source at most twice consecutively, after one
and three seconds. Sixty seconds of confirmed forward playback restores the
retry budget for a later outage; pause and seek jumps do not restore it.
Reconnects discard prepared stream URLs and resume from the
last confirmed position using mpv's per-file `start` option. Explicit pause and
seek targets accepted during loading/reconnect take precedence, and the new
file starts paused until those controls are applied. Replacements, Stop and
shutdown cancel recovery through the same generation/epoch/session checks.
Natural EOF and unknown-duration EOF retain normal completion behavior; an
EOF more than the larger of three seconds or one percent before known duration
is treated as premature. The full-duration estimate retains its largest finite
observation because some demuxers shrink duration to bytes received just
before an interrupted EOF. This tolerance accommodates delayed position events.

A 250 ms watchdog bounds each file-loading attempt to sixty seconds and
reconnects after thirty seconds without position progress while playback is
intended. Paused files are exempt from the progress watchdog; loading remains
bounded while paused. Retry exhaustion leaves a visible error with `ended`
false, so the app can offer retry without advancing the queue. These limits
bound silent loading; they cannot restore an unavailable source or network.
Recovery state stays in memory and diagnostics never include source URLs,
headers, cookie paths, session details or library contents.

Process startup has a five second IPC-connect deadline and checks shutdown or
superseding loads every 30 ms. Shutdown first requests `quit`, allows 900 ms
for mpv to exit, then forces termination. Dropping the handle joins its worker;
an idle handle owns no mpv process. Source arguments never pass through a shell.

## Validation

```powershell
.\scripts\cargo-local.ps1 -CargoArgs @('test','-p','tide-playback')
.\scripts\cargo-local.ps1 -CargoArgs @('clippy','-p','tide-playback','--all-targets','--','-D','warnings')
.\scripts\cargo-local.ps1 -CargoArgs @('run','-p','tide-playback','--example','smoke','--','.local/test-tone.wav')
.\scripts\cargo-local.ps1 -CargoArgs @('test','-p','tide-playback','native_recovery_fault_probe','--','--ignored','--nocapture')
```

The smoke example accepts a seekable local audio file or an HTTP(S) URL of at
least four seconds. It sets volume to zero before loading and checks load,
stable pause, exact seek, rapid replacement, stale-generation rejection,
Stop/reset, restart, EOF, shutdown, live Drop, startup Drop, and idle Drop.
Each wait has a 45 second deadline; helper cleanup must finish within three
seconds. It produces no sound. Live streams without finite duration/seek
support are unsuitable for this example.

Windows validation with the project-local mpv 0.41 runtime and an eight second
WAV passed all smoke assertions. The same muted checks passed for the public
[Big Buck Bunny YouTube source](https://www.youtube.com/watch?v=aqz-KE-bpKQ),
with a reported duration of 634.601 seconds. Another test source failed with
a clean playback error; success is source-dependent. No mpv, yt-dlp, or Deno
processes remained after either smoke run. Unit tests also exercise source validation,
superseded events, Stop epochs, bounded IPC reads, and actual process-tree
termination: a contained test process spawns a descendant, and dropping the
owner must signal the descendant's termination. The ignored `job_tree_probe`
test is an internal subprocess helper invoked by that cleanup test.

The opt-in native recovery probe creates its own muted local silence and kills
only the mpv owned by its test handle. It checks resume, pause/seek preservation
during reconnect, retry exhaustion, replacement and Stop cancellation. A
loopback HTTP server then truncates a WAV whose header advertises sixty
seconds and rejects internal Range refill until Tide starts recovery; the
probe checks original-source
reconnect and position preservation against subsequent complete responses.
It uses no account session, external network, persisted queue or running app
music. Deterministic tests also cover source read-error events, stale entries,
natural/unknown EOF boundaries, pending-seek observations, retry limits and
loading/stall/pause watchdog deadlines.

## Known limitations

- The current IPC implementation and supported playback backend are Windows
  only. Other targets return a clear unsupported-platform error.
- YouTube extraction is an unofficial dependency on the installed
  `yt-dlp.exe` and a compatible JavaScript runtime. Service changes, geo or
  network restrictions, and unavailable formats can cause playback errors.
- No account login, browser-cookie access, ad suppression, DRM bypass,
  credential scanning, search/catalog access, automatic downloads, or
  persistent stream URL cache is implemented.
- Metadata currently exposes mpv's media title only. Track artist and album
  enrichment belongs to the provider layer.
- Audio-device changes and Windows media keys are not handled in this crate.
- The 16 MiB cache cap and 20 second cache horizon are conservative initial
  settings, not measured performance guarantees.
- A fresh process per track adds startup overhead. This favors reliable
  cancellation and isolation; gapless playback is not implemented.
- Network extraction and device behavior depend on the packaged runtime and
  environment. The muted local smoke verifies the engine but does not measure
  CPU/RAM usage, audible output quality, or compatibility with every source.
- No YouTube account library, saved songs, subscriptions, or account sync is
  implemented. Playback failures never trigger a cookie or login fallback.

The command and event behavior follows the upstream [mpv manual's JSON IPC,
Windows named-pipe, audio/video, cache, and yt-dlp sections](https://mpv.io/manual/stable/)
and Microsoft's [Job Objects lifecycle documentation](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects).
