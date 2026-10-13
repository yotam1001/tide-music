"""Temporary-file preparation and in-memory registry checks; no device registration."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("setup-browser-companion.ps1")


@unittest.skipUnless(os.name == "nt" and shutil.which("pwsh"), "requires Windows PowerShell")
class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="tide-companion-test-")
        self.root = Path(self.temporary.name).resolve()
        assert self.root.parent == Path(tempfile.gettempdir()).resolve()
        assert self.root.name.startswith("tide-companion-test-")
        self.host = self.root / "tide-session-host.exe"
        self.host.write_bytes(b"synthetic path fixture; never executed")
        self.output = self.root / "prepared"

    def tearDown(self):
        self.temporary.cleanup()

    def prepare(self, *extra):
        return subprocess.run(
            ["pwsh", "-NoProfile", "-File", str(SCRIPT), "-Mode", "Prepare",
             "-HostExecutable", str(self.host), "-Destination", str(self.output), *extra],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15, check=False,
        )

    def test_firefox_preparation_has_exact_identity_and_no_chromium_origins(self):
        self.assertEqual(self.prepare().returncode, 0)
        manifest = json.loads((self.output / "firefox-host.json").read_text())
        self.assertEqual(manifest["allowed_extensions"], ["tide-session@tide-music"])
        self.assertEqual(manifest["name"], "org.tidemusic.session")
        self.assertEqual(Path(manifest["path"]), self.host)
        self.assertEqual(manifest["type"], "stdio")
        self.assertEqual(json.loads((self.output / "tide-session-host-policy.json").read_text()), {"allowed_origins": []})
        self.assertFalse((self.output / "chromium-host.json").exists())
        self.assertFalse((self.host.parent / "tide-session-host-policy.json").exists())

    def test_chromium_manifest_and_policy_use_the_same_exact_origin(self):
        self.assertEqual(self.prepare("-ChromiumExtensionId", "a" * 32).returncode, 0)
        expected = ["chrome-extension://" + "a" * 32 + "/"]
        for name in ("chromium-host.json", "tide-session-host-policy.json"):
            raw = (self.output / name).read_bytes()
            self.assertFalse(raw.startswith(b"\xef\xbb\xbf"))
            self.assertEqual(json.loads(raw)["allowed_origins"], expected)

    def test_invalid_id_and_wrong_executable_are_rejected_before_output(self):
        for value in ("*", "a" * 31, "z" * 32, "a" * 32 + "/"):
            self.assertNotEqual(self.prepare("-ChromiumExtensionId", value).returncode, 0)
            self.assertFalse(self.output.exists())
        self.host = self.root / "other.exe"
        self.host.write_bytes(b"fixture")
        self.assertNotEqual(self.prepare().returncode, 0)
        self.assertFalse(self.output.exists())

    def test_existing_destination_and_files_are_preserved(self):
        self.output.mkdir()
        sentinel = self.output / "keep.txt"
        sentinel.write_bytes(b"existing output")
        self.assertNotEqual(self.prepare().returncode, 0)
        self.assertEqual(sentinel.read_bytes(), b"existing output")
        self.assertEqual(list(self.output.iterdir()), [sentinel])

    def run_mock_registration(self, scenario):
        self.assertEqual(self.prepare("-ChromiumExtensionId", "a" * 32).returncode, 0)
        harness = self.root / "mock-registration.ps1"
        harness.write_text(r'''
param($SetupScript, $Prepared, $FixtureRoot, $Scenario)
$ErrorActionPreference = 'Stop'
$global:FixtureRegistry = @{'HKCU:\Software'=$null}
$global:FixtureSetCount = 0
$global:FixtureRoot = $FixtureRoot
function Assert-FakeRegistry($Value) {
    if ($Value -notmatch '^HKCU:\\Software(?:\\(?:Mozilla|Google|Microsoft)(?:\\(?:Chrome|Edge))?(?:\\NativeMessagingHosts(?:\\org\.tidemusic\.session)?)?)?$') { throw 'Unexpected registry fixture operation' }
}
function Assert-FixtureFile($Value) {
    $absolute = [IO.Path]::GetFullPath($Value)
    if (!$absolute.StartsWith($global:FixtureRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unexpected file operation' }
}
function Test-Path {
    param($LiteralPath, $PathType)
    if ($LiteralPath.StartsWith('HKCU:', [StringComparison]::OrdinalIgnoreCase)) {
        Assert-FakeRegistry $LiteralPath
        return $global:FixtureRegistry.ContainsKey($LiteralPath)
    }
    Assert-FixtureFile $LiteralPath
    if ($PathType) { return Microsoft.PowerShell.Management\Test-Path -LiteralPath $LiteralPath -PathType $PathType }
    return Microsoft.PowerShell.Management\Test-Path -LiteralPath $LiteralPath
}
function Get-Item {
    param($LiteralPath)
    if ($LiteralPath.StartsWith('HKCU:', [StringComparison]::OrdinalIgnoreCase)) {
        Assert-FakeRegistry $LiteralPath
        $item = [pscustomobject]@{ Value=$global:FixtureRegistry[$LiteralPath] }
        $item | Add-Member -MemberType ScriptMethod -Name GetValue -Value { param($Name) if ($Name -ne '') { throw 'Unexpected value' }; return $this.Value }
        return $item
    }
    Assert-FixtureFile $LiteralPath
    return Microsoft.PowerShell.Management\Get-Item -LiteralPath $LiteralPath
}
function New-Item {
    param($Path)
    Assert-FakeRegistry $Path
    if ($global:FixtureRegistry.ContainsKey($Path)) { throw 'Registry key already exists' }
    if (!$global:FixtureRegistry.ContainsKey((Split-Path -Parent $Path))) { throw 'Registry parent missing' }
    $global:FixtureRegistry[$Path] = $null
}
function Set-Item {
    param($LiteralPath, $Value)
    Assert-FakeRegistry $LiteralPath
    $global:FixtureSetCount++
    if ($Scenario -in @('failure','preserve-existing') -and $global:FixtureSetCount -eq 2) { throw 'Synthetic registry write failure' }
    $global:FixtureRegistry[$LiteralPath] = $Value
}
function Remove-Item {
    param($LiteralPath, [switch]$Force)
    Assert-FakeRegistry $LiteralPath
    $global:FixtureRegistry.Remove($LiteralPath)
}
$firefoxKey = 'HKCU:\Software\Mozilla\NativeMessagingHosts\org.tidemusic.session'
$firefoxManifest = Join-Path $Prepared 'firefox-host.json'
if ($Scenario -eq 'preserve-existing') { $global:FixtureRegistry[$firefoxKey] = $firefoxManifest }
if ($Scenario -eq 'conflict') { $global:FixtureRegistry[$firefoxKey] = 'synthetic unrelated registration' }
if ($Scenario -eq 'chrome-only') { $global:FixtureRegistry[$firefoxKey] = 'synthetic unrelated registration' }
if ($Scenario -eq 'chrome-missing-manifest') { Microsoft.PowerShell.Management\Remove-Item -LiteralPath (Join-Path $Prepared 'chromium-host.json') }
$failed = $false
try {
    if ($Scenario -in @('chrome-only', 'chrome-missing-manifest')) { & $SetupScript -Mode Register -Destination $Prepared -Browser Chrome | Out-Null }
    else { & $SetupScript -Mode Register -Destination $Prepared | Out-Null }
} catch { $failed = $true }
switch ($Scenario) {
    'chrome-missing-manifest' {
        if (!$failed -or $global:FixtureSetCount -ne 0 -or $global:FixtureRegistry.Count -ne 1) { throw 'Missing Chrome identity touched registry' }
        if (Microsoft.PowerShell.Management\Test-Path -LiteralPath (Join-Path $FixtureRoot 'tide-session-host-policy.json')) { throw 'Missing Chrome identity wrote policy' }
    }
    'chrome-only' {
        $chromeKey = 'HKCU:\Software\Google\Chrome\NativeMessagingHosts\org.tidemusic.session'
        if ($failed -or $global:FixtureSetCount -ne 1 -or !$global:FixtureRegistry.ContainsKey($chromeKey) -or $global:FixtureRegistry[$firefoxKey] -ne 'synthetic unrelated registration') { throw 'Chrome registration touched another browser' }
        & $SetupScript -Mode Unregister -Destination $Prepared -Browser Chrome | Out-Null
        if ($global:FixtureRegistry.ContainsKey($chromeKey) -or $global:FixtureRegistry[$firefoxKey] -ne 'synthetic unrelated registration') { throw 'Chrome removal touched another browser' }
    }
    'success' {
        if ($failed -or @($global:FixtureRegistry.Keys | Where-Object { $_.EndsWith('\org.tidemusic.session') }).Count -ne 3) { throw 'Successful registration fixture failed' }
        & $SetupScript -Mode Register -Destination $Prepared | Out-Null
        if ($global:FixtureSetCount -ne 3) { throw 'Idempotent registration rewrote keys' }
        & $SetupScript -Mode Unregister -Destination $Prepared | Out-Null
        if (@($global:FixtureRegistry.Keys | Where-Object { $_.EndsWith('\org.tidemusic.session') }).Count -ne 0) { throw 'Matching registrations not removed' }
    }
    'failure' { if (!$failed -or @($global:FixtureRegistry.Keys | Where-Object { $_.EndsWith('\org.tidemusic.session') }).Count -ne 0) { throw 'Partial registration did not roll back' } }
    'preserve-existing' {
        if (!$failed -or @($global:FixtureRegistry.Keys | Where-Object { $_.EndsWith('\org.tidemusic.session') }).Count -ne 1 -or $global:FixtureRegistry[$firefoxKey] -ne $firefoxManifest) { throw 'Rollback removed existing registration' }
    }
    'conflict' {
        if (!$failed -or @($global:FixtureRegistry.Keys | Where-Object { $_.EndsWith('\org.tidemusic.session') }).Count -ne 1 -or $global:FixtureSetCount -ne 0) { throw 'Conflicting registration was changed' }
        if (Microsoft.PowerShell.Management\Test-Path -LiteralPath (Join-Path $FixtureRoot 'tide-session-host-policy.json')) { throw 'Conflict wrote policy' }
    }
    default { throw 'Unknown fixture scenario' }
}
'PASS synthetic registry only'
''', encoding="utf-8")
        result = subprocess.run(
            ["pwsh", "-NoProfile", "-File", str(harness), str(SCRIPT.resolve()),
             str(self.output), str(self.root), scenario],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15, check=False)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
        self.assertIn(b"PASS synthetic registry only", result.stdout)

    def test_mocked_registration_and_removal_are_idempotent(self):
        self.run_mock_registration("success")

    def test_mocked_registration_failure_rolls_back_created_keys(self):
        self.run_mock_registration("failure")

    def test_mocked_failure_preserves_existing_registration(self):
        self.run_mock_registration("preserve-existing")

    def test_mocked_conflict_changes_nothing(self):
        self.run_mock_registration("conflict")

    def test_chrome_only_preserves_a_different_firefox_registration(self):
        self.run_mock_registration("chrome-only")

    def test_chrome_without_a_manifest_changes_nothing(self):
        self.run_mock_registration("chrome-missing-manifest")


if __name__ == "__main__":
    unittest.main()
