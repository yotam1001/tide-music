# Browser connection requirements

This is a design for work still needed, not a description of a completed login
feature. Manual header import remains a fallback and does not meet the intended
first-time setup experience.

## Intended experience

1. Choose Connect in Tide.
2. Open YouTube Music in the user's default browser.
3. Sign into an existing account and choose the intended Music identity.
4. Explicitly approve transferring that site's session to Tide.
5. Tide verifies the identity, saves an encrypted session, and displays the
   account name. Playback continues after the browser closes.

Do not require Zen, a new Google account, developer tools, or a persistent browser
process. Support for browser launching and support for session transfer are
separate: opening a default browser does not grant access to its credentials.

## Current implementation and API boundary

Tide opens account help through Windows' default HTTPS handler. Direct database
selection currently supports normal Zen profiles. Manual request-header imports
are browser-independent, but require too much work for everyday use.

[Google's desktop OAuth flow](https://developers.google.com/identity/protocols/oauth2/native-app)
supports browser authorization and token refresh.
[The official YouTube Data API](https://developers.google.com/youtube/v3/docs/)
provides data resources such as playlists; it does not document the native Music
stream or personalized Music home endpoints needed by Tide.
[ytmusicapi's OAuth mode](https://ytmusicapi.readthedocs.io/en/stable/setup/oauth.html)
is a candidate for library access, but it still needs compatibility verification
with the selected Music identity. It cannot establish playback authentication:
[yt-dlp documents that YouTube OAuth login no longer works](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#logging-in-with-oauth).

## Prototype session handoff

An optional browser extension and native messaging receiver are implemented as
a source prototype in [browser-companion](../browser-companion/README.md) and
`crates/provider/src/bin/tide-session-host.rs`. They are not signed,
publicly distributed, or verified with a real browser session. A temporary Zen
companion is loaded for local testing, but its real native handoff is not yet
working. Firefox
and Chromium families need separately validated manifests, permissions and
distribution. Browser signing/installation is a real part of release work;
unpacked developer extensions are not a finished public onboarding solution.

The provider now keeps an opaque connection generation inside encrypted session
storage. Explicit imports replace it; ordinary API/playback cookie persistence
preserves it. Legacy sessions remain readable without a generation. A
credential-free descriptor exposes that generation and the selected account and
Music identity to the application; the internal metadata is removed before any
headers reach a helper. Provider compatibility and refresh regression tests
cover this foundation. The application now probes the local connection descriptor
on a background thread and stages adoption after cancelling obsolete library
work. A reconnect of the same identity refreshes playback cookies without
replacing the active playback engine. Identity changes reconfigure playback and
clear old account results. Failed local adoption has bounded retries and an
explicit Library refresh retry. The isolated native application probe now verifies same-index playback
continuity, superseded account switches, retained pause/seek intent and disconnect using silent local audio
and a fake identity helper. Real browser handoff validation remains unfinished;
synthetic checks do not prove onboarding works end to end.

Run the Windows regression against explicitly selected binaries:

```powershell
python scripts/native-connection-smoke.py --app target/debug/tide-music.exe --host target/debug/tide-session-host.exe
```

The receiver accepts one bounded, length-prefixed message from the allowed
extension identity. It verifies the session through the identity endpoint before
saving a DPAPI-encrypted copy. A changed saved session or a disconnected native
port cancels the pending commit. Cancellation after a completed commit cannot
undo that commit; the user can disconnect in Tide. Protocol, cancellation,
rejection and encrypted-save probes currently use synthetic credentials and a
fake identity helper. They do not prove live browser authentication works.

Lavalink's alternative [OAuth handler](https://github.com/lavalink-devs/youtube-source/blob/main/common/src/main/java/dev/lavalink/youtube/http/YoutubeOauth2Handler.java)
uses hardcoded YouTube client credentials and internal TV authorization endpoints.
Its documented OAuth playback support is specific to its TV client and does not
establish compatibility with a Tide-owned OAuth registration or Music library.
Do not transplant those credentials or replace the working playback backend
without checking identity selection, refresh, Premium behavior and resource use.

The user must initiate a short-lived transfer. Capture only a successful YouTube
Music request from the explicitly selected tab, preserving its account/container
context. Transfer only the allowlisted session inputs that Tide already verifies;
never collect passwords, unrelated cookies, browsing history or page content.
Do not start a debugging port, copy a browser profile, or silently read all browser
databases. Reject unsolicited messages, other extension identities, malformed or
oversized input, stale transfers and account changes during verification.

The bridge must use a framed local channel with a fixed application identity;
credentials must never appear in URLs, command arguments, public logs or files
outside encrypted account storage. Disconnect must revoke local transfer state
and erase stored credentials. Temporary failures must preserve the last verified
session rather than replacing it with an unverified candidate.

## Acceptance evidence

### Diagnose the local channel without transferring a session

The companion's **Check local connection** button sends only a versioned
`check_host` message with empty headers. The host answers `host_ready` before
opening provider/account storage. The check does not arm Music capture, inspect
a Music tab, read credentials or change a saved session. Success proves the
native transport works, not that authenticated import or playback works.

Missing/unusable registration, permission, launch, timeout and outdated-host
errors have fixed messages. Firefox's `Port.error` and Chromium's
`runtime.lastError` are consumed without retaining or displaying their raw text.
An older host's valid `invalid_request` response is distinguished from a broken
transport. Import still requires its separate verified `imported` response.

On 2026-10-13 the real Zen 1.23.2b check reported an unusable/missing native
application despite a matching current-user Mozilla registration, readable
manifest, allowed extension ID and executable. The registered executable, run
from Zen's installation directory with Firefox's manifest/extension arguments,
returned a valid framed `host_ready`. This narrows the failure to browser lookup
or validation; it does not establish the exact cause. Zen has an
[open Windows native-messaging report](https://github.com/zen-browser/desktop/issues/15432)
with similar symptoms. Treat that as related evidence, not proof that every
Tide failure has the same cause. Do not change browser security settings or
replace user profiles to work around it.

### Required release checks

- Default-browser launch on Windows without a browser-specific installation path.
- Explicit transfer and verified account-name display in Firefox/Zen and
  Chrome/Edge, including a second Google account and a Music brand identity.
- Cancellation, duplicate transfers, container isolation, stale account responses,
  rejected origins and native-message size/identity checks.
- Existing playback continues during connection and cancellation.
- Browser closure and subsequent Tide restart preserve authenticated playback.
- Sustained playback, repeated recommendations and session updates work without
  manual headers or keeping the browser open.
- Signed installation, removal and privacy documentation are tested on a fresh
  Windows environment before this becomes the default Connect flow.
