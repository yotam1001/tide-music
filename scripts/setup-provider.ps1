#requires -Version 5.1
param([switch]$Managed)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'install-common.ps1')
$taskRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$taskVenv = Join-Path $taskRoot '.tools/provider-venv'
Assert-TidePlainPath $taskVenv
if (!$Managed) {
    if (!(Test-Path -LiteralPath (Join-Path $taskVenv 'Scripts/python.exe'))) {
        & python -m venv $taskVenv
        if ($LASTEXITCODE -ne 0) { throw 'Python 3.10+ is required for the development account helper. Packaged setup manages Python automatically.' }
    }
    & (Join-Path $taskVenv 'Scripts/python.exe') -m pip install --disable-pip-version-check -r (Join-Path $taskRoot 'helpers/requirements.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Could not install the account helper dependencies.' }
    Write-Output 'Optional account helper is ready.'
    return
}
if (Test-Path -LiteralPath $taskVenv) { throw 'Managed provider destination exists; refusing to replace another environment.' }
$taskDownloads = Join-Path $taskRoot ('.tools/provider-downloads-' + [Guid]::NewGuid().ToString('N'))
Assert-TidePlainPath $taskDownloads
[IO.Directory]::CreateDirectory($taskDownloads) | Out-Null
$taskPythonZip = Join-Path $taskDownloads 'python.zip'
# Official CPython Windows release index pins this embeddable x64 distribution.
# https://www.python.org/ftp/python/3.14.0/windows-3.14.0.json
Get-TideDownload 'https://www.python.org/ftp/python/3.14.0/python-3.14.0-embeddable-amd64.zip' 'd12b48967ab48e78c5bb7d7aead048c809e5004f68c6cb50811fd3961bb0f6ed' $taskPythonZip
$taskInterpreter = Join-Path $taskVenv 'Scripts'
Expand-TideArchive $taskPythonZip $taskInterpreter
# Embedded Python stays isolated: no system PATH, registry, user site packages,
# pip bootstrap or existing environment. All dependencies are pure Python wheels.
$taskPackages = Join-Path $taskInterpreter 'site-packages'
[IO.Directory]::CreateDirectory($taskPackages) | Out-Null
$taskWheelHashes = @{
    'ytmusicapi-1.12.3-py3-none-any.whl'='f65db840907a43ca23b29bd563951a1e390fbe2089d7aa4ddbe2043824b2acd9'
    'requests-2.34.2-py3-none-any.whl'='2a0d60c172f83ac6ab31e4554906c0f3b3588d37b5cb939b1c061f4907e278e0'
    'charset_normalizer-3.5.2-py3-none-any.whl'='b6b751274acb69d77b3323d6b7dbaa3c7fdfc1eb829b7eb61d262f32e1af9685'
    'idna-3.20-py3-none-any.whl'='ab7ae7122974553370f0bdb919e1a960b2cd1bc1ef0276416d896db81c14582c'
    'urllib3-2.8.0-py3-none-any.whl'='0cf3cae568d36aa9576b28dfb35f11328f1cb974ca7647d9475ebb86c75ac6e3'
    'certifi-2026.7.22-py3-none-any.whl'='62f22742b58a1a33014a2b6b706588a8d7e2a88ae7bd1a6ebe8c992928483775'
}
foreach ($taskRequirement in Get-Content -LiteralPath (Join-Path $taskRoot 'helpers/requirements.txt')) {
    if ($taskRequirement -notmatch '^([a-zA-Z0-9_-]+)==([0-9.]+)$') { throw 'Only exact pinned helper requirements are supported.' }
    $taskName = $Matches[1]; $taskVersion = $Matches[2]
    $taskMetadata = Invoke-RestMethod -Uri "https://pypi.org/pypi/$taskName/$taskVersion/json"
    $taskWheels = @($taskMetadata.urls | Where-Object { $_.packagetype -eq 'bdist_wheel' -and $_.filename -match '-(?:py3|py2\.py3)-none-any\.whl$' -and !$_.yanked })
    if ($taskWheels.Count -ne 1) { throw "Exactly one universal wheel is required for $taskName $taskVersion." }
    $taskWheel = $taskWheels[0]
    if ($taskWheel.url -notmatch '^https://files\.pythonhosted\.org/' -or $taskWheel.filename -notmatch '^[a-zA-Z0-9_.-]+\.whl$') { throw 'Unexpected wheel source.' }
    if (!$taskWheelHashes.ContainsKey($taskWheel.filename) -or $taskWheel.digests.sha256 -ne $taskWheelHashes[$taskWheel.filename]) { throw 'Helper wheel digest differs from its pinned checksum; review the dependency before updating.' }
    $taskWheelPath = Join-Path $taskDownloads $taskWheel.filename
    Get-TideDownload $taskWheel.url $taskWheelHashes[$taskWheel.filename] $taskWheelPath
    Expand-TideArchive $taskWheelPath $taskPackages
}
[IO.File]::WriteAllText((Join-Path $taskInterpreter 'python314._pth'), "python314.zip`n.`nsite-packages`n", [Text.UTF8Encoding]::new($false))
& (Join-Path $taskInterpreter 'python.exe') -I -c 'import ytmusicapi, requests, certifi; assert callable(ytmusicapi.YTMusic)'
if ($LASTEXITCODE -ne 0) { throw 'Managed Python dependency verification failed.' }
Write-Output 'Private account-helper runtime ready. No system Python installation is required.'
