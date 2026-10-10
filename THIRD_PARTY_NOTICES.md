# Third-party notices

Tide's original application code uses the MIT license in LICENSE. Dependencies and external tools retain their own licensing terms.

- [Slint](https://slint.dev/): interface toolkit, used under its Royalty-free Desktop, Mobile, and Web Applications License. Tide discloses its use here and in the interface. See [Slint license](https://slint.dev/agreements/slint-royalty-free-license.pdf).
- [mpv](https://mpv.io/): external native playback tool. Licensing depends on the build and enabled dependencies; consult [mpv licensing](https://github.com/mpv-player/mpv/blob/master/Copyright) before redistributing a binary. Runtime downloads are not committed here.
- [yt-dlp](https://github.com/yt-dlp/yt-dlp): external extraction tool. See its [license information](https://github.com/yt-dlp/yt-dlp#license) and the selected binary's third-party notices before redistribution.
- [Deno](https://deno.com/): external JavaScript runtime, MIT licensed with third-party dependencies. See [license](https://github.com/denoland/deno/blob/main/LICENSE.md).
- [ytmusicapi](https://github.com/sigma67/ytmusicapi): optional, transient library helper dependency, MIT licensed. It uses [Requests](https://github.com/psf/requests), Apache-2.0 licensed, and its dependencies. Their packages and licenses are installed by pip in the local virtual environment and are not vendored in this repository.
- [Playwright](https://github.com/microsoft/playwright-python): optional local sign-in helper, Apache-2.0 licensed. Uses installed Microsoft Edge in a temporary profile; browser sign-in is not resident during playback. Packages and their licenses are installed locally through pip.

YTerMusic informed the architecture investigation. Its source has not been vendored into Tide. Reused code in future contributions must preserve applicable upstream license and attribution notices.

This source repository does not bundle the playback runtimes. A future binary distribution must include its required notices, licenses and any source-offer obligations for the particular builds it ships.
