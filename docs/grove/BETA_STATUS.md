# Grove private beta status — 2026-10-22

**Release hold: Spotify access and audible playback are not verified.**

Grove 0.1.0-beta.1 has an installable Windows x64 candidate. It is not ready
to distribute as a working Spotify player. No public release has been made.

## What is blocking release

- The live account saved a library approval. The final read-only check verified
  the account profile, but library requests hit HTTP 429 and the local cooldown
  blocked further library/search requests. No catalog checks passed. This response
  did not identify QUOTA_EXCEEDED; it remains a recurring rate limit.
- The final profile check confirmed the account is not Premium. Streaming in Grove requires
  Premium, so audible playback, pause/seek/resume and volume remain unverified.
- There is no dedicated Grove Spotify Client ID. A reliable private beta needs
  an app owner who meets Spotify's current development requirements and adds
  the testers to the allowlist. The tester does not need to share any secret.
- Two-device Spotify jams and audible Tide ↔ Grove synchronization still need
  live testing with eligible accounts. Jams remain experimental and local-network only.

Spotify's [quota modes](https://developer.spotify.com/documentation/web-api/concepts/quota-modes)
describe Premium ownership and the five-user limit for a new development app.
The [quota update](https://developer.spotify.com/blog/2026-07-23-web-api-quota-updates)
explains developer-wide quotas and the QUOTA_EXCEEDED response reason.

## Candidate checks

- 16 Grove and 15 shared-jam unit tests pass. Strict Clippy passes for both
  packages. The final optimized build and package results accompany the download.
- Native candidate reviews passed 18 feature stages, 16 jam callback stages,
  eight muted local-audio stages and a 19-view smoke run.
- Installer tests passed payload corruption/traversal checks, eight bootstrap
  rejection cases and 22 installation/rollback/uninstall scenarios.
- An isolated installation downloaded the real pinned mpv runtime, created real
  shortcuts in a temporary folder, ran native local-audio checks and uninstalled.
  Windows application registration was simulated; this is not a clean-machine test.
- Package allowlists, SHA-256 checks and a credential/private-path scan passed.
  The installer is unsigned and updates are manual.

## Remaining acceptance checks

1. Configure an eligible dedicated Spotify app and allowlist a small tester group.
2. Verify fresh consent, catalog/library access and reconnect after restarting.
3. With a Premium tester, hear a song and test pause, seek, resume and volume.
4. Verify Like/Unlike and a reversible edit to a disposable owned playlist.
5. Test installation/removal on a clean Windows account, hardware media keys,
   and output-device reconnection. Test two-device jams before promoting them.

Use Settings → Save support info for a report without account names, music
history or tokens. Do not send authorization codes or full login URLs.
