# Connect your YouTube Music account

Tide can read your existing playlists, Liked Songs and Home recommendations and pass your signed-in YouTube session to native playback. This is an unofficial integration. Premium benefits depend on a valid Premium account and compatible YouTube extraction; Tide does not independently verify a subscription or guarantee that every track is available.

## Install the optional library helper

For the Windows preview, run **TideSetup.exe** and choose **Install Tide**.
**Connect YouTube Music** on the completion screen opens Account in Tide. Setup provisions a private Python runtime and the helper dependencies; no
system Python is required. See [installation and removal](INSTALLATION.md).

The commands below are for a development checkout.

From the project directory, with Python 3.10 or newer installed:

```powershell
pwsh -File scripts/setup-runtime.ps1
pwsh -File scripts/setup-provider.ps1
cargo run -p tide-music
```

Python runs only while a library request is active and exits afterward. The native app and mpv handle playback. No application API key or Google Cloud project is required.

## Import a session locally

Manual request-header import can use your preferred browser; Zen is not required
for that path. Account help opens in the Windows default browser. Automatic
session transfer across browser families is still pending the work described in
[browser connection requirements](BROWSER_CONNECTION.md); opening a browser alone
does not connect its account to Tide.

For the most isolated session, use the manual request-header import below from a
private Zen window signed into your existing Google account. Select the intended
YouTube Music identity, copy a successful request, then close the private window
without signing out. This path passed the current signed-in continuity checks;
weekly session lifetime is still being verified. It does not require a new account
or keeping Zen open.

Alternatively, sign in to YouTube Music in your existing regular Zen browser.
Open Tide's **Account** menu and choose **Connect account**. The file picker opens
the normal Zen Profiles directory. Open your profile folder and select
**cookies.sqlite**. Tide reads only scoped YouTube cookies from that selected file
using read-only SQLite, verifies account access and encrypts the session locally.
Regular-browser import can be rejected when saved cookies differ from an active
request; use manual import if this happens. Continued browser cookie rotation can
also invalidate an exported session.

The initial connection defaults to your first signed-in Google account. **Reconnect preserves the account selected in the menu**: Account 2 selects your second signed-in account, and so on. You can also use **Switch** to verify a different selection before saving it; an unavailable account leaves the stored connection unchanged. The heading says **Connected** only after account verification; a saved session alone is not proof of access. Brand channels and custom Zen profile locations can use the manual import below. If you are unsure which profile is active, Zen's `about:support` page shows its Profile Folder. Do not share the cookie database or its contents.

Profile import selects cookies that apply to `music.youtube.com`; cookies limited
to the root host `youtube.com` are excluded. If matching authentication cookies
contain conflicting values, Tide asks for manual request-header import rather
than guessing which sign-in the browser uses.

Manual request-header import:

1. Open [YouTube Music](https://music.youtube.com/) in your browser and sign in to the account with your Premium subscription. Confirm the correct identity and that official playback is ad-free.
2. Follow the request-header instructions in the [ytmusicapi browser authentication guide](https://ytmusicapi.readthedocs.io/en/stable/setup/browser.html): open browser developer tools, select Network, filter for `browse`, and navigate inside YouTube Music until a request appears. Copy its **request headers**. Tide also accepts the browser authentication JSON produced by that guide.
3. Save those headers as a UTF-8 text file **outside this repository**. The request must include its Cookie header, including `__Secure-3PAPISID`. Never send the file or its contents through chat, GitHub, screenshots or bug reports. It grants account access.
4. In Tide, open **Account → Advanced**, choose **Import session**, and select the local file. Wait for the account name and playlists to appear. Check that the name matches your intended account. Choose Liked Songs, a playlist or a playable recommendation. These steps are the fallback if Connect account cannot sign in.

Tide encrypts its saved copy with Windows DPAPI for your Windows user and restricts the account directory to its owner and SYSTEM. Connect account reads only the cookie database you explicitly selected from your Zen profile. With manual import, the original file remains under your control: remove it when you no longer need it, or store it securely outside Git. A session may expire or rotate; reconnect when prompted. Disconnect removes Tide's saved session and clears the displayed library.

Zen does not need to remain open after importing. Successful account-helper requests retain scoped cookie updates in the encrypted session and refresh the playback lease. Native extraction returns bounded cookie updates for the same account; Tide merges them only against their original session, preserving newer credentials and unrelated cookies. A content request returning HTTP 401/403 triggers a separate identity check before Tide asks for reconnect; a timeout or temporary content denial leaves the verified account intact. These mechanisms do not guarantee a minimum session lifetime.

API refreshes compare the authentication-cookie baseline rather than rejecting
every change to the Cookie header. A concurrent non-authentication update is
preserved while a valid sign-in refresh is saved; a conflicting newer sign-in
still prevents a stale response from replacing it.

Playback requires a temporary Netscape cookie file restricted to `.youtube.com` in that private directory. Normal shutdown removes it after the owned player processes end. After a crash, the next startup/disconnect removes leases whose owning process has exited; inaccessible or reused process IDs are conservatively retained. The temporary file is plaintext while it exists. Persistent session storage is encrypted.

## What sync does

- Reads up to 200 playlists and 2,000 tracks per playlist/Liked Songs, reporting truncation.
- Reads up to three Home shelves, 40 cards each. Song and playlist recommendations are playable. Album/artist cards are displayed but do not yet open.
- Fetches fresh account data on refresh; selecting a playlist replaces Tide's queue. This does not copy audio files, change your YouTube library, or migrate data to another service.
- Sends no likes, playlist edits or playback-history updates. Listening in Tide therefore does not currently train YouTube recommendations.

YouTube playback is gated until the imported account has been verified by a successful library response. Local audio continues to work without library verification. A successful library response verifies account access, **not Premium entitlement**. For additional Google accounts, native extraction passes the selected account header and skips YouTube webpage/config bootstraps so they cannot replace that selection. This uses documented yt-dlp options; some videos may need additional provider metadata or tokens. No ad-blocking feature is implemented: signed-in playback uses the session you supplied. A local selected-account extraction test detected a Premium subscription and returned playable audio formats. This is a short live check, not a guarantee for every track; extraction can fail or require additional provider tokens on some content. Tide does not silently discard the imported cookies to retry anonymous playback.

## Troubleshooting

Run the provider setup script if a library request reports an unavailable helper. Reimport headers from a new signed-in request if access fails. Browser-authenticated extraction is described in [yt-dlp's YouTube notes](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies); cookies alone do not guarantee all formats. OAuth is not the default here because the unofficial Music endpoint's compatibility is unreliable.

The same upstream notes explain that open YouTube tabs can rotate account
cookies and recommend an isolated private-browser session for stable exports.
Closing one Music tab does not establish that every browser window or background
session has ended. Tide's own cookie updates cannot reconcile a separate browser
rotation it never received. An isolated session still uses your existing Google
account; it does not require creating a new account. Session longevity remains
under live verification, so repeated reconnects should be reported as a bug.

Portable copies need the optional helper runtime too. For custom locations, set `TIDE_PROVIDER_PYTHON` to the virtual environment's Python executable and `TIDE_PROVIDER_HELPER` to `helpers/youtube_music.py`. Credentials are passed through stdin, never command-line arguments or application logs. Queue metadata (titles and source URLs) is saved separately under the user application-data directory.
