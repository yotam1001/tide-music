# Private HTTPS relay experiment

This is an operator/developer path for the isolated Jam CLI, not a one-click Tide
feature. No service is deployed or paid for by building it. Keep the ordinary
beta installer separate. A real two-computer listening trial and native host/join
controls are still required before advertising Jams.

## Trust and data

Each participant resolves and plays media independently. The relay sees video IDs,
queue order, playback position and room capabilities. HTTPS protects the connection
to its trusted proxy, not against the proxy/relay operator. No audio or Google
cookies/headers are transmitted to it. The lab itself uses public extraction;
it does not import Tide accounts or grant Premium access to other participants.

Only trusted hosts receive the **operator key**, which grants creation of rooms
on the chosen relay. Each room has a different host capability. Guests receive a
separate invite file and gain only read/follow membership. An invite or member token
cannot create rooms or control playback; the operator key cannot control an
existing room. Invites may contact their specified HTTPS endpoint, so distribute
them privately and check which operator you trust.

## Backend contract

Build the lab as described in [README](README.md). Create a key in a private user
folder outside the checkout (substitute the placeholder paths):

```text
tide-jam-lab keygen --create-key-file <private-folder>/relay.jam-operator-key
tide-jam-lab relay --port 8765 --public-endpoint https://jam.example.test --create-key-file <private-folder>/relay.jam-operator-key
```

The name `jam.example.test` is a documentation placeholder. Real remote clients
need your own hostname and a certificate trusted by Mozilla's public roots.
Default port 443 is omitted from the endpoint. Generate a separate key for every
relay. The key is generated using OS randomness, never printed, and never
overwrites an existing file. Windows inherits directory permissions; Unix creation
requests 0600. Keep the key and invite files out of backups or shares you do not
trust. The matching filename extensions are Git-ignored, but that is not encryption.

Missing/malformed endpoint or key configuration stops startup before binding.
The backend still binds **127.0.0.1 only**. The TLS proxy must run on the same
machine/network namespace and preserve the exact public Host. It must remove
`Forwarded`, `X-Forwarded-Host` and `X-Forwarded-Proto`: the backend rejects them,
including when a proxy inserts them automatically. Do not strip Origin or the Jam
protocol header to bypass their checks. Never rewrite unrecognized Host values to
the accepted authority. Never expose backend port 8765 through a router/tunnel.

Use a dedicated, bounded proxy with certificate/hostname verification, no request
logging/tracing/caching, small bodies/headers, short deadlines, connection limits
and request-rate limits. [nginx-private.example.conf](nginx-private.example.conf)
shows that contract using standard nginx modules. It is a standalone **template**,
not a verified deployment: replace the domain and certificate locations, verify
with `nginx -t`, and test its actual behavior before granting remote access. It
discards logs because even an error log may include a room URL. Use aggregate
metrics without capabilities if operating it long-term. Disabling logs has an
operational cost; do not enable debug capture to troubleshoot real session traffic.

The template preserves Host and forwards to plaintext loopback, as described by
the [nginx proxy documentation](https://nginx.org/en/docs/http/ngx_http_proxy_module.html),
and uses the [standard request limiter](https://nginx.org/en/docs/http/ngx_http_limit_req_module.html).
Its fixed worker connection budget also bounds connections awaiting headers; this
does not promise resistance to a distributed denial-of-service attack.

## Hosting and joining

Give the operator key only to trusted room hosts through a private channel. The
host selects its relay explicitly; an invite can never supply a creation key:

```text
tide-jam-lab host --endpoint https://jam.example.test --create-key-file <private-folder>/relay.jam-operator-key --queue-file <private-folder>/queue.txt --invite-file <private-folder>/friends.jam-invite.json --seconds 300
tide-jam-lab join --invite-file <private-folder>/friends.jam-invite.json --seconds 300
```

Run the second command on the guest's machine after privately sending that guest
only `friends.jam-invite.json`. Each needs trusted native runtime tools. No browser
needs to stay open. The first command publishes only the queue's strict video IDs
and clock. Existing CLI limits/commands and media availability apply. Local file
or signed media URLs cannot be shared. There is no automatic hosted relay fallback.

Close a room with the host's `quit` command. Closing or expiration revokes every
invite/member capability for that room. Restarting the relay forgets all rooms;
rotating its operator key requires a restart and distributing the replacement only
to intended hosts. Delete obsolete private files yourself. Host process crashes
pause followers after the stale heartbeat threshold; they do not delete files.

## Required real-host checks

Before a remote trial, verify a valid certificate succeeds and an invalid
certificate fails; HTTP does not reach the backend; an unknown authority, Origin,
missing/wrong operator key and oversized/slow requests fail. Forwarded authority
headers must never override routing: this template strips them, while direct
backend requests containing them are rejected. Confirm logs, traces, caches and temporary body files contain no requests,
responses or capabilities. Test shutdown, room expiration and key rotation.

Automated tests cover the backend/client TLS contract using generated certificates
and a local TLS fixture. They do not establish that this proxy template has been
installed correctly, nor measure two-machine drift, live YouTube buffering or
long-session behavior. No remote deployment or those manual checks have been
performed as part of this implementation.
