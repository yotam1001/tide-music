"""Exercise install/update/removal in temp folders with no downloads/device changes."""
import hashlib
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parent.parent
COMMON = ROOT / "scripts/install-common.ps1"
PAYLOAD = [
    "tide-music.exe", "LICENSE", "THIRD_PARTY_NOTICES.md", "DEPENDENCIES.md", "START_HERE.md",
    "Install Tide.cmd", "Uninstall Tide.cmd", "docs/ACCOUNT_SETUP.md", "docs/INSTALLATION.md",
    "scripts/install-windows.ps1", "scripts/install-common.ps1", "scripts/setup-runtime.ps1",
    "scripts/setup-provider.ps1", "helpers/youtube_music.py", "helpers/requirements.txt",
    "tide-session-host.exe", "scripts/setup-browser-companion.ps1", "docs/BROWSER_COMPANION.md",
]

HARNESS = r'''
param($Common, $Fixture, $Scenario)
$ErrorActionPreference = 'Stop'
. $Common
$root = Join-Path $Fixture 'installed'
$links = Join-Path $Fixture 'start-menu'
$package = Join-Path $Fixture 'package'
$session = Join-Path $Fixture 'saved-session-sentinel'
[IO.File]::WriteAllText($session, 'synthetic unchanged data')
$global:Busy = $false
$global:FailDependency = $false
$global:FailShortcut = $false
$global:FailRegistration = $false
$global:Registry = @{}
function Get-TideOwnedProcesses { param($InstallRoot) if ($global:Busy) { return @('synthetic owned process') }; return @() }
function Get-TideShortcut {
    param($Path)
    if (Test-Path -LiteralPath $Path) { return Read-TideJson $Path }
    return $null
}
function Set-TideShortcut {
    param($Path, $Value)
    if ($global:FailShortcut) { $global:FailShortcut = $false; throw 'Synthetic shortcut failure' }
    Write-TideJson $Path $Value
}
function Invoke-TideDependencies {
    param($Release)
    $tools = Join-Path $Release 'tools'
    [IO.Directory]::CreateDirectory($tools) | Out-Null
    [IO.File]::WriteAllText((Join-Path $tools 'mpv.exe'), 'synthetic tool, never executed')
    if ($global:FailDependency) { throw 'Synthetic dependency failure' }
    $python = Join-Path $Release '.tools/provider-venv/Scripts'
    [IO.Directory]::CreateDirectory($python) | Out-Null
    [IO.File]::WriteAllText((Join-Path $python 'python.exe'), 'synthetic interpreter, never executed')
}
function Invoke-TideBrowserSetup {
    param($Release, $Mode, $Browser, $ExtensionId)
    if ($Browser -eq 'None') { return }
    switch ($Mode) {
        'Prepare' {
            $folder = Join-Path $Release 'native-host'
            [IO.Directory]::CreateDirectory($folder) | Out-Null
            Write-TideJson (Join-Path $folder 'fixture-host.json') @{Path=$Release; Id=$ExtensionId}
        }
        'Register' {
            if ($global:FailRegistration) { $global:FailRegistration = $false; throw 'Synthetic registration failure' }
            if ($global:Registry.ContainsKey($Browser) -and $global:Registry[$Browser] -ne $Release) { throw 'Conflicting synthetic registration' }
            $global:Registry[$Browser] = $Release
        }
        'Unregister' {
            if ($global:Registry.ContainsKey($Browser) -and $global:Registry[$Browser] -ne $Release) { throw 'Changed synthetic registration' }
            $global:Registry.Remove($Browser)
        }
    }
}
function Must-Fail([scriptblock]$Action) {
    $failed = $false
    try { & $Action | Out-Null } catch { $failed = $true }
    if (!$failed) { throw 'Expected fixture failure was not observed' }
}
function New-Install { Install-Tide $package $root $links 'Chrome' ('a' * 32) | Out-Null }
function Current-Release { $state = Get-TideInstallState $root; return Join-Path $root ('releases/' + $state.Release) }
switch ($Scenario) {
    'fresh-remove' {
        New-Install
        $release = Current-Release
        if (!(Test-Path -LiteralPath (Join-Path $links 'Tide.lnk')) -or $global:Registry['Chrome'] -ne $release) { throw 'Missing registration/shortcut' }
        Uninstall-Tide $root $links | Out-Null
        if ((Test-Path -LiteralPath $root) -or (Test-Path -LiteralPath $links) -or $global:Registry.Count) { throw 'Owned removal incomplete' }
        Uninstall-Tide $root $links | Out-Null
    }
    'upgrade' {
        New-Install; $previous = Current-Release
        Install-Tide $package $root $links 'None' '' | Out-Null
        $current = Current-Release
        if ($current -eq $previous -or (Test-Path -LiteralPath $previous) -or $global:Registry['Chrome'] -ne $current) { throw 'Upgrade failed to replace owned release' }
        Uninstall-Tide $root $links | Out-Null
    }
    'preserve-extra' {
        New-Install; $previous = Current-Release
        [IO.File]::WriteAllText((Join-Path $previous 'user-extra.txt'), 'keep')
        Install-Tide $package $root $links 'None' '' | Out-Null
        Uninstall-Tide $root $links | Out-Null
        if ([IO.File]::ReadAllText((Join-Path $previous 'user-extra.txt')) -ne 'keep') { throw 'Removed unowned file' }
    }
    'dependency-failure-first' {
        $global:FailDependency = $true; Must-Fail { New-Install }
        if ((Test-Path -LiteralPath $root) -or (Test-Path -LiteralPath $links) -or $global:Registry.Count) { throw 'Failed fresh install left owned output' }
    }
    { $_ -in @('dependency-failure-update','shortcut-failure','registration-failure') } {
        New-Install; $previous = Current-Release
        $before = [IO.File]::ReadAllText((Join-Path $links 'Tide.lnk'))
        if ($Scenario -eq 'dependency-failure-update') { $global:FailDependency = $true }
        if ($Scenario -eq 'shortcut-failure') { $global:FailShortcut = $true }
        if ($Scenario -eq 'registration-failure') { $global:FailRegistration = $true }
        Must-Fail { New-Install }
        if ((Current-Release) -ne $previous -or $global:Registry['Chrome'] -ne $previous -or [IO.File]::ReadAllText((Join-Path $links 'Tide.lnk')) -ne $before) { throw 'Failed update did not restore previous install' }
        if (@(Get-ChildItem -LiteralPath (Join-Path $root 'releases')).Count -ne 1) { throw 'Failed release was retained' }
        Uninstall-Tide $root $links | Out-Null
    }
    'busy' {
        New-Install; $previous = Current-Release; $global:Busy = $true
        Must-Fail { New-Install }; Must-Fail { Uninstall-Tide $root $links }
        if ((Current-Release) -ne $previous) { throw 'Busy install changed' }
        $global:Busy = $false; Uninstall-Tide $root $links | Out-Null
    }
    'unknown-root' {
        [IO.Directory]::CreateDirectory($root) | Out-Null
        $sentinel = Join-Path $root 'unrelated.txt'; [IO.File]::WriteAllText($sentinel, 'keep')
        Must-Fail { New-Install }; Must-Fail { Uninstall-Tide $root $links }
        if ([IO.File]::ReadAllText($sentinel) -ne 'keep') { throw 'Unrelated install changed' }
    }
    'shortcut-conflict' {
        [IO.Directory]::CreateDirectory($links) | Out-Null
        Write-TideJson (Join-Path $links 'Tide.lnk') @{Target='different.exe'; Arguments=''}
        Must-Fail { New-Install }
        if ((Test-Path -LiteralPath $root) -or (Get-TideShortcut (Join-Path $links 'Tide.lnk')).Target -ne 'different.exe') { throw 'Conflicting shortcut changed' }
    }
    'registration-conflict' {
        $global:Registry['Chrome'] = 'unrelated installed helper'; Must-Fail { New-Install }
        if ($global:Registry['Chrome'] -ne 'unrelated installed helper' -or (Test-Path -LiteralPath $root)) { throw 'Conflicting registration changed' }
    }
    'changed-shortcut-removal' {
        New-Install
        Write-TideJson (Join-Path $links 'Tide.lnk') @{Target='different.exe'; Arguments=''}
        Uninstall-Tide $root $links | Out-Null
        if ((Get-TideShortcut (Join-Path $links 'Tide.lnk')).Target -ne 'different.exe') { throw 'Changed shortcut removed' }
    }
    'changed-registration-removal' {
        New-Install; $previous = Current-Release
        $global:Registry['Chrome'] = 'different helper'; Must-Fail { Uninstall-Tide $root $links }
        if ((Current-Release) -ne $previous -or $global:Registry['Chrome'] -ne 'different helper') { throw 'Changed registration removed' }
    }
    'corrupt-package' {
        [IO.File]::WriteAllText((Join-Path $package 'tide-music.exe'), 'corrupted')
        Must-Fail { New-Install }
        if (Test-Path -LiteralPath $root) { throw 'Corrupt package wrote installation' }
    }
    'traversal-record' {
        New-Install; $release = Current-Release
        Write-TideJson (Join-Path $release '.tide-release.json') @{Product='org.tidemusic.windows.preview'; Files=@('../../../../saved-session-sentinel')}
        Must-Fail { Uninstall-Tide $root $links }
        if (!(Test-Path -LiteralPath (Join-Path $release 'tide-music.exe'))) { throw 'Invalid ownership deleted files' }
    }
    'reparse' {
        $outside = Join-Path $Fixture 'outside'; [IO.Directory]::CreateDirectory($outside) | Out-Null
        New-Item -ItemType Junction -Path $root -Target $outside | Out-Null
        Must-Fail { New-Install }; Must-Fail { Uninstall-Tide $root $links }
        if (@(Get-ChildItem -LiteralPath $outside).Count) { throw 'Reparse target changed' }
        Remove-Item -LiteralPath $root
    }
    'lock' {
        New-Install; $previous = Current-Release
        $stream = [IO.File]::Open((Join-Path $root '.tide-install.lock'), 'OpenOrCreate', 'ReadWrite', 'None')
        try { Must-Fail { New-Install }; Must-Fail { Uninstall-Tide $root $links } } finally { $stream.Dispose() }
        if ((Current-Release) -ne $previous) { throw 'Locked install changed' }
        Uninstall-Tide $root $links | Out-Null
    }
    'download' {
        $global:Downloads = 0
        function Invoke-WebRequest { param([switch]$UseBasicParsing, $Uri, $OutFile) $global:Downloads++; [IO.File]::WriteAllText($OutFile, 'fixture') }
        Must-Fail { Get-TideDownload 'https://example.invalid/fixture' '' (Join-Path $Fixture 'missing') }
        if ($global:Downloads) { throw 'Missing digest started download' }
        Must-Fail { Get-TideDownload 'https://example.invalid/fixture' ('0' * 64) (Join-Path $Fixture 'bad') }
        if ($global:Downloads -ne 1) { throw 'Download checksum fixture did not run' }
    }
    'archive-paths' {
        foreach ($name in @('traversal','rooted','duplicate')) {
            Must-Fail { Expand-TideArchive (Join-Path $Fixture ($name + '.zip')) (Join-Path $Fixture $name) }
            if (Test-Path -LiteralPath (Join-Path $Fixture $name)) { throw 'Invalid archive wrote files' }
        }
    }
    'runtime-missing-digest' {
        function Invoke-RestMethod { param($Uri) return @{ assets=@(@{name='mpv-v0.41.0-x86_64-pc-windows-msvc.zip'; browser_download_url='https://example.invalid/mpv.zip'}) } }
        function Invoke-WebRequest { throw 'Network download must not occur without an upstream digest' }
        Must-Fail { & (Join-Path $Fixture 'source/scripts/setup-runtime.ps1') -Destination (Join-Path $Fixture 'tools') }
        if (Test-Path -LiteralPath (Join-Path $Fixture 'tools')) { throw 'Missing digest wrote runtime tools' }
    }
    'runtime-changed-digest' {
        function Invoke-RestMethod { param($Uri) return @{ assets=@(@{name='mpv-v0.41.0-x86_64-pc-windows-msvc.zip'; digest=('sha256:' + ('0'*64)); browser_download_url='https://example.invalid/mpv.zip'}) } }
        function Invoke-WebRequest { throw 'Network download must not occur for a changed upstream digest' }
        Must-Fail { & (Join-Path $Fixture 'source/scripts/setup-runtime.ps1') -Destination (Join-Path $Fixture 'tools') }
        if (Test-Path -LiteralPath (Join-Path $Fixture 'tools')) { throw 'Changed digest wrote runtime tools' }
    }
    default { throw 'Unknown scenario' }
}
if ([IO.File]::ReadAllText($session) -ne 'synthetic unchanged data') { throw 'Saved session fixture changed' }
'PASS isolated installer lifecycle'
'''


@unittest.skipUnless(os.name == "nt" and shutil.which("pwsh"), "requires Windows PowerShell")
class WindowsInstallTests(unittest.TestCase):
    def run_scenario(self, scenario, shell="pwsh"):
        with tempfile.TemporaryDirectory(prefix="tide-install-test-") as temporary:
            fixture = Path(temporary).resolve()
            self.assertEqual(fixture.parent, Path(tempfile.gettempdir()).resolve())
            package = fixture / "package"
            hashes = []
            for name in PAYLOAD:
                path = package / name
                path.parent.mkdir(parents=True, exist_ok=True)
                data = b"public synthetic payload, never executed\n"
                path.write_bytes(data)
                hashes.append(hashlib.sha256(data).hexdigest() + "  " + name)
            (package / "SHA256SUMS.txt").write_text("\n".join(hashes), encoding="utf-8")
            harness = fixture / "lifecycle.ps1"
            harness.write_text(HARNESS, encoding="utf-8")
            if scenario.startswith("runtime-"):
                scripts = fixture / "source/scripts"
                scripts.mkdir(parents=True)
                for name in ("setup-runtime.ps1", "install-common.ps1"):
                    shutil.copyfile(ROOT / "scripts" / name, scripts / name)
            if scenario == "archive-paths":
                for name, entries in {"traversal": ["../outside.txt"], "rooted": ["/outside.txt"],
                                      "duplicate": ["same.txt", "SAME.txt"]}.items():
                    with zipfile.ZipFile(fixture / (name + ".zip"), "w") as archive:
                        for entry in entries:
                            archive.writestr(entry, b"public fixture")
            environment = os.environ.copy()
            if shell == "powershell.exe":
                # The tool runner is PowerShell 7 and exports its own module
                # path. Give the built-in 5.1 subprocess its normal OS modules.
                environment["PSModulePath"] = str(Path(os.environ["SystemRoot"]) /
                    "System32/WindowsPowerShell/v1.0/Modules")
            result = subprocess.run(
                [shell, "-NoProfile", "-File", str(harness), str(COMMON), str(fixture), scenario],
                capture_output=True, timeout=30, check=False, env=environment)
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
            self.assertIn(b"PASS isolated installer lifecycle", result.stdout)

    def test_isolated_lifecycle(self):
        for scenario in ("fresh-remove", "upgrade", "preserve-extra", "dependency-failure-first",
                         "dependency-failure-update", "shortcut-failure", "registration-failure", "busy",
                         "unknown-root", "shortcut-conflict", "registration-conflict",
                         "changed-shortcut-removal", "changed-registration-removal", "corrupt-package",
                         "traversal-record", "reparse", "lock", "download", "archive-paths",
                         "runtime-missing-digest", "runtime-changed-digest"):
            with self.subTest(scenario=scenario):
                self.run_scenario(scenario)

    def test_actual_package_integrates_installer_allowlist(self):
        with tempfile.TemporaryDirectory(prefix="tide-package-test-") as temporary:
            source = Path(temporary).resolve()
            for name in PAYLOAD:
                path = source / name
                path.parent.mkdir(parents=True, exist_ok=True)
                original = ROOT / name
                if original.is_file():
                    shutil.copyfile(original, path)
                else:
                    path.write_bytes(b"public fixture executable or notice, never executed\n")
            shutil.copyfile(ROOT / "scripts/package-preview.ps1", source / "scripts/package-preview.ps1")
            (source / "browser-companion").mkdir()
            shutil.copyfile(ROOT / "browser-companion/README.md", source / "browser-companion/README.md")
            result = subprocess.run(
                ["pwsh", "-NoProfile", "-File", str(source / "scripts/package-preview.ps1"),
                 "-DependenciesNotice", "DEPENDENCIES.md", "-StartHere", "START_HERE.md",
                 "-Executable", "tide-music.exe", "-BrowserHostExecutable", "tide-session-host.exe",
                 "-Destination", "dist/fixture-package"], capture_output=True, timeout=30, check=False)
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
            spec = importlib.util.spec_from_file_location("package_gate", ROOT / "scripts/check-preview-package.py")
            gate = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(gate)
            gate.check(source / "dist/fixture-package.zip", with_browser_host=True)
            self.assertEqual(set(PAYLOAD), gate.PAYLOAD | gate.BROWSER_HOST_PAYLOAD)

    @unittest.skipUnless(shutil.which("powershell.exe"), "requires built-in Windows PowerShell 5.1")
    def test_builtin_windows_powershell_lifecycle(self):
        for scenario in ("fresh-remove", "upgrade", "registration-failure", "runtime-missing-digest"):
            with self.subTest(scenario=scenario):
                self.run_scenario(scenario, "powershell.exe")

    def test_real_shortcut_roundtrip_in_temp_only(self):
        with tempfile.TemporaryDirectory(prefix="tide-shortcut-test-") as temporary:
            fixture = Path(temporary).resolve()
            self.assertEqual(fixture.parent, Path(tempfile.gettempdir()).resolve())
            harness = fixture / "shortcut.ps1"
            harness.write_text(r'''
param($Common, $Fixture)
$ErrorActionPreference = 'Stop'
. $Common
$target = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'
$expected = @{Target=$target; Arguments='-NoProfile'; WorkingDirectory=$Fixture; Icon=$target}
$path = Join-Path $Fixture 'Tide.lnk'
Set-TideShortcut $path $expected
if (!(Test-TideShortcutMatch (Get-TideShortcut $path) $expected)) { throw 'Real shortcut roundtrip failed' }
Remove-Item -LiteralPath $path
'PASS temp shortcut roundtrip; no executable was launched'
''', encoding="utf-8")
            result = subprocess.run(["pwsh", "-NoProfile", "-File", str(harness), str(COMMON), str(fixture)],
                                    capture_output=True, timeout=15, check=False)
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
            self.assertIn(b"PASS temp shortcut roundtrip", result.stdout)


if __name__ == "__main__":
    unittest.main()
