# Experimental Jam security review

Reviewed on 2026-10-16. This is a bounded source review and synthetic test record,
not a guarantee that Jams or a future deployment are free of security issues.
An independent review agent inspected the source; this is not a professional
deployment audit or certification.
No real session, browser profile, clipboard, private invite or operator key was
read for the independent review. No relay was deployed.

## Findings and boundaries

The independent review found no confirmed path for YouTube cookies, session
headers or signed media URLs to enter the Jam relay. Native hosting extracts
strict video IDs; the protocol accepts IDs, queue order and timing, with unknown
fields rejected. Each guest resolves audio through its own local runtime/account.
The Jam client is separate from the provider and does not inherit its credentials.

The client verifies HTTPS certificates, disables redirects and inherited proxies,
and bounds requests and replies. Host, invite and member capabilities have
separate roles; guest credentials cannot publish or close a room. Native invite
files contain the endpoint, room and guest invite, without host or operator keys.
Account changes leave the Jam. These boundaries are covered by source inspection
and the existing role, HTTP/TLS, protocol and native ownership tests. The
independent reviewer inspected those tests; the parent ran them separately.

The relay operator intentionally learns participants' IP addresses, song IDs,
queue/timing and room capabilities. HTTPS protects transport to that operator;
it does not hide this metadata from them. Invites select an endpoint: accept
files only from someone you trust and use a relay operator you trust. Google
credentials and audio are not part of that coordination protocol.

## Remaining risks

- **Invite reuse can fill a room.** One holder of a bearer invite can acquire all
  eight guest memberships and keep them active by polling. This is bounded
  availability abuse, rather than host control or account disclosure. The current
  prototype is for trusted friends; per-recipient invites or host removal would
  be needed for stronger admission control. Ending the room revokes its tokens.
- **General filesystem deadlines remain incomplete.** The direct Windows
  pipe/device/share case is hardened as described below. Mapped network drives,
  parent reparse traversal, Unix precheck/open races and a stalled filesystem are
  not covered by that narrowly demonstrated fix. Use ordinary files in a private
  local folder. The documented bounded HTTP cleanup time is not a universal
  filesystem deadline.
- **Deployment has not been validated.** The nginx template disables logging and
  buffering, but an actual proxy/host may introduce logs, temporary request files,
  different routing or network exposure. Follow the real-host checklist in
  [PRIVATE_RELAY.md](PRIVATE_RELAY.md), including TLS, Host, slow-client, resource
  and logging checks. No public-service security claim follows from the template.

Jams remain opt-in and unmerged for the user's trial. These review results do not
close two-PC authenticated playback, long-session or deployed-relay release gates.

## File-input follow-up

A synthetic Windows named-pipe server reproduced the production guest worker
blocking after cancellation: its file reader connected to the pipe and waited
for data indefinitely. Only the isolated test process was stopped. This verifies
the reader behavior, not a remote exploit or normal Windows-picker attack.

Native and CLI reads now share a size-bounded, zeroizing file reader. On Windows,
it rejects direct UNC/device namespaces, reserved device names and alternate data
streams before filesystem access. Both platforms check that the selected input
is an ordinary file before opening and recheck the opened handle and size.
Windows additionally opens the selected final reparse point without following it,
rejects its reparse attributes, and permits only read sharing while reading.
Native invite and CLI private-file creation use the same lexical path guard and
retain exclusive creation. This adds no account access or relay protocol fields.

After hardening, the same named-pipe probe exits successfully without connecting
to the pipe. CI runs this isolated native probe with a five-second supervisor
deadline, and requires the exact test to exist so a zero-test run cannot pass.
The independent follow-up identified that missing-test weakness; a real unrelated
Rust test binary reproduced the old false pass and is rejected by the corrected
gate. The remaining filesystem limitations above are deliberately not represented
as fixed.

## Shared relay development follow-up (21 October 2026)

The new shared relay is not deployed. It retains literal-loopback binding behind
an explicit HTTPS proxy, certificate validation, no CORS/redirects/proxy inheritance,
strict video IDs, body/connection limits and role-separated capabilities. Shared
room creation distributes no operator key to clients. Its mandatory peer header
must be overwritten by the same-machine TLS proxy from the socket address; it is
not a trust contract for arbitrary CDN/forwarding chains. Local/private modes
reject the peer header. Global and per-peer quotas bound room allocation; IPv6
addresses are grouped by /64 and only keyed process-lifetime fingerprints remain
in the store. Distributed abuse can still exhaust the finite room pool.

Invite codes have a 2048-byte input bound, strict schema and canonical endpoint
validation. New code paths avoid invite file reads/writes. Explicit Copy invite
uses the system clipboard; OS clipboard history/sync is outside Tide's control.
The host capability is absent from invitations and guest views. Tests exercise
these boundaries with generated fixture credentials, including a real TLS proxy.
This follow-up is a source/test-design record, not an independent audit or a
successful production deployment. Host-only removal now revokes the member and replaces the invite atomically;
lock/unlock and explicit invite rotation are implemented. Existing listeners keep
their capabilities. A bearer holder can still occupy slots before the host acts,
so explicit approval remains a possible stronger public-launch gate. No Google
names are exposed: the roster uses room-local anonymous listener IDs.
Real-host release gates remain open; see RELEASE_PLAN.md.


## Artwork and guest skip follow-up (candidate)

Jams do not upload audio, images, installers, cookies, or executable files. The
queue protocol accepts strict eleven-byte YouTube video IDs; each client resolves
and plays the song through its own local account. New fallback thumbnails are
constructed locally as a fixed HTTPS `i.ytimg.com` URL from that validated ID.
They reuse the existing public-image decoder and its response, pixel, queue and
cache limits. No peer-supplied image URL or new relay image-storage path is added.
This reduces the exposed input surface; it is not a claim that image or media
parsers can never contain vulnerabilities.

The optional `/skip` operation accepts a play ID and validated video ID only.
Only an active member may submit it; only the host may read it. The relay retains
at most one pending skip per room, coalesces requests for the same play, shares
the five-per-minute member budget with queue additions, and clears a request when
its submitting member leaves/expires/is removed or the published play changes.
The native host independently checks both identifiers and handles that play at
most once. Guests never publish authoritative timelines, shell commands, file
paths, media URLs or account headers. There is no automatic POST retry after an
ambiguous network result. Old relays report the unsupported operation without
ending synchronized playback. Both the relay and host need this candidate for
shared skipping; it is not an installed-release claim.

The desktop updater is separate from Jams. The previous review identified that
its SHA-256 came from the same GitHub release API as installer metadata. The 0.4.1
candidate now requires independent Ed25519-signed installer identity metadata,
verified against a public key built into the app. Missing, invalid, mismatched
or expired signatures stop the update; there is no unsigned fallback. The local
DPAPI-protected signing key is outside the repository and CI. Existing clients
through 0.4.0 cannot gain that check before their first upgrade; see
[the update trust model](../../docs/IN_APP_UPDATES.md). Jams do not invoke the
updater or supply executable download locations.
