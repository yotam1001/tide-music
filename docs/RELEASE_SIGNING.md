# Maintainer release signing

Users install Tide normally. This document is for maintainers publishing updates.
The application verifies Ed25519 signatures using ring, which is already part of
its TLS dependency tree. Python cryptography is a maintainer/CI test dependency,
not an extra process or dependency added to the music player's listening path.

## Key custody

The public key is `crates/app/update-public-key.hex`. The private key is outside
the checkout in `%LOCALAPPDATA%/TideReleaseSigning/ed25519.dpapi`, encrypted by
Windows DPAPI CurrentUser in a directory restricted to its owner. It is not a
GitHub Actions secret. Never upload it, print it, attach it to issues, or put it
in a package. Normal uninstall does not touch this separate maintainer store.

The one-time `python scripts/sign-update.py --init-key` command refuses to
overwrite either an existing store or a pinned public key. It is not a routine
release command or a recovery mechanism. Keep an encrypted Windows user-profile
backup: copying the DPAPI blob alone to another Windows account is not sufficient
for recovery. A lost key requires a separately planned trust migration; silently
generating a new key would strand existing clients. A compromised signing PC is
outside the protection offered against a release-account-only compromise.

## Publish

1. Review the exact source revision, successful hosted tests, installer checks,
   browser packages and fresh-clone privacy scan. Download the tested artifacts.
2. Independently inspect the installer payload, version and source correspondence.
   Do not automatically sign whatever the latest CI job supplies.
3. On the signing PC, install `cryptography` for this maintainer tool if necessary,
   then run:

   ```powershell
   python scripts/sign-update.py --installer <reviewed-folder>/TideSetup.exe --version 0.4.1 --output <publish-folder>/TideUpdate.json
   python scripts/sign-update.py --installer <reviewed-folder>/TideSetup.exe --version 0.4.1 --verify <publish-folder>/TideUpdate.json
   ```

4. Run the packaged verifier against the same files. This checks the actual key
   embedded in the built app without executing setup or reading any account:

   ```powershell
   tide-music.exe --verify-update-files <publish-folder>/TideUpdate.json <reviewed-folder>/TideSetup.exe
   ```

5. Upload `TideUpdate.json`, `TideSetup.exe`, packages and checksums to a draft
   stable release at the matching `v<version>` tag. Verify every uploaded digest
   and size before publishing. After publication, anonymously download and verify
   the manifest and installer again using the packaged verifier.

The signed identity expires in 90 days. Renew it only after reviewing the same
installer again, using a new output path and replacing only the public manifest,
or publish a newer reviewed release. The signing tool never overwrites an output
file implicitly. Version, platform, filename, size, SHA-256 and validity times are
all signed. The server cannot select a new verification key. Version checks stop
an older release being offered to a newer app; withholding updates can still
deny availability. This is a focused signed-update scheme, not a full TUF client.

The first upgrade from clients through 0.4.0 still uses their previous trust
model; code in a future release cannot retroactively protect an older executable.
Manual installer downloads still use Windows' existing publisher/SmartScreen
checks: the update signature does not provide Authenticode signing.
