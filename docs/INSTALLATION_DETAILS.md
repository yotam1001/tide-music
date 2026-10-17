# Installer internals for contributors

For normal setup, use [Install Tide](INSTALLATION.md). This reference keeps the
payload, runtime verification and ownership details separate from the user guide.
Current release evidence is tracked in [Release readiness](RELEASE_READINESS.md).

Double-click **TideSetup.exe** and choose **Install Tide**. Setup shows progress
and a retryable error if a step fails. At completion choose **Open Tide**, or
**Connect YouTube Music** to open Account directly. Installing the player requires
no commands or ZIP extraction. Optional Chrome connection requires loading the
separate companion extension, as described below. The full ZIP with **Install
Tide.cmd** remains an advanced alternative.
Double-click the CMD launchers so each runs in its own console. They close that
console after showing the result; from an existing CMD session, launch them through
`cmd /c` to preserve the calling shell.
Setup needs internet access and Windows x64. It installs for the current user in
`%LOCALAPPDATA%\Programs\Tide`, without administrator privileges, PATH changes,
system Python or manual PowerShell commands. Launch **Tide** from the Start menu.
The installed release contains its own mpv, yt-dlp, Deno and private account-helper
Python runtime. The downloaded installer can be removed after setup succeeds.

Installer packages contain only the explicit application payload. Rust compilers,
`target/` build outputs and local development/test caches are excluded; package
validation rejects them. Installed Tide does not compile Rust or create a Cargo
build cache while listening.

The single executable embeds the preview ZIP and its SHA-256 digest. It validates
the embedded digest, exact file allowlist and individual checksums before staging
any files in a private temporary folder. The temporary payload is removed after
setup. It does not contain credentials, a browser profile or an embedded browser.

Setup validates the packaged SHA-256 manifest before copying its explicit payload.
Playback downloads use exact upstream release URLs and must match the SHA-256
checksums pinned in the setup script before extraction or execution. A missing
pin or mismatching download stops setup. This does not require a GitHub account,
API token or release-metadata API request; an API rate limit cannot block these
playback downloads. GitHub's file hosting and your connection must still work.
Embedded CPython 3.14.0
uses the checksum from the [official Windows release index](https://www.python.org/ftp/python/3.14.0/windows-3.14.0.json).
The exactly pinned helper dependencies are universal wheels fetched from PyPI;
each wheel must match both its published and script-pinned SHA-256 digest. No existing Python environment,
browser profile or saved account data is copied into the installation. Upstream
archives, Python license and wheel license metadata remain in the owned release.

## Optional browser companion

For Chrome, follow the [companion beta guide](BROWSER_COMPANION_BETA.md): load the
matching Chromium extension in Chrome first, then run **TideSetup.exe** and select
**Enable Chrome companion connection (optional)**. Paste the **actual extension ID
displayed by Chrome** into the ID field before choosing **Install Tide**. This
option starts unchecked and registers the local helper only when explicitly
selected. It accepts exactly 32 lowercase letters `a` through `p`; it never
guesses an ID or grants wildcard origins. An existing different Tide native-host
registration is preserved and causes setup to stop.

Setup does not ask for request headers or install an unsigned extension into the
browser automatically. **Connect YouTube Music** opens Account; it does not
import a session. After setup, use the companion's **Check local connection**,
then explicitly **Import selected tab** as the guide describes. Successful
registration alone does not prove account access. The new graphical registration
path still needs a live Chrome setup/import check. Signed companion distribution
and Zen lookup remain unresolved.

In the advanced ZIP launcher, if the ZIP includes `tide-session-host.exe`, setup can register the native helper
for one selected browser under the current user's registry. Install the separate
Tide companion extension in that browser first. Choose Chrome or Edge at the setup
prompt, then paste the **actual extension ID displayed by that browser**. Setup
accepts only a complete 32-letter Chromium ID; it never guesses an ID or grants
wildcard origins. An existing different Tide native-host registration is preserved
and causes setup to stop.

Firefox registration uses the companion's fixed add-on ID. Zen's real native-host
lookup is still unresolved, and signed public companion distribution remains an
open release gate. A prepared registration does not prove that a browser can use
the helper. Leaving the graphical Chrome option unchecked (or choosing None in
the advanced launcher) skips first-install registration. On update, the existing
browser choice is retained unless another browser is explicitly selected.
The browser extension itself is installed and removed in the browser; setup owns
only its Tide native-host registration. Manual import remains available in
[ACCOUNT_SETUP.md](ACCOUNT_SETUP.md).

## Update

Installed Windows copies check GitHub for a newer **published stable release**
once after startup. Turn this off with **Settings → Startup checks**; **Check for
updates** remains available. Drafts, prereleases and CI artifacts are excluded.
The current app version comes from Cargo; maintainers must increase it for each
new published release and make the release tag match `v<version>`.
This development build identifies itself as `0.1.0-preview.2`, so it can also
recognize a future stable `0.1.0` as newer than the preview.

When an update is available, Settings gains an indicator. Choose **Install and
restart** to download it, stop playback, run setup and reopen Tide. Nothing is
downloaded or installed merely because a version is available. This initial
updater-enabled build still needs installing manually in older copies of Tide.
Portable/development copies can check versions but do not install automatically.

The updater requires the exact Tide repository's `TideSetup.exe` asset and its
GitHub SHA-256 digest. Downloads use HTTPS, restricted redirect hosts, bounded
streaming and checksum verification again immediately before launch. This trusts
Tide's GitHub release publishing account; it is not a separate code-signing
signature. No Google session, account data, API key or listening history is sent.
The public version check contacts GitHub, which sees normal connection metadata.
There is no resident update service or continuously running download thread.

Only one installer and one partial download are retained in the owned local
`TideMusic/updates` cache, each limited to 96 MiB. Failed verification leaves the
previous installer intact. Setup waits up to two minutes for Tide to exit;
it never force-stops playback processes. Existing browser choices are preserved.

You can also download and run the newer installer yourself; uninstalling first
is unnecessary.

Close Tide and let any pending companion request finish. Run the new
**TideSetup.exe** and choose **Install Tide** again. Setup refuses to stop running processes. It builds
a fresh private release and verifies dependencies before changing Start menu
shortcuts or native-host registration. A failed preparation leaves the previous
installation usable. Registration/shortcut failures restore the previous links
and matching helper registration. Successful updates remove previous owned files;
additional files placed there by the user are preserved. Sessions and preferences
stay in Tide's application-data directory, outside the install directory.

## Remove

Choose **Tide → Uninstall** in **Windows Settings → Apps**, **Uninstall Tide**
in the Start menu, or **Uninstall Tide.cmd** from the advanced ZIP. Close Tide first. Removal deletes recorded app/runtime files
and only shortcuts and native-host keys that still point to that installation.
Different registrations, modified shortcuts and additional unowned files are
preserved. Paths containing junctions or symbolic links are rejected. Removal
does not inspect or delete saved preferences or encrypted sessions. To remove
a saved session, use **Disconnect** in Tide before uninstalling. Remove the browser
companion extension through the browser's extension manager separately.

This is a graphical, unsigned preview installer, not a verified stable release. Windows may display an unsigned-download warning.
Enterprise PowerShell policy may prevent the launcher from running. Real clean
Windows install/update/removal and graphical update/restart have passed CI.
Fresh-user interactive Chrome onboarding, public companion distribution and
longer playback/session reliability remain release validation requirements.

## Build setup from source

On Windows, build and validate the preview ZIP first, then run:

```powershell
pwsh -File scripts/build-installer.ps1 -Package dist/tide-windows-preview.zip
```

The wrapper uses the Windows .NET Framework compiler, WinForms, and the existing
installer core. No additional installer framework or player runtime is introduced.
CI builds **TideSetup.exe** alongside the advanced ZIP and verifies its embedded
payload without installing it. `--preview` exercises only the setup screens;
`--verify-payload` checks and removes the temporary payload without installing.
