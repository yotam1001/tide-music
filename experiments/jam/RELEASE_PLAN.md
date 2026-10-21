# Jams release plan

Goal: friends on separate PCs listen to the same queue with synchronized playback, with a simple Start / Copy invite / Paste and Join flow.

## Cost and distribution

Tide and Jams stay free. No subscription, payment, third-party account key or paid-hosting commitment is introduced. A shared relay is the preferred default only if it can be operated sustainably without charging users. Self-hosting is a first-class fallback. No shared service has been provisioned or deployed. Do not put server operator keys in the app or invite codes.

## Current development batch

- Integrate public 0.1.3 UI, artists, output recovery, updates and performance changes into the opt-in Jams branch.
- Code-based hosting/joining and explicit Copy invite, with file/operator setup under Advanced.
- Standalone Linux-capable shared relay with TLS-proxy configuration; bounded anonymous room creation, per-peer and global limits, no remote plaintext listener.
- Guest-only invite codes, strict URL/field/size validation, account-independent coordination.
- Host-only participant removal, room lock/unlock and invite replacement. Removing a listener revokes their membership and the old invite, preventing reuse of that code to rejoin. Participants use anonymous Listener IDs, not Google account names.
- Up to eight-hour active rooms; abandoned rooms reclaimed after two minutes. Paused hosts continue heartbeats.
- Hosted tests for real TLS protocol, independent native players, code flow, role isolation, quotas, fake-clock four-hour lifetime and UI ownership.

## Gates before a public release

1. All matching hosted tests and normal-installer isolation checks pass.
2. Validate the new host moderation/admission controls end to end in the native UI. Invite reuse can still occupy slots until the host removes the listener/locks the room; consider explicit host approval before a broad untrusted public launch.
3. Shared queue UX: decide and implement which guest additions are permitted while retaining host authority over playback and timing.
4. Measure transitions, late joins, seek/pause, unavailable tracks and reconnect behavior with two independent authenticated players on distinct PCs/networks. Include long sessions and sleep/wake recovery.
5. Select free shared hosting (verify current limits and terms) or document a usable self-host path. Validate the actual deployed TLS/proxy, resource limits and disabled sensitive logs. Set a default endpoint only after this.
6. Inspect native layouts at full/compact/mini sizes, keyboard access and pending/error states. Verify failed joins and Leave preserve local queue/account/volume.
7. Check account/stream isolation and invite privacy in source, captured protocol, logs, package contents and installer setup. No Google authentication data or signed media URLs can pass through the relay.
8. Package a new private trial for owner review. Jams PR remains draft/unmerged and stable installer excludes it until approved.

A fake-clock multi-hour test proves expiry logic, not a real multi-hour listening session. Local/synthetic fixtures do not establish two-PC authenticated playback.
