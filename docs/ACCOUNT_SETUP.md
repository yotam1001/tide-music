# Connect your YouTube Music account

Connecting lets Tide read your playlists, Liked Songs and recommendations and use
your signed-in session for native playback. Keep your existing Google account.
Tide does not ask for your password or require an API key.

## Use the Chrome companion

Follow the [Chrome connection guide](https://github.com/yotam1001/tide-music/blob/main/docs/BROWSER_COMPANION_BETA.md):

1. Load Tide's unpacked companion in Chrome.
2. Copy its extension ID into **TideSetup.exe → Enable Chrome companion connection**.
3. In your signed-in YouTube Music tab, choose **Import selected tab** in the
   companion, then click **Library** in YouTube Music.
4. Wait for **Session imported**, then open Tide and confirm your account under
   **Account**, beside Settings.

Already installed? Rerun setup to enable the companion. **Connect YouTube Music**
in Tide opens the instructions; the explicit Chrome import completes connection.
**Check local connection** checks the helper, not your Google sign-in.

Chrome is the preview path with a recorded successful import. Broader fresh-user
coverage remains a preview test item. Zen's native connection is unresolved;
Edge and Firefox are not recommended setup paths yet.

## Check that it is your account

Open **Account** in Tide and read the displayed name and connection state.
Check that **Library** shows the expected playlists. For the wrong profile,
select the intended YouTube Music identity in Chrome and import that tab again.
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

- **Reconnect:** sign in to the intended Music profile in Chrome and repeat the
  companion import. Refresh Library, or close and reopen Tide if needed.
- **Disconnect:** use **Account → Disconnect account**. This removes Tide's saved
  session; your Google browser sign-in is separate.
- **Other methods:** Zen profile and local session-file imports are under
  **Account → Advanced connection options**. Use the
  [advanced guide](https://github.com/yotam1001/tide-music/blob/main/docs/ACCOUNT_SETUP_ADVANCED.md)
  only if you need them.

[Troubleshooting](https://github.com/yotam1001/tide-music/blob/main/docs/TROUBLESHOOTING.md)
