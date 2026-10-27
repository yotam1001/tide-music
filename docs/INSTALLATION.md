# Windows preview installation

Double-click **TideSetup.exe** and choose **Install Tide**. Setup shows progress
and a retryable error if a step fails. At completion choose **Open Tide**, or
**Connect YouTube Music** to open Account directly. No commands or ZIP extraction
are required. The full ZIP with **Install Tide.cmd** remains an advanced alternative.
Setup needs internet access and Windows x64. It installs for the current user in
`%LOCALAPPDATA%\Programs\Tide`, without administrator privileges, PATH changes,
system Python or manual PowerShell commands. Launch **Tide** from the Start menu.
The installed release contains its own mpv, yt-dlp, Deno and private account-helper
Python runtime. The downloaded installer can be removed after setup succeeds.

The single executable embeds the preview ZIP and its SHA-256 digest. It validates
the embedded digest, exact file allowlist and individual checksums before staging
any files in a private temporary folder. The temporary payload is removed after
setup. It does not contain credentials, a browser profile or an embedded browser.

Setup validates the packaged SHA-256 manifest before copying its explicit payload.
Playback downloads use exact upstream release versions and require upstream
SHA-256 digests matching the checksums pinned in the setup script; a missing digest
or mismatch stops setup. Embedded CPython 3.14.0
uses the checksum from the [official Windows release index](https://www.python.org/ftp/python/3.14.0/windows-3.14.0.json).
The exactly pinned helper dependencies are universal wheels fetched from PyPI;
each wheel must match both its published and script-pinned SHA-256 digest. No existing Python environment,
browser profile or saved account data is copied into the installation. Upstream
archives, Python license and wheel license metadata remain in the owned release.

## Optional browser companion

The graphical installer opens Account for connection guidance and does not ask
for request headers or extension IDs. It does not install an unsigned extension
into the browser automatically. Signed companion distribution and Zen lookup
remain unresolved.

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
the helper. Choosing None skips first-install registration. On update, the existing
browser choice is retained unless another browser is explicitly selected.
The browser extension itself is installed and removed in the browser; setup owns
only its Tide native-host registration. Manual import remains available in
[ACCOUNT_SETUP.md](ACCOUNT_SETUP.md).

## Update

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
Enterprise PowerShell policy may prevent the launcher from running. Fresh
installation with real downloads, public companion distribution and longer
playback/session reliability remain release validation requirements.

## Build setup from source

On Windows, build and validate the preview ZIP first, then run:

```powershell
pwsh -File scripts/build-installer.ps1 -Package dist/tide-windows-preview.zip -AppVersion 0.1.0-preview
```

The wrapper uses the Windows .NET Framework compiler, WinForms, and the existing
installer core. No additional installer framework or player runtime is introduced.
CI builds **TideSetup.exe** alongside the advanced ZIP and verifies its embedded
payload without installing it. `--preview` exercises only the setup screens;
`--verify-payload` checks and removes the temporary payload without installing.

Newly built installers also support `--silent`: this runs the same verified
payload without opening setup screens, waits for installation, and returns the
underlying install script's exit code. Close Tide before updating. `--help`
lists supported arguments; unknown or combined arguments fail without opening
the GUI. Always pass the actual release version as `-AppVersion` when building;
this is embedded in setup and used for Windows Apps version detection.

See [WINGET.md](WINGET.md) for manifest generation and submission checks.
