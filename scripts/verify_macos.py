"""Verify the assembled bundle without a user session or GUI."""
import argparse
import json
from pathlib import Path
import struct
import subprocess

from package_macos import macho, run


def verify_session_host(host: Path):
    if not host.is_file():
        raise RuntimeError('Browser companion native host is missing')
    payload = json.dumps({'version': 1, 'action': 'check_host', 'headers': {}}).encode('utf-8')
    framed = struct.pack('<I', len(payload)) + payload
    # Send one framed check_host message and keep stdin open briefly via Popen so the
    # host can reply before EOF signals cancellation.
    proc = subprocess.Popen(
        [str(host), '/Applications/Tide.app/org.tidemusic.session.json', 'tide-session@tide-music'],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        assert proc.stdin is not None and proc.stdout is not None
        proc.stdin.write(framed)
        proc.stdin.flush()
        header = proc.stdout.read(4)
        if len(header) != 4:
            raise RuntimeError('Browser companion host did not return a framed reply header')
        length = struct.unpack('<I', header)[0]
        body = proc.stdout.read(length)
        reply = json.loads(body.decode('utf-8'))
        if reply != {'version': 1, 'ok': True, 'code': 'host_ready'}:
            raise RuntimeError(f'Unexpected browser companion host reply: {reply}')
    finally:
        if proc.stdin is not None:
            proc.stdin.close()
        proc.wait(timeout=10)
    print('PASS: browser companion host offline check_host probe')


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('app', type=Path)
    parser.add_argument('--no-runtime', action='store_true')
    args = parser.parse_args()
    app = args.app.resolve()
    run('plutil', '-lint', app / 'Contents/Info.plist')
    run('codesign', '--verify', '--strict', '--verbose=2', app)
    if not (app / 'Contents/MacOS/tide-login').is_file():
        raise RuntimeError('Native sign-in helper is missing')
    verify_session_host(app / 'Contents/MacOS/tide-session-host')
    for family in ('chromium', 'firefox'):
        if not (app / 'Contents/Resources/browser-companion' / family / 'manifest.json').is_file():
            raise RuntimeError(f'Bundled {family} browser companion manifest is missing')
    for path in app.rglob('*'):
        if macho(path):
            deps = subprocess.check_output(['otool', '-L', str(path)], text=True)
            print(deps)
            if '/opt/homebrew/' in deps or '/usr/local/' in deps:
                raise RuntimeError(f'Nonportable dependency in {path}')
            commands = subprocess.check_output(['otool', '-l', str(path)], text=True)
            for block in commands.split('Load command '):
                if 'cmd LC_RPATH\n' in block and any(
                        prefix in block for prefix in ['/opt/homebrew/', '/usr/local/']):
                    raise RuntimeError(f'Nonportable runtime search path in {path}')
    if not args.no_runtime:
        mpv = app / 'Contents/Resources/bin/mpv'
        run(mpv, '--version')
        options = run(mpv, '--no-config', '--media-controls=no', '--list-options',
                      capture_output=True, text=True).stdout
        media_options = [line for line in options.splitlines() if 'media' in line.lower()]
        print('\n'.join(media_options), flush=True)
        if not any(line.split()[0] == '--media-controls' for line in media_options):
            raise RuntimeError('Bundled mpv must accept --media-controls=no')
        run(app / 'Contents/Resources/bin/yt-dlp', '--version')
        # -B prevents imports from adding files after the resource seal is signed.
        run(app / 'Contents/Resources/python/bin/python3', '-I', '-B', '-c',
            'import ytmusicapi, requests, certifi, ruamel.yaml; print("helper imports ok")')
        if not (app / 'Contents/Resources/helpers/youtube_music.py').is_file():
            raise RuntimeError('Account helper script is missing')
    player = str(app / 'Contents/MacOS/tide-music')
    # Exercise the packaged Jam client without signing in or opening a room.
    jam = subprocess.run([player, '--smoke-jam-ownership'],
                         capture_output=True, text=True, timeout=30)
    if jam.returncode != 0 or 'PASS offline Jam ownership:' not in jam.stdout + jam.stderr:
        raise RuntimeError(f'Stable macOS bundle is missing Jams: {jam.returncode}: {jam.stderr}')
    # This argument validation returns before any Slint/window/account setup.
    probe = subprocess.run([player, '--verify-update-files'],
                           capture_output=True, text=True, timeout=30)
    if probe.returncode != 1 or 'Expected signed manifest and installer paths' not in probe.stderr:
        raise RuntimeError(f'Headless CLI check failed: {probe.returncode}: {probe.stderr}')
    print('PASS: headless Tide argument-validation launch')


if __name__ == '__main__':
    main()
