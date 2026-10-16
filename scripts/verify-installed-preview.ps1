#requires -Version 5.1
# Opt-in integration probe: real downloads and shortcuts, no real registry changes.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$PackageRoot)
$ErrorActionPreference = 'Stop'
$taskWorkspace = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$taskPackage = [IO.Path]::GetFullPath($PackageRoot)
. (Join-Path $taskPackage 'scripts/install-common.ps1')
$taskFixture = Join-Path $taskWorkspace ('.local/verify-install-' + [Guid]::NewGuid().ToString('N'))
Assert-TidePlainPath $taskFixture
[IO.Directory]::CreateDirectory($taskFixture) | Out-Null
$taskRoot = Join-Path $taskFixture 'installed'
$taskLinks = Join-Path $taskFixture 'shortcuts'
# Override only uninstall registry I/O. Real core, payload copies, .lnk creation,
# dependency downloads, hash verification and removal all execute normally.
$script:FixtureEntry = $null
function Get-TideUninstallEntry {
    if ($null -eq $script:FixtureEntry) { return $null }
    $copy = @{}; foreach ($name in $script:FixtureEntry.Keys) { $copy[$name] = $script:FixtureEntry[$name] }; return $copy
}
function New-TideUninstallEntry {
    if ($null -ne $script:FixtureEntry) { throw 'Fixture entry already exists.' }
    $script:FixtureEntry = @{}
}
function Write-TideUninstallValue([string]$Name, $Value) { $script:FixtureEntry[$Name] = $Value }
function Remove-TideUninstallValue([string]$Name) { $script:FixtureEntry.Remove($Name) }
function Remove-TideUninstallEntryKey { $script:FixtureEntry = $null }
function Assert-Probe([bool]$Condition, [string]$Message) { if (!$Condition) { throw $Message } }
function Test-InstalledRelease([string]$Release) {
    foreach ($name in @('mpv.exe','yt-dlp.exe','deno.exe')) {
        & (Join-Path $Release "tools/$name") --version | Select-Object -First 1
        Assert-Probe ($LASTEXITCODE -eq 0) 'A managed playback tool failed its version probe.'
    }
    & (Join-Path $Release '.tools/provider-venv/Scripts/python.exe') -c 'import ytmusicapi, requests; print("PASS: installed Python imports")'
    Assert-Probe ($LASTEXITCODE -eq 0) 'The installed Python helper failed its import probe.'
    $python = Join-Path $Release '.tools/provider-venv/Scripts/python.exe'
    & $python -c 'import sys,wave; w=wave.open(sys.argv[1],"wb"); w.setparams((1,2,8000,0,"NONE","not compressed")); w.writeframes(bytes(8000*2*60)); w.close()' (Join-Path $taskFixture 'silent.wav')
    Assert-Probe ($LASTEXITCODE -eq 0) 'Could not create silent local audio fixture.'
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = Join-Path $Release 'tide-music.exe'
    $start.WorkingDirectory = $Release
    $screens = Join-Path $taskFixture ([IO.Path]::GetFileName($Release) + '-screens')
    $start.Arguments = '--smoke-ui "' + $screens + '" --smoke-populated --smoke-library --fixture-audio "' + (Join-Path $taskFixture 'silent.wav') + '"'
    $start.UseShellExecute = $false
    $start.EnvironmentVariables['APPDATA'] = Join-Path $taskFixture 'isolated-settings'
    $start.EnvironmentVariables['LOCALAPPDATA'] = Join-Path $taskFixture 'isolated-local'
    $start.EnvironmentVariables.Remove('TIDE_RUNTIME_DIR')
    $start.EnvironmentVariables.Remove('TIDE_PROVIDER_PYTHON')
    $start.EnvironmentVariables.Remove('TIDE_PROVIDER_HELPER')
    $process = [Diagnostics.Process]::Start($start)
    try {
        if (!$process.WaitForExit(120000)) { $process.Kill(); $process.WaitForExit(); throw 'Installed UI smoke exceeded its deadline.' }
        Assert-Probe ($process.ExitCode -eq 0) 'Installed application smoke failed.'
        Assert-Probe (@(Get-ChildItem -LiteralPath $screens -Filter '*.png').Count -ge 5) 'Installed application did not produce expected UI screenshots.'
    } finally { $process.Dispose() }
    Write-Output 'PASS: actual installed app and private runtimes'
}

Install-Tide $taskPackage $taskRoot $taskLinks 'None' '' | Out-Host
$taskFirst = Get-TideInstallState $taskRoot
$taskFirstRelease = Join-Path $taskRoot ('releases/' + $taskFirst.Release)
Test-InstalledRelease $taskFirstRelease
Assert-Probe ($script:FixtureEntry.DisplayName -eq 'Tide') 'Fixture uninstall registration missing.'
Assert-Probe ((Get-TideShortcut (Join-Path $taskLinks 'Tide.lnk')).Target -eq (Join-Path $taskFirstRelease 'tide-music.exe')) 'Actual shortcut points to the wrong executable.'
Install-Tide $taskPackage $taskRoot $taskLinks 'None' '' | Out-Host
$taskSecond = Get-TideInstallState $taskRoot
Assert-Probe ($taskSecond.Release -ne $taskFirst.Release) 'Update did not publish a fresh release.'
Assert-Probe (!(Test-Path -LiteralPath $taskFirstRelease)) 'Update did not remove the previous owned release.'
Test-InstalledRelease (Join-Path $taskRoot ('releases/' + $taskSecond.Release))
Uninstall-Tide $taskRoot $taskLinks | Out-Host
Assert-Probe (!(Test-Path -LiteralPath $taskRoot)) 'Owned install directory remains after removal.'
Assert-Probe (!(Test-Path -LiteralPath $taskLinks)) 'Owned shortcuts remain after removal.'
Assert-Probe ($null -eq $script:FixtureEntry) 'Fixture uninstall entry remains after removal.'
Write-Output 'PASS: real provision/install/update/remove; registry was simulated; clean-Windows GUI test remains required'
