# Experimental Jam security review

Reviewed on 2026-10-16. This is a bounded source review and synthetic test record,
not a guarantee that Jams or a future deployment are free of security issues.
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
- **File-input cancellation is incomplete.** Native private-file reads are
  synchronous and bounded by size, but lack a regular-file check or read deadline.
  A blocked share/device input could strand the worker and its joining destructor.
  This was a source-level hardening observation, not a reproduced Windows-picker
  exploit. Use ordinary files in a private local folder; the documented bounded
  HTTP cleanup time does not bound such filesystem operations.
- **Deployment has not been validated.** The nginx template disables logging and
  buffering, but an actual proxy/host may introduce logs, temporary request files,
  different routing or network exposure. Follow the real-host checklist in
  [PRIVATE_RELAY.md](PRIVATE_RELAY.md), including TLS, Host, slow-client, resource
  and logging checks. No public-service security claim follows from the template.

Jams remain opt-in and unmerged for the user's trial. These review results do not
close two-PC authenticated playback, long-session or deployed-relay release gates.
