#requires -Version 5.1
[CmdletBinding()]
param(
    [ValidateSet('Install','Uninstall')][string]$Mode = 'Install',
    [string]$InstallRoot = (Join-Path $env:LOCALAPPDATA 'Programs/Tide'),
    [ValidateSet('None','Firefox','Chrome','Edge')][string]$Browser = 'None',
    [string]$ChromiumExtensionId,
    [ValidatePattern('^[0-9][A-Za-z0-9.+-]{0,63}$')][string]$AppVersion = '0.1.0-preview',
    [switch]$Interactive
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$ProgressPreference = 'SilentlyContinue'
. (Join-Path $PSScriptRoot 'install-common.ps1')
$taskInstallRoot = [IO.Path]::GetFullPath($InstallRoot).TrimEnd('\')
$taskShortcuts = Join-Path ([Environment]::GetFolderPath('Programs')) 'Tide'
if (!$env:PROCESSOR_ARCHITEW6432 -and $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { throw 'This preview supports Windows x64.' }
if ($Mode -eq 'Uninstall') { Uninstall-Tide $taskInstallRoot $taskShortcuts; exit }
if ($Interactive) {
    Write-Host 'Tide preview setup (per user; internet required; no administrator access).'
    Write-Host 'Playback tools and a private Python runtime will be downloaded. Saved sessions are preserved.'
    Write-Host 'Optional browser helper: install the companion extension separately first. Firefox/Zen public onboarding remains unverified.'
    $taskAnswer = Read-Host 'Browser helper [None / Chrome / Edge / Firefox] (Enter keeps the existing choice on update)'
    if ($taskAnswer) {
        if ($taskAnswer -notin @('None','Chrome','Edge','Firefox')) { throw 'Unrecognised browser choice.' }
        $Browser = $taskAnswer
    }
    if ($Browser -in @('Chrome','Edge')) { $ChromiumExtensionId = Read-Host 'Paste the exact 32-letter ID shown for the installed Tide companion extension' }
}
Install-Tide (Split-Path -Parent $PSScriptRoot) $taskInstallRoot $taskShortcuts $Browser $ChromiumExtensionId $AppVersion
