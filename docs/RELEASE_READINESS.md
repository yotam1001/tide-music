# Release readiness

Tide is not yet verified ready for a stable release. The following gates define
the remaining work; individual unit tests or short startup probes are insufficient.

| Gate | Required evidence | Current evidence |
| --- | --- | --- |
| Continuous playback | A sustained muted live session with multiple natural track transitions and repeated skip/pause/seek actions, with no unexplained stop | Revision 5e49e9b passed 30 minutes signed in: eight natural transitions, pause/resume, skip and seek; post-run account library check passed. Natural transitions still take about four seconds. EOF preparation fix and longer longevity checks remain pending |
| Recovery | Native fault injection verifies helper loss, read interruption, loading/stall bounds, resume position, pause intent, cancellation and retry limits | Native probe passed: process loss, truncated HTTP stream, resume/pause/seek intent, retry exhaustion and cancellation; live service soak still pending |
| Recommendations | Signed-in radio appends, advances at EOF and refills repeatedly; temporary request failure retries without treating it as expired sign-in | Ten recommendation refills passed in the 30-minute signed-in run. Service-outage recovery under real requests remains to be verified |
| Resource bounds | Process-tree memory samples during playback and transitions; helpers exit after Stop/Drop; queue, artwork and metadata stay bounded | Thirty-minute signed-in process tree: median working set 79.90 MiB, p95 86.76, peak 407.33; median private bytes 54.16 MiB, peak 341.25. Extraction peaks and gaming impact need further work |
| UI controls | Actual pointer/key checks for slider release, live volume, queue drag, modal isolation, Escape and media controls | Focused native harness passed 11 checks for sliders, accessibility, modal focus and Escape. Queue pointer drag and long edge-autoscroll passed without changing playback generation. Windows broker delivered play, pause, next and previous exactly once to an unfocused test window; media-session cleanup passed. Three isolated production-mode windows closed normally. Physical Fn keys and closing during authenticated playback remain to be checked |
| Rendered layouts | Review full, compact, mini, dark/light, overlays, minimum sizes, resize boundaries and scaled display layouts | Focused harness layouts reviewed at minima, queue docking threshold and simulated 150%/200% rendering; actual monitor DPI migration pending |
| Accounts | Identity changes clear private preview state; expired sessions give an actionable prompt; network errors do not falsely invalidate a verified account | Revision 5e49e9b persisted sixteen native updates and passed a fresh account library check after thirty minutes. Its session came from private Zen; regular Zen could remain open. Weekly session lifetime and direct profile reconnect remain unproven |
| Open source package | Portable documented installation, license, privacy/security notes, dependency versions and reproducible checks; fresh remote privacy audit; CI passes | Revision 5e49e9b passed both Windows CI jobs, fresh public-clone privacy audits, ZIP payload/checksum/privacy scans and five-minute continuity from an extracted package outside the checkout. Subsequent EOF fix is under validation |

Validation must use transient test settings and generic diagnostics. Never
publish session values, signed stream URLs, private library contents or local
machine paths. Any service outage or authentication requirement must be reported
clearly rather than hidden by an endless retry loop.
