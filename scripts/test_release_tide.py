"""Check that a failed release preparation leaves no tracked edits behind for retry."""
from datetime import date
from pathlib import Path
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


if __name__ == "__main__":
    unittest.main()
