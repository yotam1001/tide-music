"""Isolated unit tests for scripts/setup-browser-companion-macos.py."""
import importlib.util
import json
import os
from pathlib import Path
import stat
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "setup_macos",
    Path(__file__).with_name("setup-browser-companion-macos.py"),
)
setup = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(setup)


@unittest.skipIf(os.name == "nt", "macOS companion setup requires POSIX permissions and symlinks")
class MacOSCompanionSetupTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="tide-macos-nmh-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve(strict=True)
        self.home = self.root / "home"
        self.home.mkdir()
        self.host = self.root / "Tide.app/Contents/MacOS/tide-session-host"
        self.host.parent.mkdir(parents=True)
        self.host.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        self.host.chmod(0o755)
        self.ext_id = "a" * 32
        self.ext_id_2 = "b" * 32

    def test_register_and_unregister_chromium_and_firefox(self):
        res = setup.register(self.home, ["chrome", "edge", "brave", "arc", "firefox"], self.host, self.ext_id)
        policy_path = self.home / "Library/Application Support/TideMusic/tide-session-host-policy.json"
        self.assertTrue(policy_path.is_file())
        self.assertEqual(stat.S_IMODE(policy_path.stat().st_mode), 0o600)
        self.assertEqual(
            json.loads(policy_path.read_text(encoding="utf-8")),
            {"allowed_origins": [f"chrome-extension://{self.ext_id}/"]},
        )
        for browser in ("chrome", "edge", "brave", "arc"):
            manifest_path = Path(res["manifests"][browser])
            data = json.loads(manifest_path.read_text(encoding="utf-8"))
            self.assertEqual(data["name"], "org.tidemusic.session")
            self.assertEqual(data["path"], str(self.host))
            self.assertEqual(data["type"], "stdio")
            self.assertEqual(data["allowed_origins"], [f"chrome-extension://{self.ext_id}/"])
            self.assertNotIn("allowed_extensions", data)
        ff = json.loads(Path(res["manifests"]["firefox"]).read_text(encoding="utf-8"))
        self.assertEqual(ff["allowed_extensions"], ["tide-session@tide-music"])
        self.assertNotIn("allowed_origins", ff)

        # Registering a second Chromium extension ID merges cleanly into policy.
        setup.register(self.home, ["chrome"], self.host, f"chrome-extension://{self.ext_id_2.upper()}/")
        self.assertEqual(
            json.loads(policy_path.read_text(encoding="utf-8"))["allowed_origins"],
            [
                f"chrome-extension://{self.ext_id}/",
                f"chrome-extension://{self.ext_id_2}/",
            ],
        )

        st = setup.status(self.home, ["chrome", "firefox"])
        self.assertTrue(st["browsers"]["chrome"]["registered"])
        self.assertTrue(st["browsers"]["firefox"]["registered"])

        removed = setup.unregister(self.home, ["chrome", "firefox"])
        self.assertEqual(removed["removed"], {"chrome": True, "firefox": True})
        self.assertFalse(Path(res["manifests"]["chrome"]).exists())
        self.assertFalse(Path(res["manifests"]["firefox"]).exists())

    def test_chromium_requires_valid_extension_id(self):
        with self.assertRaises(setup.SetupError):
            setup.register(self.home, ["chrome"], self.host, None)
        for bad in ("short", "z" * 32, "chrome-extension://*/", ""):
            with self.subTest(bad=bad), self.assertRaises(setup.SetupError):
                setup.register(self.home, ["chrome"], self.host, bad)

    def test_refuses_symlinks_and_foreign_manifests(self):
        outside = self.root / "outside.json"
        outside.write_text('{"name":"other"}', encoding="utf-8")
        policy_dir = self.home / "Library/Application Support/TideMusic"
        policy_dir.mkdir(parents=True)
        (policy_dir / "tide-session-host-policy.json").symlink_to(outside)
        with self.assertRaises(setup.SetupError):
            setup.register(self.home, ["chrome"], self.host, self.ext_id)
        (policy_dir / "tide-session-host-policy.json").unlink()

        ff_dir = self.home / "Library/Application Support/Mozilla/NativeMessagingHosts"
        ff_dir.mkdir(parents=True)
        foreign = ff_dir / "org.tidemusic.session.json"
        foreign.write_text('{"name":"someone.else"}', encoding="utf-8")
        with self.assertRaises(setup.SetupError):
            setup.register(self.home, ["firefox"], self.host)
        with self.assertRaises(setup.SetupError):
            setup.unregister(self.home, ["firefox"])
        self.assertEqual(json.loads(foreign.read_text(encoding="utf-8"))["name"], "someone.else")

    def test_refuses_non_executable_or_wrong_name_host(self):
        wrong = self.root / "other-bin"
        wrong.write_text("#!/bin/sh\n", encoding="utf-8")
        wrong.chmod(0o755)
        with self.assertRaises(setup.SetupError):
            setup.register(self.home, ["firefox"], wrong)
        os.chmod(self.host, 0o644)
        with self.assertRaises(setup.SetupError):
            setup.register(self.home, ["firefox"], self.host)


if __name__ == "__main__":
    unittest.main()
