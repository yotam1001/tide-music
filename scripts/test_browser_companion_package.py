"""Deterministic packaging tests; synthetic temporary workspaces only."""
import hashlib
import importlib.util
import json
from pathlib import Path
import stat
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock
import zipfile

SPEC = importlib.util.spec_from_file_location("browser_packager", Path(__file__).with_name("package-browser-companion.py"))
packager = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(packager)


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="tide-package-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "workspace"
        self.source = self.root / "browser-companion"
        self.source.mkdir(parents=True)
        # Windows CI's TEMP may use an 8.3 alias. Match the production
        # packager's canonical workspace before targeting lstat mocks.
        self.root = self.root.resolve(strict=True)
        self.source = self.root / "browser-companion"
        self.csp = "default-src 'self'; script-src 'self'; object-src 'none'; connect-src 'none'"
        for name in (*packager.COMMON_FILES, "background-firefox.js", "background-chromium.js"):
            (self.source / name).write_bytes(f"synthetic local fixture {name}\n".encode())
        self.manifests = {
            "firefox": {
                "manifest_version": 2, "name": "Synthetic", "version": "0.1.0",
                "permissions": ["activeTab", "webRequest", "nativeMessaging", packager.MUSIC_PATTERN],
                "background": {"scripts": ["background.js", "background-firefox.js"], "persistent": True},
                "browser_action": {"default_popup": "popup.html"},
                "browser_specific_settings": {"gecko": {"id": "tide-session@tide-music"}},
                "content_security_policy": self.csp,
            },
            "chromium": {
                "manifest_version": 3, "name": "Synthetic", "version": "0.1.0",
                "permissions": ["activeTab", "webRequest", "nativeMessaging"],
                "host_permissions": [packager.MUSIC_PATTERN],
                "background": {"service_worker": "background-chromium.js"},
                "action": {"default_popup": "popup.html"},
                "content_security_policy": {"extension_pages": self.csp},
            },
        }
        for family, manifest in self.manifests.items():
            self.write_manifest(family, manifest)

    def write_manifest(self, family, manifest):
        (self.source / f"manifest.{family}.json").write_text(json.dumps(manifest), encoding="utf-8")

    def assert_no_staging(self):
        self.assertFalse(list((self.root / "dist").glob(".tide-browser-stage-*")))

    def test_exact_payload_manifest_and_checksums_both_families(self):
        # Unrelated source additions, native-host paths and synthetic credentials cannot enter the ZIP.
        (self.source / "native-host.json").write_text('{"path":"C:/outside/native-host.exe"}')
        (self.source / "session.json").write_text('{"cookie":"synthetic-runtime-credential"}')
        (self.source / "README.md").write_text("not extension payload")
        for family in packager.FAMILY_FILES:
            with self.subTest(family=family):
                output = packager.package_companion(self.root, family)
                expected = {*packager.COMMON_FILES, f"background-{family}.js", "manifest.json"}
                self.assertEqual({p.name for p in (output / "extension").iterdir()}, expected)
                archive = output / f"tide-browser-{family}.zip"
                with zipfile.ZipFile(archive) as content:
                    self.assertEqual(set(content.namelist()), expected)
                    self.assertEqual(json.loads(content.read("manifest.json")), self.manifests[family])
                    for name in expected:
                        self.assertEqual(content.read(name), (output / "extension" / name).read_bytes())
                        self.assertEqual(content.getinfo(name).date_time, (1980, 1, 1, 0, 0, 0))
                    self.assertNotIn(b"synthetic-runtime-credential", b"".join(content.read(name) for name in expected))
                    self.assertNotIn(b"C:/outside", b"".join(content.read(name) for name in expected))
                checksums = (output / "SHA256SUMS").read_text().splitlines()
                self.assertEqual(len(checksums), len(expected) + 1)
                for line in checksums:
                    digest, name = line.split("  ", 1)
                    self.assertEqual(digest, hashlib.sha256((output / name).read_bytes()).hexdigest())
                self.assert_no_staging()

    def test_zip_bytes_are_deterministic(self):
        first = packager.package_companion(self.root, "firefox", "first")
        second = packager.package_companion(self.root, "firefox", "second")
        self.assertEqual((first / "first.zip").read_bytes(), (second / "second.zip").read_bytes())

    def test_preexisting_destination_is_preserved(self):
        existing = self.root / "dist" / "tide-browser-firefox"
        existing.mkdir(parents=True)
        (existing / "keep").write_text("unchanged")
        with self.assertRaises(packager.PackageError):
            packager.package_companion(self.root, "firefox")
        self.assertEqual((existing / "keep").read_text(), "unchanged")
        self.assert_no_staging()

    def test_outside_dist_and_reserved_destination_rejected(self):
        # Construct a synthetic drive path without embedding an absolute path in public source.
        synthetic_drive_path = "C" + chr(58) + "\\outside"
        for destination in [str(self.root.parent / "outside"), "../outside", "dist/output", "dist\\output", synthetic_drive_path, ".", "..", "NUL", "con.txt", "trailing.", " spaced", "name:stream"]:
            with self.subTest(destination=destination), self.assertRaises(packager.PackageError):
                packager.package_companion(self.root, "firefox", destination)
        self.assertFalse((self.root / "dist").exists())

    def test_missing_allowlisted_file_rejected_before_output(self):
        (self.source / "popup.js").unlink()
        with self.assertRaises(packager.PackageError):
            packager.package_companion(self.root, "firefox")
        self.assertFalse((self.root / "dist").exists())

    def test_link_and_windows_reparse_metadata_are_always_rejected(self):
        # Exercise the production lstat gate on every host, even when creating links requires privilege.
        dist = self.root / "dist"
        dist.mkdir()
        real_lstat = Path.lstat
        for target in [self.source, self.source / "popup.js", dist]:
            for mode, attributes in [(stat.S_IFLNK | 0o777, 0), (stat.S_IFDIR | 0o755, 0x400)]:
                def synthetic_lstat(path, *args, **kwargs):
                    if path == target:
                        return SimpleNamespace(st_mode=mode, st_file_attributes=attributes)
                    return real_lstat(path, *args, **kwargs)
                with self.subTest(target=target.name, mode=mode, attributes=attributes):
                    with mock.patch.object(Path, "lstat", synthetic_lstat), self.assertRaises(packager.PackageError):
                        packager.package_companion(self.root, "firefox")
                self.assertEqual(list(dist.iterdir()), [])

    def test_new_destination_between_stage_and_publish_is_preserved(self):
        final = self.root / "dist" / "tide-browser-firefox"
        real_zip = packager.zipfile.ZipFile
        def create_competing_destination(*args, **kwargs):
            final.mkdir()
            (final / "keep").write_text("competing output")
            return real_zip(*args, **kwargs)
        with mock.patch.object(packager.zipfile, "ZipFile", create_competing_destination):
            with self.assertRaises(packager.PackageError):
                packager.package_companion(self.root, "firefox")
        self.assertEqual((final / "keep").read_text(), "competing output")
        self.assert_no_staging()

    def test_empty_destination_created_at_rename_cannot_be_overwritten(self):
        final = self.root / "dist" / "tide-browser-firefox"
        publish = packager._publish_no_replace
        def competing_empty_destination(staging, destination):
            destination.mkdir()
            publish(staging, destination)
        with mock.patch.object(packager, "_publish_no_replace", competing_empty_destination):
            with self.assertRaises(OSError):
                packager.package_companion(self.root, "firefox")
        self.assertTrue(final.is_dir())
        self.assertEqual(list(final.iterdir()), [])
        self.assert_no_staging()

    def test_manifest_broad_capability_and_wrong_family_rejected(self):
        cases = [
            {**self.manifests["firefox"], "permissions": ["<all_urls>"]},
            {**self.manifests["firefox"], "background": {"scripts": ["remote.js"]}},
            {**self.manifests["firefox"], "content_scripts": []},
            self.manifests["chromium"],
            {**self.manifests["firefox"], "content_security_policy": "script-src https://outside.invalid"},
        ]
        for manifest in cases:
            self.write_manifest("firefox", manifest)
            with self.assertRaises(packager.PackageError):
                packager.package_companion(self.root, "firefox")
        self.assertFalse((self.root / "dist").exists())

    def symlink(self, link, target, is_directory=False):
        try:
            link.symlink_to(target, target_is_directory=is_directory)
        except (OSError, NotImplementedError) as error:
            self.skipTest(f"Symlink creation unavailable on this test host: {error}")

    def test_allowlisted_source_symlink_rejected(self):
        target = self.root.parent / "outside.js"
        target.write_text("outside fixture")
        (self.source / "popup.js").unlink()
        self.symlink(self.source / "popup.js", target)
        with self.assertRaises(packager.PackageError):
            packager.package_companion(self.root, "firefox")
        self.assertFalse((self.root / "dist").exists())

    def test_dist_symlink_rejected_without_writing_target(self):
        outside = self.root.parent / "outside"
        outside.mkdir()
        self.symlink(self.root / "dist", outside, True)
        with self.assertRaises(packager.PackageError):
            packager.package_companion(self.root, "firefox")
        self.assertEqual(list(outside.iterdir()), [])

    def test_source_directory_symlink_rejected(self):
        actual = self.root / "actual-source"
        self.source.rename(actual)
        self.symlink(self.source, actual, True)
        with self.assertRaises(packager.PackageError):
            packager.package_companion(self.root, "firefox")

    def test_failed_zip_cleans_only_own_staging(self):
        unrelated = self.root / "dist" / ".tide-browser-stage-unrelated"
        unrelated.mkdir(parents=True)
        (unrelated / "keep").write_text("untouched")
        with mock.patch.object(packager.zipfile, "ZipFile", side_effect=OSError("synthetic failure")):
            with self.assertRaises(OSError):
                packager.package_companion(self.root, "firefox")
        self.assertEqual((unrelated / "keep").read_text(), "untouched")
        self.assertEqual(list((self.root / "dist").iterdir()), [unrelated])


if __name__ == "__main__":
    unittest.main()
