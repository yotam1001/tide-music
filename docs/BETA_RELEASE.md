# Windows beta release preparation

The candidate binaries come from successful Windows CI at `1e403b7`. They are
unsigned previews, not a stable release. The graphical installer contains the
player and native session receiver; playback tools and the private Python
runtime are downloaded and checksum-verified during installation.

## Candidate downloads

- `TideSetup.exe`: graphical per-user installation, launch and Windows Settings removal.
- `tide-windows-preview.zip`: advanced installation, including explicit Chrome native-helper registration.
- `tide-browser-chromium.zip`: unpacked Chrome extension; extract to a stable folder and load the folder containing `manifest.json`.
- `CHROME_SETUP.md`: complete beta connection instructions.
- `SHA256SUMS.txt`: download integrity checks; use only assets from this repository.

The Chrome companion is not on the Chrome Web Store. The graphical setup alone
does not register its native helper. Follow [Chrome beta setup](BROWSER_COMPANION_BETA.md)
for the advanced launcher and exact extension-ID step. This extra step remains
a public-onboarding limitation; do not advertise automatic one-click login.

## Verification performed

The downloaded CI installer passes its actual embedded-resource extraction and
checksum verification. Its matching ZIP passes payload allowlist, checksum and
privacy checks. CI checks and preview packaging both passed.

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
