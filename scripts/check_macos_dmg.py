"""Assess a quarantined copy from a DMG with the same tools as Gatekeeper."""
import argparse
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

from package_macos import macho, run


def report(*args):
    result = subprocess.run([str(arg) for arg in args], text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
    print(result.stdout, end='', flush=True)
    return result


def assess(app):
    signature = report('codesign', '-dvvv', app)
    seal = report('codesign', '--verify', '--deep', '--strict', '--verbose=4', app)
    policy = report('spctl', '-a', '-vvv', '-t', 'exec', app)
    if shutil.which('syspolicy_check'):
        report('syspolicy_check', 'distribution', app)
    # Ad-hoc signatures can be intact without providing a Developer ID to spctl.
    adhoc_rejection = ('Signature=adhoc' in signature.stdout
                       and 'source=no usable signature' in policy.stdout)
    failures = []
    if signature.returncode or seal.returncode:
        failures.append('The app signature or resource seal is invalid')
    if policy.returncode and not adhoc_rejection:
        failures.append('Unexpected Gatekeeper assessment')

    count = 0
    for path in app.rglob('*'):
        relative = path.relative_to(app)
        if path.is_symlink():
            try:
                path.resolve(strict=True).relative_to(app)
            except (OSError, RuntimeError, ValueError):
                failures.append(f'Broken or external bundle symlink: {relative}')
        elif path.name.startswith('._'):
            failures.append(f'AppleDouble file in bundle: {relative}')
        elif macho(path):
            count += 1
            result = subprocess.run(
                ['codesign', '--verify', '--strict', '--verbose=2', str(path)],
                capture_output=True, text=True, timeout=60)
            if result.returncode:
                failures.append(f'Invalid nested signature: {relative}\n{result.stderr}')
    attributes = run('xattr', '-r', app, capture_output=True, text=True).stdout
    for line in attributes.splitlines():
        if line.endswith(('com.apple.ResourceFork', 'com.apple.FinderInfo')):
            failures.append(f'Unexpected signing-sensitive metadata: {line}')
    print(f'Checked {count} Mach-O signatures, bundle symlinks and extended attributes.',
          flush=True)
    if failures:
        raise RuntimeError('\n'.join(failures))


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('dmg', type=Path)
    dmg = parser.parse_args().dmg.resolve()
    quarantine = f'0081;{int(time.time()):x};Safari;'
    with tempfile.TemporaryDirectory(prefix='tide-dmg-audit-') as temp:
        work = Path(temp).resolve()
        image = work / dmg.name
        shutil.copyfile(dmg, image)
        run('xattr', '-w', 'com.apple.quarantine', quarantine, image)
        mount = work / 'mounted'
        mount.mkdir()
        run('hdiutil', 'attach', '-readonly', '-nobrowse', '-mountpoint', mount, image)
        try:
            app = work / 'Tide.app'
            run('ditto', mount / 'Tide.app', app)
            # Finder can propagate download quarantine. Apply it explicitly
            # because the command-line mount and copy need not do so.
            run('xattr', '-w', 'com.apple.quarantine', quarantine, app)
            run('xattr', '-p', 'com.apple.quarantine', app)
            assess(app)
        finally:
            run('hdiutil', 'detach', mount)


if __name__ == '__main__':
    main()
