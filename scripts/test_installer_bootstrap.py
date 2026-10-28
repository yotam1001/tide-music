"""Compile the Windows setup wrapper; verify payload gates without installing."""
import os
import hashlib
from pathlib import Path
import subprocess
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parent.parent
COMPILER = Path(os.environ.get("SystemRoot", "C:/Windows")) / "Microsoft.NET/Framework64/v4.0.30319/csc.exe"


@unittest.skipUnless(os.name == "nt" and COMPILER.is_file(), "requires Windows .NET Framework compiler")
class InstallerBootstrapTests(unittest.TestCase):
    def test_silent_executable_extracts_runs_and_cleans_up(self):
        # The script is a fixture: no app, dependency, shortcut or registry writes.
        from scripts.test_windows_install import PAYLOAD
        for exit_code in (0, 23):
            with self.subTest(exit_code=exit_code), tempfile.TemporaryDirectory(prefix="tide-cli-test-") as temporary:
                root = Path(temporary)
                archive = root / "payload.zip"
                script = (
                    "param($Mode, $AppVersion)\n"
                    "$ErrorActionPreference = 'Stop'\nGet-FileHash -LiteralPath $PSCommandPath | Out-Null\n"
                    "if ($Mode -ne 'Install' -or $AppVersion -ne '0.4.5') { exit 99 }\n"
                    f"exit {exit_code}\n"
                ).encode()
                checksums = []
                with zipfile.ZipFile(archive, "w") as package:
                    for name in PAYLOAD:
                        data = script if name == "scripts/install-windows.ps1" else b"fixture, never executed"
                        package.writestr(name, data)
                        checksums.append(hashlib.sha256(data).hexdigest() + "  " + name)
                    package.writestr("SHA256SUMS.txt", "\n".join(checksums) + "\n")
                digest = root / "digest.txt"
                digest.write_text(hashlib.sha256(archive.read_bytes()).hexdigest(), encoding="utf-8")
                version = root / "version.txt"
                version.write_text("0.4.5", encoding="utf-8")
                output = root / "setup.exe"
                result = subprocess.run([
                    str(COMPILER), "/nologo", "/target:winexe", "/warnaserror+", "/out:" + str(output),
                    "/reference:System.Windows.Forms.dll", "/reference:System.Drawing.dll",
                    "/reference:System.IO.Compression.dll", "/reference:System.IO.Compression.FileSystem.dll",
                    f"/resource:{archive},Tide.Payload", f"/resource:{digest},Tide.Payload.sha256",
                    f"/resource:{version},Tide.Version", str(ROOT / "installer/TideSetup.cs")
                ], capture_output=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stdout.decode(errors="replace"))
                environment = dict(os.environ, TEMP=str(root), TMP=str(root), PSModulePath=str(root / 'invalid-parent-modules'))
                for args, expected in ((["--silent"], exit_code), (["--verify-payload"], 0),
                                       (["--help"], 0), (["--bad"], 2), (["--silent", "--preview"], 2)):
                    result = subprocess.run([str(output), *args], env=environment, capture_output=True, timeout=15)
                    self.assertEqual(result.returncode, expected, result.stderr.decode(errors="replace"))
                    self.assertEqual(list(root.glob("TideSetup-*")), [], "Extracted payload was left behind")

    def test_payload_gate(self):
        with tempfile.TemporaryDirectory(prefix="tide-bootstrap-test-") as temporary:
            output = Path(temporary) / "payload-tests.exe"
            version = Path(temporary) / "version.txt"
            version.write_text("0.1.0-preview.1", encoding="utf-8")
            result = subprocess.run([
                str(COMPILER), "/nologo", "/target:exe", "/main:PayloadTests", "/warnaserror+",
                "/out:" + str(output), "/reference:System.Windows.Forms.dll", "/reference:System.Drawing.dll",
                "/reference:System.IO.Compression.dll", "/reference:System.IO.Compression.FileSystem.dll",
                "/resource:" + str(version) + ",Tide.Version",
                str(ROOT / "installer/TideSetup.cs"), str(ROOT / "installer/PayloadTests.cs")
            ], capture_output=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stdout.decode(errors="replace"))
            # Leave headroom beyond the isolated child's bounded startup deadline.
            result = subprocess.run([str(output)], capture_output=True, timeout=90)
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
            self.assertIn(b"PASS: payload", result.stdout)
            self.assertIn(b"PASS: silent setup", result.stdout)


if __name__ == "__main__":
    unittest.main()
