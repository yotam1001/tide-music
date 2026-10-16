# Release readiness

Tide is not yet verified ready for a stable release. These gates retain the full
playback, account, UI and open-source release requirements. A short passing test
cannot establish long-term service reliability.

Latest installer evidence (2026-10-15): candidate `f026915` passed both
[push CI](https://github.com/yotam1001/tide-music/actions/runs/38751976892) and
[PR CI](https://github.com/yotam1001/tide-music/actions/runs/38751986879),
including real-download installation, update, real HKCU uninstall/native-host
registration, actual Start-menu shortcuts, installed runtimes/local-audio UI smoke
checks and registered-command removal on a disposable hosted Windows runner.
Downloaded evidence reports success. Chrome used a synthetic extension ID;
no browser, authenticated import, interactive installer clicks or high-DPI setup
were tested. Downloaded candidate artifacts pass payload/privacy/checksum checks,
including the fixed CMD launchers. The unpublished beta draft uses these matching
`f026915` binaries; older `1e403b7` downloads have been superseded.
This does not close GUI installation or public account-onboarding gates.
See [beta release preparation](BETA_RELEASE.md)
and [validation details](VALIDATION.md#final-candidate-and-real-windows-lifecycle-2026-10-15).

Newer development source includes player-indicator and queue-text fixes, varied
Home selections, and setup hardening that downloads pinned playback releases
without a GitHub release-metadata API lookup. Actual downloaded files must still
match their pinned checksums. These changes are not in the existing `f026915`
draft assets. The volume repaint mitigation and a new installer candidate still
need the user's manual checks; experimental Jams remain unmerged for user review.

| Gate | Required evidence | Current evidence |
| --- | --- | --- |
| Continuous playback | Sustained live playback with natural transitions and repeated skip/pause/seek, without unexplained stops | The e777f29 one-hour run passed 17 natural transitions, radio refills and pause/resume/skip/seek. App and sampler exited zero; all 156 observed helpers ended, with zero sampling races. Playback source is unchanged through 369f8a1. Historical six-hour app behavior passed, but its enclosing supervisor failed; preserve that failure |
| Recovery | Helper loss, stream interruption, bounded stall/retry handling, retained position and pause/seek intent, cancellation | Native fault injection passes these paths. Real service outage recovery and longer live behavior remain to be verified |
| Recommendations | Radio appends and repeatedly refills, advances at EOF, and retries temporary failures without false reconnect | Repeated signed-in refills and a live queue-depletion/EOF probe pass. Temporary content retries preserve request identity; real outage verification remains |
| Resource bounds | Process-tree memory across playback/transitions; helpers end; artwork/metadata/queue remain bounded; acceptable gaming impact | Latest one-hour tree working-set median/p95 70.20/76.81 MiB, transient peak 408.52 MiB; private bytes median/p95 52.54/58.89 MiB, peak 348.70 MiB. Helper cleanup passed. Historical six-hour peak was 457.54 MiB. Gaming impact and extraction peaks remain open |
| UI controls | Actual sliders, volume display, queue drag, modal focus, Escape, media keys and normal close during playback | Eleven focused native checks, queue drag/autoscroll, production broker transport, async-picker EOF/control/close, and fifty Mini/Expand pairs pass. Normal packaged authenticated GUI playback and title-bar close passed with exit zero and nine observed helpers ended. Physical Fn keys with game focus remain unverified |
| Rendered layouts | Full/compact/mini, dark/light, panels, minima, resizing and high DPI all usable | Rendered layouts and new 650 by 420 minimum reviewed. Native synthetic 150%/200% DPI messages, maximize/DPI-change/restore and Mini/Expand fit the monitor work area with audio continuing. Account settings remain reachable by scrolling. Physical monitor migration remains unverified |
| Accounts | Identity changes isolate private state; expiry is actionable; temporary failures do not falsely invalidate verified login; reconnect rarely needed | The completed latest one-hour run and a subsequent current-provider Library check passed with the saved session and no new import. Historical private-Zen sessions passed shorter checks and six-hour playback. Weekly session longevity and direct profile reconnect remain unproven |
| Public login setup | Simple connection using the user's browser, exact selected account, verified native handoff and documented distribution/removal | Both companion packages and native host pass synthetic protocol/package/adoption tests. Actual Zen lookup fails with both GNU and downloaded MSVC hosts despite matching registration and successful direct framing. Actual Chrome local-host check and selected-tab session import passed, followed by an authenticated Tide Library check. Zen lookup and signed distribution remain pending; temporary/unpacked loading is not public onboarding |
| Open-source package | Documented fresh installation, license/security/privacy/dependencies, current CI, fresh remote audit and portable validation | Both f026915 CI runs and fresh public-clone privacy audit pass. Real clean hosted Windows install/update/remove and downloaded installer/ZIP/companion verification pass. All five draft assets were downloaded back with matching hashes. Interactive GUI setup, signed distribution and the other stable-release gates remain open |

See [VALIDATION.md](VALIDATION.md) for revisions, scope and measurements. Validation
uses transient settings and generic diagnostics. Never publish session values,
signed stream URLs, private library contents or local machine paths. Service
failures must remain actionable, without endless hidden retries.
