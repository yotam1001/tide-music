[CmdletBinding()]
param([Parameter(Mandatory)][string]$Package, [Parameter(Mandatory)][string]$Destination)
$ErrorActionPreference = 'Stop'
$taskRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
. (Join-Path $PSScriptRoot 'grove-install-common.ps1')
$taskPackage = [IO.Path]::GetFullPath($Package)
$taskOutput = [IO.Path]::GetFullPath($Destination)
if (!$taskOutput.StartsWith((Join-Path $taskRoot 'dist') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Grove setup output must be inside dist.' }
Assert-GrovePlainPath $taskPackage
Assert-GrovePlainPath $taskOutput
if (Test-Path -LiteralPath $taskOutput) { throw 'Refusing to overwrite setup.' }
& python (Join-Path $PSScriptRoot 'package-grove-beta.py') --check $taskPackage
if ($LASTEXITCODE -ne 0) { throw 'Beta validation failed.' }
$taskCompiler = Join-Path $env:SystemRoot 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($taskOutput)) | Out-Null
$taskStage = $taskOutput + '.staging-' + [Guid]::NewGuid().ToString('N')
$taskDigest = $taskStage + '.payload-sha256'
$taskIcon = $taskStage + '.ico'
if ((Test-Path -LiteralPath $taskDigest) -or (Test-Path -LiteralPath $taskIcon)) { throw 'Setup staging path exists.' }
[IO.File]::WriteAllText($taskDigest, (Get-FileHash -LiteralPath $taskPackage -Algorithm SHA256).Hash.ToLowerInvariant(), [Text.UTF8Encoding]::new($false))
$taskExport = Start-Process -FilePath (Join-Path $taskRoot 'target/release/grove-music.exe') -ArgumentList @('--export-icon', ('"' + $taskIcon + '"')) -WindowStyle Hidden -Wait -PassThru
if ($taskExport.ExitCode -ne 0 -or !(Test-Path -LiteralPath $taskIcon)) { throw 'Could not export Grove icon.' }
try {
    & $taskCompiler /nologo /target:winexe /platform:x64 /optimize+ /debug- /warnaserror+ `
        "/out:$taskOutput" "/win32icon:$taskIcon" "/win32manifest:$(Join-Path $taskRoot 'installer/grove.manifest')" `
        /reference:System.Windows.Forms.dll /reference:System.Drawing.dll /reference:System.IO.Compression.dll /reference:System.IO.Compression.FileSystem.dll `
        "/resource:$taskPackage,Grove.Payload" "/resource:$taskDigest,Grove.Payload.sha256" `
        (Join-Path $taskRoot 'installer/GroveSetup.cs')
    if ($LASTEXITCODE -ne 0) { throw 'Grove setup compilation failed.' }
    $taskVerify = Start-Process -FilePath $taskOutput -ArgumentList '--verify-payload' -WindowStyle Hidden -Wait -PassThru
    if ($taskVerify.ExitCode -ne 0) { throw 'Embedded Grove payload validation failed.' }
    Write-Output "Grove installer built and payload verified: $taskOutput"
} finally {
    Remove-Item -LiteralPath $taskDigest -Force
    Remove-Item -LiteralPath $taskIcon -Force
}
