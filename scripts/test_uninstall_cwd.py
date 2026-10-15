"""Run the real entry script against an empty, disposable owned-directory fixture."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


@unittest.skipUnless(os.name == "nt", "requires Windows working-directory semantics")
class UninstallWorkingDirectoryTests(unittest.TestCase):
    def test_entry_leaves_legacy_working_directory_before_removing_owned_root(self):
        for shell in ("pwsh", "powershell.exe"):
            if not shutil.which(shell):
                continue
            with self.subTest(shell=shell), tempfile.TemporaryDirectory(prefix="tide-uninstall-cwd-") as temporary:
                fixture = Path(temporary).resolve()
                scripts = fixture / "package/scripts"
                scripts.mkdir(parents=True)
                entry = scripts / "install-windows.ps1"
                shutil.copyfile(ROOT / "scripts/install-windows.ps1", entry)
                (scripts / "install-common.ps1").write_text(r'''
function Uninstall-Tide($InstallRoot, $ShortcutRoot) {
    if ([Environment]::CurrentDirectory.TrimEnd('\') -eq $InstallRoot.TrimEnd('\') -or
        (Get-Location).Path.TrimEnd('\') -eq $InstallRoot.TrimEnd('\')) {
        throw 'Uninstall entry retained its owned working directory.'
    }
    # This stub removes only the empty directory created by this test. Real
    # package/registry ownership is covered by the other lifecycle probes.
    Remove-Item -LiteralPath $InstallRoot -Force -ErrorAction Stop
    Write-Output 'PASS legacy uninstall working directory'
}
''', encoding="utf-8")
                owned = fixture / "installed"
                owned.mkdir()
                self.assertEqual(owned.resolve().parent, fixture)
                environment = os.environ.copy()
                if shell == "powershell.exe":
                    environment["PSModulePath"] = str(Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/Modules")
                result = subprocess.run(
                    [shell, "-NoProfile", "-File", str(entry), "-Mode", "Uninstall", "-InstallRoot", str(owned)],
                    cwd=owned, env=environment, capture_output=True, timeout=25,
                )
                self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
                self.assertIn(b"PASS legacy uninstall working directory", result.stdout)
                self.assertFalse(owned.exists())


if __name__ == "__main__":
    unittest.main()
