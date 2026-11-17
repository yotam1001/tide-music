"""Build a stable Tide DMG on macOS. No Homebrew runtime dependencies."""
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
import zipfile

ROOT = Path(__file__).resolve().parent.parent
PACK = ROOT / 'packaging/macos'


def run(*args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, **kwargs)


def download(entry, destination):
    run('curl', '--fail', '--location', '--retry', '3', '--proto', '=https',
        '--proto-redir', '=https', entry['url'], '--output', destination)
    if hashlib.sha256(destination.read_bytes()).hexdigest() != entry['sha256']:
        raise RuntimeError(f'Checksum mismatch: {entry["url"]}')


def runtime(arch, resources, work):
    lock = json.loads((PACK / 'runtime.lock.json').read_text())
    mpv = lock['mpv'][arch]
    archive = work / 'mpv.archive'
    download(mpv, archive)
    unpack = work / 'unpack'
    unpack.mkdir()
    if mpv['format'] == 'zip-tar':
        with zipfile.ZipFile(archive) as z:
            archive.write_bytes(z.read('mpv.tar.gz'))
    with tarfile.open(archive) as t:
        t.extractall(unpack, filter='data')
    # Preserve @executable_path/../Frameworks, adjacent lib/, resources, etc.
    shutil.copytree(unpack / 'mpv.app', resources / 'mpv.app', symlinks=True)
    # Upstream's empty .gitkeep/AppleDouble files in MacOS are not code and
    # codesign treats them as unsigned subcomponents. Discard build scaffolding.
    for path in (resources / 'mpv.app').rglob('*'):
        if path.is_file() and (path.name == '.gitkeep' or path.name.startswith('._')):
            path.unlink()
    relocate(resources / 'mpv.app')
    (resources / 'bin/mpv').symlink_to('../mpv.app/Contents/MacOS/mpv')
    ytdlp = lock['yt-dlp']
    download(ytdlp, resources / 'bin/yt-dlp')
    sums = work / 'SHA2-256SUMS'
    download(ytdlp['checksums'], sums)
    expected = f'{ytdlp["sha256"]}  yt-dlp_macos'
    if expected not in sums.read_text().splitlines():
        raise RuntimeError('yt-dlp digest not found in official SHA2-256SUMS')
    (resources / 'bin/yt-dlp').chmod(0o755)
    account_helper(arch, lock, resources, work)
    shutil.copy(PACK / 'runtime.lock.json', resources)


def account_helper(arch, lock, resources, work):
    """Standalone CPython plus the same pinned pure-Python wheels Windows uses."""
    python = lock['python'][arch]
    archive = work / 'python.tar.gz'
    download(python, archive)
    with tarfile.open(archive) as t:
        t.extractall(resources, filter='data')  # creates Resources/python
    wheels = work / 'wheels'
    wheels.mkdir()
    for wheel in lock['helper_wheels']:
        name = wheel['url'].rsplit('/', 1)[1]
        if not re.fullmatch(r'[A-Za-z0-9_.-]+-py3-none-any\.whl', name):
            raise RuntimeError(f'Helper wheels must be pure Python: {name}')
        download(wheel, wheels / name)
    interpreter = resources / 'python/bin/python3'
    run(interpreter, '-I', '-m', 'pip', 'install', '--no-index', '--no-deps',
        '--disable-pip-version-check', '--no-warn-script-location',
        *sorted(wheels.glob('*.whl')))
    (resources / 'helpers').mkdir()
    shutil.copy2(ROOT / 'helpers/youtube_music.py', resources / 'helpers')


def icon(resources, work):
    # PNG dimensions come from the existing artwork, never synthesized artwork.
    import struct
    images = []
    for p in (ROOT / 'ui/assets').glob('*.png'):
        width, height = struct.unpack('>II', p.read_bytes()[16:24])
        images.append((width * height, p))
    source = max(images)[1]
    iconset = work / 'Tide.iconset'
    iconset.mkdir()
    for size in (16, 32, 128, 256, 512):
        for scale in (1, 2):
            name = f'icon_{size}x{size}' + ('@2x' if scale == 2 else '') + '.png'
            run('sips', '-z', size * scale, size * scale, source,
                '--out', iconset / name, stdout=subprocess.DEVNULL)
    run('iconutil', '-c', 'icns', iconset, '-o', resources / 'Tide.icns')


def macho(path):
    return path.is_file() and not path.is_symlink() and 'Mach-O' in subprocess.check_output(
        ['file', '-b', str(path)], text=True)


def relocate(bundle):
    """Remove upstream Homebrew install IDs/search paths before re-signing."""
    for path in bundle.rglob('*'):
        if not macho(path):
            continue
        commands = subprocess.check_output(['otool', '-l', str(path)], text=True)
        for block in commands.split('Load command '):
            if 'cmd LC_RPATH\n' in block:
                match = re.search(r'path (.+) \(offset', block)
                if match and match[1].startswith(('/opt/homebrew/', '/usr/local/')):
                    run('install_name_tool', '-delete_rpath', match[1], path)
            if 'cmd LC_ID_DYLIB\n' in block:
                match = re.search(r'name (.+) \(offset', block)
                if match and match[1].startswith(('/opt/homebrew/', '/usr/local/')):
                    run('install_name_tool', '-id', '@rpath/' + path.name, path)


def sign(app):
    identity = os.environ.get('MACOS_SIGNING_IDENTITY', '-')
    flags = ['--force', '--sign', identity]
    flags += ['--timestamp=none'] if identity == '-' else [
        '--timestamp', '--options', 'runtime', '--entitlements', str(PACK / 'Tide.entitlements')]
    for path in sorted(app.rglob('*'), key=lambda p: len(p.parts), reverse=True):
        if macho(path):
            run('codesign', *flags, path)
    for path in sorted(app.rglob('*.app'), key=lambda p: len(p.parts), reverse=True):
        run('codesign', *flags, path)
    run('codesign', *flags, app)
    run('codesign', '--verify', '--strict', '--verbose=2', app)


def notarize(dmg):
    if not os.environ.get('MACOS_SIGNING_IDENTITY'):
        print('Ad-hoc signed; notarization skipped.')
        return
    profile = os.environ.get('MACOS_NOTARY_PROFILE')
    apple_id = os.environ.get('MACOS_NOTARY_APPLE_ID')
    password = os.environ.get('MACOS_NOTARY_PASSWORD')
    team = os.environ.get('MACOS_NOTARY_TEAM_ID')
    if profile:
        credentials = ['--keychain-profile', profile]
    elif apple_id and password and team:
        credentials = ['--apple-id', apple_id, '--password', password, '--team-id', team]
    else:
        print('No notarytool credentials; notarization skipped.')
        return
    run('xcrun', 'notarytool', 'submit', dmg, *credentials, '--wait')
    run('xcrun', 'stapler', 'staple', dmg)
    run('xcrun', 'stapler', 'validate', dmg)


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--arch', choices=['arm64', 'x86_64', 'universal'], default=platform.machine())
    parser.add_argument('--no-runtime', action='store_true')
    parser.add_argument('--out', type=Path, default=Path('dist'))
    args = parser.parse_args()
    if platform.system() != 'Darwin':
        parser.error('Packaging requires macOS (sips, iconutil, codesign, hdiutil).')
    os.chdir(ROOT)
    os.environ['MACOSX_DEPLOYMENT_TARGET'] = '14.0'
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['package']['version']
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    arches = ['arm64', 'x86_64'] if args.arch == 'universal' else [args.arch]
    binaries = []
    logins = []
    for arch in arches:
        target = ('aarch64' if arch == 'arm64' else arch) + '-apple-darwin'
        run('rustup', 'target', 'add', target)
        run('cargo', 'build', '--release', '--locked', '-p', 'tide-music',
            '--no-default-features', '--target', target)
        # Native sign-in runs in its own owned helper process beside the player.
        run('cargo', 'build', '--release', '--locked', '-p', 'tide-provider',
            '--features', 'native-login', '--bin', 'tide-login', '--target', target)
        target_dir = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'target'))
        binaries.append(target_dir / target / 'release/tide-music')
        logins.append(target_dir / target / 'release/tide-login')
    # Runtime builds are per arch: universal emits two DMGs with a universal Tide.
    for arch in arches:
        with tempfile.TemporaryDirectory(prefix='tide-macos-') as temporary:
            work = Path(temporary)
            stage = work / 'stage'
            app = stage / 'Tide.app'
            contents = app / 'Contents'
            resources = contents / 'Resources'
            (contents / 'MacOS').mkdir(parents=True)
            (resources / 'bin').mkdir(parents=True)
            binary = contents / 'MacOS/tide-music'
            login = contents / 'MacOS/tide-login'
            if len(binaries) == 2:
                run('lipo', '-create', *binaries, '-output', binary)
                run('lipo', '-create', *logins, '-output', login)
            else:
                shutil.copy2(binaries[0], binary)
                shutil.copy2(logins[0], login)
            (contents / 'Info.plist').write_text((PACK / 'Info.plist').read_text().replace('@VERSION@', version))
            icon(resources, work)
            shutil.copytree(PACK / 'licenses', resources / 'licenses')
            shutil.copy(PACK / 'THIRD_PARTY_NOTICES.md', resources)
            if not args.no_runtime:
                runtime(arch, resources, work)
            run('xattr', '-cr', app)
            sign(app)
            run('python3', ROOT / 'scripts/verify_macos.py', app,
                *(['--no-runtime'] if args.no_runtime else []))
            (stage / 'Applications').symlink_to('/Applications')
            dmg = out / f'Tide-{version}-{arch}.dmg'
            run('hdiutil', 'create', '-ov', '-volname', 'Tide', '-srcfolder', stage,
                '-format', 'UDZO', dmg)
            notarize(dmg)
            run('python3', ROOT / 'scripts/check_macos_dmg.py', dmg)
            # Stable names for releases/latest/download links on the website.
            if arch == 'arm64':
                shutil.copy2(dmg, out / 'Tide.dmg')
            else:
                shutil.copy2(dmg, out / 'Tide-Intel.dmg')
            print(f'{dmg.name}: {dmg.stat().st_size} bytes', flush=True)


if __name__ == '__main__':
    main()
