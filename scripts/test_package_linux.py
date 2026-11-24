import configparser
import json
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PACK = ROOT / 'packaging/linux'


class LinuxPackagingTests(unittest.TestCase):
    def test_runtime_lock(self):
        lock = json.loads((PACK / 'runtime.lock.json').read_text())
        self.assertEqual(lock['schema_version'], 1)
        self.assertEqual(set(lock['mpv']), {'x86_64'})
        entries = [lock['appimage_runtime']['x86_64'], lock['mpv']['x86_64'], lock['yt-dlp'],
                   lock['yt-dlp']['checksums'], lock['python']['x86_64']]
        for entry in entries:
            with self.subTest(url=entry['url']):
                self.assertTrue(entry['url'].startswith('https://github.com/'))
                self.assertRegex(entry['sha256'], r'^[0-9a-f]{64}$')
                self.assertTrue(entry['license'].strip())
        # Same mpv as the macOS arm64 bundle: 0.42 rejects a Tide audio filter.
        mac = json.loads((ROOT / 'packaging/macos/runtime.lock.json').read_text())
        self.assertEqual(lock['mpv']['x86_64']['version'], mac['mpv']['arm64']['version'])
        self.assertEqual(lock['mpv']['x86_64']['format'], 'appimage')
        self.assertEqual(lock['yt-dlp']['version'], mac['yt-dlp']['version'])
        self.assertEqual(lock['yt-dlp']['checksums'], mac['yt-dlp']['checksums'])
        self.assertEqual(lock['python']['x86_64']['version'], mac['python']['arm64']['version'])

    def test_packaging_scripts(self):
        for script in ('scripts/package-linux.sh', 'packaging/linux/AppRun'):
            mode = subprocess.check_output(['git', 'ls-files', '-s', script],
                                           cwd=ROOT, text=True).split()[0]
            self.assertEqual(mode, '100755', script)
        source = (ROOT / 'scripts/package_linux.py').read_text()
        self.assertIn("'--locked'", source)
        # Stable packages always include Jams: never build without default features.
        self.assertNotIn('--no-default-features', source)
        self.assertIn("STABLE_NAME = 'Tide-x86_64.AppImage'", source)

    def test_desktop_entry(self):
        entry = configparser.ConfigParser(interpolation=None)
        entry.optionxform = str
        entry.read(PACK / 'tide-music.desktop')
        desktop = entry['Desktop Entry']
        self.assertEqual(desktop['Exec'], 'tide-music')
        self.assertEqual(desktop['Icon'], 'tide-music')
        self.assertEqual(desktop['StartupWMClass'], 'tide-music')
        self.assertEqual(desktop['X-AppImage-Version'], '@VERSION@')
        self.assertIn('AudioVideo', desktop['Categories'])

    def test_apprun_starts_the_player_and_keeps_host_paths(self):
        apprun = (PACK / 'AppRun').read_text()
        self.assertTrue(apprun.startswith('#!/bin/sh\n'))
        self.assertIn('exec "$HERE/usr/bin/tide-music" "$@"', apprun)
        self.assertIn('TIDE_HOST_', apprun)

    def test_update_package_name_matches_the_player(self):
        updater = (ROOT / 'crates/app/src/updater.rs').read_text()
        self.assertIn('"Tide-x86_64.AppImage"', updater)
        self.assertIn('TideUpdate-linux.json', updater)
        signer = (ROOT / 'scripts/sign-update.py').read_text()
        self.assertIn('Tide-x86_64.AppImage', signer)
        self.assertIn('linux-x86_64', signer)


if __name__ == '__main__':
    unittest.main()
