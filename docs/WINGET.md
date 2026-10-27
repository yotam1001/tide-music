# WinGet packaging

Proposed package identifier: `TideMusic.Tide`. This document describes packaging
preparation; it does not mean the package is already in Microsoft's catalog.

The original v0.4.5 `TideSetup.exe` has no silent mode. Do not submit a manifest
that passes `--silent` to that file. The app-only preview ZIP also omits managed
playback dependencies and must not be listed as a self-contained portable app.

## Installer artifact

For an installer-only packaging revision of v0.4.5, build the new wrapper around
the exact published `tide-windows-preview.zip`, preserving the application bytes:

```powershell
pwsh -File scripts/build-installer.ps1 -Package .local/winget-prep/tide-windows-preview.zip -Destination dist/winget/TideSetup-winget-r2.exe -AppVersion 0.4.5
```

The ZIP's published SHA-256 is
`609badd91ae376ef203d126d38c9dff5a728bdc7189a0b6d558df997c282c379`.
The wrapper validates the ZIP and each embedded file before installation. It
installs per user, downloads checksum-pinned private playback dependencies,
registers `Tide` / `Tide contributors` / `0.4.5` in Windows Apps, and supports
`--silent`. Its existing uninstall command is noninteractive. Neither operation
requires elevation. The original release asset must not be overwritten. Revision r2 also isolates the Windows PowerShell module path so setup works when launched from PowerShell 7.

## Manifests

```powershell
python scripts/generate-winget-manifests.py --installer dist/winget/TideSetup-winget-r2.exe --version 0.4.5 --installer-url https://github.com/yotam1001/tide-music/releases/download/v0.4.5/TideSetup-winget-r2.exe --output .local/winget-prep/manifests/t/TideMusic/Tide/0.4.5
winget validate --manifest .local/winget-prep/manifests/t/TideMusic/Tide/0.4.5 --disable-interactivity
```

The generator hashes the installer supplied to it. Its URL is a proposed asset
location until publication; schema validation alone does not establish that the
asset exists, can be downloaded, or installs successfully.

## Release and submission gates

1. Test on a clean Windows VM: unattended install, playback dependency setup,
   Apps version detection, repeat install/update, and unattended uninstall.
   Test ordinary interactive setup too. Preserve saved account data.
2. Obtain approval to publish the additional `TideSetup-winget-r2.exe` release asset.
   Publish its SHA-256 alongside it. Download the public asset and compare its hash
   with `InstallerSha256`; never point these manifests at the old GUI-only EXE.
3. In the test VM, enable local manifests if needed and run
   `winget install --manifest <directory> --silent`. Confirm `winget list --id
   TideMusic.Tide --exact` detects the intended version, then test uninstall.
4. Check the community repository for an existing package or competing PR.
   Follow its current contribution instructions and pull request template.
5. Obtain approval for the public contribution. Submit only the three YAML
   files under `manifests/t/TideMusic/Tide/0.4.5`. Do not attest that a CLA is
   signed or end-to-end installation was tested unless it actually was.

Future releases should use the versioned wrapper as the normal `TideSetup.exe`,
pass their exact version at build time, and regenerate manifests from the final
public artifact. Do not reuse an older installer's hash.
