# Memory test behind the promo video

The promo's "About 11× less memory" comes from one matched run on one PC (2026-10-14). It is a single measurement, not a benchmark suite.

| | Working set, median | Private bytes, median | Processes |
|---|---|---|---|
| Tide 74039fb | **80.7 MiB** | 62.4 MiB | 2 (tide-music + mpv) |
| Edge, YouTube Music web | **885.5 MiB** | 632.9 MiB | 11–12 (all msedge processes of the test profile) |

**Conditions**

- i7-12700, Windows 11. Both played the same track: "Weightless Part 3" (`lt2IBf2R7fU`, official audio-only upload).
- Both were muted. Tide ran with `--benchmark-mode compact` (volume 0). Edge ran with `--mute-audio`, a fresh profile, signed out.
- `scripts/measure-process.ps1` sampled each whole process tree once per second for 400 s. The median covers the final 300 s, when both were playing.
- Playback was confirmed in both. Edge reported `playing: true` and `videoWidth: 0` (audio only) at 45 s and at 6:40. Edge blocked autoplay, so playback was started at about 45 s, before the median window.
- Not in Tide's median: the song lookup (yt-dlp + Deno) when a song starts briefly runs up to 5 processes at about 150 MiB for 2–3 s.

Results vary with the browser, its extensions and tabs, the song, and the machine.
