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

    def test_missing_directory_is_not_created(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "not-built"
            result = self.run_ps("""
                . $env:TIDE_BUDGET_HELPER
                Assert-TideBuildBudget $env:TIDE_TEST_TARGET -AvailableBytes 10GB
                """, TIDE_TEST_TARGET=target)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(target.exists())

    def test_free_space_boundary_and_failure_preserve_files(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            data = target / "generated.bin"
            data.write_bytes(b"12345678")
            for available, success in [(5, True), (4, False)]:
                with self.subTest(available=available):
                    result = self.run_ps(
                        ". $env:TIDE_BUDGET_HELPER; Assert-TideBuildBudget $env:TIDE_TEST_TARGET "
                        f"-MinimumFreeBytes 5 -AvailableBytes {available}",
                        TIDE_TEST_TARGET=target)
                    self.assertEqual(result.returncode == 0, success, result.stderr)
                    self.assertEqual(data.read_bytes(), b"12345678")

    def wrapper(self, arguments, low_space=False):
        # A fake Cargo reports metadata and records whether compilation began.
        # The injected guard reports low free space only for the selected target.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            scripts = root / "scripts"
            scripts.mkdir()
            shutil.copyfile(SCRIPTS / "cargo-local.ps1", scripts / "cargo-local.ps1")
            shutil.copyfile(SCRIPTS / "build-budget.ps1", scripts / "real-budget.ps1")
            (scripts / "build-budget.ps1").write_text("""
. (Join-Path $PSScriptRoot 'real-budget.ps1')
function Assert-TideBuildBudget([string]$TargetDirectory) {
    if ($env:TIDE_TEST_LOW_SPACE -eq 'yes' -and (Split-Path -Leaf $TargetDirectory) -eq 'actual output') {
        throw 'Fixture low free space'
    }
}
""", encoding="utf-8")
            target = root / "actual output"
            target.mkdir()
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
                    }
                }
                Set-Location $env:TIDE_TEST_ROOT
                $requested = ConvertFrom-Json $env:TIDE_TEST_REQUEST
                & (Join-Path $env:TIDE_TEST_ROOT 'scripts/cargo-local.ps1') -CargoArgs $requested
                """, TIDE_TEST_ROOT=root, TIDE_TEST_MARKER=marker,
                TIDE_TEST_ARGS=root / "metadata-args.json",
                TIDE_METADATA_TARGET=target if not arguments else root / "unused",
                TIDE_TEST_LOW_SPACE="yes" if low_space else "no",
                TIDE_TEST_REQUEST=json.dumps(arguments or ["build"]))
            compiled = marker.read_text().strip() if marker.exists() else None
            inspection = (root / "metadata-args.json").read_text()
            return result, compiled, inspection

    def test_wrapper_blocks_before_compilation(self):
        result, compiled, _ = self.wrapper([], low_space=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Fixture low free space", result.stderr)
        self.assertIsNone(compiled)

    def test_wrapper_uses_explicit_target_and_workspace(self):
        for target_arg in (["--target-dir", "actual output"], ["--target-dir=actual output"]):
            result, compiled, inspection = self.wrapper(
                ["build", "--manifest-path", "other/Cargo.toml", "--config=build.jobs=1", *target_arg],
                low_space=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Fixture low free space", result.stderr)
            self.assertIsNone(compiled)
            self.assertIn("other/Cargo.toml", inspection)
            self.assertIn("--config=build.jobs=1", inspection)

    def test_wrapper_disables_incremental_for_allowed_build(self):
        result, compiled, _ = self.wrapper([])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(compiled, "0")


if __name__ == "__main__":
    unittest.main()
