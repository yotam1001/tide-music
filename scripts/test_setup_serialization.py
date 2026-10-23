"""Use two real PowerShell processes; no real install or registry mutation."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

COMMON = Path(__file__).with_name("install-common.ps1").resolve()

HOLDER = r'''
param($Common, $Ready, $Release)
$ErrorActionPreference='Stop'
. $Common
Invoke-TideSetupOperation {
    [IO.File]::WriteAllText($Ready, 'ready')
    $deadline=[DateTime]::UtcNow.AddSeconds(25)
    while (!(Test-Path -LiteralPath $Release)) {
        if ([DateTime]::UtcNow -gt $deadline) { throw 'Test holder timed out' }
        Start-Sleep -Milliseconds 50
    }
}
'''

CONTENDER = r'''
param($Common, $Root)
$ErrorActionPreference='Stop'
. $Common
# Reproduce the old pre-lock phase seeing a partially committed update:
# ownership record still v1.2.3, registry already v1.2.4. All data is synthetic.
function Get-TideInstallState { [pscustomobject]@{Release=('a'*32); AppVersion='1.2.3'; Browser='None'; ExtensionId=''} }
function Get-TideVerifiedPayload { @('tide-music.exe') }
function Assert-TideStopped { }
$script:HalfCommitted = Get-TideUninstallRegistration (Join-Path $Root ('releases/'+('a'*32))) $Root '1.2.4'
function Get-TideUninstallEntry { $script:HalfCommitted }
try {
    Install-TideCore 'unused' $Root (Join-Path $Root 'links') 'None' '' '1.2.5'
    throw 'FAIL partial snapshot unexpectedly accepted'
} catch {
    if ($_.Exception.Message -notmatch 'fields: DisplayVersion') { throw }
}
if (Test-Path -LiteralPath $Root) { throw 'FAIL reproduction mutated an installation' }
'PASS old pre-lock phase reproduced misleading ownership conflict'
function Get-TideInstallState { throw 'FAIL read installation while another setup was active' }
function Get-TideUninstallEntry { throw 'FAIL read registry while another setup was active' }
function Get-TideVerifiedPayload { throw 'FAIL read package while another setup was active' }
foreach ($operation in @('Install','Uninstall')) {
    try {
        if ($operation -eq 'Install') { Install-Tide 'unused' 'unused' 'unused' 'None' '' }
        else { Uninstall-Tide 'unused' 'unused' }
        throw 'FAIL concurrent operation proceeded'
    } catch {
        if ($_.Exception.Message -ne 'Another Tide setup or uninstall is running. Wait for it to finish, then retry.') { throw }
    }
}
'PASS competing setup rejected before reading any snapshots'
'''

PROBE = r'''
param($Common)
$ErrorActionPreference='Stop'
. $Common
try { Invoke-TideSetupOperation { throw 'synthetic failure' } } catch {
    if ($_.Exception.Message -ne 'synthetic failure') { throw }
}
Invoke-TideSetupOperation { 'PASS mutex released after failure' }
$expected=@{TideProduct='owned'; DisplayVersion='1.2.3'; UninstallString='expected command'}
$actual=@{TideProduct='owned'; DisplayVersion='1.2.4'; UninstallString='private-value'; 'private-name'='private-value'}
$message=Get-TideUninstallConflictMessage $actual $expected
if ($message -notmatch 'DisplayVersion' -or $message -notmatch 'UninstallString' -or $message -notmatch 'AdditionalRegistryFields' -or $message -match 'private|1\.2\.|expected command') { throw 'Unsafe or incomplete diagnostics' }
if (Test-TideUninstallEntryMatch $actual $expected) { throw 'Conflict guard weakened' }
'PASS conflict diagnostics hide values and arbitrary registry names'
'''


@unittest.skipUnless(os.name == "nt", "requires Windows named mutexes")
class SetupSerializationTests(unittest.TestCase):
    def test_real_process_serialization_and_failure_release(self):
        import time
        for shell in ("powershell.exe", "pwsh"):
            if not shutil.which(shell):
                continue
            with self.subTest(shell=shell), tempfile.TemporaryDirectory(prefix="tide-setup-lock-") as directory:
                root = Path(directory).resolve()
                env = os.environ.copy()
                if shell == "powershell.exe":
                    env["PSModulePath"] = str(Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/Modules")
                for name, source in (("holder", HOLDER), ("contender", CONTENDER), ("probe", PROBE)):
                    (root / (name + ".ps1")).write_text(source, encoding="utf-8")
                def command(name, *args):
                    return [shell, "-NoProfile", "-File", str(root / (name + ".ps1")), str(COMMON), *map(str, args)]
                holder = subprocess.Popen(command("holder", root / "ready", root / "release"), env=env,
                                          stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                try:
                    deadline = time.monotonic() + 15
                    while not (root / "ready").exists():
                        if holder.poll() is not None:
                            self.fail("Isolated mutex holder failed: " + holder.stderr.read().decode(errors="replace"))
                        if time.monotonic() > deadline:
                            self.fail("Isolated mutex holder did not become ready")
                        time.sleep(0.05)
                    result = subprocess.run(command("contender", root / "uncreated-install"), env=env, capture_output=True, timeout=15)
                    self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
                    self.assertIn(b"PASS old pre-lock phase reproduced", result.stdout)
                    self.assertIn(b"PASS competing setup rejected", result.stdout)
                finally:
                    (root / "release").write_text("release", encoding="utf-8")
                    try:
                        stdout, stderr = holder.communicate(timeout=10)
                    except subprocess.TimeoutExpired:
                        holder.kill()
                        holder.communicate()
                        raise
                self.assertEqual(holder.returncode, 0, stderr.decode(errors="replace"))
                result = subprocess.run(command("probe"), env=env, capture_output=True, timeout=15)
                self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
                self.assertIn(b"PASS mutex released after failure", result.stdout)
                self.assertIn(b"PASS conflict diagnostics", result.stdout)


if __name__ == "__main__":
    unittest.main()
