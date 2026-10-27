# Install Tide on Windows

You need **Windows x64** and internet access. The installer sets up the playback
tools for you. No Rust, Python, terminal commands or API key are needed.
Installation is for your Windows user and does not require administrator access.

Download [TideSetup.exe](https://github.com/yotam1001/tide-music/releases/latest/download/TideSetup.exe)
from the [Windows release](https://github.com/yotam1001/tide-music/releases/latest).
For YouTube Music, get its matching **tide-browser-chromium.zip** too.

## Install and open

For YouTube Music, begin with the
[Chrome connection guide](https://github.com/yotam1001/tide-music/blob/main/docs/BROWSER_COMPANION_BETA.md).
Loading the companion before opening setup lets you enable its connection during
the first installation. For local files only, follow the steps below directly.

1. Open the release and expand **Assets** if needed.
2. Download **TideSetup.exe**, rather than **Source code (zip)**.
3. Open it and choose **Install Tide**. Stay online while setup downloads and
   checks the playback tools.
4. Choose **Open Tide**, or search for **Tide** in the Windows Start menu.

The current installer is unsigned, so Windows may show an unknown-publisher or
download warning. Verify that it came from this repository before deciding to
continue. If a device policy blocks it, follow your administrator's rules and
report the exact message rather than disabling protections.

For YouTube Music, follow the
[Chrome connection guide](https://github.com/yotam1001/tide-music/blob/main/docs/BROWSER_COMPANION_BETA.md).
It explains **Enable Chrome companion connection** and the extension ID. For
local audio only, leave that checkbox off and use **Settings → More options → Open audio file**.
Rerun setup if you want to enable the companion later.

## Update Tide

For a new **stable** version, open **Settings → Check for updates**. Choose
**Update Tide** when offered. Downloading and verification happen while music
continues, with progress and a Cancel action inside Tide. Then choose **Restart
to update** when convenient. The update applies in the background and reopens
Tide; your account and preferences are kept. See [in-app updates](IN_APP_UPDATES.md).
You can turn startup checks off in Settings.

Setup repairs an older Windows version/icon/uninstall entry only when its
product, installation root and command match Tide and its registered release
passes the ownership and package checksum checks. The readiness check does not
change that entry. Unrelated or modified registrations remain protected.

Updates keep your listening-history choice, Discord presence, theme, volume,
shortcuts and other preferences in your Windows user profile, outside the app's
versioned installation folder. Settings are saved by atomic replacement, with
one recovery backup. An unreadable queue does not reset your other preferences.
History reporting and Discord sharing stay off by default until you enable them.

Preview 1 and preview 2 can discover stable **0.1.3** through this updater.
The updater skips prerelease targets and drafts; it does not skip an installed
preview when a newer stable version exists. If needed, download **TideSetup.exe**,
close Tide and run setup again without uninstalling first. Copies without an
updater need that manual update once.

App updates retain the browser choice recorded by setup. The unpacked Chrome
extension is updated separately: replace its files and choose **Reload** in
Chrome, following the connection guide.

If an update fails, setup aims to leave the previous installation usable. Read
the error before retrying. Setup never force-closes another Tide process.

Only one setup or uninstall can run at a time for the current Windows user.
If setup says another operation is running, let that operation finish before
retrying. Ownership conflicts report the differing field names, without their
values. Do not delete the Windows uninstall entry to bypass a conflict; include
those field names when reporting the problem.

## Remove Tide

1. To remove the saved sign-in too, first choose **Account → Disconnect account**
   in Tide.
2. Close Tide and finish or cancel any companion import.
3. In **Windows Settings → Apps**, find **Tide** and choose **Uninstall**.
   **Uninstall Tide** in Start does the same job.
4. Remove the companion separately from Chrome if you used it.

Uninstalling preserves saved preferences and sessions. Disconnecting removes
Tide's saved session, without signing you out of Google in your browser.

## Common questions

**Where is Tide installed?** Under `%LOCALAPPDATA%\Programs\Tide`. Launch it
from Start; you do not need to browse its installation folders.

**Can I delete the installer afterward?** Yes, once setup succeeds.

**Does listening create huge build files?** No. Installed Tide never compiles
Rust. Development build folders and toolchains are excluded from the package.
The updater keeps one installer and one partial download, each capped at 96 MiB.
The installed playback tools need additional disk space.

**I downloaded a ZIP.** Prefer the installer for normal use. The advanced preview
ZIP contains **Install Tide.cmd**. A GitHub Actions artifact can contain both
**TideSetup.exe** and that advanced ZIP; use the EXE in the outer download.

[Troubleshooting](https://github.com/yotam1001/tide-music/blob/main/docs/TROUBLESHOOTING.md) ·
[Account help](ACCOUNT_SETUP.md) ·
[Installer internals](https://github.com/yotam1001/tide-music/blob/main/docs/INSTALLATION_DETAILS.md)
