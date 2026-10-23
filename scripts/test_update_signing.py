import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

spec = importlib.util.spec_from_file_location("sign_update", Path(__file__).with_name("sign-update.py"))
signing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(signing)


class SignedUpdateTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.installer = Path(self.directory.name) / "TideSetup.exe"
        self.installer.write_bytes(b"Synthetic installer fixture, never executed")
        self.key = Ed25519PrivateKey.generate()
        self.public = self.key.public_key().public_bytes_raw()
        self.now = 1_800_000_000
        self.envelope = signing.make_envelope(self.installer, "0.4.1", self.key, self.now)

    def verify(self, envelope=None, **kwargs):
        signing.verify_envelope(self.envelope if envelope is None else envelope, self.installer,
                                kwargs.get("version", "0.4.1"), kwargs.get("key", self.public),
                                kwargs.get("now", self.now))

    def test_valid_signature_and_rejection_of_changed_installer(self):
        self.verify()
        self.installer.write_bytes(b"Different installer, same server")
        with self.assertRaises(ValueError):
            self.verify()

    def test_wrong_key_expiry_and_version_are_rejected(self):
        for options in [dict(key=Ed25519PrivateKey.generate().public_key().public_bytes_raw()),
                        dict(version="0.4.2"), dict(now=self.now + signing.MAX_VALIDITY)]:
            with self.assertRaises(Exception):
                self.verify(**options)

    def test_server_cannot_edit_signed_bytes_or_select_another_key(self):
        value = json.loads(self.envelope)
        value["payload"] = value["payload"].replace("0.4.1", "9.0.0")
        with self.assertRaises(Exception):
            self.verify(json.dumps(value).encode())
        value = json.loads(self.envelope)
        value["public_key"] = Ed25519PrivateKey.generate().public_key().public_bytes_raw().hex()
        with self.assertRaises(ValueError):
            self.verify(json.dumps(value).encode())

    def test_non_stable_versions_and_oversized_manifests_are_rejected(self):
        for version in ["0.4.1-preview.1", "../other", "v0.4.1"]:
            with self.assertRaises(ValueError):
                signing.make_envelope(self.installer, version, self.key, self.now)
        with self.assertRaises(ValueError):
            self.verify(b" " * (16 * 1024 + 1))

    @unittest.skipUnless(os.name == "nt", "Windows DPAPI integration")
    def test_dpapi_roundtrip_and_tampering_fail_closed(self):
        data = b"Disposable test material, not the publisher's signing key"
        encrypted = signing.protect(data)
        self.assertNotIn(data, encrypted)
        self.assertEqual(signing.protect(encrypted, decrypt=True), data)
        with self.assertRaises(RuntimeError):
            signing.protect(encrypted[:-8] + b"changed!", decrypt=True)


if __name__ == "__main__":
    unittest.main()
