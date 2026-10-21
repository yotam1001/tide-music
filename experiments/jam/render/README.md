# Free Render trial

Deploy only the experimental relay, not the Tide player or installer. Use the
Free compute plan and do not add a payment method. Render suspends free services
when included bandwidth is exhausted without a payment method; it sleeps after
15 idle minutes and can take about a minute to wake. This is a small trial, not
a production availability promise.

1. In Render create a Web Service from the public Tide repository.
2. Select branch `codex/jam-feasibility`, runtime Docker, Free, Frankfurt.
3. Keep the repository root as build context and set the Dockerfile path to
   `experiments/jam/render/Dockerfile`. Set health check path `/healthz`.
4. Disable automatic deployment. Do not add a database, disk, secrets or card.
5. Deploy only after the matching `render-container` CI check passes.
6. Use the assigned `https://<service>.onrender.com` address in experimental
   Tide's Jam panel. Each friend imports their own YouTube session locally.

The equivalent Blueprint is `experiments/jam/render/render.yaml`; using a public
repository URL does not require granting Render access to private repositories.
The Docker build context is allowlisted in the root `.dockerignore` and the final
image contains just the relay, nginx, their runtime libraries and startup script.
It does not contain the app, browser companion, credentials or music.

## Trust and resource boundaries

Render terminates public HTTPS before forwarding to the container's single HTTP
port. nginx listens there; the Rust backend still binds only 127.0.0.1:8765.
This configuration is specific to Render, not a publicly exposed plaintext
self-host setup. Do not expose port 8765 or use this image behind an arbitrary
proxy or on an internet-facing VM without an appropriate TLS setup.

The trial deliberately ignores all incoming forwarded/client-IP headers. nginx
overwrites `X-Tide-Jam-Peer` with one fixed quota identity, so all visitors share
the existing limit of **two active rooms and two creations per minute**. This
avoids trusting undocumented proxy IP behavior; it also means this deployment
is unsuitable for a broad launch. A host can have eight listeners per room.
Per-request body limits, global connection/rate limits, strict host/origin checks
and room expiry still apply. A malicious visitor can consume the shared quota;
do not advertise the trial endpoint publicly.

Application access/body logging is disabled. Render still operates the upstream
proxy and can observe connection metadata; do not treat this as end-to-end
encryption against the host/provider. No account cookies or signed audio URLs
are part of the Jam protocol.

The startup supervisor stops both processes if either fails. `/healthz` exposes
only a fixed readiness marker. Tide waits up to about 90 seconds for this marker
on Render endpoints, with no credentials and without retrying room-creation
POSTs. Cancel/Leave interrupts the wait. A restart loses active rooms; guests
pause safely and must use a new invite. No artificial keep-alive traffic is used
to avoid Render's sleep policy.

Sources: https://render.com/docs/free, https://render.com/docs/web-services,
https://render.com/docs/environment-variables.
