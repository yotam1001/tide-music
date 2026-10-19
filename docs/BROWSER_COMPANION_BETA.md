# Connect YouTube Music with Chrome

This connects your existing YouTube Music account to Tide without copying request
headers. Sign in to Google in Chrome; Tide never asks for your password.

**Preview setup:** the companion is not on the Chrome Web Store yet. You load it
once from a folder and enable its connection in Tide's installer. Chrome is the
connection path with a recorded successful import. Broader fresh-user setup
coverage remains a preview test item; Zen's native connection is unresolved,
and Edge and Firefox are not recommended paths yet.

## 1. Get the two matching downloads

From [v0.1.3](https://github.com/yotam1001/tide-music/releases/tag/v0.1.3), get:

- [**TideSetup.exe**](https://github.com/yotam1001/tide-music/releases/download/v0.1.3/TideSetup.exe) — installs the Windows player and its local connection helper.
- [**tide-browser-chromium.zip**](https://github.com/yotam1001/tide-music/releases/download/v0.1.3/tide-browser-chromium.zip) — the Chrome companion.

Contributors testing later source changes can use matching successful
[Windows build artifacts](https://github.com/yotam1001/tide-music/actions/workflows/ci.yml).
GitHub may require sign-in for those downloads. The app artifact is named
`tide-windows-preview-<commit>` and the companion artifact
`tide-browser-companion-prototypes-<commit>`. Use the same commit for both.

Extract the companion into a folder you will keep, rather than a temporary folder.
For the release's inner ZIP, the extracted folder contains `manifest.json`
directly. For an outer Actions artifact, look inside
`tide-browser-chromium/extension/`. **Choose the folder containing `manifest.json`,
not the ZIP, parent folder or Firefox package.**

## 2. Load the companion in Chrome

1. Open `chrome://extensions` in Chrome.
2. Turn on **Developer mode** and choose **Load unpacked**.
3. Select the folder containing the Chromium `manifest.json`.
4. Find **Tide session companion (prototype)** and copy its **ID**.
   It is a 32-letter extension identifier, not a password.
5. Pin Tide from Chrome's Extensions menu if you want its icon on the toolbar.

Keep the folder in place after loading it. Chrome reads the extension from disk.

## 3. Enable its connection in setup

1. Close Tide if it is running and finish or cancel pending companion imports.
2. Open **TideSetup.exe**. Already installed? Run it again; do not uninstall first.
3. Check **Enable Chrome companion connection (optional)**.
4. Paste the exact extension ID copied from Chrome into **Extension ID**.
5. Choose **Install Tide** and wait for setup to finish.

This registers the local helper for your extension and Windows user. It does not
sign into Google or import your account yet. The checkbox starts off; leaving it
off on a first install does not enable the connection. Later app updates retain
the browser choice already recorded by setup.

Open Tide's Chrome extension popup and select **Check local connection**. Continue
when it says **The browser can reach Tide's local host**. This checks setup only;
it does not read a session. If it fails, use the table below.

## 4. Import your selected YouTube Music tab

1. Open [YouTube Music](https://music.youtube.com/) in Chrome and sign in to your
   existing Google account. Select the YouTube Music profile you actually use.
   If you have several identities, check the avatar menu before continuing.
2. Go to **Home** in YouTube Music and keep that tab selected.
3. Open Tide's extension popup and choose **Import selected tab** or **Retry import**.
4. Close the popup, then click **Library** in YouTube Music within two minutes.
5. Reopen the popup and wait for **Session imported. Open Tide to use your Library.**
6. Open Tide from Start. In **Account**, confirm the name and connected state.
   Open **Library** and check that the expected playlists appear.

If Tide was already open and does not show the imported account, close and reopen
it, then refresh Library. **Connect YouTube Music** on the installer's completion
screen opens Tide's Account panel; it does not replace the import steps above.

The Music tab can be closed after import; the browser is for connection and Tide
handles playback. Sessions can expire or rotate. A minimum session lifetime is
not guaranteed, and repeated reconnect prompts should be reported as a bug.

## If something goes wrong

| What you see | What to do |
| --- | --- |
| Folder name is invalid / cannot load extension | Extract the ZIP first. Select the folder that contains `manifest.json` directly. |
| Browser cannot find the local host | Rerun matching Tide setup with its Chrome checkbox enabled and the exact ID currently shown in `chrome://extensions`. Use the same Windows user. |
| ID not allowed / invalid reply / host outdated | Check the ID and matching app/companion versions. Retry setup once. Report a persistent conflict rather than editing the registry. |
| Capture expired / nothing imported | Open Music Home, retry import, close the popup, then click Library promptly. Capture lasts at most two minutes and Chrome can suspend it. |
| Wrong account or playlists | Select the intended Music identity in Chrome and explicitly import again. Successful import replaces Tide's saved session. |
| YouTube rejected the session | Confirm Library works in Chrome, then try one fresh import. Report repeated failures without sending credentials. |

## Update or remove the companion

For an extension update, cancel pending imports, replace the files in the same
folder with the matching new package, then choose **Reload** on its card at
`chrome://extensions`. Recheck its ID and **Check local connection**. Moving the
folder can change the ID; if it changes, rerun setup with the new ID. App updates
do not update these unpacked extension files automatically.

To remove the saved session, use **Account → Disconnect account** in Tide first.
Remove the extension from Chrome separately. Uninstalling Tide removes its owned
local-helper registration but preserves saved preferences and sessions.

## Privacy

Import transfers allowed sign-in/session information from one selected Music tab
to Tide's local helper, which verifies and encrypts it for your Windows user.
It does not send your Google password or session to the project maintainers.
The helper and player contact YouTube to verify access and play music.

Never put cookies, request headers or session files in chat, issues or screenshots.
For a bug report, give the app version, Chrome version, exact popup message and
step that failed. [Privacy details](https://github.com/yotam1001/tide-music/blob/main/PRIVACY.md) ·
[Troubleshooting](https://github.com/yotam1001/tide-music/blob/main/docs/TROUBLESHOOTING.md) · [Advanced imports](https://github.com/yotam1001/tide-music/blob/main/docs/ACCOUNT_SETUP_ADVANCED.md)

Chrome's own [unpacked-extension instructions](https://developer.chrome.com/docs/extensions/get-started/tutorial/hello-world#load-unpacked) explain Developer mode and Load unpacked.
