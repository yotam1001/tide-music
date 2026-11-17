"""Verify the assembled bundle without a user session or GUI."""
import argparse
from pathlib import Path
import subprocess

from package_macos import macho, run


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
        run(app / 'Contents/Resources/bin/mpv', '--version')
        run(app / 'Contents/Resources/bin/yt-dlp', '--version')
        # The account helper's isolated interpreter imports its pinned packages.
        run(app / 'Contents/Resources/python/bin/python3', '-I', '-c',
            'import ytmusicapi, requests, certifi, ruamel.yaml; print("helper imports ok")')
        if not (app / 'Contents/Resources/helpers/youtube_music.py').is_file():
            raise RuntimeError('Account helper script is missing')
        # Exercise the packaged client without signing in or opening a room.
        run(app / 'Contents/MacOS/tide-music', '--smoke-jam-ownership')
    # This argument validation returns before any Slint/window/account setup.
    probe = subprocess.run([str(app / 'Contents/MacOS/tide-music'), '--verify-update-files'],
                           capture_output=True, text=True, timeout=30)
    if probe.returncode != 1 or 'Expected signed manifest and installer paths' not in probe.stderr:
        raise RuntimeError(f'Headless CLI check failed: {probe.returncode}: {probe.stderr}')
    print('PASS: headless Tide argument-validation launch')


if __name__ == '__main__':
    main()
