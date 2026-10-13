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

## Candidate session handoff

Evaluate an optional browser extension with a native messaging bridge. Firefox
and Chromium families need separately validated manifests, permissions and
distribution. Browser signing/installation is a real part of release work;
unpacked developer extensions are not a finished public onboarding solution.

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
