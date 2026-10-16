# Synchronized listening feasibility lab

**Experimental, loopback only. Not included in Tide or its beta installer.**
This tests whether Tide's native playback engine can follow a host's queue and
playback clock without sharing accounts or audio. It is not a finished Jam feature;
friends on different computers cannot use this implementation yet.

The lab is a separate Cargo workspace with its own lockfile. Normal Tide builds
do not compile it, start a listener, add its dependencies or run background Jam
threads. Existing beta assets and account files are unaffected.

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

The relay and client **only accept literal IPv4 loopback**. There is no public
bind switch or remote-client option. Do not expose this through port forwarding,
a tunnel or a reverse proxy. Public deployment has not been reviewed or tested.
Internet support needs a separate authenticated TLS transport/deployment design;
removing the loopback checks is not an implementation of that design.

Room and member capabilities use OS-backed randomness. Host control is never
included in an invite or poll response. Capabilities are passed in sensitive
authorization headers, not query strings or command-line arguments; Debug/errors
redact them. Tokens in owned Rust wrappers are zeroized on drop, but this is not
a claim that every HTTP/JSON allocation is erased. The transport is plaintext
loopback; it is not end-to-end encryption and does not defend against local malware.

Requests require an exact Host, no Origin, the protocol header and JSON for
POST/PUT. No CORS is enabled. The server limits rooms to 32, guests to eight per
room, bodies to 32 KiB, active connections to 16 and retained connection tasks to
32. Header/body/connection deadlines are 2/2/4 seconds. Create/publish rates are
bounded. Rooms expire after two hours; idle memberships after 30 seconds. Ten-second
maintenance removes expired state. Restarting the relay forgets every room.

Clients reject redirects, proxies, oversized responses and invalid incoming
states as well as outgoing data. A host heartbeat stale for six seconds pauses
guests; transport failure pauses after a three-second grace period, with at most
five failed poll attempts. Explicit leave permanently disables that follower.
Seeking is limited to ready matching generations and a three-second correction
cooldown. It is approximate synchronization, not sample-accurate audio.

Invite files give access to room state and guest membership. Keep them private,
share only with intended participants, and never paste them in issues/chat logs.
The required `*.jam-invite.json` suffix is ignored by Git. Use a private user
folder, never a shared or repository folder. Windows files inherit that folder's
permissions; Unix creation requests mode 0600. Files are not automatically erased;
their capabilities expire when the room closes or expires.

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
queue editing and a native Jam UI are not implemented. Removing `--mute` enables
audio only in that lab instance. Ordinary YouTube availability limits still apply;
the lab does not promise Premium access or reconnect an account.

## Validation

```powershell
cargo fmt --manifest-path experiments/jam/Cargo.toml --all -- --check
cargo test --manifest-path experiments/jam/Cargo.toml --locked
cargo clippy --manifest-path experiments/jam/Cargo.toml --all-targets --locked -- -D warnings
```

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

## Before enabling Jams in Tide

1. Design and review trusted TLS coordination, deployment abuse controls and costs;
   decide whether to self-host or operate a service. Share no Google credentials
   or audio with that service. Disclose its visibility of room/listening metadata.
2. Test two physical computers with real independently authenticated streams,
   extraction delays, regional/unavailable tracks, asymmetric latency, outages
   and repeated host/guest reconnects. Define acceptable measured drift.
3. Integrate host/join/leave, queue ownership and buffering status into the native
   UI; restore the guest's original local queue on leave; gate local media keys
   while following the host. Do not treat this CLI as finished app UX.
4. Measure complete process-tree resource use and obtain a separate security
   review of the public transport before advertising Internet rooms.

Until these pass, keep this lab isolated and the production feature on the roadmap.
