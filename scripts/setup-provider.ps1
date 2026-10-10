$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskVenv = Join-Path $taskRoot '.tools/provider-venv'
if (-not (Test-Path -LiteralPath (Join-Path $taskVenv 'Scripts/python.exe'))) {
    & python -m venv $taskVenv
    if ($LASTEXITCODE -ne 0) { throw 'Python 3.10+ is required to create the optional account helper environment.' }
}
& (Join-Path $taskVenv 'Scripts/python.exe') -m pip install --disable-pip-version-check -r (Join-Path $taskRoot 'helpers/requirements.txt')
if ($LASTEXITCODE -ne 0) { throw 'Could not install the account helper dependencies.' }
Write-Output 'Optional account helper is ready. It runs only during library requests.'
