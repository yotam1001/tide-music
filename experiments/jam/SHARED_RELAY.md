# Free / self-hosted Jam relay

The same small relay can support a shared Tide service or a server run by friends. No service has been deployed and no default public endpoint is promised. The software is free; use an existing machine or evaluate a hosting provider's free limits before choosing infrastructure. No account credentials, audio runtime or operator secret is needed by this shared-relay binary.

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

Use an `experimental-jams` build. Play a YouTube song, open Jam, enter the trusted shared server endpoint, choose Start Jam, then Copy invite. Friends paste the `tide-jam:v1.` code into Join a Jam. The code contains the server, room ID and a guest capability only. Never send a private server operator key. Legacy private operator-key/file setup remains under Advanced.

For two players on one PC, the existing literal-loopback lab relay also accepts code hosting. An invite pointing to 127.0.0.1 cannot connect a different PC.

See RELEASE_PLAN.md for the public-release gates. Do not advertise an experimental artifact as the stable Tide installer.
