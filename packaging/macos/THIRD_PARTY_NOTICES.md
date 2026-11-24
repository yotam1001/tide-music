# Tide macOS runtime notices

Tide is MIT licensed. Runtime components are separate programs loaded or
launched by Tide. Exact binary URLs, SHA-256 digests, versions and license
selections are recorded in `runtime.lock.json`, also copied into the bundle.

* arm64: official mpv 0.41.0 macOS 14 build from
  https://github.com/mpv-player/mpv/releases/tag/v0.41.0.
* Intel: stolendata mpv 0.39.0, built for macOS 11+, from
  https://laboratory.stolendata.net/~djinn/mpv_osx/.
  Official 0.41/0.42 Intel binaries require macOS 15, so cannot serve our 14 floor.
* mpv is GPL-2.0-or-later, with optional LGPL configurations. These distributions
  include GPL dependencies. FFmpeg 8.0.1 (arm64) / 6.1.2 (Intel) explicitly report
  GPL version 3 or later, configured with `--enable-gpl --enable-version3`.
  The bundled FFmpeg libraries are consequently GPL-3.0-or-later, not LGPL-only.
  GPL and LGPL texts and upstream mpv/FFmpeg notices are in `licenses/`.
* yt-dlp source is Unlicense; its official macOS PyInstaller executable contains
  GPLv3+ components. See `licenses/yt-dlp-LICENSE.txt` and
  https://github.com/yt-dlp/yt-dlp/tree/2026.11.23#license.

Source references: https://github.com/mpv-player/mpv/tree/v0.41.0,
https://github.com/mpv-player/mpv/tree/v0.39.0,
https://github.com/FFmpeg/FFmpeg/tree/n8.0.1,
https://github.com/FFmpeg/FFmpeg/tree/n6.1.2,
https://github.com/yt-dlp/yt-dlp/tree/2026.11.23.
The preserved upstream mpv.app carries its original dependent libraries,
frameworks and resources. Redistribution of these binaries must retain their
applicable notices and provide corresponding source as required by their
licenses; these source references alone are not a written source offer.
