param()
$ErrorActionPreference = 'Stop'
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    & powershell -NoProfile -File (Join-Path $PSScriptRoot 'cargo-local.ps1') build --package grove-music --locked
    if ($LASTEXITCODE -ne 0) { throw 'Grove build failed.' }
    & (Join-Path (Get-Location) 'target\debug\grove-music.exe')
    if ($LASTEXITCODE -ne 0) { throw 'Grove exited with an error.' }
} finally {
    Pop-Location
}
