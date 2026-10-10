param([string]$Destination = '.tools/runtime')
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
if (-not [IO.Path]::IsPathRooted($Destination)) { $Destination = Join-Path $taskRoot $Destination }
New-Item -ItemType Directory -Force -Path $Destination | Out-Null
$taskDownloads = Join-Path $taskRoot '.tools/downloads'
New-Item -ItemType Directory -Force -Path $taskDownloads | Out-Null

function Get-VerifiedAsset($Repo, $Tag, $Name, $Output) {
    $taskRelease = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/tags/$Tag"
    $taskAsset = $taskRelease.assets | Where-Object name -EQ $Name | Select-Object -First 1
    if (-not $taskAsset) { throw "Release asset not found: $Repo $Tag $Name" }
    Invoke-WebRequest -Uri $taskAsset.browser_download_url -OutFile $Output
    if ($taskAsset.digest -like 'sha256:*') {
        $taskExpected = $taskAsset.digest.Substring(7)
        if ((Get-FileHash -LiteralPath $Output -Algorithm SHA256).Hash -ne $taskExpected) {
            throw "Checksum failed for $Name"
        }
    }
}

Get-VerifiedAsset 'mpv-player/mpv' 'v0.41.0' 'mpv-v0.41.0-x86_64-pc-windows-msvc.zip' (Join-Path $taskDownloads 'mpv.zip')
Expand-Archive -LiteralPath (Join-Path $taskDownloads 'mpv.zip') -DestinationPath (Join-Path $taskDownloads 'mpv') -Force
$taskMpv = Get-ChildItem -LiteralPath (Join-Path $taskDownloads 'mpv') -Recurse -Filter mpv.exe | Select-Object -First 1
if (-not $taskMpv) { throw 'mpv archive did not contain mpv.exe' }
Copy-Item -LiteralPath $taskMpv.FullName -Destination (Join-Path $Destination 'mpv.exe')
Get-ChildItem -LiteralPath $taskMpv.DirectoryName -Filter '*.dll' | Copy-Item -Destination $Destination
Get-VerifiedAsset 'yt-dlp/yt-dlp' '2026.08.19' 'yt-dlp.exe' (Join-Path $Destination 'yt-dlp.exe')
Get-VerifiedAsset 'denoland/deno' 'v2.9.7' 'deno-x86_64-pc-windows-msvc.zip' (Join-Path $taskDownloads 'deno.zip')
Expand-Archive -LiteralPath (Join-Path $taskDownloads 'deno.zip') -DestinationPath $Destination -Force
Write-Output "Playback tools ready in $Destination. No system configuration was changed."
