# Synchronized listening feasibility lab

**Experimental. Not included in Tide or its beta installer.**
This tests whether Tide's native playback engine can follow a host's queue and
playback clock without sharing accounts or audio. It is not a finished Jam feature;
the CLI supports an explicitly configured private HTTPS relay, and a separate
opt-in Tide developer build has native host/join/leave controls. Validation on two
physical computers with authenticated streams is still outstanding.

The lab is a separate Cargo workspace with its own lockfile. Normal Tide builds
do not link its transport or run background Jam threads. The optional native
client links protocol/client/controller code only, without the relay server.
Existing beta assets are unchanged.

## Implemented

- Private in-memory rooms, separate host/invite/member capabilities, at most eight
  guests. Only the host can publish queue/index/pause/position state or close a room.
- Shared queues containing up to 100 strict YouTube video IDs. No remote paths,
  URLs, signed streams, artwork requests or arbitrary player commands are accepted.
- Generation-aware loading, late joining, drift correction, host pause/seek/skip,
  repeated-track reloads, stale-host pausing and bounded connection retries.
- Native independent playback: each player loads its own media. The runnable lab
  uses public YouTube extraction without an account; it never reads Tide's saved
  session, updates account cookies, changes settings or registers media keys.

## Security boundary

The relay **always binds literal IPv4 loopback**. Ordinary local mode accepts
only its listener's exact Host. Private remote mode requires both an explicit
canonical HTTPS endpoint and a private operator key, behind a trusted TLS proxy
on the same machine. The client verifies certificates using rustls and Mozilla's
roots. There is no certificate bypass, arbitrary plaintext remote endpoint, public
bind switch, redirect following or environment proxy support. See the separate
[private relay guide](PRIVATE_RELAY.md); do not expose ordinary local mode.

Room and member capabilities use OS-backed randomness. Host control is never
included in an invite or poll response. Capabilities are passed in sensitive
authorization headers, not query strings or command-line arguments; Debug/errors
redact them. Tokens in owned Rust wrappers are zeroized on drop, but this is not
a claim that every HTTP/JSON allocation is erased. The backend hop is plaintext
loopback; HTTPS terminates at the proxy. This is not end-to-end encryption and
does not defend against local malware or a malicious relay operator. That operator
can see listening metadata and room credentials, but receives no Google credentials.

Requests require an exact Host, no Origin, a single protocol/authentication header
and JSON for POST/PUT. Forwarded authority headers, absolute request targets,
queries and encoded/ambiguous paths are rejected. No CORS is enabled. The server
admits at most 512 requests/second globally and limits rooms to 32, guests to eight per
room, bodies to 32 KiB, active connections to 16 and retained connection tasks to
32. Header/body/connection deadlines are 2/2/4 seconds. Create/publish rates are
bounded. Private mode authenticates creation before allocating room state; the
operator key is never placed in a guest invite or retained as a client-wide header.
Rooms expire after two hours; idle memberships after 30 seconds. Ten-second
maintenance removes expired state. Restarting the relay forgets every room.

Clients reject redirects, proxies, oversized responses and invalid incoming
states as well as outgoing data. A host heartbeat stale for six seconds pauses
guests; transport failure pauses after a three-second grace period, with at most
five failed poll attempts. Explicit leave permanently disables that follower.
Seeking is limited to ready matching generations and a three-second correction
cooldown. It is approximate synchronization, not sample-accurate audio.

See the [bounded security review](SECURITY_REVIEW.md) for the credential boundary
and remaining invite-admission, filesystem-cancellation and deployment risks.

Invite files give access to room state and guest membership. Keep them private,
share only with intended participants, and never paste them in issues/chat logs.
The required `*.jam-invite.json` suffix is ignored by Git. Use a private user
folder on a local disk, never a shared or repository folder. Native and CLI input
checks reject non-regular files and oversized inputs. Windows also rejects direct
network-share/device paths, reserved device filenames and alternate data streams.
This does not impose a deadline on arbitrary filesystem stalls; see the security
review's remaining limits. Windows files inherit that folder's
permissions; Unix creation requests mode 0600. Files are not automatically erased;
their capabilities expire when the room closes or expires.
Operator keys use a separately ignored `*.jam-operator-key` file and must never
be shared with guests. Generated Unix files request mode 0600; on Windows use a
private user folder with restrictive inherited permissions.

## Run locally

Windows native playback requires the existing checksum-verified runtime tools:

```powershell
pwsh -File scripts/setup-runtime.ps1
$env:TIDE_RUNTIME_DIR = (Resolve-Path .tools/runtime).Path
cargo build --manifest-path experiments/jam/Cargo.toml --locked
```

The lab executable is `experiments/jam/target/debug/tide-jam-lab.exe`. In separate
terminals, run:

```powershell
# Terminal 1: relay, bound only to this computer
experiments/jam/target/debug/tide-jam-lab.exe relay --port 8765

# Terminal 2: host. Queue file: 1–100 YouTube IDs, one per line, not links.
experiments/jam/target/debug/tide-jam-lab.exe host --endpoint http://127.0.0.1:8765 --queue-file <private-queue-file> --invite-file <private-folder>/test.jam-invite.json --seconds 300 --mute

# Terminal 3: independent guest player using the host-created invite
experiments/jam/target/debug/tide-jam-lab.exe join --invite-file <private-folder>/test.jam-invite.json --seconds 300 --mute
```

Replace the placeholder paths. The host refuses to overwrite an existing invite.
Runs are bounded to 5–1800 seconds. Host commands: `pause`, `resume`, `next`,
`previous`, `seek 30`, `quit`. Queue order comes from the file at startup; interactive
queue editing is not implemented in the CLI. See the optional native controls below.
Removing `--mute` enables
audio only in that lab instance. Ordinary YouTube availability limits still apply;
the lab does not promise Premium access or reconnect an account.

The client accepts canonical `https://jam.example.test` or nondefault HTTPS ports
for the private relay mode. It rejects URL repairs/normalization such as whitespace,
backslashes, credentials (including empty userinfo), dot paths, alternative IPv4
notation, uppercase hostnames and an explicitly written default `:443`. A final
root slash is optional. Invites select a server to contact: accept them only from
someone you trust. Joining never reads or sends an operator key.

## Optional native Tide controls

```powershell
cargo run -p tide-music --locked --features experimental-jams
```

This is a developer build, not the normal installer. Open **Jam** in the header.
Hosting shares the playing YouTube song and up to 99 upcoming tracks. Enter the
trusted relay endpoint; for HTTPS choose its operator key, then save a **new**
private guest invite. Send only that invite to intended guests. Joining opens a
trusted invite file, without reading the operator key. Setup of the private relay
and its TLS proxy remains a separate operator task.

Guests use their existing **local** playback runtime/account. No account data or
audio enters the coordination worker. Local transport, queue edits, autoplay,
media keys and account controls are guarded while following; volume stays local.
Leave remains available in mini mode. If one song is unavailable locally, the
guest pauses and waits without repeatedly retrying it; the host's next play can
load normally without rejoining. Relay/protocol failures still use their bounded
failure handling. The original queue is kept in memory and
restored paused on Leave; the guest queue is never saved to settings. Changing
the saved local account ends the Jam before applying the account change. A failed
initial connection restores the local queue paused. Track failures, stale hosts
and terminal network errors do not silently switch to local autoplay.

Networking runs in one on-demand worker with bounded latest-state mailboxes.
Leaving cancels delivery immediately; a retiring worker finishes bounded HTTP
cleanup before another session can start. App shutdown joins it; in-flight HTTP
plus cleanup can take up to about six seconds. If a safety pause/stop cannot enter
the playback command queue, the engine is shut down through its separate shutdown
channel before restoring the local player. A local source or invalid host queue
ends hosting rather than sharing arbitrary paths or streams.

Upcoming songs unknown to the guest may initially display their YouTube IDs;
native metadata replaces the current title when available. This is an experimental
limitation, not the final shared-library UX. Synchronization remains approximate.

```powershell
cargo test -p tide-music --locked --features experimental-jams
cargo clippy -p tide-music --all-targets --locked --features experimental-jams -- -D warnings
cargo run -p tide-music --locked --features experimental-jams -- --smoke-jam-ownership
```

The explicit ownership probe uses transient synthetic state, no provider/account,
and never shows its window or loads audio. It exercises native callbacks and media
commands directly, including disabled-control bypasses, local volume and queue
restoration. Other tests render/input-test full, compact, minimum and mini layouts
entirely in software, with no desktop window. These checks do not establish two-PC
or live authenticated YouTube behavior.

The separate Windows file-input probe uses an isolated named pipe rather than a
real invite or key. It must reject the pipe before opening it and cancel without
blocking. Build the explicit ignored test, then run its supervised gate:

```powershell
New-Item -ItemType Directory -Force .local | Out-Null
cargo test -p tide-music --locked --features experimental-jams --bin tide-music --no-run --message-format=json | Set-Content .local/jam-pipe-build.jsonl -Encoding utf8
if ($LASTEXITCODE -ne 0) { throw 'Probe build failed' }
python experiments/jam/check-private-pipe.py --cargo-artifacts .local/jam-pipe-build.jsonl
```

This does not read a session, create a listener on the network or load audio.

## Validation

```powershell
cargo fmt --manifest-path experiments/jam/Cargo.toml --all -- --check
cargo test --manifest-path experiments/jam/Cargo.toml --locked
cargo clippy --manifest-path experiments/jam/Cargo.toml --all-targets --locked -- -D warnings
```

The transport suite exercises the real backend through a generated-certificate
TLS proxy fixture: room creation, separate guest join, shared queue, role isolation,
leave/close, untrusted/expired/wrong-name certificates, plaintext impostors and
redirects that must not contact a second server. Certificate roots are injected
only in the private unit-test module; production exposes no custom-root option.
Other checks cover canonical endpoints, duplicate security headers, forwarded
authority spoofing, unauthenticated allocation attempts, invalid/missing operator
files, safe key generation and strict CLI option parsing. This fixture does not
validate an actual nginx installation, Internet reachability or physical devices.

The explicit Windows native test requires a **silent local WAV at least 60 seconds
long** and trusted mpv in `TIDE_RUNTIME_DIR`. It runs two muted native players and a
real loopback relay, without an account, UI, SMTC or settings access:

```powershell
$env:JAM_NATIVE_FIXTURE = '<absolute-path-to-silent-fixture.wav>'
cargo test --manifest-path experiments/jam/Cargo.toml --locked --test native -- --ignored --nocapture
```

On 2026-10-16 the native test passed late join, an actually injected drift,
pause/resume, seek while paused, repeated IDs with a new playback generation,
the next queued ID, stale-host pause, heartbeat recovery and room closure. It took
16.16 seconds and issued six bounded correction seeks. Reported synchronized
checkpoints sampled 0–207 ms drift. These are sparse samples on one computer, not
a maximum-drift guarantee. The stale-host checkpoint intentionally has a paused
guest and advancing host and is not a synchronization measurement.

A second run using the lab-only pinned mpv fixture also passed in 16.22 seconds,
with six corrections and synchronized checkpoint samples of 0–212 ms. Neither
run supplies evidence about a remote network or real YouTube extraction.

The native test maps its two fixed IDs to the silent fixture **inside test code
only**. The runnable lab always constructs fixed-domain YouTube URLs from strict
IDs. Tests prove the transport/controller/native-clock paths; they do not prove
multi-computer Internet synchronization, simultaneous YouTube extraction,
buffering under real networks, Premium playback or long sessions.

The dedicated CI job downloads only the existing checksum-pinned mpv asset via
`prepare-native.ps1`, preserving companion DLLs/licenses. It verifies the exact
reviewed SHA-256 rather than querying release metadata, avoiding shared-runner
unauthenticated API rate limits. It does not change production runtime setup.
The initial CI attempts stopped before native playback: one hit that API rate
limit, and one had an invalid `$LASTEXITCODE` check after a successful PowerShell
script. The lab setup no longer uses that API/check; failed jobs remain recorded.
Subsequent runner diagnostics established that hosted Windows has no playback
device (WASAPI failed; a direct null-sink fixture succeeded). CI therefore uses
`--features headless-native` to opt into the playback crate's compile-time-only
`jam-lab-null-audio` seam. That test constructor accepts no account parameters;
normal constructors still use the system output. Nothing in a room, invite,
environment setting or ordinary Tide UI can select the null sink. This exercises
real mpv decoding, IPC and timing without claiming physical audio-device coverage.
The normal-device tests above remain separate evidence, not replaced by CI.

On 2026-10-16 an additional native test passed with a test-only loopback fault
proxy, two muted system-audio mpv players and a silent local WAV. It injects
75/250 ms delay in each direction, drops actual guest HTTP requests during an
outage, verifies pause/recovery with the same membership, and exercises seven
plays. Both native tests passed together in 38.59 seconds. A separately sampled
22.45-second run passed with 32 dropped requests and eight correction seeks;
synchronized checkpoints sampled 10–126 ms drift. These samples are not a
maximum drift guarantee. The sampler confirmed successful exit and all observed
helpers ended. This uses local decoding and simulated network faults, not live
YouTube, two physical computers or a long-duration listening session.

## Before enabling Jams in Tide

1. Validate the private HTTPS deployment on a real host, including its TLS proxy,
   resource/abuse controls, certificate lifecycle and costs. No Tide-operated public
   service is provided. The private transport has a code review and automated TLS
   tests; those do not constitute an audit of any deployed service.
2. Test two physical computers with real independently authenticated streams,
   extraction delays, regional/unavailable tracks, asymmetric latency, outages
   and repeated host/guest reconnects. Define acceptable measured drift.
3. Validate native host/join/leave, queue ownership, buffering, safety shutdown and
   account changes against real participants. Controls and offline ownership/
   rendering tests are implemented, but physical/native/service behavior still
   needs end-to-end validation before enabling the feature by default.
4. Measure complete process-tree resource use and obtain a separate security
   review of the public transport before advertising Internet rooms.

Until these pass, keep this lab isolated and the production feature on the roadmap.
