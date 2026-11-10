# Real package/runtime/shortcut verification in an explicitly isolated TEMP folder.
# Windows Apps registration is replaced with an in-memory fixture; no real install is changed.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$PackageRoot, [Parameter(Mandatory)][string]$OutputRoot)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'grove-install-common.ps1')
$taskOutput = [IO.Path]::GetFullPath($OutputRoot).TrimEnd('\')
$taskTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
if ([IO.Path]::GetDirectoryName($taskOutput) -ne $taskTemp -or [IO.Path]::GetFileName($taskOutput) -notmatch '^grove-beta-installed-[a-zA-Z0-9-]+$') { throw 'Use a new grove-beta-installed-* direct child of TEMP.' }
Assert-GrovePlainPath $taskOutput
if (Test-Path -LiteralPath $taskOutput) { throw 'Verification directory already exists.' }
[IO.Directory]::CreateDirectory($taskOutput) | Out-Null
$script:FixtureEntry = $null
function Get-GroveUninstallEntry {
    if ($null -eq $script:FixtureEntry) { return $null }
    $copy=@{}; foreach ($name in $script:FixtureEntry.Keys) { $copy[$name]=$script:FixtureEntry[$name] }; return $copy
}
function New-GroveUninstallEntry { if ($null -ne $script:FixtureEntry) { throw 'Fixture entry exists.' }; $script:FixtureEntry=@{} }
function Write-GroveUninstallValue($Name,$Value) { $script:FixtureEntry[$Name]=$Value }
function Remove-GroveUninstallValue($Name) { $script:FixtureEntry.Remove($Name) }
function Remove-GroveUninstallEntryKey { $script:FixtureEntry=$null }
$taskRoot=Join-Path $taskOutput 'installed'
$taskLinks=Join-Path $taskOutput 'start-menu'
Install-Grove ([IO.Path]::GetFullPath($PackageRoot)) $taskRoot $taskLinks 'None' '' '0.1.0-beta.1'
$taskState=Get-GroveInstallState $taskRoot
$taskRelease=Get-GroveChild $taskRoot ('releases/' + $taskState.Release)
if (!(Test-Path -LiteralPath (Join-Path $taskRelease 'tools/mpv.exe'))) { throw 'Installed local audio runtime is missing.' }
if (!(Test-GroveShortcutMatch (Get-GroveShortcut (Join-Path $taskLinks 'Grove.lnk')) (Get-GroveShortcuts $taskRelease $taskRoot)['Grove.lnk'])) { throw 'Real temporary Start shortcut is wrong.' }
$taskPreviousRuntime=$env:TIDE_RUNTIME_DIR
try {
    $env:TIDE_RUNTIME_DIR=Join-Path $taskRelease 'tools'
    $taskLog=Join-Path $taskOutput 'local-audio.log'
    $taskProbe=Start-Process -FilePath (Join-Path $taskRelease 'grove-music.exe') -ArgumentList @('--review-local-audio', ('"' + (Join-Path $taskOutput 'audio-check') + '"')) -WindowStyle Hidden -Wait -PassThru -RedirectStandardError $taskLog
    if ($taskProbe.ExitCode -ne 0 -or !([IO.File]::ReadAllText($taskLog).Contains('PASS:'))) { throw 'Installed local audio verification failed.' }
} finally { $env:TIDE_RUNTIME_DIR=$taskPreviousRuntime }
Uninstall-Grove $taskRoot $taskLinks
if ((Test-Path -LiteralPath $taskRoot) -or (Test-Path -LiteralPath $taskLinks) -or $null -ne $script:FixtureEntry) { throw 'Isolated uninstall left owned registration or files.' }
[IO.File]::WriteAllText((Join-Path $taskOutput 'result.json'), '{"installed":true,"real_shortcuts_verified":true,"pinned_runtime_downloaded":true,"local_audio_passed":true,"uninstalled":true,"real_windows_registration_changed":false}')
'PASS: real beta package install, verified runtime, temporary shortcuts, native local audio and uninstall; no real Apps registration changed'
