#requires -Version 5.1
[CmdletBinding()]
param(
    [ValidateSet('Install','Uninstall')][string]$Mode = 'Install',
    [string]$InstallRoot = (Join-Path $env:LOCALAPPDATA 'Programs/Grove')
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$ProgressPreference = 'SilentlyContinue'
. (Join-Path $PSScriptRoot 'grove-install-common.ps1')
$taskInstallRoot = [IO.Path]::GetFullPath($InstallRoot).TrimEnd('\')
$taskShortcuts = Join-Path ([Environment]::GetFolderPath('Programs')) 'Grove'
if (!$env:PROCESSOR_ARCHITEW6432 -and $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { throw 'Grove beta supports Windows x64.' }
if ($Mode -eq 'Uninstall') { Uninstall-Grove $taskInstallRoot $taskShortcuts; exit }
Install-Grove (Split-Path -Parent $PSScriptRoot) $taskInstallRoot $taskShortcuts 'None' '' '0.1.0-beta.1'
