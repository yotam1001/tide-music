# Tide privacy notice

Last updated: 2026-10-22


This notice covers the Tide Windows player, its optional session companion
extension, and its local native helper in this repository. Tide is an unofficial
YouTube Music client. The companion's single purpose is to transfer a session
from a YouTube Music tab you explicitly select to your local Tide installation,
so Tide can access your library and play music using that account.

**Tide handles authentication information.** A session can grant account access;
local processing does not make it nonsensitive. There is no Tide account service,
advertising SDK, analytics service or automatic crash-report uploader in this
version. Tide does not send your session to the project maintainers or a Tide
server, sell it, or use it for advertisements, profiling or unrelated purposes.
Google/YouTube and download providers have their own data practices.

Installed Windows players check the public Tide GitHub release endpoint once
after startup for newer stable versions. Disable startup checks in Settings;
manual checks are also available. GitHub receives normal network metadata such
as the IP address and Tide's app version in its User-Agent, without Google
credentials, account names, listening history or a GitHub token. Installer
downloads start only after choosing **Install and restart** and can contact
GitHub's release-asset hosts. One installer and one partial download are kept
in the local `TideMusic/updates` cache, each capped at 96 MiB. No telemetry or
crash reporting is included in the updater.

## Optional listening rooms

Jams are available in the normal installer. No room is contacted until you host
or join. The default shared relay is hosted on Render; Advanced setup supports
your own relay. The relay and its TLS proxy can see video IDs, queue order, position,
pause/loading state and room capabilities. They receive no Google cookies,
account headers or audio from Tide. HTTPS protects transport to that operator;
room metadata is not end-to-end encrypted against the operator.

Invites contact their specified server, so use only trusted invites/operators.
Operator keys allow room creation and must not be sent to guests. Invite/key files
remain in the private folders chosen by the user; Tide does not automatically
erase them. Capabilities remain in memory during a session. The original guest
queue is restored paused on leave, and the temporary Jam queue is not saved to
preferences. See the [experimental Jam guide](experiments/jam/README.md).


Code-based Jam invitations contain a relay endpoint, room identifier and guest
capability. They never include a host/operator key or Google account data. Codes
are kept in memory rather than saved automatically. Choosing **Copy invite**
puts that guest capability on the system clipboard; OS clipboard history/sync
and other local applications may retain it. Tide does not clear unrelated
clipboard contents when a room ends. Share the code only with intended guests.
The explicit shared-server mode uses temporary per-process keyed network-address
fingerprints to limit room creation; the server's TLS proxy still sees raw IPs.
Hosts can see anonymous room-local Listener IDs and remove listeners, lock the
room or replace its invite. Removing a listener also replaces the invite; no
Google account names, session headers or network addresses are exposed in that
participant list.

## What the browser companion processes

Opening its popup does not authorize import. **Import selected tab** arms a
capture for up to two minutes in the selected tab at
`https://music.youtube.com`. The companion observes a matching Music browse
request and waits for a successful response before sending one local transfer.
Chrome may suspend its background worker earlier, requiring an explicit retry.

The allowed request headers are:

- `cookie`: the selected request's Cookie header, including authentication and
  other cookies sent to YouTube Music;
- `x-goog-authuser` and optional `x-goog-pageid`: selected Google account and
  Music identity identifiers;
- optional `x-goog-visitor-id` and `user-agent`: visitor and browser information;
- `origin`: the YouTube Music origin.

The extension uses request URL, tab, method and response status to select and
validate that transfer. It does not send the URL query string to the helper.
`Authorization` and headers outside the allowlist are discarded. It does not
read passwords, page contents, unrelated browsing history or your browser's
cookie database. It has no content scripts, browser storage, remote scripts,
telemetry or independent network fetches. Its Music-site access and `webRequest`
permission observe requests; `activeTab` identifies your chosen tab and
`nativeMessaging` communicates with the local helper.

Session headers are held temporarily in memory and transferred through Chrome's
native-messaging channel to the registered Tide host on the same computer. The
popup receives status codes, not the session headers. References are released
after sending, cancellation, expiry or other termination conditions; JavaScript
garbage collection and browser buffers do not guarantee secure memory erasure.
**Cancel** after the helper has committed cannot undo a completed import. Use
**Disconnect** in Tide to remove its saved session.

## What the native player and helper process

The native helper independently verifies a candidate session with YouTube before
saving it. A successful explicit import replaces Tide's previous saved session.
The player uses the session to request account identity, playlists, liked songs,
personalized recommendations and related track information, and to authenticate
native audio extraction/playback. These requests can include account identifiers,
cookies, visitor/browser information and the requested music identifiers. Native
playback contacts YouTube and its media infrastructure, including Google video
servers. Those services receive network information such as your IP address.

Public cover art is downloaded from allowed HTTPS hosts under `ytimg.com`,
`ggpht.com` and `googleusercontent.com`. These artwork requests do not include
Tide's account cookies, authorization headers or Referer. Artwork URLs still
identify a requested image, and the remote service receives ordinary network
metadata. Decoded artwork is cached in bounded application memory.

The app stores local preferences and queue metadata, including track titles,
artists, albums, durations, artwork/source URLs and any selected local audio
paths. This metadata is not encrypted by Tide.

Explicit Like/Dislike and playlist menu actions send the selected rating, video
ID and, for playlist edits, playlist/entry IDs to your selected YouTube Music
account. Tide verifies playlist ownership and does not automatically retry an
uncertain write. These operations do not alter the shared Jam queue or another
listener's account. The relay receives no rating or playlist-edit requests.

Listening-history feedback is off by default. Enabling it in Settings permits
new qualified listening to be submitted to YouTube using that account's playback
tracking data. Only observed actual playback counts, once per playback run;
seeking, paused/buffering time and automated probes do not count. Tracking URLs
stay within the local helper and are restricted to YouTube's HTTPS playback
endpoint; redirects are disabled. They are not sent to the relay or maintainers.
Turning the setting off stops new submissions but does not delete history
already sent. Ordinary upstream playback requests remain visible to YouTube
regardless of this optional history setting.

Manual connection alternatives process the local authentication file or scoped
YouTube cookies from the Zen cookie database you explicitly select. Original
manual import files remain under your control. Those alternatives do not make
the companion collect other browser profiles or passwords.

## Local storage and security boundaries

On Windows, the persistent session is stored at
`%APPDATA%\TideMusic\account\session.dpapi`, encrypted using Windows DPAPI for
your Windows user. Tide restricts its account directory to its owner and SYSTEM.
This does not protect against every program running as your user, a compromised
computer, privileged access or all backup/recovery scenarios.

Playback creates a temporary **plaintext** Netscape cookie file in that private
account directory, scoped for YouTube extraction. Normal cleanup removes its
owned file after playback processes finish. Startup and disconnect try to remove
leases whose owning process has exited. Inaccessible processes or reused process
IDs can conservatively leave a lease behind. Persistent session encryption does
not mean these temporary files or in-memory credentials are encrypted.

Preferences and queue metadata normally live in
`%APPDATA%\TideMusic\settings.json`, with a durable local/user-directory fallback
when roaming app data is unavailable. Runtime executables, installation ownership
records, the exact allowed extension ID and native-host registration are separate
installation data. The extension ID is not a session credential.

Installation downloads public playback/helper dependencies from GitHub, Python's
download service and PyPI/file hosting. These services receive download request
metadata; Tide does not attach the imported Music session to those downloads.
Opening setup/account help or repository pages also contacts their hosting
services through your browser. Your browser extensions, history and sign-in
state are governed separately by your browser and those services.

## Disconnect, removal and retention

- **Disconnect** removes Tide's persistent encrypted session and clears the
  displayed account library. It reconfigures playback and performs the stale
  lease cleanup described above. It does not erase saved queue metadata,
  preferences, your original import files or copies held by other running Tide
  instances, backups or external tools.
- Removing the companion in Chrome stops that installed extension from future
  captures. It does not delete Tide's saved session or Windows helper registration.
- Uninstalling Tide removes its owned installation files, shortcuts and matching
  native-host registration. It intentionally preserves preferences and saved
  sessions. Disconnect before uninstalling if you want the saved session removed.
- Session removal does not sign you out of Google in your browser or revoke the
  upstream Google session. For an exposed session, use Google's account/session
  security controls as well; deleting local files cannot recall shared copies.

There is no fixed automatic expiry for retained preferences or a saved session:
they remain until replaced or removed, although YouTube can invalidate a session.
Deletion is ordinary file removal, not a guarantee of forensic secure erasure.

## Diagnostics, questions and reports

The companion displays fixed status messages, and its native host returns bounded
status replies rather than raw headers or browser exceptions. Session credentials
are passed to local helpers through stdin, not command-line arguments. Developer
tests, smoke tools or manually captured diagnostic output can produce local logs
and screenshots. Review those files before sharing: track information, account
names, local paths, URLs and developer-tools captures can reveal private data.
No report is uploaded automatically by Tide.

For a non-sensitive question, open an issue in the
[official repository](https://github.com/yotam1001/tide-music/issues). For a security
issue, follow [SECURITY.md](SECURITY.md) and use private vulnerability reporting
where available. **Never send cookies, request headers, authentication files or
credentials in public issues, chat or screenshots.** This notice describes the
current implementation; it is not a claim of Chrome Web Store approval or a
guarantee of security.
