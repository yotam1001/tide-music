#requires -Version 5.1
<#
.SYNOPSIS
Creates an app-only Windows preview folder and ZIP from an explicit file allowlist.
.DESCRIPTION
Runtime tools and the optional Python environment are installed separately by the
included setup scripts. This script never downloads or launches anything. Every
output must be under this project's dist directory, and existing outputs are
never overwritten or removed. SHA256SUMS.txt covers every packaged payload file;
the manifest itself is included in the ZIP but cannot checksum itself.
.EXAMPLE
./scripts/package-preview.ps1 -DependenciesNotice ./.local/DEPENDENCIES.md -StartHere ./docs/START_HERE.md
#>
[CmdletBinding()]
param(
    [Alias('ExecutablePath')]
    [string]$Executable = 'target/release/tide-music.exe',
    [Parameter(Mandatory = $true)]
    [string]$DependenciesNotice,
    [Parameter(Mandatory = $true)]
    [string]$StartHere,
    [string]$Destination
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$taskRoot = (Resolve-Path -LiteralPath (Split-Path -Parent $PSScriptRoot)).ProviderPath
$taskDist = [IO.Path]::GetFullPath((Join-Path $taskRoot 'dist'))

function Get-ProjectPath([string]$Path) {
    if ([string]::IsNullOrWhiteSpace($Path)) { throw 'A file path must not be empty.' }
    if (-not [IO.Path]::IsPathRooted($Path)) { $Path = Join-Path $taskRoot $Path }
    return [IO.Path]::GetFullPath($Path)
}

function Assert-NoReparseAncestor([string]$Path) {
    $taskAncestor = $Path
    while (-not [string]::IsNullOrEmpty($taskAncestor)) {
        if (Test-Path -LiteralPath $taskAncestor) {
            $taskItem = Get-Item -LiteralPath $taskAncestor -Force
            if (($taskItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Symbolic links and junctions are not allowed in package paths: $taskAncestor"
            }
        }
        $taskAncestor = [IO.Path]::GetDirectoryName($taskAncestor)
    }
}

function Assert-DistOutput([string]$Path) {
    $taskPrefix = $taskDist.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    if (-not $Path.StartsWith($taskPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Package outputs must be inside the project dist directory: $Path"
    }
    Assert-NoReparseAncestor $Path
    if (Test-Path -LiteralPath $Path) { throw "Refusing to overwrite an existing output: $Path" }
}

if ([string]::IsNullOrWhiteSpace($Destination)) {
    $taskPackageName = 'tide-windows-preview-' + [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss') + '-' + [Guid]::NewGuid().ToString('N').Substring(0, 8)
    $Destination = Join-Path 'dist' $taskPackageName
}
$taskPackage = (Get-ProjectPath $Destination).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
$taskZip = $taskPackage + '.zip'
Assert-DistOutput $taskPackage
Assert-DistOutput $taskZip

# Map sources to fixed package names. Never enumerate the checkout or build tree.
$taskFiles = @(
    @{ Source = $Executable; Relative = 'tide-music.exe' },
    @{ Source = 'LICENSE'; Relative = 'LICENSE' },
    @{ Source = 'THIRD_PARTY_NOTICES.md'; Relative = 'THIRD_PARTY_NOTICES.md' },
    @{ Source = 'docs/ACCOUNT_SETUP.md'; Relative = 'docs/ACCOUNT_SETUP.md' },
    @{ Source = 'scripts/setup-runtime.ps1'; Relative = 'scripts/setup-runtime.ps1' },
    @{ Source = 'scripts/setup-provider.ps1'; Relative = 'scripts/setup-provider.ps1' },
    @{ Source = 'helpers/youtube_music.py'; Relative = 'helpers/youtube_music.py' },
    @{ Source = 'helpers/requirements.txt'; Relative = 'helpers/requirements.txt' },
    @{ Source = $DependenciesNotice; Relative = 'DEPENDENCIES.md' },
    @{ Source = $StartHere; Relative = 'START_HERE.md' }
) | Sort-Object -Property Relative

# Validate the complete input set before creating any package output.
foreach ($taskFile in $taskFiles) {
    $taskFile.Source = Get-ProjectPath $taskFile.Source
    if (-not (Test-Path -LiteralPath $taskFile.Source -PathType Leaf)) {
        throw "Required package file does not exist: $($taskFile.Source)"
    }
    Assert-NoReparseAncestor $taskFile.Source
}

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$taskZipStream = $null
$taskArchive = $null
try {
    # New-Item without -Force and CreateNew both fail if a concurrent process has
    # claimed either output since the validation above.
    New-Item -ItemType Directory -Path $taskPackage | Out-Null
    $taskManifestLines = foreach ($taskFile in $taskFiles) {
        $taskTarget = Join-Path $taskPackage $taskFile.Relative
        [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($taskTarget)) | Out-Null
        [IO.File]::Copy($taskFile.Source, $taskTarget, $false)
        $taskHash = (Get-FileHash -LiteralPath $taskTarget -Algorithm SHA256).Hash.ToLowerInvariant()
        "$taskHash  $($taskFile.Relative)"
    }
    $taskManifest = Join-Path $taskPackage 'SHA256SUMS.txt'
    [IO.File]::WriteAllLines($taskManifest, [string[]]$taskManifestLines, (New-Object Text.UTF8Encoding($false)))

    $taskZipStream = [IO.File]::Open($taskZip, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    $taskArchive = New-Object IO.Compression.ZipArchive($taskZipStream, [IO.Compression.ZipArchiveMode]::Create, $true)
    $taskEntryNames = @($taskFiles | ForEach-Object { $_.Relative }) + @('SHA256SUMS.txt')
    foreach ($taskEntryName in ($taskEntryNames | Sort-Object)) {
        [IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
            $taskArchive, (Join-Path $taskPackage $taskEntryName), $taskEntryName,
            [IO.Compression.CompressionLevel]::Optimal
        ) | Out-Null
    }
}
catch {
    throw "Packaging failed. Any partial outputs are retained without deletion: $taskPackage; $taskZip. $($_.Exception.Message)"
}
finally {
    if ($null -ne $taskArchive) { $taskArchive.Dispose() }
    if ($null -ne $taskZipStream) { $taskZipStream.Dispose() }
}

[pscustomobject]@{
    PackageDirectory = $taskPackage
    ArchivePath = $taskZip
    Sha256Manifest = $taskManifest
    PayloadFiles = $taskFiles.Count
    ArchiveSha256 = (Get-FileHash -LiteralPath $taskZip -Algorithm SHA256).Hash.ToLowerInvariant()
}
