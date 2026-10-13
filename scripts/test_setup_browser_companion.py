"""Preparation-only checks: never register a host or access browser profiles."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("setup-browser-companion.ps1")


@unittest.skipUnless(os.name == "nt" and shutil.which("pwsh"), "requires Windows PowerShell")
class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="tide-companion-test-")
        self.root = Path(self.temporary.name).resolve()
        assert self.root.parent == Path(tempfile.gettempdir()).resolve()
        assert self.root.name.startswith("tide-companion-test-")
        self.host = self.root / "tide-session-host.exe"
        self.host.write_bytes(b"synthetic path fixture; never executed")
        self.output = self.root / "prepared"

    def tearDown(self):
        self.temporary.cleanup()

    def prepare(self, *extra):
        return subprocess.run(
            ["pwsh", "-NoProfile", "-File", str(SCRIPT), "-Mode", "Prepare",
             "-HostExecutable", str(self.host), "-Destination", str(self.output), *extra],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15, check=False,
        )

    def test_firefox_preparation_has_exact_identity_and_no_chromium_origins(self):
        self.assertEqual(self.prepare().returncode, 0)
        manifest = json.loads((self.output / "firefox-host.json").read_text())
        self.assertEqual(manifest["allowed_extensions"], ["tide-session@tide-music"])
        self.assertEqual(manifest["name"], "org.tidemusic.session")
        self.assertEqual(Path(manifest["path"]), self.host)
        self.assertEqual(manifest["type"], "stdio")
        self.assertEqual(json.loads((self.output / "tide-session-host-policy.json").read_text()), {"allowed_origins": []})
        self.assertFalse((self.output / "chromium-host.json").exists())
        self.assertFalse((self.host.parent / "tide-session-host-policy.json").exists())

    def test_chromium_manifest_and_policy_use_the_same_exact_origin(self):
        self.assertEqual(self.prepare("-ChromiumExtensionId", "a" * 32).returncode, 0)
        expected = ["chrome-extension://" + "a" * 32 + "/"]
        for name in ("chromium-host.json", "tide-session-host-policy.json"):
            raw = (self.output / name).read_bytes()
            self.assertFalse(raw.startswith(b"\xef\xbb\xbf"))
            self.assertEqual(json.loads(raw)["allowed_origins"], expected)

    def test_invalid_id_and_wrong_executable_are_rejected_before_output(self):
        for value in ("*", "a" * 31, "z" * 32, "a" * 32 + "/"):
            self.assertNotEqual(self.prepare("-ChromiumExtensionId", value).returncode, 0)
            self.assertFalse(self.output.exists())
        self.host = self.root / "other.exe"
        self.host.write_bytes(b"fixture")
        self.assertNotEqual(self.prepare().returncode, 0)
        self.assertFalse(self.output.exists())

    def test_existing_destination_and_files_are_preserved(self):
        self.output.mkdir()
        sentinel = self.output / "keep.txt"
        sentinel.write_bytes(b"existing output")
        self.assertNotEqual(self.prepare().returncode, 0)
        self.assertEqual(sentinel.read_bytes(), b"existing output")
        self.assertEqual(list(self.output.iterdir()), [sentinel])


if __name__ == "__main__":
    unittest.main()
