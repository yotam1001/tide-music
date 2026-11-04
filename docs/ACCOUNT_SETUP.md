# Connect your YouTube Music account

Connecting lets Tide read your playlists, Liked Songs and recommendations and use
your signed-in session for native playback. Keep your existing Google account.
Google handles sign-in; Tide does not capture your password or require an API key.

## Sign in inside Tide

1. Open the profile icon at the top right, then **Sign in with Google**.
2. Complete sign-in on Google's page in the temporary window.
3. In YouTube Music, choose the profile that owns your playlists. Open **Library**.
4. Choose **Connect this profile** at the bottom of the window.

Tide verifies the connection and saves the session encrypted for your Windows user.
The temporary browser closes and its private profile is removed. You do not need
to keep a browser open while listening. A cancelled or failed verification keeps
your existing saved connection. If the window cannot open, use the companion below.

The native window requires the Microsoft Edge WebView2 Runtime, included on many
Windows installations. Google handles your password and any two-step verification;
Tide does not capture form fields or log your credentials.

## Browser companion fallback

Follow the [Chrome connection guide](https://github.com/yotam1001/tide-music/blob/main/docs/BROWSER_COMPANION_BETA.md):

1. Load Tide's unpacked companion in Chrome.
2. Copy its extension ID into **TideSetup.exe → Enable Chrome companion connection**.
3. In your signed-in YouTube Music tab, choose **Import selected tab** in the
   companion, then click **Library** in YouTube Music.
4. Wait for **Session imported**, then open Tide and confirm your account under
   **Account**, beside Settings.

Already installed? Rerun setup to enable the companion. **Use browser companion instead**
in Tide opens the instructions; the explicit Chrome import completes connection.
**Check local connection** checks the helper, not your Google sign-in.

Chrome is the preview path with a recorded successful import. Broader fresh-user
coverage remains a preview test item. Zen's native connection is unresolved;
Edge and Firefox are not recommended setup paths yet.

## Check that it is your account

Open **Account** in Tide and read the displayed name and connection state.
Check that **Library** shows the expected playlists. For the wrong profile,
use **Reconnect YouTube Music** and choose the intended Music profile, or select
that identity in Chrome and import the tab again.
A successful explicit import replaces Tide's saved session.

The browser is used for connection while Tide handles playback natively. It is
intended to work after closing the Music tab. YouTube can expire or rotate a
session; Tide cannot promise a minimum lifetime. Report repeated reconnect
prompts instead of repeatedly exporting credentials.

## Your data and Premium

Your library stays on YouTube Music. Tide does not download your whole collection.
Likes and playlist edits happen only through their explicit controls. Optional
listening-history feedback is off by default; enable it in Settings to send new
qualified playback to your selected account. See [the controls](USING_TIDE.md).

Saved sign-in data is encrypted for your Windows user. Temporary scoped cookie
files used for playback are plaintext while they exist; see
[privacy details](https://github.com/yotam1001/tide-music/blob/main/PRIVACY.md).
Never share session files, cookies or request headers.

Premium availability depends on your account and the accessible streams. Tide
does not provide Premium or implement ad blocking.

## Reconnect or disconnect

- **Reconnect:** use **Account → Reconnect YouTube Music** and choose the intended
  profile. The browser companion import remains an alternative.
- **Disconnect:** use **Account → Disconnect account**. This removes Tide's saved
  session; your Google browser sign-in is separate.
- **Other methods:** Zen profile and local session-file imports are under
  **Account → Advanced connection options**. Use the
  [advanced guide](https://github.com/yotam1001/tide-music/blob/main/docs/ACCOUNT_SETUP_ADVANCED.md)
  only if you need them.

[Troubleshooting](https://github.com/yotam1001/tide-music/blob/main/docs/TROUBLESHOOTING.md)
