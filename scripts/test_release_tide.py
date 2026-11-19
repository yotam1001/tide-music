"""Check that a failed release preparation leaves no tracked edits behind for retry."""
from datetime import date
from pathlib import Path
import json
import os
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import release_tide


def run_git(cwd, *args):
    return subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True, text=True).stdout


class PrepareRollbackTests(unittest.TestCase):
    def make_checkout(self, root):
        origin = root / "origin.git"
        work = root / "work"
        run_git(root, "init", "--bare", "--initial-branch=main", str(origin))
        work.mkdir()
        run_git(work, "init", "--initial-branch=main")
        run_git(work, "config", "user.email", "test@example.invalid")
        run_git(work, "config", "user.name", "Test")
        (work / "Cargo.toml").write_text('[workspace.package]\nversion = "0.1.0"\n', "utf-8")
        (work / "Cargo.lock").write_text('[[package]]\nname = "tide-core"\nversion = "0.1.0"\n', "utf-8")
        (work / "experiments/jam").mkdir(parents=True)
        (work / "experiments/jam/Cargo.lock").write_text('[[package]]\nname = "tide-jam"\nversion = "0.1.0"\n', "utf-8")
        (work / "CHANGELOG.md").write_text("# Changelog\n\n## Unreleased\n\n- Change.\n", "utf-8")
        run_git(work, "add", "--", ".")
        run_git(work, "commit", "-m", "initial")
        run_git(work, "remote", "add", "origin", str(origin))
        run_git(work, "push", "-u", "origin", "main")
        return work

    def test_failed_check_leaves_clean_tree_for_retry(self):
        with tempfile.TemporaryDirectory(prefix="tide-release-test-") as temporary:
            work = self.make_checkout(Path(temporary).resolve())
            args = mock.Mock(version="0.2.0", date=date(2026, 11, 18), pin=[])
            release = release_tide.Release(args)
            with mock.patch.object(release_tide, "ROOT", work), \
                    mock.patch.object(release_tide, "gh", return_value="[]"), \
                    mock.patch.object(release_tide, "python", side_effect=release_tide.ReleaseError("injected check failure")):
                with self.assertRaises(release_tide.ReleaseError):
                    release.prepare()
            status = run_git(work, "status", "--porcelain", "--untracked-files=no")
            self.assertEqual(status, "", "Tracked edits left behind for retry")
            self.assertEqual(run_git(work, "rev-parse", "--abbrev-ref", "HEAD").strip(), "release/0.2.0")

            with mock.patch.object(release_tide, "ROOT", work), \
                    mock.patch.object(release_tide, "gh", return_value="[]"), \
                    mock.patch.object(release_tide, "python", return_value=None):
                release.prepare()
            self.assertEqual(run_git(work, "show", "-s", "--format=%s", "HEAD").strip(), "Release 0.2.0")

    def test_concurrent_edits_survive_validation(self):
        with tempfile.TemporaryDirectory() as temporary:
            work = self.make_checkout(Path(temporary).resolve())
            changelog = work / "CHANGELOG.md"
            args = mock.Mock(version="0.2.0", date=date(2026, 11, 18), pin=[])

            def edit_during_check(*args, cwd):
                self.assertNotEqual(cwd, work)
                changelog.write_bytes(b"staged by user\n")
                run_git(work, "add", "CHANGELOG.md")
                changelog.write_bytes(b"edited again\n")

            with mock.patch.object(release_tide, "ROOT", work), \
                    mock.patch.object(release_tide, "gh", return_value="[]"), \
                    mock.patch.object(release_tide, "python", side_effect=edit_during_check):
                with self.assertRaisesRegex(release_tide.ReleaseError, "CHANGELOG.md"):
                    release_tide.Release(args).prepare()
            self.assertEqual(run_git(work, "show", ":CHANGELOG.md"), "staged by user\n")
            self.assertEqual(changelog.read_bytes(), b"edited again\n")
            self.assertEqual(run_git(work, "rev-parse", "HEAD"), run_git(work, "rev-parse", "origin/main"))

    def test_retry_after_process_exits_during_validation(self):
        with tempfile.TemporaryDirectory() as temporary:
            work = self.make_checkout(Path(temporary).resolve())
            child = """
import os, sys
from datetime import date
from pathlib import Path
from types import SimpleNamespace
import release_tide as tide

tide.ROOT = Path(sys.argv[1])
tide.gh = lambda *args: "[]"
tide.python = lambda *args, **kwargs: os._exit(27)
tide.Release(SimpleNamespace(version="0.2.0", date=date.today(), pin=[])).prepare()
"""
            result = subprocess.run([sys.executable, "-c", child, str(work)],
                                    cwd=Path(__file__).resolve().parent,
                                    env={**os.environ, "TMPDIR": temporary, "TEMP": temporary, "TMP": temporary})
            self.assertEqual(result.returncode, 27)
            self.assertEqual(run_git(work, "status", "--porcelain"), "")
            args = mock.Mock(version="0.2.0", date=date.today(), pin=[])
            with mock.patch.object(release_tide, "ROOT", work), \
                    mock.patch.object(release_tide, "gh", return_value="[]"), \
                    mock.patch.object(release_tide, "python"):
                release_tide.Release(args).prepare()
            self.assertEqual(run_git(work, "show", "-s", "--format=%s", "HEAD").strip(), "Release 0.2.0")


class LatestOrderTests(unittest.TestCase):
    """A stale draft must never be promoted over a newer public Latest."""

    RELEASES = [
        {"tagName": "v0.5.0", "isDraft": False, "isPrerelease": False, "isLatest": True},
        {"tagName": "v0.4.9", "isDraft": True, "isPrerelease": False, "isLatest": False},
        {"tagName": "v0.4.8", "isDraft": False, "isPrerelease": False, "isLatest": False},
        {"tagName": "v0.6.0", "isDraft": False, "isPrerelease": True, "isLatest": False},
    ]

    def release(self, version, view):
        args = mock.Mock(version=version)
        release = release_tide.Release(args)
        listing = json.dumps(self.RELEASES)
        calls = []

        def gh(*command):
            calls.append(command)
            if command[:2] == ("release", "list"):
                return listing
            if command[:2] == ("release", "edit"):
                return "edited"
            raise AssertionError(f"unexpected gh call {command}")

        patches = [
            mock.patch.object(release_tide, "gh", side_effect=gh),
            mock.patch.object(release, "verify_release_assets", return_value=(view, {"x": "y"})),
            mock.patch.object(release, "windows_dir", return_value=Path(self.temp)),
            mock.patch.object(release, "merge_sha", return_value="abc"),
            mock.patch.object(release_tide, "git", return_value="abc	refs/tags/v" + version),
        ]
        (Path(self.temp) / "verified-release-digests.json").write_text(json.dumps({"x": "y"}), "ascii")
        return release, patches, calls

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.temp = temporary.name

    def run_publish(self, version):
        release, patches, calls = self.release(version, {"isDraft": True})
        with patches[0], patches[1], patches[2], patches[3], patches[4]:
            release.publish()
        return [call for call in calls if call[:2] == ("release", "edit")]

    def test_older_draft_is_rejected_before_editing_the_release(self):
        release, patches, calls = self.release("0.4.9", {"isDraft": True})
        with patches[0], patches[1], patches[2], patches[3], patches[4]:
            with self.assertRaisesRegex(release_tide.ReleaseError, "older than published v0.5.0"):
                release.publish()
        self.assertFalse([call for call in calls if call[:2] == ("release", "edit")],
                         "The stale draft was edited before the Latest check")

    def test_newer_draft_is_promoted(self):
        self.assertEqual(len(self.run_publish("0.5.1")), 1)

    def test_prereleases_and_drafts_do_not_block_and_odd_tags_do(self):
        self.assertEqual(release_tide.newer_public_releases(self.RELEASES, "v0.5.1"), [])
        self.assertEqual(release_tide.newer_public_releases(self.RELEASES, "v0.4.9"), ["v0.5.0"])
        odd = [{"tagName": "nightly", "isDraft": False, "isPrerelease": False, "isLatest": True}]
        self.assertEqual(release_tide.newer_public_releases(odd, "v0.5.1"), ["nightly"])

    def test_verify_rejects_a_downgraded_latest(self):
        release, patches, _ = self.release("0.4.9", {"isDraft": False})
        with patches[0], patches[1], patches[2], patches[3], patches[4]:
            with self.assertRaisesRegex(release_tide.ReleaseError, "backward"):
                release.verify()


if __name__ == "__main__":
    unittest.main()
