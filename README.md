# Tide Music

A Windows-first native Rust music player designed to leave resources available for games.

## Promo video

[![Animated preview of the Tide Music promo video](docs/media/tide-promo-preview.gif)](docs/media/tide-promo.mp4)

[Watch the 48-second video](docs/media/tide-promo.mp4) · [Edit the HyperFrames project](video/README.md)

The video shows the current prototype: real screen recordings of Tide with a connected account. Its memory figure comes from one matched test on one PC; see [how it was measured](docs/media/memory-test.md). Music: Lensko – Let's Go! [NCS Release].

**Working early prototype:** native interface, local/YouTube music playback, search, queue controls and Windows media controls. Optional account integration reads playlists, Liked Songs and recommendations and supplies your signed-in session to playback. Local signed-in library, Premium extraction and muted native playback have been verified; each installation still needs its own session. YouTube features use unofficial tooling and can fail when YouTube changes. This is not an official YouTube product.

## Design

- Home and Library as the two main destinations, with search in the header and an optional Queue drawer.
- Independent dark/light themes and full/compact sizing; the light palette uses warm grey, terracotta and muted purple.
- A mini player with an expand icon using the same queue and playback engine.
- Playlist previews with Play/Shuffle, song menus, upcoming-track reordering/removal, song radio and recommendation autoplay.
- Home Refresh rotates a 24-card selection from up to 240 personalized candidates. Recent IDs are kept only in memory; small upstream pools can still repeat.
- Small real thumbnails with a bounded memory cache and no generic artwork banner.
- Native Slint interface; no embedded browser or Electron runtime.

![Full native interface](docs/screenshots/full.png)
![Light native interface](docs/screenshots/light.png)
![Queue beside Home](docs/screenshots/queue.png)
![Playlist details](docs/screenshots/playlist-detail.png)
![Native account library](docs/screenshots/library.png)

[Settings](docs/screenshots/settings.png) · [Song actions](docs/screenshots/song-menu.png) · [Account](docs/screenshots/account.png)

These are native screenshots with explicitly selected visual-test metadata; playback is inactive in the screenshots. The screenshot fixtures omit artwork URLs; the live app fetches small provider thumbnails when available. Earlier design references are in `docs/concepts/`.

## Install the Windows preview

Run **TideSetup.exe** from the Windows preview build and choose **Install Tide**.
Setup shows progress, installs private playback tools, and adds Tide to the Start
menu and Windows Settings. At completion choose **Open Tide** or **Connect YouTube
Music**. No separate Rust/Python installation or terminal commands are needed.
See [installation and removal](docs/INSTALLATION.md). Builds are still unsigned
previews; browser extension distribution and stable-release checks remain open.
Installed copies can check for published stable updates in Settings and download,
verify, install and restart with one action. Startup checks can be disabled.
Draft/prerelease builds are excluded; the initial updater build needs manual installation.
See [beta release preparation](docs/BETA_RELEASE.md) for verified candidate assets
and the remaining clean-Windows checks, and [Chrome beta setup](docs/BROWSER_COMPANION_BETA.md)
for the current unpacked extension connection flow.
Read [privacy and local data handling](PRIVACY.md) before connecting an account.

## Build and run

Install stable Rust from [rustup](https://rustup.rs/). On Windows the normal MSVC toolchain requires Visual Studio C++ Build Tools. The repository also supports a development-local GNU setup through `scripts/cargo-local.ps1` when those tools are already present under the ignored `.tools` directory.

```powershell
pwsh -File scripts/setup-runtime.ps1
cargo run -p tide-music
```

The setup script downloads mpv, yt-dlp and Deno into `.tools/runtime`; it does not alter system PATH, register startup tasks or read browser credentials. Alternatively place trusted binaries in `tools/` beside the application, set `TIDE_RUNTIME_DIR` to their directory, or use PATH. mpv is the native audio engine; yt-dlp/Deno handle the current YouTube extraction requirements. Ordinary local playback only needs mpv.

Paste a supported http/https media link, enter a YouTube search query, or open an audio file. Search currently shows up to eight results; select a song to play it or use its menu to queue it. Use the sun/moon control in the header for a direct theme switch. Settings holds window size, local audio and About. Account is a labeled header button next to Settings; its Advanced section holds manual imports and Google account-slot selection. Mini/Restore changes the player layout; theme changes preserve window size. Library playlists open a track preview with Play and Shuffle. Search results remain separate from the queue until selected. Playing a song or playlist preserves the browsing screen. Song actions open a theme-matched popup menu with Play next, Add to queue and Start song radio (which replaces upcoming tracks). Queue song menus also offer Remove from queue; opening a menu does not expand the row. Queue opens beside content at wide widths and overlays it at narrow widths; it separates manual tracks from autoplay suggestions and exposes reordering, removal, shuffle, repeat and Autoplay. Disabling autoplay removes upcoming automatic suggestions while preserving the current song and your choices. Autoplay requests recommendations from the current song when the queue is nearly finished; it requires a connected account and an available provider. The prototype saves queue metadata and preferences under the current user's application-data directory. For account features, follow the [Chrome beta connection guide](docs/BROWSER_COMPANION_BETA.md): load the companion, enable its connection in the graphical installer with its exact extension ID, then import your selected YouTube Music tab. Manual session import remains a fallback. See [local account setup](docs/ACCOUNT_SETUP.md). Sessions are encrypted with Windows DPAPI; the browser is used for connection while native playback streams audio rather than downloading a library.

## Audio quality and transitions

Tide requests `bestaudio/best` and plays the selected stream without re-encoding it. YouTube Music documents High quality as up to 256 kbps AAC or Opus. Availability depends on the song, session and formats the extractor can access; requesting the best stream does not guarantee that every song reaches that bitrate. See [YouTube Music's audio quality guide](https://support.google.com/youtubemusic/answer/9076559).

Once the current track has loaded, Tide prepares the next queued song's stream address so early skips can use it too. A short scheduler checks for a candidate every 250 ms, including while paused. Preparation is limited to one upcoming song and keeps only metadata in memory, rather than downloading full songs. A ready address lasts at most ten minutes, or until thirty seconds before its signed expiry, whichever comes first. Queue changes replace that preparation. Failed preparation can retry after twenty seconds; a skip without a ready result uses normal extraction. Network connection and audio-device startup can still leave a short gap; this is not guaranteed gapless playback.

Drag an upcoming queue row by its handle to reorder it within its manual or autoplay section. The current song stays fixed. Removal and song radio remain separate actions.

## Resource use

The default software renderer avoids GPU-driver startup overhead for this mostly static interface. Include mpv and any transient extractor/runtime processes in measurements. Mode switching releases the inactive interface; the audio engine stays shared. See [validation and measurements](docs/VALIDATION.md). A small executable or a native language alone does not prove lower RAM use.

## Checks

```powershell
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
python scripts/check_public_files.py
python -m unittest discover -s helpers -p "test_*.py"
```

Native screenshots: `cargo run -p tide-music -- --smoke-ui .local/screens --smoke-populated`. Add `--smoke-library` for synthetic account/library metadata, and optionally `--fixture-audio path/to/silent.wav` to run native interaction assertions with a silent local file. This shows and captures Home, Library, playlist detail, search, account settings, Queue drawer, light/dark, compact and mini/restore views, then exits. UI smoke/benchmark modes never load your saved account session. The opt-in `--smoke-autoplay` check uses the existing encrypted session, mutes audio, tests native EOF advancing into song recommendations, then exits without saving its temporary queue or volume.

For a muted five-minute local continuity check, use
`cargo run -p tide-music -- --smoke-local-listening 300 path/to/short-audio.wav`.
Choose a file about 18 seconds long: the check requires ten natural transitions
and exercises pause/resume, skip and seek. It uses temporary settings, bypasses
saved accounts and does not register a Windows media session. The separate
`--smoke-listening-session 1200` check uses the saved verified account and tests
live recommendation playback; the local check cannot prove YouTube reliability.

## Build a preview ZIP

After building the release executable, generate dependency notices for the same
Windows target. For a normal MSVC build:

```powershell
powershell -File scripts/build-preview.ps1
cargo metadata --locked --filter-platform x86_64-pc-windows-msvc --format-version 1 > .local/cargo-metadata.json
python scripts/generate-dependency-notices.py .local/cargo-metadata.json .local/DEPENDENCIES.md
powershell -File scripts/package-preview.ps1 -DependenciesNotice .local/DEPENDENCIES.md -StartHere docs/START_HERE.md
python scripts/check-preview-package.py dist/path-to-generated-preview.zip
```

Create `.local` first if it does not exist. For a GNU build, use
`x86_64-pc-windows-gnu` for the metadata target. The script packages only its
explicit file list, generates SHA-256 checksums and refuses existing outputs.
The ZIP includes the app, setup scripts, helper source and notices. It does not
include downloaded tools, Python environments, credentials or browser profiles.
Extracted previews use the included START_HERE.md instructions to install their
tools and optional account helper locally. A preview ZIP is not a stable-release
claim; see [release readiness](docs/RELEASE_READINESS.md).

## Privacy and credentials

No application API key is required. Never commit keys, tokens, cookies, `.env` files or local credential files. The ignore rules and tracked-file scan provide defense in depth; they do not replace reviewing every commit. Search and link playback contact external providers; local file playback stays local. There is no analytics or automatic browser-cookie access.

Provider terms and access restrictions still apply. The app does not implement account bypass, ad suppression or DRM circumvention. Unofficial playback compatibility is not guaranteed.

## License

Tide's original source is MIT licensed. Slint and playback tools retain their own licenses; see [third-party notices](THIRD_PARTY_NOTICES.md). Binaries/toolchains are excluded from Git. See [implementation plan](docs/IMPLEMENTATION_PLAN.md) and [contributing guide](CONTRIBUTING.md).
