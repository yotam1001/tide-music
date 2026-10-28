# Build one self-contained Windows setup EXE from a validated public preview ZIP.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Package, [string]$Destination = 'dist/TideSetup.exe',
    [ValidatePattern('^[0-9][A-Za-z0-9.+-]{0,63}$')][string]$AppVersion)
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
$taskVersionFile = $taskOutput + '.version'
$taskAssemblyInfo = $taskOutput + '.AssemblyInfo.cs'
if (Test-Path -LiteralPath $taskDigest) { throw 'Digest staging path exists.' }
if (Test-Path -LiteralPath $taskVersionFile) { throw 'Version staging path exists.' }
if (Test-Path -LiteralPath $taskAssemblyInfo) { throw 'Assembly-version staging path exists.' }
$taskCargoText = Get-Content -LiteralPath (Join-Path $taskRoot 'Cargo.toml') -Raw
$taskVersionMatch = [regex]::Match($taskCargoText, '(?ms)^\[workspace\.package\]\s*\r?\nversion\s*=\s*"([^"]+)"')
if (!$taskVersionMatch.Success) { throw 'Cannot determine installer version from Cargo.toml.' }
$taskVersion = if ($AppVersion) { $AppVersion } else { $taskVersionMatch.Groups[1].Value }
$taskNumericMatch = [regex]::Match($taskVersion, '^(\d+)\.(\d+)\.(\d+)(?:[-+][0-9A-Za-z.+-]+)?$')
if (!$taskNumericMatch.Success) { throw 'Installer version must be a valid numeric release with an optional suffix.' }
$taskVersionParts = @(1..3 | ForEach-Object { [int]$taskNumericMatch.Groups[$_].Value })
if (@($taskVersionParts | Where-Object { $_ -gt 65534 }).Count -ne 0) { throw 'Installer version exceeds the Windows assembly version range.' }
$taskWindowsVersion = ($taskVersionParts -join '.') + '.0'
[IO.File]::WriteAllLines($taskAssemblyInfo, [string[]]@(
    'using System.Reflection;',
    '[assembly: AssemblyTitle("Tide Setup")]',
    '[assembly: AssemblyProduct("Tide Music")]',
    ('[assembly: AssemblyVersion("{0}")]' -f $taskWindowsVersion),
    ('[assembly: AssemblyFileVersion("{0}")]' -f $taskWindowsVersion),
    ('[assembly: AssemblyInformationalVersion("{0}")]' -f $taskVersion)
), [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText($taskVersionFile, $taskVersion, [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText($taskDigest, (Get-FileHash -LiteralPath $taskPackage -Algorithm SHA256).Hash.ToLowerInvariant(), [Text.UTF8Encoding]::new($false))
try {
    & $taskCompiler /nologo /target:winexe /platform:x64 /optimize+ /debug- /warnaserror+ `
        "/out:$taskOutput" "/win32icon:$(Join-Path $taskRoot 'ui/assets/tide.ico')" `
        "/win32manifest:$(Join-Path $taskRoot 'installer/app.manifest')" `
        /reference:System.Windows.Forms.dll /reference:System.Drawing.dll /reference:System.IO.Compression.dll /reference:System.IO.Compression.FileSystem.dll `
        "/resource:$taskPackage,Tide.Payload" "/resource:$taskDigest,Tide.Payload.sha256" `
        "/resource:$taskVersionFile,Tide.Version" `
        (Join-Path $taskRoot 'installer/TideSetup.cs') $taskAssemblyInfo
    if ($LASTEXITCODE -ne 0) { throw 'Installer compilation failed.' }
    $taskFileInfo = (Get-Item -LiteralPath $taskOutput).VersionInfo
    if ($taskFileInfo.FileVersion -ne $taskWindowsVersion -or $taskFileInfo.ProductVersion -ne $taskVersion) { throw 'Setup file properties do not match the release version.' }
    $taskVerify = Start-Process -FilePath $taskOutput -ArgumentList '--verify-payload' -WindowStyle Hidden -Wait -PassThru
    if ($taskVerify.ExitCode -ne 0) { throw 'Embedded payload verification failed.' }
    Write-Output "Installer built and payload verified: $taskOutput"
} finally { Remove-Item -LiteralPath $taskDigest -Force; Remove-Item -LiteralPath $taskVersionFile -Force }
