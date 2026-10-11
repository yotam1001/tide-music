# Build distributable executables without embedding private development paths.
$ErrorActionPreference = 'Stop'
$taskRoot = (Resolve-Path -LiteralPath (Split-Path -Parent $PSScriptRoot)).Path
$taskPreviousFlags = $env:CARGO_ENCODED_RUSTFLAGS
$taskLocalCargo = Test-Path -LiteralPath (Join-Path $taskRoot '.tools/cargo/bin/cargo.exe')
$taskFlags = @()
# Development-local toolchain/cache paths are already inside the workspace.
# A standard installation may put Cargo's cache elsewhere in the user profile.
if (-not $taskLocalCargo -and $env:USERPROFILE) {
    $taskFlags += "--remap-path-prefix=$env:USERPROFILE=/user"
}
$taskFlags += "--remap-path-prefix=$taskRoot=."
if ($taskLocalCargo) {
    $taskFlags += '-Clink-self-contained=yes'
}
try {
    # Unit separators preserve paths containing spaces as single rustc arguments.
    $env:CARGO_ENCODED_RUSTFLAGS = $taskFlags -join [char]31
    Push-Location $taskRoot
    try {
        & (Join-Path $PSScriptRoot 'cargo-local.ps1') -CargoArgs @('build', '--release', '-p', 'tide-music', '--locked')
        if ($LASTEXITCODE -ne 0) { throw 'Preview build failed.' }
    } finally { Pop-Location }
} finally {
    $env:CARGO_ENCODED_RUSTFLAGS = $taskPreviousFlags
}
