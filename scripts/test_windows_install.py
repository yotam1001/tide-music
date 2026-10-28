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
$global:MockProcesses = $null
$global:LateProcesses = $null
$global:ReopenedProcesses = @{}
$global:Busy = $false
$global:FailDependency = $false
$global:FailShortcut = $false
$global:FailRegistration = $false
$global:Registry = @{}
$global:AppsEntry = $null
$global:AppWrites = 0
$global:FailAppWrite = 0
$global:RaceNewAppEntry = $false
function Get-TideUninstallEntry {
    if ($null -eq $global:AppsEntry) { return $null }
    $copy = @{}; foreach ($name in $global:AppsEntry.get_Keys()) { $copy[$name] = $global:AppsEntry[$name] }
    return $copy
}
function New-TideUninstallEntry {
    if ($global:RaceNewAppEntry) { $global:AppsEntry = @{Foreign='preserve'}; throw 'Synthetic concurrent entry claim' }
    if ($null -ne $global:AppsEntry) { throw 'Synthetic entry already exists' }
    $global:AppsEntry = @{}
}
function Write-TideUninstallValue {
    param($Name, $Value)
    $global:AppWrites++
    if ($global:FailAppWrite -eq $global:AppWrites) { $global:FailAppWrite = 0; throw 'Synthetic uninstall registry write failure' }
    $global:AppsEntry[$Name] = $Value
}
function Remove-TideUninstallValue { param($Name) $global:AppsEntry.Remove($Name) }
function Remove-TideUninstallEntryKey { $global:AppsEntry = $null }
function Get-TideCloseCandidate { param($Id) if ($global:ReopenedProcesses.ContainsKey($Id)) { return $global:ReopenedProcesses[$Id] }; return @($global:MockProcesses | Where-Object Id -EQ $Id)[0] }
function Get-TideOwnedProcesses { param($InstallRoot) if ($null -ne $global:MockProcesses) { return @($global:MockProcesses | Where-Object { !$_.HasExited }) }; if ($global:Busy) { return @('synthetic owned process') }; return @() }
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
    if ($null -ne $global:LateProcesses) { $global:MockProcesses = $global:LateProcesses; $global:LateProcesses = $null }
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
function New-MockPlayer([string]$Path, [string]$Name = 'tide-music') {
    $p = [pscustomobject]@{Id=123; StartTime=[DateTime]::UtcNow; Path=$Path; ProcessName=$Name; SessionId=[Diagnostics.Process]::GetCurrentProcess().SessionId; HasExited=$false; Handle=[IntPtr]::Zero; CloseCount=0}
    $p | Add-Member -MemberType ScriptMethod -Name CloseMainWindow -Value { $this.CloseCount++; $this.HasExited=$true; return $true }
    $p | Add-Member -MemberType ScriptMethod -Name Dispose -Value { }
    return $p
}
switch ($Scenario) {
    'multiple-player-update' {
        New-Install; $previous = Current-Release
        $first = New-MockPlayer (Join-Path $previous 'tide-music.exe')
        $second = New-MockPlayer (Join-Path $previous 'tide-music.exe')
        $second.Id = 124
        $global:MockProcesses = @($first,$second)
        Test-TideInstall $package $root $links 'None' '' '1.2.3' | Out-Null
        if ($first.CloseCount -or $second.CloseCount) { throw 'Preflight closed a player before COMMIT' }
        Install-Tide $package $root $links 'None' '' '1.2.3' -CloseRunningPlayers | Out-Null
        if ($first.CloseCount -ne 1 -or $second.CloseCount -ne 1 -or (Current-Release) -eq $previous) { throw 'Multiple windows did not close gracefully for update' }
        $global:MockProcesses = $null
        Uninstall-Tide $root $links | Out-Null
    }
    'late-player-update' {
        New-Install; $previous = Current-Release
        $late = New-MockPlayer (Join-Path $previous 'tide-music.exe')
        $global:MockProcesses = @(); $global:LateProcesses = @($late)
        Install-Tide $package $root $links 'None' '' '1.2.3' -CloseRunningPlayers | Out-Null
        if ($late.CloseCount -ne 1 -or (Current-Release) -eq $previous) { throw 'A player opened during downloads blocked update' }
        $global:MockProcesses = $null
        Uninstall-Tide $root $links | Out-Null
    }
    'close-player-boundaries' {
        New-Install; $release = Current-Release
        foreach ($mode in @('outside','helper','session','unowned','tampered')) {
            $p = New-MockPlayer (Join-Path $release 'tide-music.exe')
            switch ($mode) {
                'outside' { $p.Path = Join-Path $Fixture 'tide-music.exe' }
                'helper' { $p.ProcessName = 'mpv' }
                'session' { $p.SessionId++ }
                'unowned' { $p.Path = Join-Path $root ('releases/' + ('c' * 32) + '/tide-music.exe') }
                'tampered' { [IO.File]::AppendAllText((Join-Path $release 'tide-music.exe'), 'tampered') }
            }
            $global:MockProcesses = @($p)
            Must-Fail { Wait-TideStopped $root 0 }
            if ($p.CloseCount -or $p.HasExited) { throw ('Unexpected player close: ' + $mode) }
        }
        [IO.File]::Copy((Join-Path $package 'tide-music.exe'), (Join-Path $release 'tide-music.exe'), $true)
        $stale = New-MockPlayer (Join-Path $release 'tide-music.exe')
        $recycled = New-MockPlayer (Join-Path $Fixture 'foreign.exe')
        $global:MockProcesses = @($stale); $global:ReopenedProcesses[$stale.Id] = $recycled
        Must-Fail { Wait-TideStopped $root 0 }
        if ($stale.CloseCount -or $recycled.CloseCount -or $recycled.HasExited) { throw 'A recycled PID inherited stale close authorization' }
        $global:ReopenedProcesses = @{}
        $global:MockProcesses = $null
        # Restore fixture payload so ordinary owned cleanup remains verifiable.
        [IO.File]::Copy((Join-Path $package 'tide-music.exe'), (Join-Path $release 'tide-music.exe'), $true)
        Uninstall-Tide $root $links | Out-Null
    }
    'preflight' {
        $events = @(Test-TideInstall $package $root $links 'None' '' '1.2.3')
        if ($events[-1] -ne 'TIDE_PREFLIGHT_OK' -or (Test-Path -LiteralPath $root) -or (Test-Path -LiteralPath $links)) { throw 'Fresh preflight wrote an installation' }
        New-Install; $previous = Current-Release; $entry = Get-TideUninstallEntry
        $global:Busy = $true; $global:FailDependency = $true
        $events = @(Test-TideInstall $package $root $links 'None' '' '1.2.3')
        if ($events[-1] -ne 'TIDE_PREFLIGHT_OK' -or (Current-Release) -ne $previous -or !(Test-TideUninstallEntryMatch (Get-TideUninstallEntry) $entry)) { throw 'Running-player preflight changed ownership or registration' }
        $global:AppsEntry.UninstallString = 'different.exe'
        Must-Fail { Test-TideInstall $package $root $links 'None' '' '1.2.3' }
        if ($global:AppsEntry.UninstallString -ne 'different.exe') { throw 'Conflicting preflight changed registration' }
        $global:AppsEntry = $entry; $global:Busy = $false; $global:FailDependency = $false
        Uninstall-Tide $root $links | Out-Null
    }
    'fresh-remove' {
        New-Install
        $release = Current-Release
        if (!(Test-Path -LiteralPath (Join-Path $links 'Tide.lnk')) -or $global:Registry['Chrome'] -ne $release) { throw 'Missing registration/shortcut' }
        $expected = Get-TideUninstallRegistration $release $root '0.1.0-preview'
        if (!(Test-TideUninstallEntryMatch $global:AppsEntry $expected) -or $global:AppsEntry.DisplayName -ne 'Tide' -or $global:AppsEntry.NoModify -ne 1 -or $global:AppsEntry.NoRepair -ne 1) { throw 'Missing Windows Settings uninstall fields' }
        Uninstall-Tide $root $links | Out-Null
        if ((Test-Path -LiteralPath $root) -or (Test-Path -LiteralPath $links) -or $global:Registry.Count -or $null -ne $global:AppsEntry) { throw 'Owned removal incomplete' }
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
        function Invoke-WebRequest { param([switch]$UseBasicParsing, $Uri, $OutFile, $TimeoutSec) if ($TimeoutSec -ne 300) { throw 'Missing bounded download timeout' }; $global:Downloads++; [IO.File]::WriteAllText($OutFile, 'fixture') }
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
    'runtime-api-independent' {
        $global:MetadataRequests = 0
        $global:Downloads = 0
        function Invoke-RestMethod { $global:MetadataRequests++; throw 'Synthetic GitHub API rate limit' }
        function Invoke-WebRequest {
            param([switch]$UseBasicParsing, $Uri, $OutFile, $TimeoutSec)
            if ($Uri -cne 'https://github.com/mpv-player/mpv/releases/download/v0.41.0/mpv-v0.41.0-x86_64-pc-windows-msvc.zip') { throw 'Unexpected runtime source' }
            if ($TimeoutSec -ne 300) { throw 'Missing bounded download timeout' }
            $global:Downloads++
            [IO.File]::WriteAllBytes($OutFile, [Text.Encoding]::UTF8.GetBytes('fixture'))
        }
        # Load the real asset function without running the bootstrap's other
        # downloads. Common's real checksum/path/overwrite checks remain active.
        $tokens = $null; $errors = $null
        $ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $Fixture 'source/scripts/setup-runtime.ps1'), [ref]$tokens, [ref]$errors)
        if ($errors.Count) { throw 'Runtime script did not parse' }
        $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-VerifiedAsset' }, $true)
        if (!$definition) { throw 'Missing production asset function' }
        . ([scriptblock]::Create($definition.Extent.Text))
        $output = Join-Path $Fixture 'verified.zip'
        $hash = 'f16d05ec6b29248d2c61adb1e9263f78e4f7bace1b955014a2d17872cfe4064d'
        Get-VerifiedAsset 'mpv-player/mpv' 'v0.41.0' 'mpv-v0.41.0-x86_64-pc-windows-msvc.zip' $output $hash
        if ([IO.File]::ReadAllText($output) -cne 'fixture' -or $global:Downloads -ne 1 -or $global:MetadataRequests) { throw 'Verified download depended on the API or changed bytes' }
        Must-Fail { Get-VerifiedAsset 'mpv-player/mpv' 'v0.41.0' 'mpv-v0.41.0-x86_64-pc-windows-msvc.zip' $output $hash }
        Must-Fail { Get-VerifiedAsset 'mpv-player/mpv' 'v0.41.0' 'mpv-v0.41.0-x86_64-pc-windows-msvc.zip' (Join-Path $Fixture 'missing.zip') '' }
        if ($global:Downloads -ne 1) { throw 'Missing pin or existing output started a download' }
    }
    'runtime-corrupt-download' {
        $global:MetadataRequests = 0; $global:Downloads = 0
        function Invoke-RestMethod { $global:MetadataRequests++; throw 'Synthetic GitHub API rate limit' }
        function Invoke-WebRequest {
            param([switch]$UseBasicParsing, $Uri, $OutFile, $TimeoutSec)
            $global:Downloads++
            [IO.File]::WriteAllText($OutFile, 'corrupted runtime archive')
        }
        $failure = $null
        try { & (Join-Path $Fixture 'source/scripts/setup-runtime.ps1') -Destination (Join-Path $Fixture 'tools') | Out-Null }
        catch { $failure = $_.Exception.Message }
        if ($failure -cne 'Dependency download checksum failed.') { throw 'Corrupt runtime was not rejected by its actual checksum' }
        if ($global:MetadataRequests -or $global:Downloads -ne 1) { throw 'Corrupt runtime did not reach the checksum check without the API' }
        if (Test-Path -LiteralPath (Join-Path $Fixture 'tools')) { throw 'Corrupt runtime wrote extracted tools' }
    }
    'app-entry-conflict' {
        $global:AppsEntry = @{DisplayName='unrelated product'; UninstallString='unrelated.exe'}
        Must-Fail { New-Install }
        if ((Test-Path -LiteralPath $root) -or $global:AppsEntry.DisplayName -ne 'unrelated product') { throw 'Foreign uninstall entry overwritten' }
    }
    'app-entry-update-conflict' {
        New-Install; $previous = Current-Release
        $global:AppsEntry.UninstallString = 'different.exe'
        Must-Fail { New-Install }; Must-Fail { Uninstall-Tide $root $links }
        if ((Current-Release) -ne $previous -or $global:AppsEntry.UninstallString -ne 'different.exe') { throw 'Changed uninstall entry modified' }
    }
    'app-entry-write-failure-first' {
        $global:FailAppWrite = 3; Must-Fail { New-Install }
        if ((Test-Path -LiteralPath $root) -or $null -ne $global:AppsEntry -or $global:Registry.Count) { throw 'Partial first uninstall entry not rolled back' }
    }
    'app-entry-write-failure-update' {
        New-Install; $previous = Current-Release; $before = Get-TideUninstallEntry
        $global:FailAppWrite = $global:AppWrites + 3; Must-Fail { New-Install }
        if ((Current-Release) -ne $previous -or !(Test-TideUninstallEntryMatch $global:AppsEntry $before) -or $global:Registry['Chrome'] -ne $previous) { throw 'Partial updated uninstall entry not restored' }
        Uninstall-Tide $root $links | Out-Null
    }
    'app-entry-marker-failure' {
        New-Install; $previous = Current-Release; $before = Get-TideUninstallEntry
        $pending = Join-Path $root '.tide-install.pending.json'; [IO.File]::WriteAllText($pending, 'preserve interrupted record')
        Must-Fail { New-Install }
        if ((Current-Release) -ne $previous -or !(Test-TideUninstallEntryMatch $global:AppsEntry $before)) { throw 'Final marker failure did not restore uninstall entry' }
        Remove-Item -LiteralPath $pending
        Uninstall-Tide $root $links | Out-Null
    }
    'app-entry-version-update' {
        New-Install
        Install-Tide $package $root $links 'None' '' '1.2.3-preview' | Out-Null
        $expected = Get-TideUninstallRegistration (Current-Release) $root '1.2.3-preview'
        if (!(Test-TideUninstallEntryMatch $global:AppsEntry $expected)) { throw 'Version/uninstall path did not update' }
        Uninstall-Tide $root $links | Out-Null
    }
    { $_ -in @('owned-stale-registration','owned-removed-registration','removed-registration-tampered','removed-registration-unowned','removed-registration-foreign-folder') } {
        Install-Tide $package $root $links 'Chrome' ('a' * 32) '0.4.2' | Out-Null
        $previous = Current-Release
        $retained = Join-Path $root ('releases/' + ('b' * 32))
        if ($Scenario -eq 'owned-stale-registration') { Copy-Item -LiteralPath $previous -Destination $retained -Recurse }
        # Match the reported three-field conflict: a previous owned version
        # and its icon/uninstall paths, with the correct product/root identity.
        $global:AppsEntry = Get-TideUninstallRegistration $retained $root '0.4.1'
        $before = Get-TideUninstallEntry
        $expected = Get-TideUninstallRegistration $previous $root '0.4.2'
        if ((Get-TideUninstallConflictMessage $before $expected) -notmatch 'DisplayIcon, DisplayVersion, UninstallString') { throw 'Fixture does not reproduce the reported conflict' }
        if ($Scenario -eq 'removed-registration-tampered') { [IO.File]::WriteAllText((Join-Path $previous 'scripts/install-windows.ps1'), 'tampered active script') }
        if ($Scenario -eq 'removed-registration-unowned') { Remove-Item -LiteralPath (Join-Path $previous '.tide-release.json') }
        if ($Scenario -eq 'removed-registration-foreign-folder') { [IO.Directory]::CreateDirectory($retained) | Out-Null }
        if ($Scenario -like 'removed-registration-*') {
            Must-Fail { Test-TideInstall $package $root $links 'None' '' '0.4.3' }
            if ((Current-Release) -ne $previous -or !(Test-TideUninstallEntryMatch (Get-TideUninstallEntry) $before)) { throw 'Unverified recovery changed installation' }
            break
        }
        $events = @(Test-TideInstall $package $root $links 'None' '' '0.4.3')
        if ($events[-1] -ne 'TIDE_PREFLIGHT_OK' -or !(Test-TideUninstallEntryMatch (Get-TideUninstallEntry) $before) -or (Current-Release) -ne $previous) { throw 'Recovery preflight changed the installation' }
        Install-Tide $package $root $links 'None' '' '0.4.3' | Out-Null
        $current = Current-Release
        if ($current -eq $previous -or !(Test-TideUninstallEntryMatch (Get-TideUninstallEntry) (Get-TideUninstallRegistration $current $root '0.4.3'))) { throw 'Owned stale registration did not upgrade' }
        Uninstall-Tide $root $links | Out-Null
    }
    'owned-stale-registration-rollback' {
        New-Install; $previous = Current-Release
        $global:AppsEntry.DisplayVersion = '0.4.1'
        $before = Get-TideUninstallEntry
        $pending = Join-Path $root '.tide-install.pending.json'
        [IO.File]::WriteAllText($pending, 'preserve interrupted record')
        Must-Fail { Install-Tide $package $root $links 'None' '' '0.4.3' }
        if ((Current-Release) -ne $previous -or !(Test-TideUninstallEntryMatch (Get-TideUninstallEntry) $before)) { throw 'Recovery rollback did not restore exact previous registration' }
        Remove-Item -LiteralPath $pending
        # A later successful retry repairs the version metadata.
        Install-Tide $package $root $links 'None' '' '0.4.3' | Out-Null
        Uninstall-Tide $root $links | Out-Null
    }
    'owned-stale-registration-tampered' {
        New-Install; $previous = Current-Release
        $global:AppsEntry.DisplayVersion = '0.4.1'
        $before = Get-TideUninstallEntry
        [IO.File]::WriteAllText((Join-Path $previous 'scripts/install-windows.ps1'), 'foreign modified script')
        Must-Fail { Test-TideInstall $package $root $links 'None' '' '0.4.3' }
        if ((Current-Release) -ne $previous -or !(Test-TideUninstallEntryMatch (Get-TideUninstallEntry) $before)) { throw 'Tampered recovery changed the installation' }
    }
    'owned-stale-registration-foreign' {
        New-Install; $previous = Current-Release
        foreach ($attack in @('command','root','product','extra-count','extra-keys','traversal')) {
            $global:AppsEntry = Get-TideUninstallRegistration $previous $root '0.4.1'
            switch ($attack) {
                'command' { $global:AppsEntry.UninstallString = 'foreign.exe' }
                'root' { $global:AppsEntry.InstallLocation = (Join-Path $Fixture 'another-product') }
                'product' { $global:AppsEntry.TideProduct = 'another.product' }
                'extra-count' { $global:AppsEntry['Count'] = 10 }
                'extra-keys' { $global:AppsEntry['Keys'] = 'foreign-value' }
                'traversal' { $global:AppsEntry.DisplayIcon = (Join-Path $root 'releases/../foreign.exe') }
            }
            $before = Get-TideUninstallEntry
            Must-Fail { Test-TideInstall $package $root $links 'None' '' '0.4.3' }
            if ((Current-Release) -ne $previous -or !(Test-TideUninstallEntryMatch (Get-TideUninstallEntry) $before)) { throw 'Foreign registration was changed' }
        }
        $global:AppsEntry = Get-TideUninstallRegistration $previous $root '0.1.0-preview'
        Uninstall-Tide $root $links | Out-Null
    }
    'owned-stale-registration-manifest' {
        New-Install; $previous = Current-Release
        $global:AppsEntry.DisplayVersion = '0.4.1'
        $before = Get-TideUninstallEntry
        $record = Join-Path $previous '.tide-release.json'
        $original = [IO.File]::ReadAllText($record)
        foreach ($attack in @('traversal','rooted','alias','self','missing-script','product')) {
            $manifest = $original | ConvertFrom-Json
            switch ($attack) {
                'traversal' { $manifest.Files += '../foreign.txt' }
                'rooted' { $manifest.Files += (Join-Path $Fixture 'foreign.txt') }
                'alias' { $manifest.Files += 'scripts/./install-windows.ps1' }
                'self' { $manifest.Files += '.tide-release.json' }
                'missing-script' { $manifest.Files = @($manifest.Files | Where-Object { $_ -ne 'scripts/install-windows.ps1' }) }
                'product' { $manifest.Product = 'another.product' }
            }
            Write-TideJson $record $manifest
            Must-Fail { Test-TideInstall $package $root $links 'None' '' '0.4.3' }
            if ((Current-Release) -ne $previous -or !(Test-TideUninstallEntryMatch (Get-TideUninstallEntry) $before)) { throw 'Invalid ownership manifest changed the installation' }
        }
        [IO.File]::WriteAllText($record, $original)
        $global:AppsEntry.DisplayVersion = '0.1.0-preview'
        Uninstall-Tide $root $links | Out-Null
    }
    'app-entry-migration' {
        New-Install; $global:AppsEntry = $null
        Install-Tide $package $root $links 'None' '' | Out-Null
        if ($global:AppsEntry.DisplayName -ne 'Tide') { throw 'Previous preview did not gain Windows uninstall entry' }
        Uninstall-Tide $root $links | Out-Null
    }
    'app-entry-new-race' {
        $global:RaceNewAppEntry = $true; Must-Fail { New-Install }
        if ((Test-Path -LiteralPath $root) -or $global:AppsEntry.Foreign -ne 'preserve') { throw 'Concurrent foreign entry removed' }
    }
    'protocol' {
        $events = @(Install-Tide $package $root $links 'None' '')
        $expectedResult = 'TIDE_RESULT|' + (Join-Path (Current-Release) 'tide-music.exe')
        if ($events[-1] -ne $expectedResult -or @($events | Where-Object { $_ -like 'TIDE_RESULT|*' }).Count -ne 1) { throw 'Missing/duplicate exact installed result' }
        $previousPercent = -1
        foreach ($event in $events | Where-Object { $_ -like 'TIDE_PROGRESS|*' }) {
            if ($event -notmatch '^TIDE_PROGRESS\|([0-9]+)\|[^|\r\n]+$') { throw 'Malformed progress event' }
            $percent = [int]$Matches[1]
            if ($percent -lt $previousPercent -or $percent -gt 100) { throw 'Invalid progress ordering' }
            $previousPercent = $percent
        }
        if ($previousPercent -ne 100 -or $global:Registry.Count) { throw 'Default setup required manual browser registration' }
        Uninstall-Tide $root $links | Out-Null
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
        for scenario in ("multiple-player-update", "late-player-update", "close-player-boundaries", "preflight", "fresh-remove", "upgrade", "preserve-extra", "dependency-failure-first",
                         "dependency-failure-update", "shortcut-failure", "registration-failure", "busy",
                         "unknown-root", "shortcut-conflict", "registration-conflict",
                         "changed-shortcut-removal", "changed-registration-removal", "corrupt-package",
                         "traversal-record", "reparse", "lock", "download", "archive-paths",
                         "runtime-api-independent", "runtime-corrupt-download", "app-entry-conflict",
                         "app-entry-update-conflict", "app-entry-write-failure-first", "app-entry-write-failure-update",
                         "app-entry-marker-failure", "app-entry-version-update", "owned-stale-registration", "owned-removed-registration", "removed-registration-tampered", "removed-registration-unowned", "removed-registration-foreign-folder", "owned-stale-registration-rollback", "owned-stale-registration-tampered", "owned-stale-registration-foreign", "owned-stale-registration-manifest", "app-entry-migration", "app-entry-new-race", "protocol"):
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
        for scenario in ("multiple-player-update", "late-player-update", "close-player-boundaries", "preflight", "fresh-remove", "upgrade", "registration-failure", "runtime-api-independent", "runtime-corrupt-download", "app-entry-write-failure-update", "app-entry-version-update", "owned-stale-registration", "owned-removed-registration", "removed-registration-tampered", "removed-registration-unowned", "removed-registration-foreign-folder", "owned-stale-registration-rollback", "owned-stale-registration-tampered", "owned-stale-registration-foreign", "owned-stale-registration-manifest", "protocol"):
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
