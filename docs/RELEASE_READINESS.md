# Release readiness

Tide is not yet verified ready for a stable release. These gates retain the full
playback, account, UI and open-source release requirements. A short passing test
cannot establish long-term service reliability.

## Latest verified runtime (2026-10-17)

Source `e02e99e`, merged through PR #13 (`ce0d142`), passed both
[push CI](https://github.com/yotam1001/tide-music/actions/runs/39041897082) and
[PR CI](https://github.com/yotam1001/tide-music/actions/runs/39041899612).
This includes workspace/updater/UI tests, strict Clippy, package/privacy checks
and the real Windows installation lifecycle. Actual graphical setup waited for
an installed Tide process to close, updated the owned release, retained the
synthetic Chrome registration, reopened the new player and preserved saved
settings. Installed local-audio UI and registered removal also passed.

Exact-head menu, Account, Settings and updater renders were reviewed in both
themes. The downloaded installer/ZIP passed checksum, allowlist and privacy
validation, and its embedded ZIP matched the separately validated package.
No personal browser, saved account or user installation was used by these tests.
Live updating between published releases remains untested because no stable
release has been published.

## Before the public preview

The remaining fresh-user gate is following the installer and Chrome connection
guides with a real selected account, then confirming playback and browser-closure
continuity. Physical media keys in games, monitor migration and longer session
life remain separate manual checks. See the [launch checklist](BETA_RELEASE.md)
for the preview/stable distinction.

The old `v0.1.0-beta.1` draft still contains `f026915` binaries. They lack later UI
and updater work and must not be presented as the latest candidate. Historical
candidate evidence remains in [VALIDATION.md](VALIDATION.md).

## Broader stable-release gates

The rows below retain their scoped historical live-test evidence; they do not
claim a new authenticated test for every later UI/documentation revision.

| Gate | Required evidence | Current evidence |
| --- | --- | --- |
| Continuous playback | Sustained live playback with natural transitions and repeated skip/pause/seek, without unexplained stops | The e777f29 one-hour run passed 17 natural transitions, radio refills and pause/resume/skip/seek. App and sampler exited zero; all 156 observed helpers ended, with zero sampling races. Playback source is unchanged through 369f8a1. Historical six-hour app behavior passed, but its enclosing supervisor failed; preserve that failure |
| Recovery | Helper loss, stream interruption, bounded stall/retry handling, retained position and pause/seek intent, cancellation | Native fault injection passes these paths. Real service outage recovery and longer live behavior remain to be verified |
| Recommendations | Radio appends and repeatedly refills, advances at EOF, and retries temporary failures without false reconnect | Repeated signed-in refills and a live queue-depletion/EOF probe pass. Temporary content retries preserve request identity; real outage verification remains |
| Resource bounds | Process-tree memory across playback/transitions; helpers end; artwork/metadata/queue remain bounded; acceptable gaming impact | Latest one-hour tree working-set median/p95 70.20/76.81 MiB, transient peak 408.52 MiB; private bytes median/p95 52.54/58.89 MiB, peak 348.70 MiB. Helper cleanup passed. Historical six-hour peak was 457.54 MiB. Gaming impact and extraction peaks remain open |
| UI controls | Actual sliders, volume display, queue drag, modal focus, Escape, media keys and normal close during playback | Eleven focused native checks, queue drag/autoscroll, production broker transport, async-picker EOF/control/close, and fifty Mini/Expand pairs pass. Normal packaged authenticated GUI playback and title-bar close passed with exit zero and nine observed helpers ended. Physical Fn keys with game focus remain unverified |
| Rendered layouts | Full/compact/mini, dark/light, panels, minima, resizing and high DPI all usable | Rendered layouts and new 650 by 420 minimum reviewed. Native synthetic 150%/200% DPI messages, maximize/DPI-change/restore and Mini/Expand fit the monitor work area with audio continuing. Account settings remain reachable by scrolling. Physical monitor migration remains unverified |
| Accounts | Identity changes isolate private state; expiry is actionable; temporary failures do not falsely invalidate verified login; reconnect rarely needed | The completed latest one-hour run and a subsequent current-provider Library check passed with the saved session and no new import. Historical private-Zen sessions passed shorter checks and six-hour playback. Weekly session longevity and direct profile reconnect remain unproven |
| Public login setup | Simple connection using the user's browser, exact selected account, verified native handoff and documented distribution/removal | Both companion packages and native host pass synthetic protocol/package/adoption tests. Actual Zen lookup fails with both GNU and downloaded MSVC hosts despite matching registration and successful direct framing. Actual Chrome local-host check and selected-tab session import passed, followed by an authenticated Tide Library check. Fresh-user Chrome onboarding, Zen lookup and store/signed distribution remain pending |
| Open-source package | Documented fresh installation, license/security/privacy/dependencies, current CI, fresh remote audit and portable validation | Latest e02e99e CI and fresh public-clone privacy audit pass; see the current evidence above. Real clean hosted Windows install/update/remove and downloaded installer/ZIP/companion verification pass. The five historical f026915 draft assets were downloaded back with matching hashes. Interactive GUI setup, signed distribution and the other stable-release gates remain open |

See [VALIDATION.md](VALIDATION.md) for revisions, scope and measurements. Validation
uses transient settings and generic diagnostics. Never publish session values,
signed stream URLs, private library contents or local machine paths. Service
failures must remain actionable, without endless hidden retries.
