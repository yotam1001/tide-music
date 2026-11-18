import plistlib
import unittest
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


class MacOSPlistTests(unittest.TestCase):
    def test_runtime_lock(self):
        lock = json.loads((ROOT / 'packaging/macos/runtime.lock.json').read_text())
        self.assertEqual(lock['schema_version'], 1)
        self.assertEqual(lock['minimum_macos'], '14.0')
        self.assertEqual(set(lock['mpv']), {'arm64', 'x86_64'})
        self.assertEqual(set(lock['python']), {'arm64', 'x86_64'})
        entries = [*lock['mpv'].values(), lock['yt-dlp'], lock['yt-dlp']['checksums'],
                   *lock['python'].values(), *lock['helper_wheels']]
        for entry in entries:
            with self.subTest(url=entry['url']):
                self.assertTrue(entry['url'].startswith('https://'))
                self.assertRegex(entry['sha256'], r'^[0-9a-f]{64}$')
                self.assertTrue(entry['license'].strip())
        for entry in lock['mpv'].values():
            self.assertIn(entry['format'], ['tar', 'zip-tar'])
            self.assertTrue(entry['version'])
            self.assertEqual(entry['ffmpeg_license'], 'GPL-3.0-or-later')
        for name in ['GPL-2.0.txt', 'GPL-3.0.txt', 'LGPL-2.1.txt', 'LGPL-3.0.txt',
                     'yt-dlp-LICENSE.txt', 'mpv-Copyright.txt', 'FFmpeg-LICENSE.md']:
            self.assertTrue((ROOT / 'packaging/macos/licenses' / name).is_file())

    def test_helper_wheels_match_windows_pins(self):
        # The Mac bundle must ship exactly the account-helper wheels Windows trusts.
        import re
        lock = json.loads((ROOT / 'packaging/macos/runtime.lock.json').read_text())
        windows = dict(re.findall(r"'([A-Za-z0-9_.-]+\.whl)'='([0-9a-f]{64})'",
                                  (ROOT / 'scripts/setup-provider.ps1').read_text()))
        mac = {w['url'].rsplit('/', 1)[1]: w['sha256'] for w in lock['helper_wheels']}
        self.assertTrue(windows)
        self.assertEqual(mac, windows)

    def test_packaging_scripts(self):
        self.assertTrue((ROOT / 'scripts/package-macos.sh').is_file())
        mode = subprocess.check_output(['git', 'ls-files', '-s', 'scripts/package-macos.sh'],
                                       cwd=ROOT, text=True).split()[0]
        self.assertEqual(mode, '100755')
        source = (ROOT / 'scripts/package_macos.py').read_text()
        self.assertIn("'--no-default-features'", source)
        self.assertIn("'--features', 'jams'", source)
        self.assertIn("'--locked'", source)
        self.assertNotIn("'--deep'", source)

    def test_bundle_metadata(self):
        with (ROOT / 'packaging/macos/Info.plist').open('rb') as source:
            info = plistlib.load(source)
        self.assertEqual(info['CFBundleIdentifier'], 'com.tidemusic.Tide')
        self.assertEqual(info['CFBundleExecutable'], 'tide-music')
        self.assertEqual(info['CFBundleShortVersionString'], '@VERSION@')
        self.assertEqual(info['LSMinimumSystemVersion'], '14.0')
        self.assertEqual(info['LSApplicationCategoryType'], 'public.app-category.music')
        self.assertTrue(info['NSHighResolutionCapable'])
        self.assertEqual(info['NSAppTransportSecurity'], {})
        self.assertNotIn('NSMicrophoneUsageDescription', info)

    def test_entitlements_are_minimal(self):
        with (ROOT / 'packaging/macos/Tide.entitlements').open('rb') as source:
            self.assertEqual(plistlib.load(source), {})


if __name__ == '__main__':
    unittest.main()
