"""Verify an assembled Tide AppDir without a user session, network or window."""
import argparse
import json
from pathlib import Path
import struct
import subprocess


def run(*args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, **kwargs)


def verify_session_host(host: Path):
    payload = json.dumps({'version': 1, 'action': 'check_host', 'headers': {}}).encode('utf-8')
    proc = subprocess.Popen(
        [str(host), '/home/tide/.mozilla/native-messaging-hosts/org.tidemusic.session.json',
         'tide-session@tide-music'],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        proc.stdin.write(struct.pack('<I', len(payload)) + payload)
        proc.stdin.flush()
        header = proc.stdout.read(4)
        if len(header) != 4:
            raise RuntimeError('Browser companion host did not return a framed reply header')
        reply = json.loads(proc.stdout.read(struct.unpack('<I', header)[0]).decode('utf-8'))
        if reply != {'version': 1, 'ok': True, 'code': 'host_ready'}:
            raise RuntimeError(f'Unexpected browser companion host reply: {reply}')
    finally:
        proc.stdin.close()
        proc.wait(timeout=10)
    print('PASS: browser companion host offline check_host probe')


def verify_portable(binary: Path, allow_webkit=False):
    """Fail on libraries the target system may lack; dlopen'd X11/Wayland
    libraries are found at runtime and are not listed here."""
    output = run('ldd', binary, capture_output=True, text=True).stdout
    missing = [line for line in output.splitlines() if 'not found' in line]
    if missing:
        raise RuntimeError(f'{binary.name} has unresolved libraries: {missing}')
    allowed = ('linux-vdso', 'libc.so', 'libm.so', 'libgcc_s', 'ld-linux', 'libpthread',
               'libdl.so', 'librt.so',
               # Desktop font stack: present on every graphical Linux system and
               # deliberately not bundled, so Tide uses the system's font settings.
               'libfontconfig', 'libfreetype', 'libexpat', 'libz.so', 'libbz2',
               'libpng16', 'libbrotlidec', 'libbrotlicommon')
    if allow_webkit:
        allowed += ('libwebkit2gtk', 'libjavascriptcoregtk', 'libgtk-3', 'libgdk-3',
                    'libglib', 'libgobject', 'libgio', 'libsoup', 'libcairo', 'libpango',
                    'libgdk_pixbuf', 'libatk', 'libharfbuzz')
    unexpected = [line.strip() for line in output.splitlines()
                  if line.strip() and not any(name in line for name in allowed)]
    if unexpected and not allow_webkit:
        raise RuntimeError(f'{binary.name} links non-base libraries: {unexpected}')
    print(f'PASS: {binary.name} links only base desktop libraries')


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('appdir', type=Path)
    parser.add_argument('--no-runtime', action='store_true')
    args = parser.parse_args()
    appdir = args.appdir.resolve()
    bin_dir = appdir / 'usr/bin'
    for name in ('AppRun', 'tide-music.desktop', 'tide-music.png', '.DirIcon'):
        if not (appdir / name).exists():
            raise RuntimeError(f'AppImage entry {name} is missing')
    desktop = (appdir / 'tide-music.desktop').read_text()
    for line in ('Exec=tide-music', 'Icon=tide-music', 'StartupWMClass=tide-music'):
        if line not in desktop.splitlines():
            raise RuntimeError(f'Desktop entry lacks {line}')
    for name in ('tide-music', 'tide-login', 'tide-session-host'):
        if not (bin_dir / name).is_file():
            raise RuntimeError(f'{name} is missing')
    verify_portable(bin_dir / 'tide-music')
    verify_portable(bin_dir / 'tide-session-host')
    verify_portable(bin_dir / 'tide-login', allow_webkit=True)
    verify_session_host(bin_dir / 'tide-session-host')
    share = appdir / 'usr/share/tide-music'
    for family in ('chromium', 'firefox'):
        if not (share / 'browser-companion' / family / 'manifest.json').is_file():
            raise RuntimeError(f'Bundled {family} browser companion manifest is missing')
    if not args.no_runtime:
        lib = appdir / 'usr/lib/tide-music'
        hooks = sorted((lib / 'mpv/bin').glob('*.hook'))
        if hooks:
            raise RuntimeError(f'mpv AppRun hooks must not ship: {[h.name for h in hooks]}')
        if 'mpv/bin/mpv' not in (lib / 'bin/mpv').read_text():
            raise RuntimeError('The mpv launcher must start bin/mpv, never AppRun')
        mpv = lib / 'bin/mpv'
        version = run(mpv, '--no-config', '--version', capture_output=True, text=True).stdout
        print(version.splitlines()[0], flush=True)
        if not version.startswith('mpv v0.41.0'):
            raise RuntimeError('Bundled mpv must be the pinned 0.41.0 build')
        options = run(mpv, '--no-config', '--list-options',
                      capture_output=True, text=True).stdout
        for option in ('--input-ipc-server', '--input-media-keys', '--ytdl', '--af'):
            if not any(line.split()[:1] == [option] for line in options.splitlines()):
                raise RuntimeError(f'Bundled mpv lacks {option}')
        run(lib / 'bin/yt-dlp', '--version')
        run(lib / 'python/bin/python3', '-I', '-B', '-c',
            'import ytmusicapi, requests, certifi, ruamel.yaml; print("helper imports ok")')
        if not (lib / 'helpers/youtube_music.py').is_file():
            raise RuntimeError('Account helper script is missing')
    player = str(bin_dir / 'tide-music')
    jam = subprocess.run([player, '--smoke-jam-ownership'],
                         capture_output=True, text=True, timeout=30)
    if jam.returncode != 0 or 'PASS offline Jam ownership:' not in jam.stdout + jam.stderr:
        raise RuntimeError(f'Stable Linux AppImage is missing Jams: {jam.returncode}: {jam.stderr}')
    probe = subprocess.run([player, '--verify-update-files'],
                           capture_output=True, text=True, timeout=30)
    if probe.returncode != 1 or 'Expected signed manifest and installer paths' not in probe.stderr:
        raise RuntimeError(f'Headless CLI check failed: {probe.returncode}: {probe.stderr}')
    print('PASS: headless Tide argument-validation launch')


if __name__ == '__main__':
    main()
