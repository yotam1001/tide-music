# Contributing

Keep playback, provider access and interface code separate. Preserve the full charcoal/burgundy and compact earthy palette directions. Avoid adding continuous animations, polling or unbounded artwork caches.

Run `cargo fmt --all -- --check`, `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` before proposing changes. Windows runtime smoke checks are required for playback and media-control changes.

Do not commit credentials, local toolchains, runtime binaries, personal music, logs or machine-specific paths. Explain resource impact when changing buffering, rendering or background work.

After source checks pass, Windows CI builds a preview, generates target-specific
dependency notices and validates the ZIP's exact payload, checksums and privacy
patterns before uploading it as a temporary workflow artifact. This is a preview
for review, not an automatic stable release. Runtime tools and account sessions
are not bundled. A green package build does not replace the live playback and
UI evidence required in [docs/RELEASE_READINESS.md](docs/RELEASE_READINESS.md).
