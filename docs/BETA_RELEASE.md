# Windows beta release preparation

The candidate binaries come from successful Windows CI at `f026915`. They are
unsigned previews, not a stable release. The graphical installer contains the
player and native session receiver; playback tools and the private Python
runtime are downloaded and checksum-verified during installation.

## Candidate downloads

The `v0.1.0-beta.1` candidate is an **unpublished draft**. No public release is
available yet. Check the [official releases page](https://github.com/yotam1001/tide-music/releases)
for publication; signed-in maintainers can inspect the draft. The matching
[successful CI run](https://github.com/yotam1001/tide-music/actions/runs/38751976892)
also provides artifacts, which may require GitHub sign-in to download.

- `TideSetup.exe`: graphical per-user installation, launch and Windows Settings removal.
- `tide-windows-preview.zip`: advanced installation, including explicit Chrome native-helper registration.
- `tide-browser-chromium.zip`: unpacked Chrome extension; extract to a stable folder and load the folder containing `manifest.json`.
- `CHROME_SETUP.md`: complete beta connection instructions.
- `SHA256SUMS.txt`: download integrity checks; use only assets from this repository.

The Chrome companion is not on the Chrome Web Store. The candidate's graphical
setup offers optional native-helper registration after the extension is loaded
and its exact ID is supplied. Follow [Chrome beta setup](BROWSER_COMPANION_BETA.md) and use matching
packages. Loading the unpacked extension and importing a selected session remain
explicit steps; do not advertise automatic one-click login.

## Verification performed

The downloaded CI installer passes its actual embedded-resource extraction and
checksum verification. Its matching ZIP passes payload allowlist, checksum and
privacy checks. CI checks and preview packaging both passed.

Both final push and PR CI runs passed the real clean-Windows lifecycle job:
installation, update, actual Start-menu shortcuts and HKCU uninstall/native-host
registration, runtime checks, installed native local-audio UI and registered-command
removal. Downloaded evidence confirms success. The installer embeds the exact
matching app ZIP; both companion archives match the candidate source (text line
endings normalized), icon bytes and their checksum manifests. See
[final candidate validation](VALIDATION.md#final-candidate-and-real-windows-lifecycle-2026-10-15).
These immutable binaries remain tied to `f026915`; later evidence-only documentation
changes do not produce a new binary candidate.

`scripts/verify-installed-preview.ps1` is an opt-in network/desktop integration
probe. It uses the actual packaged installer core with real downloads, private
runtimes, files, Start-menu-format shortcuts in a disposable folder, and installed
app UI assertions with silent local audio. It tests an update and removal too.
Only Windows uninstall registry I/O is replaced by an in-memory fixture. The
player receives isolated application-data directories, so it does not use the
developer's saved account. Screenshots and diagnostics remain under ignored
`.local/verify-install-<random>/` directories. It does not install into the real
Start menu or modify the current Tide installation.

```powershell
pwsh -NoProfile -File scripts/verify-installed-preview.ps1 -PackageRoot <extracted-preview-folder>
```

This probe is not a clean Windows VM test or a test of clicking through the actual
setup GUI. It also does not establish authenticated streaming/session longevity,
physical media keys in games, or Chrome Web Store availability.

## Checks required before publishing the beta

The `installed-preview` CI job adds a separate check on a disposable hosted
Windows runner using actual HKCU registrations, Start-menu shortcuts, private
runtimes, a synthetic Chrome origin, update and removal. Its guarded script
refuses this development machine and preserves diagnostic artifacts in CI.
This closes only the system-integration checks it actually passes; it does not
replace clicking through setup or connecting a real Google account.

On a disposable Windows x64 VM or another test user/machine:

1. Download the candidate installer from the official draft/release and verify its checksum.
2. Run the actual GUI; confirm clear download progress, successful launch, and Start-menu discovery.
3. Follow the Chrome companion guide, register the exact extension ID, and import the intended account.
4. Confirm playlists and recommendations; close Chrome and test natural transitions, skip, pause and seek.
5. Test physical Fn media keys with a game focused and minimize/restore without resizing.
6. Close Tide; update through the GUI; verify the account and preferences remain available.
7. Remove Tide through Windows Settings; verify owned files/shortcuts/registrations are removed and unrelated files remain intact.
8. Record Windows version, tested commit, results and limitations without credentials, library contents or private paths.

Publish as a prerelease only after recording these results and disclosing any
accepted beta limitations. Stable release additionally requires longer live
session/recovery evidence. Windows signing and store distribution improve trust
but have not been completed by preparing these assets.
