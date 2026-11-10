# Build one self-contained Windows setup EXE from a validated public preview ZIP.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Package, [string]$Destination = 'dist/TideSetup.exe',
    [Parameter(Mandatory)][ValidatePattern('^[0-9][A-Za-z0-9.+-]{0,63}$')][string]$AppVersion)
$ErrorActionPreference = 'Stop'
$taskRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
. (Join-Path $PSScriptRoot 'install-common.ps1')
$taskPackage = if ([IO.Path]::IsPathRooted($Package)) { [IO.Path]::GetFullPath($Package) } else { [IO.Path]::GetFullPath((Join-Path $taskRoot $Package)) }
$taskOutput = if ([IO.Path]::IsPathRooted($Destination)) { [IO.Path]::GetFullPath($Destination) } else { [IO.Path]::GetFullPath((Join-Path $taskRoot $Destination)) }
if (!$taskOutput.StartsWith((Join-Path $taskRoot 'dist') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Installer output must be inside dist.' }
Assert-TidePlainPath $taskPackage
Assert-TidePlainPath $taskOutput
if (Test-Path -LiteralPath $taskOutput) { throw 'Refusing to overwrite an installer output.' }
& python (Join-Path $PSScriptRoot 'check-preview-package.py') $taskPackage --with-browser-host
if ($LASTEXITCODE -ne 0) { throw 'Package validation failed.' }
$taskCompiler = Join-Path $env:SystemRoot 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
if (!(Test-Path -LiteralPath $taskCompiler)) { throw 'The Windows .NET Framework compiler is required to build setup.' }
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($taskOutput)) | Out-Null
$taskDigest = $taskOutput + '.payload-sha256'
$taskVersion = $taskOutput + '.version'
if (Test-Path -LiteralPath $taskDigest) { throw 'Digest staging path exists.' }
if (Test-Path -LiteralPath $taskVersion) { throw 'Version staging path exists.' }
[IO.File]::WriteAllText($taskDigest, (Get-FileHash -LiteralPath $taskPackage -Algorithm SHA256).Hash.ToLowerInvariant(), [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText($taskVersion, $AppVersion, [Text.UTF8Encoding]::new($false))
try {
    & $taskCompiler /nologo /target:winexe /platform:x64 /optimize+ /debug- /warnaserror+ `
        "/out:$taskOutput" "/win32icon:$(Join-Path $taskRoot 'ui/assets/tide.ico')" `
        "/win32manifest:$(Join-Path $taskRoot 'installer/app.manifest')" `
        /reference:System.Windows.Forms.dll /reference:System.Drawing.dll /reference:System.IO.Compression.dll /reference:System.IO.Compression.FileSystem.dll `
        "/resource:$taskPackage,Tide.Payload" "/resource:$taskDigest,Tide.Payload.sha256" `
        "/resource:$taskVersion,Tide.Version" `
        (Join-Path $taskRoot 'installer/TideSetup.cs')
    if ($LASTEXITCODE -ne 0) { throw 'Installer compilation failed.' }
    $taskVerify = Start-Process -FilePath $taskOutput -ArgumentList '--verify-payload' -WindowStyle Hidden -Wait -PassThru
    if ($taskVerify.ExitCode -ne 0) { throw 'Embedded payload verification failed.' }
    Write-Output "Installer built and payload verified: $taskOutput"
} finally { Remove-Item -LiteralPath $taskDigest -Force; Remove-Item -LiteralPath $taskVersion -Force }
