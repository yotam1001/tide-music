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
arguments. It does not read browser cookies, use an account, or set yt-dlp
credentials. Child `PATH` includes the discovered runtime directory so
`yt-dlp.exe` can find an adjacent JavaScript runtime such as `deno.exe`.

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

Process startup has a five second IPC-connect deadline and checks shutdown or
superseding loads every 30 ms. Shutdown first requests `quit`, allows 900 ms
for mpv to exit, then forces termination. Dropping the handle joins its worker;
an idle handle owns no mpv process. Source arguments never pass through a shell.

## Validation

```powershell
.\scripts\cargo-local.ps1 -CargoArgs @('test','-p','tide-playback')
.\scripts\cargo-local.ps1 -CargoArgs @('clippy','-p','tide-playback','--all-targets','--','-D','warnings')
.\scripts\cargo-local.ps1 -CargoArgs @('run','-p','tide-playback','--example','smoke','--','.local/test-tone.wav')
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
