"""Check that a Mac packaging rerun never replaces assets on a published release."""
import unittest

from attach_macos_assets import AttachError, plan


LOCAL = {"Tide.dmg": "a" * 64, "Tide.dmg.sha256": "b" * 64}


def asset(name, digest):
    return {"name": name, "digest": f"sha256:{digest}"}


class PublishDecisionTests(unittest.TestCase):
    def test_draft_release_receives_every_rebuilt_file(self):
        release = {"isDraft": True, "assets": [asset("Tide.dmg", "c" * 64)]}
        self.assertEqual(plan(release, LOCAL), ["Tide.dmg", "Tide.dmg.sha256"])

    def test_public_release_with_different_rebuild_is_left_unchanged(self):
        release = {"isDraft": False, "assets": [asset("Tide.dmg", "c" * 64),
                                                asset("Tide.dmg.sha256", "d" * 64),
                                                asset("SHA256SUMS.txt", "e" * 64)]}
        self.assertEqual(plan(release, LOCAL), [])

    def test_public_release_with_identical_assets_is_left_unchanged(self):
        release = {"isDraft": False, "assets": [asset(name, digest) for name, digest in LOCAL.items()]}
        self.assertEqual(plan(release, LOCAL), [])

    def test_public_release_missing_an_asset_fails_instead_of_uploading(self):
        release = {"isDraft": False, "assets": [asset("Tide.dmg", "a" * 64)]}
        with self.assertRaisesRegex(AttachError, "Tide.dmg.sha256"):
            plan(release, LOCAL)


if __name__ == "__main__":
    unittest.main()
