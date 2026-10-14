#requires -Version 5.1
param([string]$Destination = '.tools/runtime')
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'install-common.ps1')
$taskRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
if (-not [IO.Path]::IsPathRooted($Destination)) { $Destination = Join-Path $taskRoot $Destination }
$Destination = [IO.Path]::GetFullPath($Destination)
Assert-TidePlainPath $Destination
if (Test-Path -LiteralPath $Destination) { throw 'Runtime destination exists; choose a new folder rather than overwriting tools.' }
$taskDownloads = Join-Path $taskRoot ('.tools/runtime-downloads-' + [Guid]::NewGuid().ToString('N'))
Assert-TidePlainPath $taskDownloads
[IO.Directory]::CreateDirectory($taskDownloads) | Out-Null

function Get-VerifiedAsset($Repo, $Tag, $Name, $Output, $Sha256) {
    $taskRelease = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/tags/$Tag" -TimeoutSec 30
    $taskAsset = $taskRelease.assets | Where-Object name -EQ $Name | Select-Object -First 1
    if (!$taskAsset -or !$taskAsset.PSObject.Properties['digest'] -or $taskAsset.digest -notmatch '^sha256:([a-fA-F0-9]{64})$') {
        throw "Release asset has no SHA-256 digest: $Repo $Tag $Name"
    }
    $taskExpected = $Matches[1]
    if ($taskExpected -ne $Sha256) { throw "Upstream asset digest changed for $Name; review the release before updating its pinned checksum." }
    if ($taskAsset.browser_download_url -cne "https://github.com/$Repo/releases/download/$Tag/$Name") { throw 'Unexpected release download source.' }
    Get-TideDownload $taskAsset.browser_download_url $Sha256 $Output
}

Get-VerifiedAsset 'mpv-player/mpv' 'v0.41.0' 'mpv-v0.41.0-x86_64-pc-windows-msvc.zip' (Join-Path $taskDownloads 'mpv.zip') '4e197f729f5071c6772f35fffd96e0f36e3e8a044bd9479b136bb09b7c6a80ff'
Expand-TideArchive (Join-Path $taskDownloads 'mpv.zip') (Join-Path $taskDownloads 'mpv')
$taskMpv = @(Get-ChildItem -LiteralPath (Join-Path $taskDownloads 'mpv') -Recurse -Filter mpv.exe)
if ($taskMpv.Count -ne 1) { throw 'mpv archive must contain exactly one mpv.exe.' }
[IO.Directory]::CreateDirectory($Destination) | Out-Null
# Keep companion DLLs and upstream license files with mpv, not just the EXE.
foreach ($taskFile in Get-ChildItem -LiteralPath $taskMpv[0].DirectoryName -File -Recurse) {
    $taskRelative = $taskFile.FullName.Substring($taskMpv[0].DirectoryName.Length + 1).Replace('\','/')
    $taskOutput = Get-TideChild $Destination $taskRelative
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($taskOutput)) | Out-Null
    [IO.File]::Copy($taskFile.FullName, $taskOutput, $false)
}
Get-VerifiedAsset 'yt-dlp/yt-dlp' '2026.08.19' 'yt-dlp.exe' (Join-Path $Destination 'yt-dlp.exe') '66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a'
Get-VerifiedAsset 'denoland/deno' 'v2.9.7' 'deno-x86_64-pc-windows-msvc.zip' (Join-Path $taskDownloads 'deno.zip') 'a0c3101b4158d1dfb7d6a78a7bf0f3de80c96bb423c152beec8beb22786f2238'
Expand-TideArchive (Join-Path $taskDownloads 'deno.zip') $Destination
Write-Output "Playback tools ready in $Destination. No system configuration was changed."
