# Release readiness

Tide is not yet verified ready for a stable release. The following gates define
the remaining work; individual unit tests or short startup probes are insufficient.

| Gate | Required evidence | Current evidence |
| --- | --- | --- |
| Continuous playback | A sustained muted live session with multiple natural track transitions and repeated skip/pause/seek actions, with no unexplained stop | Revision 5809a9c passed 30 minutes signed in with seven natural transitions, pause/resume, skip and seek using the same imported session. EOF preparation fix reduced sampled natural handoffs to 250–750 ms. One-hour follow-up on 8c86152 is pending |
| Recovery | Native fault injection verifies helper loss, read interruption, loading/stall bounds, resume position, pause intent, cancellation and retry limits | Native probe passed: process loss, truncated HTTP stream, resume/pause/seek intent, retry exhaustion and cancellation; live service soak still pending |
| Recommendations | Signed-in radio appends, advances at EOF and refills repeatedly; temporary request failure retries without treating it as expired sign-in | Nine recommendation refills passed in the latest 30-minute signed-in run. A targeted live probe removed all upcoming rows and verified refill plus decoded EOF continuation. Service-outage recovery under real requests remains to be verified |
| Resource bounds | Process-tree memory samples during playback and transitions; helpers exit after Stop/Drop; queue, artwork and metadata stay bounded | Latest thirty-minute signed-in process tree: median working set 69.77 MiB, p95 76.77, peak 420.05; median private bytes 52.23 MiB, peak 360.29. Extraction peaks and gaming impact need further work |
| UI controls | Actual pointer/key checks for slider release, live volume, queue drag, modal isolation, Escape and media controls | Focused native harness passed 11 checks for sliders, accessibility, modal focus and Escape. Queue pointer drag and long edge-autoscroll passed without changing playback generation. Windows broker delivered play, pause, next and previous exactly once to an unfocused test window; media-session cleanup passed. Production startup registration was fixed and actual production broker Play/Pause/Resume/Next/Previous passed with decoded local audio. Normal close during local playback and extractor loading passed, with owned helpers removed. Physical Fn keys and closing during authenticated playback remain to be checked |
| Rendered layouts | Review full, compact, mini, dark/light, overlays, minimum sizes, resize boundaries and scaled display layouts | Focused harness layouts reviewed at minima, queue docking threshold and simulated 150%/200% rendering; actual monitor DPI migration pending |
| Accounts | Identity changes clear private preview state; expired sessions give an actionable prompt; network errors do not falsely invalidate a verified account | Revision 5e49e9b persisted sixteen native updates and passed a fresh account library check after thirty minutes. Its session came from private Zen; regular Zen could remain open. Weekly session lifetime and direct profile reconnect remain unproven |
| Open source package | Portable documented installation, license, privacy/security notes, dependency versions and reproducible checks; fresh remote privacy audit; CI passes | Revision 5e49e9b passed both Windows CI jobs, fresh public-clone privacy audits, ZIP payload/checksum/privacy scans and five-minute continuity from an extracted package outside the checkout. The EOF fix passed its live run; the latest media-startup fix passed fresh public-clone privacy audits. Latest CI and portable package checks remain pending |

Validation must use transient test settings and generic diagnostics. Never
publish session values, signed stream URLs, private library contents or local
machine paths. Any service outage or authentication requirement must be reported
clearly rather than hidden by an endless retry loop.
