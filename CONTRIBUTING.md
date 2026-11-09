# Contributing

Keep playback, provider access and interface code separate. Preserve the full charcoal/burgundy and compact earthy palette directions. Avoid adding continuous animations, polling or unbounded artwork caches.

Follow the guarded format, test and Clippy commands in the
[developer guide](docs/DEVELOPMENT.md) before proposing changes. Windows runtime
smoke checks are required for playback and media-control changes.

Development and test profiles disable incremental compilation and use limited
debug information by default. Slint's generated code can otherwise retain very
large compiler snapshots across repeated builds. The tradeoff is slower repeat
compilation and less debugger detail; source builds still need several GB of
disk space. Release settings are unchanged. Installed-player users do not need
Rust, a `target/` directory or these development caches.

For local compilation use `./scripts/cargo-local.ps1 -CargoArgs @('test',
'--workspace','--locked')` (substitute `build` or `clippy` as needed). The wrapper
checks the output drive for 5 GiB of free space before and after compilation and
disables incremental snapshots. Clear the current checkout's generated Cargo
output after local validation if it is no longer needed. CI runs on disposable
hosted runners so they do not accumulate on a contributor's PC.

Do not commit credentials, local toolchains, runtime binaries, personal music, logs or machine-specific paths. Explain resource impact when changing buffering, rendering or background work.

After source checks pass, Windows CI builds a preview, generates target-specific
dependency notices and validates the ZIP's exact payload, checksums and privacy
patterns before uploading it as a temporary workflow artifact. This is a preview
for review, not an automatic stable release. Runtime tools and account sessions
are not bundled. A green package build does not replace the live playback and
UI evidence required in [docs/RELEASE_READINESS.md](docs/RELEASE_READINESS.md).

GitHub's MSVC preview uses `scripts/build-preview.ps1 -StaticMsvcRuntime` and
packages `target/x86_64-pc-windows-msvc/release/tide-music.exe`. The explicit
target keeps static C-runtime flags off host build tools. The local GNU build
continues using its self-contained linker and normal output location.
