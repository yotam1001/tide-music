# Tide Windows preview

This is a preview, not a verified stable release. Current evidence and remaining
release checks are available in the [source repository](https://github.com/yotam1001/tide-music).

Double-click **TideSetup.exe**, then choose **Install Tide**. The window shows
setup progress and offers **Open Tide** or **Connect YouTube Music** when finished.
No ZIP extraction or commands are needed. Setup installs Tide
for your Windows user, downloads pinned playback tools and a private Python
runtime, and adds **Tide** and **Uninstall Tide** to the Start menu. Internet access
and Windows x64 are required; no administrator privileges, system Python or manual
PowerShell commands are needed. See [the installation guide](docs/INSTALLATION.md)
for update behavior, Windows Settings removal and the optional advanced browser
helper setup. The ZIP and command launcher remain an alternative for developers.

Launch Tide from the Start menu. Open a local audio file, paste a music link, or
search from the header. Home and Library are the main destinations; Queue opens
the upcoming tracks. Account contains theme and window-size controls. Mini has
an expand icon to return to the full window.

## Connect YouTube Music

The optional browser companion is distributed separately. If this package includes
`tide-session-host.exe`, setup can register the helper for your selected browser;
Chrome and Edge require the exact ID of the extension you actually installed.
Chrome native transfer has passed a real import, while Zen's native-host lookup
and signed public companion distribution remain unresolved. Registration alone
does not establish that a browser can connect.

Manual connection remains available in [the account guide](docs/ACCOUNT_SETUP.md).
For an isolated session, sign into your existing account in a private Zen window,
copy the local request headers, then close that window without signing out. Zen
does not need to stay open. Tide verifies access before showing Connected.
Weekly session lifetime is still under validation.

Credentials remain on your computer, encrypted for your Windows user. Never
upload cookies, session files or your browser profile to GitHub or send them in
chat. Premium playback depends on the supplied account and service compatibility.

## Updates and removal

Close Tide and run the next **TideSetup.exe**.
Dependencies are prepared before switching the installed app. Saved preferences
and encrypted sessions are preserved. Choose **Tide → Uninstall** in **Windows Settings → Apps**, or **Uninstall Tide**
in the Start menu, to remove owned app files, shortcuts and matching native-host
registration.
Use Disconnect inside Account first if you also want to remove the saved session.
Browser extensions are removed separately in the browser.

The app source uses MIT; third-party licenses are included in LICENSE,
THIRD_PARTY_NOTICES.md and DEPENDENCIES.md. Downloaded tools retain their own
licenses and are obtained directly from upstream rather than bundled in this ZIP.
