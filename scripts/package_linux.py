"""Build the stable Tide AppImage on Linux (x86_64). No system runtime dependencies.

The AppImage carries Tide, its sign-in helpers, pinned mpv, yt-dlp and a
standalone Python for the account helper, exactly like the Windows install and
the macOS bundle. Output: dist/Tide-<version>-x86_64.AppImage plus the stable
alias dist/Tide-x86_64.AppImage used by releases/latest/download links and
in-app updates.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
PACK = ROOT / 'packaging/linux'
STABLE_NAME = 'Tide-x86_64.AppImage'


def run(*args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, **kwargs)


def download(entry, destination):
    run('curl', '--fail', '--location', '--retry', '3', '--proto', '=https',
        '--proto-redir', '=https', '--silent', '--show-error', entry['url'],
        '--output', destination)
    if hashlib.sha256(Path(destination).read_bytes()).hexdigest() != entry['sha256']:
        raise RuntimeError(f'Checksum mismatch: {entry["url"]}')


def helper_wheels():
    """The same pinned pure-Python wheels the Windows and macOS helpers use."""
    return json.loads((ROOT / 'packaging/macos/runtime.lock.json').read_text())['helper_wheels']


def extract_appimage(image, destination, work):
    """Unpack a checksum-verified AppImage with its own runtime, without FUSE.

    The pinned anylinux mpv uses a DwarFS payload, which only its embedded
    runtime can read, so this runs the already verified image once with
    --appimage-extract in a private working folder.
    """
    unpack = work / 'mpv-extract'
    unpack.mkdir()
    image = Path(image)
    image.chmod(0o755)
    run(image, '--appimage-extract', cwd=unpack, stdout=subprocess.DEVNULL,
        env={**os.environ, 'APPIMAGE_EXTRACT_AND_RUN': '0'})
    # squashfs-root is a symlink to the real AppDir in newer runtimes.
    root = (unpack / 'squashfs-root').resolve()
    if not (root / 'bin/mpv').is_file():
        raise RuntimeError(f'{image.name} did not unpack to an AppDir with bin/mpv')
    shutil.move(str(root), destination)
    # Tide starts bin/mpv directly and never mpv's AppRun. Its startup hooks
    # would offer to download yt-dlp and run appimageupdatetool against
    # $APPIMAGE (Tide's own image), so they are not shipped at all.
    for hook in sorted((Path(destination) / 'bin').glob('*.hook')):
        hook.unlink()


def runtime(lib, work):
    lock = json.loads((PACK / 'runtime.lock.json').read_text())
    (lib / 'bin').mkdir(parents=True)
    # mpv: the portable anylinux build, kept intact with its own loader and
    # libraries; bin/mpv is a relative launcher into it.
    mpv = lock['mpv']['x86_64']
    image = work / 'mpv.AppImage'
    download(mpv, image)
    extract_appimage(image, lib / 'mpv', work)
    launcher = lib / 'bin/mpv'
    launcher.write_text('#!/bin/sh\n'
                        'here="$(dirname "$(readlink -f "$0")")"\n'
                        'exec "$here/../mpv/bin/mpv" "$@"\n')
    launcher.chmod(0o755)
    # yt-dlp: the official standalone Linux executable, cross-checked against
    # the release's SHA2-256SUMS.
    ytdlp = lock['yt-dlp']
    download(ytdlp, lib / 'bin/yt-dlp')
    sums = work / 'SHA2-256SUMS'
    download(ytdlp['checksums'], sums)
    if f'{ytdlp["sha256"]}  yt-dlp_linux' not in sums.read_text().splitlines():
        raise RuntimeError('yt-dlp digest not found in official SHA2-256SUMS')
    (lib / 'bin/yt-dlp').chmod(0o755)
    account_helper(lock, lib, work)
    shutil.copy(PACK / 'runtime.lock.json', lib)


def account_helper(lock, lib, work):
    python = lock['python']['x86_64']
    archive = work / 'python.tar.gz'
    download(python, archive)
    with tarfile.open(archive) as t:
        t.extractall(lib, filter='data')  # creates lib/python
    wheels = work / 'wheels'
    wheels.mkdir()
    for wheel in helper_wheels():
        name = wheel['url'].rsplit('/', 1)[1]
        if not re.fullmatch(r'[A-Za-z0-9_.-]+-py3-none-any\.whl', name):
            raise RuntimeError(f'Helper wheels must be pure Python: {name}')
        download(wheel, wheels / name)
    interpreter = lib / 'python/bin/python3'
    run(interpreter, '-I', '-m', 'pip', 'install', '--no-index', '--no-deps',
        '--disable-pip-version-check', '--no-warn-script-location', '--no-compile',
        *sorted(wheels.glob('*.whl')))
    (lib / 'helpers').mkdir()
    shutil.copy2(ROOT / 'helpers/youtube_music.py', lib / 'helpers')


def icons(appdir):
    """The existing 256 px app icon; desktops scale it for menus and panels."""
    folder = appdir / 'usr/share/icons/hicolor/256x256/apps'
    folder.mkdir(parents=True)
    shutil.copy2(ROOT / 'ui/assets/tide-icon.png', folder / 'tide-music.png')
    shutil.copy2(ROOT / 'ui/assets/tide-icon.png', appdir / 'tide-music.png')
    (appdir / '.DirIcon').symlink_to('tide-music.png')


def stage_browser_companion(share):
    import importlib.util
    spec = importlib.util.spec_from_file_location(
        'browser_packager', ROOT / 'scripts/package-browser-companion.py')
    packager = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(packager)
    source = ROOT / 'browser-companion'
    for family in ('chromium', 'firefox'):
        payload = packager._read_payload(source, family)
        dest = share / 'browser-companion' / family
        dest.mkdir(parents=True)
        for name, data in sorted(payload.items()):
            (dest / name).write_bytes(data)


def build_appdir(appdir, binaries, version, with_runtime, work):
    bin_dir = appdir / 'usr/bin'
    bin_dir.mkdir(parents=True)
    for binary in binaries:
        shutil.copy2(binary, bin_dir / binary.name)
        (bin_dir / binary.name).chmod(0o755)
    shutil.copy2(PACK / 'AppRun', appdir / 'AppRun')
    (appdir / 'AppRun').chmod(0o755)
    desktop = (PACK / 'tide-music.desktop').read_text().replace('@VERSION@', version)
    (appdir / 'tide-music.desktop').write_text(desktop)
    apps = appdir / 'usr/share/applications'
    apps.mkdir(parents=True)
    (apps / 'tide-music.desktop').write_text(desktop)
    metainfo = appdir / 'usr/share/metainfo'
    metainfo.mkdir(parents=True)
    shutil.copy2(PACK / 'tide-music.metainfo.xml', metainfo / 'tide-music.metainfo.xml')
    icons(appdir)
    share = appdir / 'usr/share/tide-music'
    share.mkdir(parents=True)
    shutil.copytree(ROOT / 'packaging/macos/licenses', share / 'licenses')
    shutil.copy2(PACK / 'THIRD_PARTY_NOTICES.md', share)
    shutil.copy2(ROOT / 'LICENSE', share / 'LICENSE')
    stage_browser_companion(share)
    if with_runtime:
        runtime(appdir / 'usr/lib/tide-music', work)


def make_appimage(appdir, output, work):
    lock = json.loads((PACK / 'runtime.lock.json').read_text())
    runtime_file = work / 'runtime'
    download(lock['appimage_runtime']['x86_64'], runtime_file)
    squash = work / 'payload.squashfs'
    run('mksquashfs', appdir, squash, '-root-owned', '-noappend', '-comp', 'zstd',
        '-Xcompression-level', '19', '-b', '1M', '-quiet', '-all-time', '0',
        '-mkfs-time', '0')
    with open(output, 'wb') as out:
        out.write(runtime_file.read_bytes())
        with open(squash, 'rb') as payload:
            shutil.copyfileobj(payload, out)
    output.chmod(0o755)


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--no-runtime', action='store_true',
                        help='development build without mpv, yt-dlp and Python')
    parser.add_argument('--skip-build', action='store_true',
                        help='package existing release binaries')
    parser.add_argument('--out', type=Path, default=Path('dist'))
    args = parser.parse_args()
    if platform.system() != 'Linux' or platform.machine() != 'x86_64':
        parser.error('Packaging requires x86_64 Linux (mksquashfs, unsquashfs).')
    os.chdir(ROOT)
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['package']['version']
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    if not args.skip_build:
        # Stable packages always include Jams (default features).
        run('cargo', 'build', '--release', '--locked', '-p', 'tide-music')
        run('cargo', 'build', '--release', '--locked', '-p', 'tide-provider',
            '--features', 'native-login', '--bin', 'tide-login')
        run('cargo', 'build', '--release', '--locked', '-p', 'tide-provider',
            '--bin', 'tide-session-host')
    target_dir = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'target'))
    binaries = [target_dir / 'release' / name
                for name in ('tide-music', 'tide-login', 'tide-session-host')]
    for binary in binaries:
        if not binary.is_file():
            raise RuntimeError(f'Missing release binary: {binary}')
    with tempfile.TemporaryDirectory(prefix='tide-linux-') as temporary:
        work = Path(temporary)
        appdir = work / 'Tide.AppDir'
        build_appdir(appdir, binaries, version, not args.no_runtime, work)
        run('python3', '-B', ROOT / 'scripts/verify_linux.py', appdir,
            *(['--no-runtime'] if args.no_runtime else []))
        image = out / f'Tide-{version}-x86_64.AppImage'
        make_appimage(appdir, image, work)
    shutil.copy2(image, out / STABLE_NAME)
    print(f'{image.name}: {image.stat().st_size} bytes', flush=True)


if __name__ == '__main__':
    main()
