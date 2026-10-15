"""Exercise the real CMD wrappers with a harmless fixture PowerShell entry."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


@unittest.skipUnless(os.name == "nt", "requires Windows CMD and Windows PowerShell")
class SetupLauncherTests(unittest.TestCase):
    def test_wrappers_repair_inherited_modules_and_return_child_failure(self):
        for name in ("Install Tide.cmd", "Uninstall Tide.cmd"):
            with self.subTest(name=name), tempfile.TemporaryDirectory(prefix="tide launcher ") as temporary:
                fixture = Path(temporary).resolve()
                launcher = fixture / name
                shutil.copyfile(ROOT / name, launcher)
                (fixture / "scripts").mkdir()
                (fixture / "scripts/install-windows.ps1").write_text(r'''
param([switch]$Interactive, [string]$Mode)
$ErrorActionPreference = 'Stop'
Get-FileHash -LiteralPath $PSCommandPath | Out-Null
if (!$Interactive -and $Mode -ne 'Uninstall') { throw 'Launcher arguments changed.' }
Write-Output 'PASS launcher module environment and arguments'
exit 7
''', encoding="utf-8")
                environment = os.environ.copy()
                environment["PSModulePath"] = str(fixture / "incompatible-pwsh-modules")
                result = subprocess.run(
                    f'cmd.exe /d /c ""{launcher}""', input="\n", text=True,
                    capture_output=True, timeout=25, env=environment,
                )
                self.assertEqual(result.returncode, 7, result.stderr)
                self.assertIn("PASS launcher module environment and arguments", result.stdout)


if __name__ == "__main__":
    unittest.main()
