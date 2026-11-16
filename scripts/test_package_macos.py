import plistlib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


class MacOSPlistTests(unittest.TestCase):
    def test_bundle_metadata(self):
        with (ROOT / 'packaging/macos/Info.plist').open('rb') as source:
            info = plistlib.load(source)
        self.assertEqual(info['CFBundleIdentifier'], 'com.tidemusic.Tide')
        self.assertEqual(info['CFBundleExecutable'], 'tide-music')
        self.assertEqual(info['CFBundleShortVersionString'], '@VERSION@')
        self.assertEqual(info['LSMinimumSystemVersion'], '12.0')
        self.assertEqual(info['LSApplicationCategoryType'], 'public.app-category.music')
        self.assertTrue(info['NSHighResolutionCapable'])
        self.assertEqual(info['NSAppTransportSecurity'], {})
        self.assertNotIn('NSMicrophoneUsageDescription', info)

    def test_entitlements_are_minimal(self):
        with (ROOT / 'packaging/macos/Tide.entitlements').open('rb') as source:
            self.assertEqual(plistlib.load(source), {})


if __name__ == '__main__':
    unittest.main()
