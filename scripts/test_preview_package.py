"""Exercise the archive gate with checksum-valid synthetic credential leaks."""
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location(
    "preview_package", Path(__file__).with_name("check-preview-package.py"))
preview = importlib.util.module_from_spec(spec)
spec.loader.exec_module(preview)


class PreviewPackageTests(unittest.TestCase):
    def archive(self, directory, overrides=None, extra=None, wrong_hash=False, with_browser_host=False):
        names = preview.PAYLOAD | (preview.BROWSER_HOST_PAYLOAD if with_browser_host else set())
        payload = {name: b"public preview content\n" for name in names}
        payload.update(overrides or {})
        manifest = "\n".join(
            ("0" * 64 if wrong_hash else hashlib.sha256(data).hexdigest())
            + "  " + name for name, data in sorted(payload.items()))
        payload["SHA256SUMS.txt"] = manifest.encode()
        payload.update(extra or {})
        path = Path(directory) / "preview.zip"
        with zipfile.ZipFile(path, "w") as archive:
            for name, data in payload.items():
                archive.writestr(name, data)
        return path

    def test_static_binary_literals_are_accepted(self):
        with tempfile.TemporaryDirectory() as directory:
            path = self.archive(directory, {"tide-music.exe":
                b"MZ\0mask-" + b"property" * 8 + b"\0__Secure-3PAPISID=\0"})
            preview.check(path)

    def test_checksum_valid_credentials_are_rejected(self):
        for name, data in [
            ("tide-music.exe", b"\0sk-" + b"fixture" * 8 + b"\0"),
            ("tide-music.exe", b"\0__Secure-3PAPISID=" + b"fixture" * 8 + b"\0"),
            ("helpers/youtube_music.py", b"\nsk-" + b"fixture" * 8 + b"\n"),
        ]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                with self.assertRaisesRegex(ValueError, "privacy check failed"):
                    preview.check(self.archive(directory, {name: data}))

    def test_unexpected_files_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, "unexpected"):
                preview.check(self.archive(directory, extra={"private.txt": b"private"}))

    def test_development_outputs_cannot_ship_with_the_player(self):
        for name in ("target/debug/tide-music.pdb",
                     "target/debug/incremental/query-cache.bin",
                     ".tools/rustup/compiler.exe", ".local/test-run/output.log"):
            for with_host in (False, True):
                with self.subTest(name=name, with_host=with_host), tempfile.TemporaryDirectory() as directory:
                    path = self.archive(directory, extra={name: b"generated development data"},
                                        with_browser_host=with_host)
                    with self.assertRaisesRegex(ValueError, "unexpected"):
                        preview.check(path, with_browser_host=with_host)

    def test_corrupt_payload_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
                preview.check(self.archive(directory, wrong_hash=True))

    def test_browser_host_requires_explicit_complete_payload(self):
        with tempfile.TemporaryDirectory() as directory:
            path = self.archive(directory, with_browser_host=True)
            preview.check(path, with_browser_host=True)
            with self.assertRaisesRegex(ValueError, "unexpected"):
                preview.check(path)
            path = self.archive(directory)
            with self.assertRaisesRegex(ValueError, "unexpected"):
                preview.check(path, with_browser_host=True)

    def test_browser_host_credentials_and_checksums_are_checked(self):
        with tempfile.TemporaryDirectory() as directory:
            path = self.archive(directory, with_browser_host=True,
                                overrides={"tide-session-host.exe": b"\0sk-" + b"fixture" * 8 + b"\0"})
            with self.assertRaisesRegex(ValueError, "privacy check failed"):
                preview.check(path, with_browser_host=True)
            path = self.archive(directory, with_browser_host=True, wrong_hash=True)
            with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
                preview.check(path, with_browser_host=True)


if __name__ == "__main__":
    unittest.main()
