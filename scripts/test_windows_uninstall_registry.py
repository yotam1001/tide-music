"""Verify registry adapters using an in-memory HKCU fixture; no device writes."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


HARNESS = r'''
param($Common)
$ErrorActionPreference = 'Stop'
. $Common
$global:FixtureRegistry = @{'HKCU:\Software'=@{}}
$global:FixtureKinds = @{}
$global:WrittenKinds = @{}
$global:FixtureKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\TideMusic'
function Check-Path($Path) {
    if ($Path -notmatch '^HKCU:\\Software(?:\\Microsoft(?:\\Windows(?:\\CurrentVersion(?:\\Uninstall(?:\\TideMusic)?)?)?)?)?$') { throw 'Registry adapter escaped allowed key hierarchy' }
}
function Test-Path { param($LiteralPath) Check-Path $LiteralPath; return $global:FixtureRegistry.ContainsKey($LiteralPath) }
function New-Item {
    param($Path)
    Check-Path $Path
    if ($global:FixtureRegistry.ContainsKey($Path) -or !$global:FixtureRegistry.ContainsKey((Split-Path -Parent $Path))) { throw 'Invalid fixture key creation' }
    $global:FixtureRegistry[$Path] = @{}
}
function Get-Item {
    param($LiteralPath)
    Check-Path $LiteralPath
    $key = [pscustomobject]@{ Values=$global:FixtureRegistry[$LiteralPath]; SubKeyCount=0 }
    $key | Add-Member ScriptMethod GetValueNames { return @($this.Values.Keys) }
    $key | Add-Member ScriptMethod GetValue { param($Name) return $this.Values[$Name] }
    return $key
}
function New-ItemProperty {
    param($LiteralPath, $Name, $Value, $PropertyType, [switch]$Force)
    Check-Path $LiteralPath
    if ($LiteralPath -ne $global:FixtureKey -or !$Force) { throw 'Unexpected value write' }
    $global:FixtureRegistry[$LiteralPath][$Name] = $Value
    $global:FixtureKinds[$Name] = $PropertyType
    $global:WrittenKinds[$Name] = $PropertyType
}
function Remove-ItemProperty {
    param($LiteralPath, $Name)
    Check-Path $LiteralPath
    if ($LiteralPath -ne $global:FixtureKey) { throw 'Removed a shared registry value' }
    $global:FixtureRegistry[$LiteralPath].Remove($Name)
    $global:FixtureKinds.Remove($Name)
}
function Remove-Item {
    param($LiteralPath, [switch]$Force)
    Check-Path $LiteralPath
    if ($LiteralPath -ne $global:FixtureKey -or !$Force) { throw 'Removed a shared registry parent' }
    $global:FixtureRegistry.Remove($LiteralPath)
}
$registration = @{
    TideProduct='org.tidemusic.windows.preview'; DisplayName='Tide'; DisplayVersion='1.2.3-preview'
    Publisher='Tide contributors'; DisplayIcon='synthetic-executable.exe'; InstallLocation='synthetic owned root'
    UninstallString='synthetic uninstall command'; NoModify=1; NoRepair=1
}
Set-TideUninstallRegistration $null $registration
if (!(Test-TideUninstallEntryMatch (Get-TideUninstallEntry) $registration)) { throw 'Registry adapter roundtrip failed' }
foreach ($name in $registration.Keys) {
    $expected = if ($name -in @('NoModify','NoRepair')) { 'DWord' } else { 'String' }
    if ($global:WrittenKinds[$name] -ne $expected) { throw 'Incorrect Windows uninstall registry type' }
}
$next = @{}; foreach ($name in $registration.Keys) { $next[$name] = $registration[$name] }
$next.DisplayVersion = '1.2.4-preview'
Set-TideUninstallRegistration $registration $next
Set-TideUninstallRegistration $next $null
if ($global:FixtureRegistry.ContainsKey($global:FixtureKey) -or !$global:FixtureRegistry.ContainsKey('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall')) { throw 'Leaf removal touched shared parents' }
'PASS Windows uninstall registry adapters; in-memory keys only'
'''


@unittest.skipUnless(os.name == "nt" and shutil.which("pwsh"), "requires Windows PowerShell")
class WindowsUninstallRegistryTests(unittest.TestCase):
    def test_in_memory_registry_adapters_and_value_types(self):
        with tempfile.TemporaryDirectory(prefix="tide-registry-test-") as temporary:
            fixture = Path(temporary).resolve()
            self.assertEqual(fixture.parent, Path(tempfile.gettempdir()).resolve())
            harness = fixture / "registry.ps1"
            harness.write_text(HARNESS, encoding="utf-8")
            common = Path(__file__).with_name("install-common.ps1").resolve()
            result = subprocess.run(["pwsh", "-NoProfile", "-File", str(harness), str(common)],
                                    capture_output=True, timeout=15, check=False)
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
            self.assertIn(b"in-memory keys only", result.stdout)


if __name__ == "__main__":
    unittest.main()
