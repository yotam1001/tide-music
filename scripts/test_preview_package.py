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
    def archive(self, directory, overrides=None, extra=None, wrong_hash=False):
        payload = {name: b"public preview content\n" for name in preview.PAYLOAD}
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

    def test_corrupt_payload_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
                preview.check(self.archive(directory, wrong_hash=True))


if __name__ == "__main__":
    unittest.main()
