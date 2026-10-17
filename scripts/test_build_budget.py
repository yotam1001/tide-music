"""Exercise disk guards without compiling Rust or creating large files."""
import os
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parent
SHELL = shutil.which("pwsh") or shutil.which("powershell")


@unittest.skipUnless(os.name == "nt" and SHELL, "Windows PowerShell guard")
class BuildBudgetTests(unittest.TestCase):
    def run_ps(self, script, **environment):
        env = dict(os.environ, TIDE_BUDGET_HELPER=str(SCRIPTS / "build-budget.ps1"))
        env.update({key: str(value) for key, value in environment.items()})
        return subprocess.run(
            [SHELL, "-NoProfile", "-NonInteractive", "-Command",
             "$ErrorActionPreference='Stop'; " + script],
            env=env, text=True, capture_output=True, timeout=30)

    def test_nested_outputs_and_exact_boundary(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            (target / "nested").mkdir()
            (target / "one.bin").write_bytes(b"123")
            (target / "nested" / "two.bin").write_bytes(b"4567")
            result = self.run_ps("""
                . $env:TIDE_BUDGET_HELPER
                if ((Get-TideBuildOutputBytes $env:TIDE_TEST_TARGET) -ne 7) { throw 'Wrong size' }
                Assert-TideBuildBudget $env:TIDE_TEST_TARGET -MaximumBytes 7 -MinimumFreeBytes 5 -AvailableBytes 5
                """, TIDE_TEST_TARGET=target)
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_directory_is_not_created(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "not-built"
            result = self.run_ps("""
                . $env:TIDE_BUDGET_HELPER
                Assert-TideBuildBudget $env:TIDE_TEST_TARGET -AvailableBytes 10GB
                """, TIDE_TEST_TARGET=target)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(target.exists())

    def test_budget_failure_preserves_files(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            data = target / "generated.bin"
            data.write_bytes(b"12345678")
            for options, expected in [
                ("-MaximumBytes 7 -AvailableBytes 10GB", "exceeds"),
                ("-MinimumFreeBytes 6 -AvailableBytes 5", "free"),
            ]:
                with self.subTest(options=options):
                    result = self.run_ps(
                        ". $env:TIDE_BUDGET_HELPER; Assert-TideBuildBudget $env:TIDE_TEST_TARGET " + options,
                        TIDE_TEST_TARGET=target)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(expected, result.stderr)
                    self.assertEqual(data.read_bytes(), b"12345678")

    def test_linked_directory_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            outside = root / "outside"
            outside.mkdir()
            (outside / "keep.bin").write_bytes(b"keep")
            target = root / "target"
            target.mkdir()
            result = self.run_ps("""
                . $env:TIDE_BUDGET_HELPER
                New-Item -ItemType Junction -Path (Join-Path $env:TIDE_TEST_TARGET 'link') -Target $env:TIDE_TEST_OUTSIDE | Out-Null
                Assert-TideBuildBudget $env:TIDE_TEST_TARGET -AvailableBytes 10GB
                """, TIDE_TEST_TARGET=target, TIDE_TEST_OUTSIDE=outside)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("linked entry", result.stderr)
            self.assertEqual((outside / "keep.bin").read_bytes(), b"keep")

    def wrapper(self, arguments, oversized=False, grow=False):
        # A fake Cargo reports metadata and records whether compilation began.
        # Its files are bytes, not GB; an injected guard uses a seven-byte limit.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            scripts = root / "scripts"
            scripts.mkdir()
            shutil.copyfile(SCRIPTS / "cargo-local.ps1", scripts / "cargo-local.ps1")
            shutil.copyfile(SCRIPTS / "build-budget.ps1", scripts / "real-budget.ps1")
            (scripts / "build-budget.ps1").write_text("""
. (Join-Path $PSScriptRoot 'real-budget.ps1')
function Assert-TideBuildBudget([string]$TargetDirectory) {
    $bytes = Get-TideBuildOutputBytes $TargetDirectory
    if ($bytes -gt 7) { throw 'Fixture budget exceeded' }
}
""", encoding="utf-8")
            target = root / "actual output"
            target.mkdir()
            if oversized:
                (target / "generated.bin").write_bytes(b"12345678")
            marker = root / "compilation-started"
            result = self.run_ps("""
                function global:cargo {
                    param([Parameter(ValueFromRemainingArguments=$true)][string[]]$Arguments)
                    $global:LASTEXITCODE = 0
                    if ($Arguments[0] -eq 'metadata') {
                        $Arguments | ConvertTo-Json | Set-Content $env:TIDE_TEST_ARGS
                        @{target_directory=$env:TIDE_METADATA_TARGET} | ConvertTo-Json -Compress
                    } else {
                        Set-Content $env:TIDE_TEST_MARKER $env:CARGO_INCREMENTAL
                        if ($env:TIDE_TEST_GROW -eq 'yes') {
                            [IO.File]::WriteAllBytes((Join-Path $env:TIDE_METADATA_TARGET 'new.bin'), [byte[]](1..8))
                        }
                    }
                }
                Set-Location $env:TIDE_TEST_ROOT
                $requested = @(ConvertFrom-Json $env:TIDE_TEST_REQUEST)
                & (Join-Path $env:TIDE_TEST_ROOT 'scripts/cargo-local.ps1') -CargoArgs $requested
                """, TIDE_TEST_ROOT=root, TIDE_TEST_MARKER=marker,
                TIDE_TEST_ARGS=root / "metadata-args.json",
                TIDE_METADATA_TARGET=target if not arguments else root / "unused",
                TIDE_TEST_GROW="yes" if grow else "no",
                TIDE_TEST_REQUEST=json.dumps(arguments or ["build"]))
            compiled = marker.read_text().strip() if marker.exists() else None
            inspection = (root / "metadata-args.json").read_text()
            return result, compiled, inspection

    def test_wrapper_blocks_before_compilation(self):
        result, compiled, _ = self.wrapper([], oversized=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Fixture budget exceeded", result.stderr)
        self.assertIsNone(compiled)

    def test_wrapper_uses_explicit_target_and_workspace(self):
        for target_arg in (["--target-dir", "actual output"], ["--target-dir=actual output"]):
            result, compiled, inspection = self.wrapper(
                ["build", "--manifest-path", "other/Cargo.toml", "--config=build.jobs=1", *target_arg],
                oversized=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Fixture budget exceeded", result.stderr)
            self.assertIsNone(compiled)
            self.assertIn("other/Cargo.toml", inspection)
            self.assertIn("--config=build.jobs=1", inspection)

    def test_wrapper_disables_incremental_for_allowed_build(self):
        result, compiled, _ = self.wrapper([])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(compiled, "0")

    def test_wrapper_reports_output_that_grows_during_build(self):
        result, compiled, _ = self.wrapper([], grow=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Fixture budget exceeded", result.stderr)
        self.assertEqual(compiled, "0")


if __name__ == "__main__":
    unittest.main()
