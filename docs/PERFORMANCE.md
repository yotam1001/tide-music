# Repeatable local playback checks

The hosted Windows installer check runs a muted, synthetic three-second WAV
through the installed Tide executable with an explicit mpv null audio output.
Hosted runners need no sound device for this workload. Ordinary Tide playback
always uses WASAPI; output failures never trigger a silent fallback. It follows natural song transitions and
three skips for about 30 seconds, then requires the app to exit successfully.
It uses transient settings and no account, browser, internet audio or saved queue.

`scripts/measure-process.ps1` samples the app and its descendant processes once
per iteration. Windows process enumeration adds overhead, so sample intervals
can exceed one second. Private memory, working set and CPU are summed across
the observed process tree. CPU uses one logical core as its denominator.
Retained process handles distinguish an observed helper from a reused PID.
The check fails if an observed helper remains alive after the app exits.
Very short-lived helpers can escape enumeration; the report is not proof that
every process lifetime was captured.

`scripts/check-performance.py` produces a summary from the samples, cleanup
evidence and app timing report. Peaks include the first samples; settled median
and p95 omit the first five. The local regression budgets are 512 MiB peak
working set and five seconds maximum request-to-decoded-progress time. These
are CI guardrails, not promised typical usage or maximum memory limits.
The app samples decoded progress every 100 ms, which limits timing precision.
At least four natural transitions, three skips and six start samples are required.

The summary contains only counts and aggregate timings/resources. It excludes
track names, URLs, process IDs, filesystem paths and credentials. Raw resource
evidence includes observed process names and identities for cleanup diagnosis;
the synthetic workload never loads a user session. Evidence is retained in the
Windows integration artifact for seven days.

This test detects local playback, cleanup and large memory/start-time regressions.
It does not measure YouTube extraction latency, long-term session reliability,
WASAPI latency, audio device compatibility or a game's performance. Those need separate tests.
Use hosted CI for builds; follow `AGENTS.md` and the local disk budget when
developing. Running the existing installed executable with
`--smoke-performance <new-report.json> <local-three-second.wav>` is opt-in and
does not compile Rust. Reports must use a new filename.
