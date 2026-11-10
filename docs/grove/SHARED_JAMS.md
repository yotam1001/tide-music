# Shared jams: nearby prototype

Updated Tide and Grove can also join each other's rooms. Cross-service guests
confirm a matching recording before following; see [Tide + Grove jams](../CROSS_SERVICE_JAMS.md)
for the experimental setup and limits. Both apps need protocol version 2.

Grove's shared jams prioritize everyone listening on their own device. The host
shares track, play/pause and position updates; each guest uses their own Spotify
Premium playback approval to follow. Audio and Spotify credentials never pass
through the room connection. These are Grove rooms, not official Spotify Jam
rooms or invitations.

## Try it

1. On each computer, launch Grove and complete both approvals on **Account**
   using that listener's own account. Spotify generally permits one active stream
   per account; use separate accounts for independent listeners.
2. On the host, open **Shared jams**, enter a display name, then choose
   **Start a jam**. Grove selects a private local address when available.
   **Network options** allows a manual address override. A warning appears if
   the selected address is `127.0.0.1`, which only works on the same computer.
3. Copy the invitation from the host's field and paste it into **Shared jams**
   on each guest's computer. Enter a display name and choose **Join jam**.
4. Play a Spotify track on the host. Guests follow automatically. Volume remains
   independent; **Stop following** gives a guest independent playback and
   **Follow host** resumes synchronization.
5. Everyone can add individual track links; Grove looks up the song name.
   Network options also exposes an optional manual name. The host
   uses **Play next shared track** to start the first waiting entry. Guests can
   remove their own contributions; the host can remove any waiting or
   unconfirmed entry. Leaving preserves contributed tracks for the remaining room.

The host needs to remain open and reachable. Closing or ending the room disconnects
guests and pauses guests who were following. A lost connection requires manually
joining again. No firewall settings are changed automatically; Windows may require
allowing the app on your private network.

## Synchronization behavior

The host publishes a playback sample once per second. A guest estimates the
host's position using the age of the host sample, a bounded connection latency
estimate, and local elapsed time. The two computers do not need matching wall
clocks. A different track loads at the estimated position; play/pause follows the
host; drift greater than 1.5 seconds triggers a seek. Corrections have a
three-second cooldown. Playback samples older than five seconds stop corrections
and show a delayed status, even when queue changes keep arriving. Three failed
track loads stop automatic loading until the host
changes tracks or the guest stops and restarts following.

This is approximate synchronization, not sample-accurate audio. Latency is
estimated during connection, rather than continuously measured. The room does
not yet implement scheduled simultaneous starts, buffering acknowledgments,
automatic shared-queue advancement, voting, or host migration.

The shared queue is separate from Spotify's queue. An entry is removed only after
the host engine reports a new play request for that track. If confirmation is
missing after 15 seconds, it is marked unconfirmed and is not retried automatically.
Check the host player before removing an unconfirmed entry and moving on.

## Transport and limits

Rooms live only in memory and use bounded JSON messages over TCP on an ephemeral
port. Hosting and joining accept explicit loopback or private IP addresses; no
public listener, discovery, cloud service, or internet relay is configured.
**Traffic is unencrypted: use only a trusted local network.** The random invite
token grants room access to anyone who has it. End the room to invalidate it.
Invitations are not saved to disk or included in room snapshots.

The host serializes queue changes and assigns stable entry IDs. Retried add
operations with the same command ID are idempotent. Guests cannot publish host
playback, start the next shared track, or remove other guests' tracks through the
protocol. Rooms allow 16 members, 100 queued tracks, and 20 pending contributions
per guest; message sizes, handshake times and pending connections are bounded.

## Verification and next milestones

Automated checks cover actual TCP host/guest exchanges, invalid invitations,
host shutdown, queue ownership and limits, duplicate adds, bounded messages, and
the follower's drift, stale-update and retry policies. Native screenshots cover
setup, hosting, following and minimum-window layouts using synthetic fixtures.

**Real Spotify playback on two devices has not been verified.** The next live
check is two separate Premium accounts on a private network: join, load the host
track, pause/resume, seek, change tracks, contribute a track, then end the room.
Observe audible offset, failed loads and account/session behavior before treating
the feature as ready for daily use.

Internet rooms will require an authenticated encrypted relay, reconnect/resume
handling and continuous clock/latency measurement. Official Spotify Jam
compatibility is outside this prototype.
