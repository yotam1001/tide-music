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

## Prototype direction after source comparison

FeatherPlay's native popup uses ordinary Google web sign-in followed by WebView2's
cookie API; it is not a Google OAuth device-code implementation. Its source does
not establish that Tide's own login, selected Music identity and renewal work.
Test the ordinary sign-in path independently of the OAuth feasibility question.

Use a separate, owned helper executable, created suspended and assigned to Tide's
existing kill-on-close Windows job before it runs. The player should not retain a
WebView runtime after connection. Close the controller and await the environment's
BrowserProcessExited event; the owned job remains the last-resort lifetime boundary
on timeout, crash and parent termination. Never terminate unrelated browser processes.

The popup must allow the user to choose their Music profile before an explicit
**Connect this profile** action. Do not automatically import the default Google
identity on the first redirect to Music. This matters for users whose Music library
belongs to a different channel/profile, such as the previously selected brand account.
Observe only allowlisted Music browse-request metadata and read cookies with the
runtime API; do not inject account-page scripts or install a general native bridge.

Prefer calling the existing `Provider::import_browser_session` verification and
encrypted-store transaction inside the helper. The parent can adopt the result
through its existing connection monitor. This avoids a new plaintext credential
IPC protocol, clipboard step, or temporary authentication file. The helper's exit
status must contain only a fixed success/cancel/failure code. Keep the original
browser companion available until this path passes the live acceptance tests.

An isolated, feature-gated helper is implemented in `crates/windows/src/login.rs`
and `crates/provider/src/native_login.rs`. The Account UI offers native sign-in only
when `tide-login.exe` exists beside the player; otherwise the companion remains the
primary connection. The 0.4.13 release candidate includes the helper; publication is
gated on the installed helper and updater checks. The offline probe passes: restrictive
WebView settings, scoped cookie extraction, account-selection snapshot, complete
owned-process shutdown, and private-profile cleanup. Test cookies are synthetic;
the probe does not import them or contact Google. Profile ownership uses Windows
directory identity rather than path-string comparisons.

On 2026-11-04, interactive Google sign-in and explicit profile selection succeeded;
a subsequent Library request with the popup closed verified the profile name, and
the maintainer confirmed it was the intended renamed Music profile. The helper's
owned process tree stopped and its private profile directory was removed. This is
one successful sign-in, not evidence of long-session renewal or all failure paths.
Playback, closed-browser renewal and packaged app adoption still need validation.
Keep the companion available until these gates pass. Run the isolated offline
probe through `scripts/cargo-local.ps1` with package `tide-provider`, feature
`native-login-probe`, example `native_login`, and arguments `--probe`. Running the
example or `tide-login` binary without `--probe` opens the interactive sign-in helper; a successful explicit
Connect verifies and adopts the session using the existing encrypted store.

### Packaging gate

The build/package scripts have explicit opt-ins: `build-preview.ps1 -IncludeNativeLogin`,
`package-preview.ps1 -NativeLoginExecutable <built tide-login.exe>`,
`check-preview-package.py --with-native-login`, and `build-installer.ps1 -IncludeNativeLogin`.
The helper has a fixed package name and is covered by the same checksum manifest,
privacy scan and signed installer envelope as the player. No arbitrary executable
directory is accepted. Generate dependency metadata with
`--features tide-provider/native-login` so optional helper dependencies appear in
the notices. Candidate CI packaging now opts in and runs the helper's offline
lifecycle probe from the upgraded installation. This does not substitute for the
separate live Google sign-in and playback evidence above.

Sources:
- https://github.com/candygoogy/featherplay/blob/main/apps/featherplay/src/pages/sign_in/webview.rs
- https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-related-events
- https://github.com/wravery/webview2-rs

## Current decision

Bundle the isolated helper with the candidate and require its installed lifecycle
and upgrade checks before stable publication. Keep the companion as a fallback and
continue sustained renewal testing. The helper introduces no OAuth keys and requires no browser runtime in
the player during normal listening. Its browser runs only in the separate helper.

References:
- https://developers.google.com/identity/protocols/oauth2/native-app
- https://developers.google.com/identity/protocols/oauth2/policies
- https://developers.google.com/youtube/data/guides/auth/devices
- https://github.com/yt-dlp/yt-dlp/wiki/Extractors#logging-in-with-oauth
