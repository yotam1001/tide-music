# Free / self-hosted Jam relay

The same small relay can support a shared Tide service or a server run by friends. A limited Render Free trial is deployed at https://tide-jam-trial.onrender.com for the separate Jams preview. It is not part of the stable Tide installer. The software is free; use an existing machine or evaluate a hosting provider's free limits before choosing infrastructure. No account credentials, audio runtime or operator secret is needed by this shared-relay binary.

## Operator setup

Build on the host or obtain the matching experimental Linux CI artifact:

```sh
cargo build --manifest-path experiments/jam/Cargo.toml --locked --no-default-features --features relay --release --bin tide-jam-relay
experiments/jam/target/release/tide-jam-relay --endpoint https://jam.example.test --port 8765
```

Replace the example domain with one you control and use a valid certificate. Run as an unprivileged service. Install `nginx-shared.example.conf` with the same exact domain and certificate paths. The backend binds only 127.0.0.1 and must be on the same machine as nginx; do not change it to a public interface or publish the backend port. Only the TLS proxy is exposed.

The proxy **overwrites** `X-Tide-Jam-Peer` with its socket's `$remote_addr`; do not forward a client-provided value, trust an arbitrary X-Forwarded-For, or expose the backend through another proxy. This direct-to-nginx template is not a CDN configuration. Missing, duplicate or malformed peer identity is rejected. Private/local relay modes reject this header entirely.

The backend groups IPv6 clients by /64, keeps only per-process keyed address fingerprints in RAM and limits each peer to two active rooms and two creations per minute. Global room, body, connection and request limits still apply. Abandoned rooms expire after two minutes; continuously updated rooms can last up to eight hours. These limits bound resources but do not promise immunity to distributed abuse. Review actual provider limits and denial-of-service protections before publishing an endpoint.

Disable request/access/body logging and temporary body buffering as in the template. No accounts, signed streams or audio are accepted in the protocol. The operator still sees network/listening metadata; this is transport TLS, not end-to-end encryption against the operator. Restarting the service drops rooms; clients pause safely and must start a fresh room.

## In Tide

Use an `experimental-jams` build. Play a YouTube song, open Jam, enter the trusted shared server endpoint, choose Start Jam, then Copy invite. Friends paste the `tide-jam:v1.` code into Join a Jam. The code contains the server, room ID and a guest capability only. Never send a private server operator key. Hosts can lock a room, replace an invite or remove a Listener. Removal revokes that membership and replaces the invite so the removed listener cannot rejoin using the old code. Existing listeners keep playing. Guests see their anonymous Listener number so friends can identify each other without sharing account names. Legacy private operator-key/file setup remains under Advanced.

For two players on one PC, the existing literal-loopback lab relay also accepts code hosting. An invite pointing to 127.0.0.1 cannot connect a different PC.

See RELEASE_PLAN.md for the public-release gates. Do not advertise an experimental artifact as the stable Tide installer.

## Hosting research (21 October 2026)

Render documents 750 free instance-hours per workspace/month and sleeps after 15 idle minutes; cold starts can take about a minute. Its included bandwidth/build limits also matter, and some overages are billed. Source: https://render.com/docs/free

Koyeb documents one Free instance with 512 MB RAM and 0.1 vCPU, sleeping after an hour without traffic. Check current eligibility, terms and traffic limits before any deployment. Sources: https://www.koyeb.com/docs/reference/instances and https://www.koyeb.com/docs/run-and-scale/scale-to-zero

Render is selected for the initial no-card trial. See `render/README.md` for the isolated container and conservative shared quota. The trial passed the container and live HTTPS checks. A free tier is not a guarantee that operating a public service at arbitrary scale will remain free. Do not add a payment method, paid plan or automatic paid scaling. Self-hosting remains the fallback. Cold-start recovery and actual deployed proxy behavior must be validated before publishing an endpoint.

## Collaborative queue (new preview builds)

Guests can browse Home, Library, playlists and search while following the host. Selecting a song or choosing Add to shared queue submits its strict YouTube video ID. Pause, seeking, skipping, queue reordering, removal and account switching remain host-controlled. Volume is local.

The relay accepts proposals from active members only: at most five per minute per member and 32 pending per room. Only the host can read or acknowledge proposals. A monotonically increasing request ID prevents repeated delivery from adding the same request twice. A full shared queue leaves requests pending until space becomes available. Removing/leaving/expiring membership removes its unprocessed requests. Audio, stream URLs and Google credentials never enter this channel.

Enqueue POSTs are sent once; a lost reply produces an unconfirmed-addition notice rather than an automatic duplicate. This feature needs an updated host, guest and relay. The existing timeline and HostView schemas are unchanged. Both PCs should use the new preview: older clients retain their 100-song validation limit. The shared queue now matches Tide's 2,000-song cap; a maximum-sized View remains below the unchanged 32 KB transport limit. Longer queues increase relay traffic, not audio caching.
