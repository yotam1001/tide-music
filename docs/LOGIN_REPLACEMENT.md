# Login replacement investigation

Status: keep the working browser companion; no login replacement is shipped.

## Temporary WebView2

A separate helper process is the appropriate lifetime boundary if this approach
becomes viable: launch on Connect only, use an isolated profile, verify the selected
Music identity, transfer only allowlisted session inputs to the encrypted store,
close all WebView controllers, and terminate the helper process on success, cancel,
timeout or app exit. Verify the entire helper process tree exits. Deleting a UI
object alone does not prove that WebView runtime processes have exited.

The unresolved prerequisite is successful supported Google login. Google prohibits
OAuth authorization in embedded user agents. Do not spoof a browser user agent,
copy another product's OAuth credentials or suppress browser security checks to
make an embedded sign-in appear supported. A successful ordinary web page load
would not prove the full authentication, account selection and refresh path.

## OAuth/device-code

Google's normal desktop OAuth uses the system browser and a loopback redirect with
PKCE. Device authorization is designed for TVs/limited-input devices and exposes
specific API scopes. Either can authenticate to supported Google APIs; neither
establishes the browser session needed by Tide's current native playback backend.
yt-dlp explicitly documents that YouTube OAuth login no longer works for its
YouTube extraction path. Data API access alone is not a replacement for Music home,
Premium playback, selected Music identity and long-session renewal.

Do not request real user credentials or add a new OAuth consent screen until a
Tide-owned client and the required endpoint compatibility have been demonstrated.
Any future prototype must remain outside stable account handling until library,
playback, refresh, account switching, browser-closed listening and RAM tests pass.

## Current decision

Improve the existing default-browser companion onboarding while investigating
supported auth methods. Do not make an unverified popup the only way to connect.
No new session capture, WebView runtime, OAuth keys or credentials were introduced
by the gaming integration change.

References:
- https://developers.google.com/identity/protocols/oauth2/native-app
- https://developers.google.com/identity/protocols/oauth2/policies
- https://developers.google.com/youtube/data/guides/auth/devices
- https://github.com/yt-dlp/yt-dlp/wiki/Extractors#logging-in-with-oauth
