# Lab-only pinned mpv fixture; no account, system setup or GitHub API token.
#requires -Version 5.1
$ErrorActionPreference = 'Stop'
$taskRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
. (Join-Path $taskRoot 'scripts/install-common.ps1')
$taskDestination = Join-Path $taskRoot '.local/jam-native-runtime'
$taskArchive = Join-Path $taskRoot '.local/jam-native-mpv.zip'
Assert-TidePlainPath $taskDestination
Assert-TidePlainPath $taskArchive
if ((Test-Path -LiteralPath $taskDestination) -or (Test-Path -LiteralPath $taskArchive)) {
    throw 'Jam fixture paths already exist; existing files were preserved.'
}
[IO.Directory]::CreateDirectory((Join-Path $taskRoot '.local')) | Out-Null
# Same upstream URL and reviewed digest as setup-runtime.ps1. This local-file
# playback probe needs no extractor/Deno and no redundant release metadata API.
Get-TideDownload 'https://github.com/mpv-player/mpv/releases/download/v0.41.0/mpv-v0.41.0-x86_64-pc-windows-msvc.zip' `
    '4e197f729f5071c6772f35fffd96e0f36e3e8a044bd9479b136bb09b7c6a80ff' $taskArchive
Expand-TideArchive $taskArchive $taskDestination
$taskMpv = @(Get-ChildItem -LiteralPath $taskDestination -Recurse -Filter mpv.exe)
if ($taskMpv.Count -ne 1 -or $taskMpv[0].DirectoryName -ne $taskDestination) {
    throw 'Pinned mpv archive layout changed; review it before changing the lab.'
}
Write-Output 'Verified mpv-only Jam fixture ready; no user configuration changed.'
