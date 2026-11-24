# Tide for Linux — platform contract

Goal: the same Tide as on Windows (same Slint UI, playback engine, provider,
Library, lyrics, Mini player, Jams, Discord, in-app updates), behaving as
much like the Windows app as Linux desktops allow. Jams are a stable feature
and are included in the AppImage.

## Architecture

| Layer | Windows | Linux |
| --- | --- | --- |
| UI | Slint + winit, software renderer | unchanged; X11/XWayland by default (`TIDE_WAYLAND=1` for native Wayland) |
| Playback | mpv JSON IPC over a named pipe, yt-dlp | mpv JSON IPC over a Unix domain socket, yt-dlp |
| Runtime binaries | `mpv.exe`, `yt-dlp.exe` beside the app | `usr/lib/tide-music/bin/{mpv,yt-dlp}` inside the AppImage (dev fallback: `PATH`) |
| Account helper | embeddable Python beside the app | standalone CPython in `usr/lib/tide-music/python` |
| Session storage | DPAPI file in `%APPDATA%\TideMusic\account` | AES-GCM file in `~/.local/share/TideMusic/account`, key in the Secret Service keyring (0600 key file without a keyring) |
| Settings | `%APPDATA%\TideMusic` | `~/.config/TideMusic` |
| Caches / updates | `%LOCALAPPDATA%\TideMusic` | `~/.cache/TideMusic` |
| Media controls | SMTC + taskbar thumb buttons | MPRIS `org.mpris.MediaPlayer2.tide` with artwork |
| Close to tray | notification-area icon | StatusNotifierItem with the same menu |
| Global shortcuts | `RegisterHotKey` | XDG GlobalShortcuts portal on Wayland, `XGrabKey` on X11 |
| Output changes | `IMMNotificationClient` | not needed: PipeWire/PulseAudio move streams to the new default |
| Mini pin/opacity | `HWND_TOPMOST` + layered alpha | `_NET_WM_STATE_ABOVE` + `_NET_WM_WINDOW_OPACITY` |
| Work-area fitting | monitor work area | `_NET_WORKAREA` ∩ RandR monitor, `_NET_MOVERESIZE_WINDOW` |
| Theme / motion | registry, `SPI_GETCLIENTAREAANIMATION` | portal `color-scheme`, `reduced-motion` / `enable-animations` |
| Discord | `\\.\pipe\discord-ipc-N` | `$XDG_RUNTIME_DIR/discord-ipc-N`, plus Flatpak and Snap paths |
| Sign-in | WebView2 helper `tide-login.exe` | ephemeral WebKitGTK helper `tide-login` (needs system libwebkit2gtk-4.1) |
| Browser companion | installer-registered | in-app registration; host copied to `~/.local/share/TideMusic/bin` |
| Updates | verified installer download and restart | signed `TideUpdate-linux.json`, verified AppImage replaced atomically, then restart |
| Installer | `TideSetup.exe` | `Tide-x86_64.AppImage`, adds an app-menu entry on first launch |

## `tide-linux` public API (mirrors `tide-windows`)

`crates/app/src/platform.rs` re-exports `tide_linux::*` on Linux. The crate
exports the same `MediaAction`, `start`, `MediaSession::{set_close_to_tray,
set_thumbnail, update}`, `open_web_page`, `open_touchpad_settings` (always an
error), `reveal_in_file_manager`, `app_light_theme`, `local_hour`,
`client_animations_enabled`, `window_work_area`, `fit_window_to_work_area`,
`OutputWatcher`, `mini::MiniWindow`, `shortcuts::{Chord, layout_key_name,
parse_bindings, GlobalShortcuts}` and `discord_pipe::DiscordPipe`. The view
handle is the X11 window id carried in the pointer value.

Linux additions used only by the app's Linux code:

- `MediaSession::hide_to_tray()` — called from the window's close request;
  hides Tide to the tray when "Keep playing when I close Tide" is on and a
  tray is visible. `MediaSession::quit_requested()` — tray/MPRIS Quit.
- `integrate_desktop_entry()` — writes the app-menu launcher for the running
  AppImage (only launchers marked `X-Tide-AppImage=true` are ever replaced).
- `select_display_backend()` (in the app's `platform.rs`) — selects winit's
  X11 backend when `DISPLAY` is available.

Shortcut text uses the Windows rules (Ctrl or Alt, optional Shift, a letter,
digit, Space or F1–F24 except F12), so settings files are portable.

## Packaging

`scripts/package-linux.sh` builds Tide, `tide-login` and `tide-session-host`
in release mode with default features (Jams included), assembles an AppDir,
verifies it with `scripts/verify_linux.py`, and writes
`dist/Tide-<version>-x86_64.AppImage` plus the stable `Tide-x86_64.AppImage`.
Every downloaded component is pinned with a SHA-256 in
`packaging/linux/runtime.lock.json`: the AppImage type2 runtime, mpv 0.41.0
(the portable pkgforge "anylinux" build, matching the macOS pin; 0.42 rejects
one of Tide's audio filters), yt-dlp and CPython with the same helper wheels
as Windows and macOS. The AppImage is assembled with `mksquashfs` and the
pinned runtime; downloaded tools run only after their pinned checksum matches
(mpv's own runtime unpacks its DwarFS payload). Build on Ubuntu 22.04 so
the binaries need only glibc 2.35.

## Validation

`.github/workflows/linux.yml` runs formatting, workspace checks, Clippy,
tests, the offline sign-in probe and a headless smoke under Xvfb with a D-Bus
session, the playback probes against the pinned mpv, and the full AppImage
build. `linux-release.yml` builds the AppImage for tags and attaches it to the
draft release. The release script verifies those artifacts, signs
`TideUpdate-linux.json` locally and includes everything in `SHA256SUMS.txt`.

Still needing real-hardware checks: live Google sign-in in WebKitGTK and the
keyring prompt; media keys and MPRIS on GNOME, KDE and Cinnamon; tray icons
with and without the GNOME AppIndicator extension; Wayland global shortcuts
via the portal; Mini pinning across window managers; Discord; and an in-place
AppImage update from one published version to the next.
