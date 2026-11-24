# Tide Linux runtime notices

Tide is MIT licensed. Runtime components are separate programs launched by
Tide. Exact binary URLs, SHA-256 digests, versions and license selections are
recorded in `runtime.lock.json`, also copied into the AppImage under
`usr/lib/tide-music`.

* mpv 0.41.0, the portable "anylinux" build from
  https://github.com/pkgforge-dev/mpv-AppImage (unchanged upstream mpv
  packaged with its own loader and libraries). mpv is GPL-2.0-or-later; the
  bundled FFmpeg is GPL-3.0-or-later. GPL and LGPL texts and upstream
  mpv/FFmpeg notices are in `licenses/`.
* yt-dlp 2026.10.26 official `yt-dlp_linux` executable. Its source is
  Unlicense; the PyInstaller executable contains GPLv3+ components. See
  `licenses/yt-dlp-LICENSE.txt` and
  https://github.com/yt-dlp/yt-dlp/tree/2026.10.26#license.
* CPython 3.14.10 from python-build-standalone 20261026 (PSF-2.0) with the
  same pinned pure-Python wheels the Windows and macOS account helper uses.
* The AppImage type2 runtime 20251108 (MIT),
  https://github.com/AppImage/type2-runtime.

Source references: https://github.com/mpv-player/mpv/tree/v0.41.0,
https://github.com/pkgforge-dev/mpv-AppImage,
https://github.com/yt-dlp/yt-dlp/tree/2026.10.26,
https://github.com/astral-sh/python-build-standalone/releases/tag/20261026.
Redistribution of these binaries must retain their applicable notices and
provide corresponding source as required by their licenses; these source
references alone are not a written source offer.
