# Release readiness

Tide is not yet verified ready for a stable release. These gates retain the full
playback, account, UI and open-source release requirements. A short passing test
cannot establish long-term service reliability.

| Gate | Required evidence | Current evidence |
| --- | --- | --- |
| Continuous playback | Sustained live playback with natural transitions and repeated skip/pause/seek, without unexplained stops | Two 30-minute runs passed. The 8c86152 one-hour application reported 14 natural transitions and controls, but its supervisor failed after completion with final exit/cleanup unproven. The b019b14 six-hour application passed 102 natural transitions and controls; its supervisor failed after PASS. A separate observer confirms root exit 0 and its 243 observed helpers ended |
| Recovery | Helper loss, stream interruption, bounded stall/retry handling, retained position and pause/seek intent, cancellation | Native fault injection passes these paths. Real service outage recovery and longer live behavior remain to be verified |
| Recommendations | Radio appends and repeatedly refills, advances at EOF, and retries temporary failures without false reconnect | Repeated signed-in refills and a live queue-depletion/EOF probe pass. Temporary content retries preserve request identity; real outage verification remains |
| Resource bounds | Process-tree memory across playback/transitions; helpers end; artwork/metadata/queue remain bounded; acceptable gaming impact | Hour-run settled working set median/p95 70.79/77.74 MiB, transient extraction peak 404.59 MiB. Current portable local median/p95 81.82/81.96 MiB, peak 83.05 MiB; exit code zero and helper cleanup recorded. Six-hour tree working-set median/p95 79.32/89.43 MiB, peak 457.54 MiB. Gaming impact and extraction peaks need further work |
| UI controls | Actual sliders, volume display, queue drag, modal focus, Escape, media keys and normal close during playback | Eleven focused native checks, queue drag/autoscroll, production broker transport, async-picker EOF/control/close, and fifty Mini/Expand pairs pass. Physical Fn keys with game focus and close during authenticated GUI playback remain unverified |
| Rendered layouts | Full/compact/mini, dark/light, panels, minima, resizing and high DPI all usable | Rendered layouts and new 650 by 420 minimum reviewed. Native synthetic 150%/200% DPI messages, maximize/DPI-change/restore and Mini/Expand fit the monitor work area with audio continuing. Account settings remain reachable by scrolling. Physical monitor migration remains unverified |
| Accounts | Identity changes isolate private state; expiry is actionable; temporary failures do not falsely invalidate verified login; reconnect rarely needed | Same private-Zen session passed two 30-minute runs, the hour runtime report and a later independent library check without another import. The six-hour b019b14 playback run passed, and a subsequent library check passed without another import. Weekly session longevity and direct profile reconnect remain unproven |
| Public login setup | Simple connection using the user's browser, exact selected account, verified native handoff and documented distribution/removal | Firefox/Chromium companion packages and native host pass synthetic protocol/package/adoption tests. Authorized Mozilla registration works after the missing-parent fix. Real browser import and signed extension distribution remain unverified; temporary developer loading is not public onboarding |
| Open-source package | Documented fresh installation, license/security/privacy/dependencies, current CI, fresh remote audit and portable validation | Both b019b14 Windows CI jobs and fresh public-clone privacy audit pass. Its ZIP passed payload/checksum/privacy gates, fresh runtime/helper installation and five-minute local listening, with exit/cleanup recorded. A newer 5677f50 MSVC package passed synthetic login adoption against the actual downloaded executables. Stable release remains pending the other gates |

See [VALIDATION.md](VALIDATION.md) for revisions, scope and measurements. Validation
uses transient settings and generic diagnostics. Never publish session values,
signed stream URLs, private library contents or local machine paths. Service
failures must remain actionable, without endless hidden retries.
