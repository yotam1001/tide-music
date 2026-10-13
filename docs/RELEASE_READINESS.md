# Release readiness

Tide is not yet verified ready for a stable release. These gates retain the full
playback, account, UI and open-source release requirements. A short passing test
cannot establish long-term service reliability.

| Gate | Required evidence | Current evidence |
| --- | --- | --- |
| Continuous playback | Sustained live playback with natural transitions and repeated skip/pause/seek, without unexplained stops | The e777f29 one-hour run passed 17 natural transitions, radio refills and pause/resume/skip/seek. App and sampler exited zero; all 156 observed helpers ended, with zero sampling races. Playback source is unchanged through 369f8a1. Historical six-hour app behavior passed, but its enclosing supervisor failed; preserve that failure |
| Recovery | Helper loss, stream interruption, bounded stall/retry handling, retained position and pause/seek intent, cancellation | Native fault injection passes these paths. Real service outage recovery and longer live behavior remain to be verified |
| Recommendations | Radio appends and repeatedly refills, advances at EOF, and retries temporary failures without false reconnect | Repeated signed-in refills and a live queue-depletion/EOF probe pass. Temporary content retries preserve request identity; real outage verification remains |
| Resource bounds | Process-tree memory across playback/transitions; helpers end; artwork/metadata/queue remain bounded; acceptable gaming impact | Latest one-hour tree working-set median/p95 70.20/76.81 MiB, transient peak 408.52 MiB; private bytes median/p95 52.54/58.89 MiB, peak 348.70 MiB. Helper cleanup passed. Historical six-hour peak was 457.54 MiB. Gaming impact and extraction peaks remain open |
| UI controls | Actual sliders, volume display, queue drag, modal focus, Escape, media keys and normal close during playback | Eleven focused native checks, queue drag/autoscroll, production broker transport, async-picker EOF/control/close, and fifty Mini/Expand pairs pass. Normal packaged authenticated GUI playback and title-bar close passed with exit zero and nine observed helpers ended. Physical Fn keys with game focus remain unverified |
| Rendered layouts | Full/compact/mini, dark/light, panels, minima, resizing and high DPI all usable | Rendered layouts and new 650 by 420 minimum reviewed. Native synthetic 150%/200% DPI messages, maximize/DPI-change/restore and Mini/Expand fit the monitor work area with audio continuing. Account settings remain reachable by scrolling. Physical monitor migration remains unverified |
| Accounts | Identity changes isolate private state; expiry is actionable; temporary failures do not falsely invalidate verified login; reconnect rarely needed | The completed latest one-hour run and a subsequent current-provider Library check passed with the saved session and no new import. Historical private-Zen sessions passed shorter checks and six-hour playback. Weekly session longevity and direct profile reconnect remain unproven |
| Public login setup | Simple connection using the user's browser, exact selected account, verified native handoff and documented distribution/removal | Both companion packages and native host pass synthetic protocol/package/adoption tests. Actual Zen lookup fails with both GNU and downloaded MSVC hosts despite matching registration and successful direct framing. Browser comparison, real import and signed distribution remain pending; temporary loading is not public onboarding |
| Open-source package | Documented fresh installation, license/security/privacy/dependencies, current CI, fresh remote audit and portable validation | Both 369f8a1 Windows CI jobs and fresh public-clone privacy audit pass. Its downloaded MSVC ZIP passed payload/checksum/privacy and native probe/adoption checks. Both companion artifacts passed payload/checksum/source verification. Historical portable fresh-runtime checks pass. Stable release remains pending the other gates |

See [VALIDATION.md](VALIDATION.md) for revisions, scope and measurements. Validation
uses transient settings and generic diagnostics. Never publish session values,
signed stream URLs, private library contents or local machine paths. Service
failures must remain actionable, without endless hidden retries.
