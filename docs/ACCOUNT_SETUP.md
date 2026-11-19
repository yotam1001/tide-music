# Connect your YouTube Music account

Connecting lets Tide read your playlists, Liked Songs and recommendations and use
your signed-in session for native playback. Keep your existing Google account.
Google handles sign-in; Tide does not capture your password or require an API key.

## Sign in inside Tide

1. Open the profile icon at the top right, then **Sign in with Google**.
2. Complete sign-in on Google's page in the temporary window.
3. In YouTube Music, choose the profile that owns your playlists. Open **Library**.
4. Choose **Connect this profile** at the bottom of the window.

Tide verifies the connection and saves the session encrypted for your user account
(Windows DPAPI on Windows, AES-GCM with a login Keychain key on macOS).
The temporary browser closes and its private profile is removed. You do not need
to keep a browser open while listening. A cancelled or failed verification keeps
your existing saved connection. If the window cannot open or Google declines
sign-in inside the embedded window, use the browser companion or session-file
import below.

On Windows, the native window requires the Microsoft Edge WebView2 Runtime. On
macOS, it uses a separate non-persistent `WKWebView` helper (`tide-login`).
Google handles your password and any two-step verification; Tide does not spoof
a browser User-Agent, capture form fields, or log your credentials.

## Family Link and supervised accounts (macOS and Windows)

Google often blocks sign-in inside embedded WebViews (such as `WKWebView` on
macOS) for Family Link and other supervised Google accounts, even when two-step
verification is not involved. Tide does not spoof browser User-Agents or weaken
browser security checks to bypass Google's policy.

Instead, sign in to `https://music.youtube.com` in your normal browser (Chrome,
Edge, Brave, Arc, Firefox, Zen, or Safari) where Google permits your account,
and connect that signed-in tab to Tide using one of the two paths below:

1. **Browser companion (Chrome, Edge, Brave, Arc, Firefox, Zen):** best day-to-day
   experience. On macOS, `Tide.app` bundles `tide-session-host` and the
   unpacked extensions under `Tide.app/Contents/Resources/browser-companion`,
   and you can register your browser directly from **Account → Use browser
   companion instead** (or via `scripts/setup-browser-companion-macos.py`).
2. **Import session file with Copy as cURL (Safari or extension-restricted
   accounts):** if you use Safari or Family Link restricts browser extensions,
   open `https://music.youtube.com` in your browser, open Web Inspector /
   DevTools → **Network**, click **Library**, right-click a `browse` request
   → **Copy as cURL**, save it to a `.txt` file, and select it in Tide under
   **Account → Advanced connection options → Import session file**. Tide
   verifies the session online before replacing any existing connection; delete
   the temporary `.txt` file afterward.

> Tide respects your Google / Family Link account rules and cannot grant access
> to YouTube Music if your account is not permitted to use YouTube Music in a
> regular browser.

## Browser companion

### On macOS

1. Open **Account** in Tide and click **Use browser companion instead**.
2. Click **Reveal extension** to open the bundled extension folder
   (`chromium` for Chrome, Edge, Brave, or Arc; `firefox` for Firefox or Zen).
3. Load the unpacked extension in your browser:
   - **Chrome / Edge / Brave / Arc:** open `chrome://extensions` (or your
     browser's Extensions page), enable **Developer mode**, choose **Load
     unpacked** and select the `chromium` folder, then copy the 32-letter
     extension ID.
   - **Firefox / Zen:** load the `firefox` folder as an add-on (`about:debugging`
     → **This Firefox** → **Load Temporary Add-on** → `manifest.json`).
4. Back in Tide, pick your browser (and paste the 32-letter extension ID for
   Chromium browsers), then click **Connect browser**. Tide writes the
   native-messaging host manifest into your user profile and its policy to
   `~/Library/Application Support/TideMusic/tide-session-host-policy.json`
   without modifying the signed `Tide.app` bundle.
5. Open `https://music.youtube.com` in that browser, open the Tide companion
   popup, choose **Import selected tab**, and click **Library** in YouTube Music.

### On Windows

Follow the [Chrome connection guide](https://github.com/yotam1001/tide-music/blob/main/docs/BROWSER_COMPANION_BETA.md):

1. Load Tide's unpacked companion in Chrome.
2. Register its local helper through the advanced ZIP setup as the guide describes.
3. In your signed-in YouTube Music tab, choose **Import selected tab** in the
   companion, then click **Library** in YouTube Music.
4. Wait for **Session imported**, then open Tide and confirm your account under
   **Account**, beside Settings.

**Check local connection** in the companion checks the local helper, not your
Google sign-in.

## Check that it is your account

Open **Account** in Tide and read the displayed name and connection state.
Check that **Library** shows the expected playlists. For the wrong profile,
use **Reconnect YouTube Music** and choose the intended Music profile, or select
that identity in your browser and import the tab again.
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

Saved sign-in data is encrypted for your OS user (Windows DPAPI or macOS
Keychain-backed AES-GCM). Temporary scoped cookie files used for playback are
plaintext while they exist; see
[privacy details](https://github.com/yotam1001/tide-music/blob/main/PRIVACY.md).
Never share session files, cookies, cURL commands, or request headers.

Premium availability depends on your account and the accessible streams. Tide
does not provide Premium or implement ad blocking.

## Reconnect or disconnect

- **Reconnect:** use **Account → Reconnect YouTube Music** and choose the intended
  profile. The browser companion import and verified session-file import remain
  alternatives.
- **Disconnect:** use **Account → Disconnect account**. This removes Tide's saved
  session (and Keychain key on macOS); your Google browser sign-in is separate.
- **Other methods:** Zen profile and verified local session-file imports (Copy as
  cURL, raw request headers, or browser JSON) are under
  **Account → Advanced connection options**. Use the
  [advanced guide](https://github.com/yotam1001/tide-music/blob/main/docs/ACCOUNT_SETUP_ADVANCED.md)
  only if you need them.

[Troubleshooting](https://github.com/yotam1001/tide-music/blob/main/docs/TROUBLESHOOTING.md)
