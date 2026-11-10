# Tide + Grove: experimental nearby jams

Tide (YouTube) and Grove (Spotify) now share a local room protocol. Either app
can host. Each listener plays music through their own service; the room carries
song metadata, play/pause and timing, never audio or account credentials.

This is an approximate prototype, not a verified two-service audio release.
Both apps must be rebuilt with protocol version 2. Older Grove builds cannot
join the new rooms. The invitation still starts with `grove-jam://` and works in
both new apps. It is pasted inside the app, not registered as a Windows URL handler.

## Try it

1. Put the two computers on the same trusted private network. Start both updated
   apps. Connect Spotify and enable playback in Grove; Tide needs its normal
   working YouTube playback runtime. Each listener uses their own service access.
2. In Grove, open **Shared jams**. In Tide, choose **Shared jam** in the sidebar.
   Pick a display name, then **Start a jam**. The app chooses a private address
   when available. **Network options** allows a manual address override.
3. Copy the invitation, paste it in the other app, then choose **Join jam**.
   A `127.0.0.1` invitation works only on the same computer. No firewall settings
   are changed automatically.
4. Choose a song in the host app. A listener on the other service sees search
   results and confirms the **same recording**, or pastes its matching song link.
   Grove requires an individual Spotify track; Tide requires a YouTube video.
   A failed search never starts an arbitrary result. Pasting a link is the fallback.
5. Once confirmed, the guest loads its recording and follows host play/pause and
   seeking. Each new foreign song requires a choice; choices are reused during
   that room only. Choose another result or paste another link to correct a match.
6. **Stop following** gives the guest independent controls. **Follow host** resumes.
   Leaving ends synchronization. Tide guests use a temporary queue; leaving stops
   guest playback and restores their original queue without replacing its saved copy.

Choose songs directly in the host app for mixed rooms. Grove's existing shared
Spotify queue works with Grove hosts; Tide does not consume those Spotify queue
entries. Local files, albums, episodes and playlists are not cross-service room
identities, though individual songs played from a host playlist can be followed.

## Limits

- Nearby/private IPs only; no internet relay or public room discovery.
- Trusted networks only: this local TCP prototype is unencrypted. Possession of
  an invitation grants room access; ending the host room invalidates it.
- Timing correction uses elapsed time and bounded latency estimation, with a
  1.5-second drift threshold and a three-second correction cooldown. It is not
  sample-accurate or a scheduled simultaneous start.
- Music videos, intros, ads, live versions, regional edits and unavailable tracks
  can make the same timestamp refer to different audio. There is no offset editor
  or automatic recording fingerprinting yet. Prefer matching audio recordings.
- Matching choices are limited to 100 per room and are never saved to disk.
  Late search responses cannot choose a new song or cross room boundaries.
- Lost connections require joining again. Three failed loads stop automatic
  retries until the host changes songs or the listener restarts following.

## Verification

The shared crate tests use real loopback peers to deliver YouTube and Spotify
frames, pause, seek, song changes, queue ownership, invalid versions and disconnects.
Native Grove feature review covers matching results, explicit confirmation,
invalid links, stale-song responses and room isolation. Tide has a separate
credential-free, network-free native pairing review:

```powershell
./target/debug/tide-music.exe --review-cross-jam "$env:TEMP/tide-cross-jam-review"
./target/debug/grove-music.exe --review-features "$env:TEMP/grove-cross-jam-review"
```

Live YouTube-to-Spotify and Spotify-to-YouTube audio still require two working
service sessions. Validate both directions, same recording, pause/resume, forward
and backward seek, a second song, a wrong match and correction, stop/follow, leave
and host disconnect before treating this as ready for testers.
