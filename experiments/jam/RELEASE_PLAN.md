# Tide 0.2.0 Jams release checks

The owner confirmed synchronized listening with a friend and approved integration
into the stable installer on 22 October 2026. Hosting/joining remains an explicit
user action; normal playback starts no Jam networking worker.

## Candidate behavior

- Start Jam / Copy invite / Paste and Join using the preconfigured free relay.
- Guests browse their own music and add video IDs to the shared queue using +.
- Host keeps transport, queue ordering/removal and listener moderation controls.
- Queue additions are limited to five/minute/member and 32 pending/room, with
  host-only acknowledgements, replay protection and membership cleanup.
- Up to 2,000 songs fit below the existing 32 KB message limit.
- Ending the hosted Jam preserves playback and account state. Guest Leave
  restores the original local queue paused. Browsing failures name their view.
- Google credentials, stream URLs and audio never enter the relay protocol.
- The relay remains free, with two simultaneous shared rooms, a bounded wake
  flow and an Advanced self-hosted-server alternative. No paid scaling/card.

## Checks before publishing the candidate

1. Matching standard Windows and Jams CI, native ownership/input/rendering tests,
   TLS/proxy/container tests and staged/fresh-remote privacy scans pass.
2. Verify the normal installer actually includes Jams, performs clean install,
   update and removal, preserves account/queue settings and packages licenses
   for the production dependency graph.
3. Verify current live HTTPS guest addition, role isolation, acknowledgement,
   membership removal, queue/message bounds and free-server quota behavior.
4. Inspect the candidate native UI at full, compact and mini sizes and preserve
   existing Tide branding. No blank/dim Jam backdrop.
5. Merge only the verified candidate, publish its installer and checksums as a
   stable GitHub release, verify anonymous downloads and stable update discovery.
6. Publish the authorized release tweet with its generated graphic after the
   public download is available.

The next feature release is separate: likes/dislikes, owned-playlist editing and
real listening-history feedback. Spotify support requires its own API/playback
and account validation before implementation or marketing.
