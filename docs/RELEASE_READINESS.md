# Release readiness

Tide is not yet verified ready for a stable release. The following gates define
the remaining work; individual unit tests or short startup probes are insufficient.

| Gate | Required evidence | Current evidence |
| --- | --- | --- |
| Continuous playback | A sustained muted live session with multiple natural track transitions and repeated skip/pause/seek actions, with no unexplained stop | Five-minute local queue passed 15 natural transitions and pause/resume, skip and seek. Signed-in service session still pending |
| Recovery | Native fault injection verifies helper loss, read interruption, loading/stall bounds, resume position, pause intent, cancellation and retry limits | Native probe passed: process loss, truncated HTTP stream, resume/pause/seek intent, retry exhaustion and cancellation; live service soak still pending |
| Recommendations | Signed-in radio appends, advances at EOF and refills repeatedly; temporary request failure retries without treating it as expired sign-in | In progress |
| Resource bounds | Process-tree memory samples during playback and transitions; helpers exit after Stop/Drop; queue, artwork and metadata stay bounded | Five-minute local release run: median working set 79.0 MiB, peak 81.8 MiB; median private bytes 51.2 MiB, peak 53.5 MiB. All observed helpers ended. Signed-in extraction/artwork and gaming measurements pending |
| UI controls | Actual pointer/key checks for slider release, live volume, queue drag, modal isolation, Escape and media controls | Focused native harness passed 11 checks for sliders, accessibility, modal focus and Escape; live media keys and queue drag need final regression check |
| Rendered layouts | Review full, compact, mini, dark/light, overlays, minimum sizes, resize boundaries and scaled display layouts | Focused harness layouts reviewed at minima, queue docking threshold and simulated 150%/200% rendering; actual monitor DPI migration pending |
| Accounts | Identity changes clear private preview state; expired sessions give an actionable prompt; network errors do not falsely invalidate a verified account | Reconnect preserves selected account; anonymous account-menu responses classify as rejected sessions; profile-read and container failures are distinct. Saved session is currently rejected; successful reconnect and live radio verification pending |
| Open source package | Portable documented installation, license, privacy/security notes, dependency versions and reproducible checks; fresh remote privacy audit; CI passes | Extracted preview installed outside the checkout and passed five-minute local continuity. Dependency notices, pinned Python dependencies, checksums and final ZIP privacy scan pass. New packaging revision still requires remote audit and CI |

Validation must use transient test settings and generic diagnostics. Never
publish session values, signed stream URLs, private library contents or local
machine paths. Any service outage or authentication requirement must be reported
clearly rather than hidden by an endless retry loop.
