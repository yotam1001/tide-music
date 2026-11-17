# Building Tide for macOS 14+

Requires macOS, Xcode command line tools, Rust, and Python 3.12+.
Run `scripts/package-macos.sh --arch arm64` (or `x86_64`). `--out` selects
the output directory; `--no-runtime` is a development-only bundle without
playback helpers. `--arch universal` builds a universal Tide executable, then
produces separate arm64 and Intel DMGs with the matching runtime. `Tide.dmg`
is always an alias of the arm64 DMG. The stable bundles omit the experimental
Jam client.

The package script verifies runtime downloads, generates the icon from the
largest existing PNG, signs inner Mach-O files and nested apps before Tide,
and checks portability and headless CLI startup before making the DMG. The final
image is quarantined, mounted, and copied for strict signature and Gatekeeper
assessment. Keep post-signing probes read-only inside Tide.app.
The mpv bundle remains intact under Resources, with a relative bin/mpv symlink.

Signing defaults to ad-hoc. Set `MACOS_SIGNING_IDENTITY` to an installed
Developer ID identity for hardened runtime signing with Tide.entitlements and
signing the DMG.
For notarization, supply `MACOS_NOTARY_PROFILE` (notarytool keychain profile),
or all of `MACOS_NOTARY_APPLE_ID`, `MACOS_NOTARY_PASSWORD` (app-specific),
and `MACOS_NOTARY_TEAM_ID`. The DMG is submitted, stapled and validated.
Credentials and signing certificates are never included in the bundle.

Release CI generates SHA-256 files, not signed update manifests. The maintainer
must follow docs/RELEASE_SIGNING.md locally before enabling signed updates.
Ad-hoc CI artifacts still need real macOS 14 hardware playback, Gatekeeper,
account, media-key and audio-output testing. CI verifies the Intel disk image
on a native Intel runner.
