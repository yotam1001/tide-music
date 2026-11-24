# Tide on Linux (preview)

Tide for Linux is a single AppImage for 64-bit Intel/AMD PCs. It carries its
own playback tools (mpv and yt-dlp) and account helper, so you don't need Rust,
Python or any extra packages. See [LINUX_PORT.md](LINUX_PORT.md) for platform
details.

It runs on current desktop distributions with glibc 2.35 or later: Ubuntu
22.04+, Debian 12+, Fedora 36+, Linux Mint 21+, openSUSE Tumbleweed, Arch and
others, on GNOME, KDE Plasma, Cinnamon, Xfce and most other desktops.

## Install and open

1. Download [Tide-x86_64.AppImage](https://github.com/yotam1001/tide-music/releases/latest/download/Tide-x86_64.AppImage).
2. Move it somewhere permanent, for example `~/Applications`.
3. Make it executable: right-click → **Properties → Allow executing file as
   program**, or run `chmod +x ~/Applications/Tide-x86_64.AppImage`.
4. Double-click it.

On first launch Tide adds itself to your app menu (like the Windows Start menu
shortcut), so you can open it from there afterwards. If you move the AppImage,
open it once from its new place and the menu entry follows.

If nothing happens, run it from a terminal to see why. Very old or minimal
systems may need FUSE (`sudo apt install libfuse2` on Ubuntu 22.04); newer
AppImage runtimes, including Tide's, do not.

## Sign in

Open the profile icon → **Sign in with Google**. A separate sign-in window
opens. Sign in, choose the profile you use for YouTube Music, open Library,
then select **Connect this profile**. The window closes and its browsing data
is discarded.

The sign-in window uses your system's WebKitGTK. If Tide says it is missing,
install it (`sudo apt install libwebkit2gtk-4.1-0`, `sudo dnf install
webkit2gtk4.1` or `sudo pacman -S webkit2gtk-4.1`) or connect through the
browser companion instead: **Use browser companion instead** → choose Chrome,
Edge, Brave, Chromium or Firefox / Zen → **Connect browser**, then follow the
steps it shows. Google sometimes refuses embedded sign-in windows (common for
Family Link accounts); the browser companion works for those.

Your sign-in key is kept in your desktop keyring (GNOME Keyring, KWallet or
another Secret Service provider). On desktops without a keyring, Tide keeps it
in a private file readable only by you.

## Desktop integration

- **Media keys and media controls**: Tide appears in your desktop's media
  controls (GNOME's notification area, KDE's media player widget, lock screens)
  with the song, artist and artwork. Media keys and headset buttons control it.
- **Tray**: Tide shows a tray icon with the same menu as on Windows. Turn on
  **Settings → System → Keep playing when I close Tide** to close to it. GNOME
  needs the *AppIndicator and KStatusNotifierItem Support* extension (installed
  by default on Ubuntu) to show tray icons; without one, closing quits Tide.
- **Mini player**: pinning keeps it above other windows and the opacity slider
  works, as on Windows.
- **Global shortcuts**: on Wayland desktops (KDE Plasma, GNOME 48+, Hyprland)
  your desktop asks you once to confirm Tide's shortcuts. On X11 they work
  straight away unless another app already uses the same keys.
- **Discord**: Tide's listening status works with the native, Flatpak and Snap
  Discord apps.

Tide runs through XWayland on Wayland desktops so the Mini player can stay on
top, which Wayland doesn't let apps do themselves. To try native Wayland
anyway, start it with `TIDE_WAYLAND=1`; pinning, opacity and window placement
are then unavailable.

## Updates

Open **Settings → Check for updates**. Music keeps playing while the new
AppImage downloads. Tide checks its publisher signature and checksum, replaces
your copy in place and restarts when you click **Restart to update**. Your
accounts and preferences carry over. This needs write access to the folder that
holds the AppImage; otherwise download the new AppImage from the
[latest release](https://github.com/yotam1001/tide-music/releases/latest) and
replace the old one.

## Where Tide keeps your data

| What | Where |
| --- | --- |
| Settings, queue and history | `~/.config/TideMusic` |
| Encrypted account session | `~/.local/share/TideMusic/account` |
| Update downloads | `~/.cache/TideMusic/updates` |
| App menu entry and icon | `~/.local/share/applications/tide-music.desktop`, `~/.local/share/icons/hicolor/256x256/apps/tide-music.png` |

## Remove Tide

1. In Tide, sign out from the profile menu so the keyring entry is removed.
2. Quit Tide and delete the AppImage.
3. Delete the folders and files in the table above.
