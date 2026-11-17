# Tide for macOS — port plan and platform contract

Goal: the same Tide (same Slint UI, playback engine, provider, Library, lyrics,
Mini player, Discord, updates) running natively on macOS 14+ (Apple Silicon and
Intel), plus a small set of macOS-only integrations. Jams keep the same rules as
on Windows: opt-in, never in the stable bundle until explicitly approved.

## Architecture

| Layer | Windows | macOS |
| --- | --- | --- |
| UI | Slint + winit, software renderer | unchanged |
| Playback | mpv JSON IPC over a named pipe, yt-dlp | mpv JSON IPC over a Unix domain socket, yt-dlp |
| Runtime binaries | `mpv.exe`, `yt-dlp.exe` beside the app | `mpv`, `yt-dlp` in `Tide.app/Contents/Resources/bin` (dev fallback: Homebrew / `PATH`) |
| Session storage | DPAPI file in `%APPDATA%\TideMusic\account` | AES-GCM file in `~/Library/Application Support/TideMusic/account`, key in the login Keychain |
| Settings | `%APPDATA%\TideMusic` | `~/Library/Application Support/TideMusic` |
| Caches / updates | `%LOCALAPPDATA%\TideMusic` | `~/Library/Caches/TideMusic` |
| Media controls | SMTC + taskbar thumb buttons | `MPNowPlayingInfoCenter` + `MPRemoteCommandCenter` (Control Center, media keys, AirPods) |
| Close to tray | notification-area icon | menu bar extra (`NSStatusItem`) |
| Global shortcuts | `RegisterHotKey` | Carbon `RegisterEventHotKey` (no Accessibility permission) |
| Output changes | `IMMNotificationClient` | CoreAudio default-output property listener |
| Mini pin/opacity | `HWND_TOPMOST` + layered alpha | `NSWindow` floating level + `alphaValue`, joins all Spaces |
| Discord | `\\.\pipe\discord-ipc-N` | `$TMPDIR/discord-ipc-N` Unix socket |
| Sign-in | WebView2 helper `tide-login.exe` | ephemeral WKWebView helper `tide-login` |
| Updates | Verified installer download and restart | Version and architecture check, browser DMG download and manual replacement |
| Installer | `TideSetup.exe` | `Tide.dmg` or `Tide-Intel.dmg` with `Tide.app` (ad-hoc signed until a Developer ID exists) |

## macOS-only features

1. Now Playing in Control Center / lock screen, with artwork and scrubbing; hardware
   media keys and AirPods controls through `MPRemoteCommandCenter`.
2. Menu bar extra: current song, Play/Pause, Next, Previous, Show Tide, Mini player, Quit.
   Used when "close to tray" is on, as the macOS counterpart of the Windows tray icon.
3. Dock menu: Play/Pause, Next, Previous, Like, Mini player.
4. Native app menu bar (Tide / Edit / Controls / Window / Help) with standard ⌘
   shortcuts: ⌘, Settings, ⌘F Search, Space play/pause, ⌘→ / ⌘← next/previous,
   ⌘↑ / ⌘↓ volume, ⌘L like, ⌘⇧M Mini player, ⌘W hide window, ⌘Q quit.
5. Follows system appearance, accent-free palette, and "Reduce motion".
6. Optional "Notify on song change" banner (UserNotifications), off by default.
7. Mini player floats across all Spaces and over full-screen apps when pinned.

## `tide-macos` public API (must mirror `tide-windows`)

The app selects one platform crate via `crates/app/src/platform.rs`:

```rust
#[cfg(target_os = "macos")] pub use tide_macos::*;
#[cfg(not(target_os = "macos"))] pub use tide_windows::*;
```

So `tide-macos` exports, with identical signatures:

- `enum MediaAction { Play, Pause, Toggle, Next, Previous, Stop, ShowMini, Seek(f64) }`
  (macOS adds nothing to this enum; menu actions use `MenuAction` below).
- `fn start(view: *mut c_void, callback: impl Fn(MediaAction) + Send + 'static) -> Result<MediaSession, String>`
  where `view` is the winit `NSView*` (`RawWindowHandle::AppKit`).
- `MediaSession::{set_close_to_tray(&self, bool), set_thumbnail(&mut self, Option<&[u8]>) -> Result<(), String>,
  update(&mut self, title, artist, album, playing: bool, position: f64, duration: f64) -> Result<(), String>}`.
  `set_thumbnail` accepts the same bounded PNG (≤128 KiB, ≤160 px) as Windows.
  `set_close_to_tray(true)` shows the menu bar extra; `false` removes it.
- `fn open_web_page(url: &str) -> Result<(), String>` (https only, via `NSWorkspace`/`open`).
- `fn reveal_in_file_manager(path: &Path) -> Result<(), String>` (also added to `tide-windows`).
- `fn app_light_theme() -> Option<bool>`, `fn local_hour() -> Option<u8>`, `fn client_animations_enabled() -> bool`.
- `fn window_work_area(view: *mut c_void) -> Result<[i32; 4], String>` (left, top, right, bottom in physical px)
  and `fn fit_window_to_work_area(view: *mut c_void, work: [i32; 4]) -> Result<(), String>`.
- `struct OutputWatcher { fn new() -> Result<Self, String>; fn take_available(&self) -> Option<bool> }`.
- `mod mini { #[derive(Default)] struct MiniWindow; fn apply(&mut self, view: *mut c_void, pinned: bool, opacity: u8) -> Result<(), String> }`.
- `mod shortcuts { struct Chord; fn layout_key_name(char) -> Option<String>;
  fn parse_bindings(&[String; 3]) -> Result<[Option<Chord>; 3], String>; struct GlobalShortcuts {
  fn new(impl Fn(MediaAction) + Send + 'static) -> Result<Self, String>; fn apply(&mut self, [Option<Chord>; 3]) -> Result<(), String> } }`.
  On macOS, `Cmd`/`Command`/`⌘`, `Option`/`Alt`, `Ctrl`, `Shift` are accepted.
- `mod discord_pipe { struct DiscordPipe` with `connect`, `send`, `receive`, `poll`, `deadline` as on Windows `}`.
- macOS-only: `enum MenuAction { PlayPause, Next, Previous, VolumeUp, VolumeDown, Like, Search, Settings, ToggleMini, ShowWindow, HideWindow, Quit }` (Quit lets the app exit its event loop so shutdown saves state),
  `fn install_app_menu(callback: impl Fn(MenuAction) + 'static) -> Result<(), String>` (main thread, after the
  winit window exists; also installs the Dock menu), and
  `fn notify_song(title: &str, artist: &str) -> Result<(), String>`, and
  `fn hide_app() / fn show_app() -> Result<(), String>` (NSApp hide / unhide + activate;
  the window close button hides Tide instead of quitting, as in Apple Music).

On non-macOS targets the crate builds as stubs returning `Err("... macOS only")`, so
`cargo check --workspace` keeps working on Windows.

## Ownership while the port is in flight

| Area | Files | Owner |
| --- | --- | --- |
| Platform crate | `crates/macos/**` | Codex |
| Playback on Unix | `crates/playback/**` | Codex |
| Session storage / provider | `crates/provider/**` | Gemini |
| Bundle, DMG, release CI | `scripts/*macos*`, `packaging/macos/**`, `.github/workflows/macos*.yml` | Codex |
| App integration, updater, settings paths, docs | `crates/app/**`, `ui/**`, `docs/**`, `README.md` | Claude |

## Validation

The macOS workflow runs on pull requests, `mac/**` pushes, and manual dispatch.
It runs formatting, workspace checks, Clippy, unit tests, and playback smoke on
Apple Silicon. The developer job builds Tide and `tide-login` directly with Cargo
and checks Homebrew playback tooling. The bundle jobs package for Apple Silicon
and native Intel (`macos-15-intel`), check that Jams are absent from both stable
images, and confirm the pinned mpv accepts `--media-controls=no`. Tide registers
its own Now Playing session.

The published-download audit reproduced an invalid resource seal on both DMGs.
The post-signing Python import probe added `argparse.cpython-314.pyc` and
`webbrowser.cpython-314.pyc` inside the signed bundle. The packaging probe and
the runtime helper now launch Python with `-B`. The packaging gate mounts the
finished quarantined DMG, copies Tide.app, checks the outer and nested signatures,
and records Gatekeeper’s policy assessment. Ad-hoc builds still require
first-launch approval. [Published DMG audit](https://github.com/yotam1001/tide-music/actions/runs/44170335841/job/134347416698)

The Mac implementations for HTTPS links, Finder crash-log reveal, appearance,
Discord IPC, Mini pinning and opacity, Carbon shortcuts, and CoreAudio device
watching are present. Global four-finger shortcuts remain specific to Windows.
The Mac updater opens the disk image for the offered release and leaves
replacement in Applications to the listener.

Physical Apple Silicon and Intel checks are still needed for the first-launch
dialog, live Google sign-in and Keychain access, AirPods and hardware media keys,
output changes, global shortcuts, Discord, and Mini across Spaces and full-screen
apps. The hosted playback probe uses muted local audio.

## Native sign-in packaging

Ship `Contents/MacOS/tide-login`, built with
`cargo build -p tide-provider --bin tide-login --features tide-provider/native-login`.
The helper uses a non-persistent WebKit store on its main thread. Its offline
`--probe` creates an offscreen window and WebView without Google traffic.
Real Google sign-in and profile selection still require validation on a Mac.
