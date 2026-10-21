"""Compile the Windows setup wrapper; verify payload gates without installing."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
COMPILER = Path(os.environ.get("SystemRoot", "C:/Windows")) / "Microsoft.NET/Framework64/v4.0.30319/csc.exe"


@unittest.skipUnless(os.name == "nt" and COMPILER.is_file(), "requires Windows .NET Framework compiler")
class InstallerBootstrapTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
