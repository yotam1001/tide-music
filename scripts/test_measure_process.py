"""Windows process-instance checks using only owned temporary fixture processes."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("measure-process.ps1")
PWSH = shutil.which("pwsh")


@unittest.skipUnless(os.name == "nt" and PWSH, "requires Windows PowerShell")
class MeasurementTests(unittest.TestCase):
    def run_fixture(self, parent_source, child_seconds=2):
        with tempfile.TemporaryDirectory(prefix="tide-measure-test-") as temporary:
            root = Path(temporary).resolve()
            # Also exercise argument quoting for paths containing spaces.
            directory = root / "fixture with spaces"
            directory.mkdir()
            child = directory / "child.ps1"
            child.write_text(f"Start-Sleep -Seconds {child_seconds}\nexit 0\n")
            parent = directory / "parent.ps1"
            parent.write_text(parent_source)
            runner = root / "runner.ps1"
            runner.write_text(
                '& $env:TIDE_TEST_MEASURE -Executable $env:TIDE_TEST_PWSH '
                '-AppArguments @("-NoProfile","-File",$env:TIDE_TEST_PARENT) '
                '-Seconds 15 -Output $env:TIDE_TEST_OUTPUT -RequireSuccessfulExit\n'
            )
            output = root / "samples.json"
            identity = root / "child-identity.json"
            env = dict(os.environ, TIDE_TEST_PWSH=PWSH,
                       TIDE_TEST_CHILD=str(child), TIDE_TEST_PARENT=str(parent),
                       TIDE_TEST_MEASURE=str(SCRIPT.resolve()),
                       TIDE_TEST_OUTPUT=str(output), TIDE_TEST_IDENTITY=str(identity))
            try:
                result = subprocess.run([PWSH, "-NoProfile", "-File", str(runner)],
                                        env=env, capture_output=True, timeout=30)
                report = json.loads(Path(str(output) + ".exit.json").read_text(encoding="utf-8-sig"))
                failure_path = Path(str(output) + ".failure.json")
                failure = json.loads(failure_path.read_text(encoding="utf-8-sig")) if failure_path.exists() else None
                return result, report, failure
            finally:
                if identity.exists():
                    # Stop only this fixture's retained PID/start-time identity.
                    cleanup = root / "cleanup.ps1"
                    cleanup.write_text(
                        '$i=Get-Content -LiteralPath $env:TIDE_TEST_IDENTITY -Raw | ConvertFrom-Json\n'
                        '$p=Get-Process -Id $i.pid -ErrorAction SilentlyContinue\n'
                        'if($p -and $p.StartTime.Ticks -eq $i.ticks) { '
                        '$null=$p.Handle; if(!$p.HasExited) { $p.Kill(); $p.WaitForExit() }; $p.Dispose() }\n'
                    )
                    subprocess.run([PWSH, "-NoProfile", "-File", str(cleanup)],
                                   env=env, capture_output=True, timeout=10, check=True)

    def test_completed_children_and_spaced_argument_path_pass(self):
        result, report, failure = self.run_fixture(
            '$ErrorActionPreference="Stop"\n'
            'for($i=0;$i -lt 2;$i++) { '
            '$p=Start-Process -FilePath $env:TIDE_TEST_PWSH '
            '-ArgumentList @("-NoProfile","-File",("`""+$env:TIDE_TEST_CHILD+"`"")) '
            '-WindowStyle Hidden -PassThru; $p.WaitForExit(); $p.Dispose() }\nexit 0\n'
        )
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
        self.assertEqual(report["exit_code"], 0)
        self.assertTrue(report["helpers_ended"])
        self.assertTrue(report["cleanup_observations"])
        self.assertIsNone(failure)

    def test_surviving_helper_is_failure_and_records_stage(self):
        result, report, failure = self.run_fixture(
            '$ErrorActionPreference="Stop"\n'
            '$p=Start-Process -FilePath $env:TIDE_TEST_PWSH '
            '-ArgumentList @("-NoProfile","-File",("`""+$env:TIDE_TEST_CHILD+"`"")) '
            '-WindowStyle Hidden -PassThru\n'
            '@{pid=$p.Id;ticks=$p.StartTime.Ticks} | ConvertTo-Json | '
            'Set-Content -LiteralPath $env:TIDE_TEST_IDENTITY\n'
            'Start-Sleep -Seconds 3\nexit 0\n', child_seconds=20
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(report["exit_code"], 0)
        self.assertFalse(report["helpers_ended"])
        self.assertEqual(failure["stage"], "checking_helpers")


if __name__ == "__main__":
    unittest.main()
