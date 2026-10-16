# Third-party notices

Tide's original application code uses the MIT license in LICENSE. Dependencies and external tools retain their own licensing terms.

- [Slint](https://slint.dev/): interface toolkit, used under its Royalty-free Desktop, Mobile, and Web Applications License. Tide discloses its use here and in the interface. See [Slint license](https://slint.dev/agreements/slint-royalty-free-license.pdf).
- [mpv](https://mpv.io/): external native playback tool. Licensing depends on the build and enabled dependencies; consult [mpv licensing](https://github.com/mpv-player/mpv/blob/master/Copyright) before redistributing a binary. Runtime downloads are not committed here.
- [yt-dlp](https://github.com/yt-dlp/yt-dlp): external extraction tool. See its [license information](https://github.com/yt-dlp/yt-dlp#license) and the selected binary's third-party notices before redistribution.
- [Deno](https://deno.com/): external JavaScript runtime, MIT licensed with third-party dependencies. See [license](https://github.com/denoland/deno/blob/main/LICENSE.md).
- [ytmusicapi](https://github.com/sigma67/ytmusicapi): optional, transient library helper dependency, MIT licensed. It uses [Requests](https://github.com/psf/requests), Apache-2.0 licensed, and its dependencies. Development setup installs these packages with pip in a local virtual environment. Windows installer setup downloads checksum-pinned universal wheels into its private, isolated account-helper runtime, retaining their license metadata. Neither route vendors these packages in the source repository.
- [CPython](https://www.python.org/): Windows installer setup downloads the checksum-pinned official embeddable distribution for the account helper, including its upstream license. It does not install system Python. See [Python license information](https://docs.python.org/3/license.html) and the license shipped with the selected distribution.
- [Zen](https://github.com/zen-browser/desktop): installed browser whose selected YouTube session can be connected locally. Zen is MPL-2.0 licensed with upstream Firefox components and notices; its binaries are not bundled in this repository.

YTerMusic informed the architecture investigation. Its source has not been vendored into Tide. Reused code in future contributions must preserve applicable upstream license and attribution notices.

## Windows preview distribution

The preview ZIP and its containing installer include Tide's license, this file
and a generated `DEPENDENCIES.md` with the Windows Cargo dependency graph's
upstream license metadata and available license/notice texts. Some entries are
build tools rather than code included in the executable.

Playback tools and the private Python runtime are downloaded during installation;
their binaries are not embedded in the preview or committed to this repository.
Setup retains the downloaded archives, the files accompanying the selected mpv
executable, and the Python distribution and wheel license metadata in its owned
release directory. Installed tools retain their upstream terms. The generated
Cargo notice does not cover these separately downloaded tools or Python packages;
consult their upstream notices as well. Redistributing an installation with its
runtimes requires checking the notices, licenses and any source-offer obligations
for those particular builds. See [installation details](docs/INSTALLATION.md).


The thumbnail fetcher also uses ureq, url and the image crate (JPEG/WebP enabled); Windows icon embedding uses winresource. Their dependency licenses and those of transitive packages are retained in Cargo package metadata and the installed crates. These crates are obtained through Cargo rather than vendored here.
