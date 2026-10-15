# Chrome companion: Windows beta setup

The companion transfers a session from a YouTube Music tab you choose to Tide's
local helper. You sign into Google in your browser; Tide does not ask for your
password. This is an unpacked developer extension, **not a Chrome Web Store
release**. A Chrome transfer and subsequent Tide startup passed one local test;
fresh-machine setup, browser-closure continuity and longer session reliability
still need validation. See [recorded evidence](BROWSER_CONNECTION.md#acceptance-evidence).

Use Chrome for this beta path. Zen's native connection check failed in the
recorded test. Edge and Firefox require their own validation before we recommend
this flow for them. Manual import remains available in [account setup](ACCOUNT_SETUP.md).

## Get the matching packages

Use **TideSetup.exe** and the Chromium companion from the **same release
or successful build** in the [official repository](https://github.com/yotam1001/tide-music).
If there is no published release yet, a successful GitHub Actions build supplies
`tide-windows-preview-<commit>` and
`tide-browser-companion-prototypes-<commit>` artifacts. GitHub may require sign-in
to download Actions artifacts. Do not use packages posted in comments or messages.

Extract the companion download to a folder you control. The Windows artifact
contains **TideSetup.exe**; the full preview ZIP is an advanced alternative. In
the companion artifact, use
`tide-browser-chromium/extension/`: **the selected folder must contain
`manifest.json` directly**. Alternatively, extract the inner
`tide-browser-chromium.zip` to a dedicated folder and use that folder.

Keep this extension folder in a stable location; Chrome loads it from disk.
Do not select a ZIP file or the parent artifact folder. The companion's
`SHA256SUMS` covers its extension files and inner ZIP. If release-level checksums
are provided, check those before running setup too. Checksums detect changed
downloads; they do not replace using the official source.

## Load the Chrome extension

1. In Chrome, open `chrome://extensions`.
2. Turn on **Developer mode**, then select **Load unpacked**.
3. Select the stable folder containing the Chromium `manifest.json`.
4. Find **Tide session companion (prototype)**. Copy its displayed **ID**, a
   32-letter value. This ID is not a credential.
5. Optionally pin Tide using Chrome's Extensions menu so its popup is easy to find.

Chrome may display developer-extension notices. Loading this extension does not
install or register Tide's local helper, and the popup cannot do that for you.

## Register Tide's local helper

Close Tide and finish or cancel any companion transfer first. Run
**TideSetup.exe**, select **Enable Chrome companion connection (optional)**, and paste
the exact extension ID from Chrome into its ID field. The option starts
unchecked. Setup accepts only a complete 32-character ID made of lowercase
letters `a` through `p`; it does not guess an ID or allow wildcard origins.
Choose **Install Tide**. This installs or updates Tide for your Windows user and
registers the local helper for that extension ID. Internet access is needed for
runtime downloads; administrator access is not.

If Tide is already installed, rerun the matching setup with the Chrome option
enabled. Leaving it unchecked skips registration on a first install; an update
preserves the previously recorded browser choice. The installer's guide link
explains loading the extension; setup does not load it into Chrome. The
completion screen reports registration when this option succeeds, but that is
not yet proof of a working browser connection or an imported account.

The advanced alternative is to extract the full Windows preview ZIP, confirm it
contains `Install Tide.cmd` and `tide-session-host.exe`, and run **Install
Tide.cmd**. Choose **Chrome** at its browser-helper prompt and paste the same
actual extension ID. Do not guess an ID or use another person's ID. Setup
refuses to replace a conflicting Tide registration: report that conflict rather
than editing the registry or deleting it by hand.

**Connect YouTube Music** on setup's completion screen opens Tide's Account
panel; session transfer still requires the separate explicit import below.

Open the companion popup and choose **Check local connection**. Continue only
when it says **The browser can reach Tide's local host**. This check transfers no
session and does not prove that your Google account is connected.

## Connect the intended account

1. Open `https://music.youtube.com/` in Chrome and sign into your existing Google
   account. Select the intended YouTube Music identity, including the right
   channel if you use more than one. Confirm the account in the browser.
2. With that Music tab selected, open Tide's companion popup and choose
   **Import selected tab** (or **Retry import**).
3. Close the popup and click **Library** in YouTube Music within two minutes.
   Opening the popup alone does not authorize capture.
4. Reopen the popup. Wait for **Session imported. Open Tide to use your Library.**
5. Open Tide from the Start menu. Check the account, playlists and recommendations
   before playing. If an already-open Tide does not adopt the connection, close
   and reopen it, then refresh Library. Confirm the intended identity before
   importing again; a successful import replaces Tide's saved session.

The helper verifies the captured session before saving an encrypted copy for
your Windows user. The extension does not refresh sessions in the background.
You should not need to keep Chrome open after import, but that continuity still
requires release validation. Browser cookie rotation or YouTube rejection may
require reconnecting; a weekly minimum lifetime is not guaranteed.

## If it fails

| Popup result | Next step |
| --- | --- |
| Browser cannot find or accept the local host | Confirm setup completed with its Chrome option enabled, under the same Windows user, using the ID currently displayed in `chrome://extensions`. The advanced launcher's Chrome choice is equivalent. |
| Host does not allow this extension ID | Check for an ID change after moving or reloading the unpacked folder. Use the matching setup; do not weaken its allowed-origin policy. |
| Host cannot start, is outdated, or gives an invalid reply | Update from matching official app/companion packages and retry **Check local connection**. Preserve existing accounts; report persistent failures. |
| Capture expired or background stopped | Select the Music tab, retry import, close the popup and click Library promptly. Chrome may suspend the service worker. |
| No explicit account, rejected session, or failed Music request | Confirm the intended account is signed in and Library works in Chrome, then retry once. Report repeated failures rather than repeatedly exporting credentials. |
| Session imported, wrong library in Tide | Confirm the Music identity in Chrome and explicitly import again from that identity's tab. |

Bug reports can include Tide version/commit, Chrome version, the fixed popup
message and which step failed. **Do not include cookies, request headers,
authentication files or developer-tools screenshots containing them.**

## Update or remove

For extension updates, close the popup and cancel pending capture. Replace files
in the same extension folder with the matching new Chromium package, then use
**Reload** on its card at `chrome://extensions`. Check its ID and local connection
again. Moving the folder can change the unpacked ID; the helper only permits the
ID it was registered for. App updates preserve the recorded browser selection.

To remove Tide's saved credentials, choose **Disconnect** in Tide first. In
`chrome://extensions`, choose **Remove** for the companion. Remove Tide through
Windows Settings or **Uninstall Tide** in the Start menu to remove its owned
native-host registration. Removing the extension alone does not remove Tide's
saved session or the Windows registration; uninstalling Tide preserves saved
preferences and sessions. The stable unpacked extension folder can be deleted
after removing the extension from Chrome.

## What the permissions mean

- **YouTube Music site access / webRequest:** observe the allowlisted Music
  request. Capture is armed only after your explicit action for the selected tab.
- **activeTab:** identify the tab you selected for this transfer.
- **nativeMessaging:** send the bounded session inputs to the registered local
  Tide host. It does not upload them to a Tide server.

The controller only transfers allowed headers from one successful Music browse
request in the selected tab. It has no content scripts, browser storage,
telemetry, remote scripts or independent network fetch. It does not collect your
Google password or unrelated browsing history. The local provider contacts
YouTube to verify and use the session; native playback uses temporary scoped
cookies as described in [account setup](ACCOUNT_SETUP.md).
