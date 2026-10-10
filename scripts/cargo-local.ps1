param([Parameter(ValueFromRemainingArguments = $true)][string[]]$CargoArgs)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskCargo = Join-Path $taskRoot '.tools\cargo\bin\cargo.exe'
if (Test-Path -LiteralPath $taskCargo) {
    $env:CARGO_HOME = Join-Path $taskRoot '.tools\cargo'
    $env:RUSTUP_HOME = Join-Path $taskRoot '.tools\rustup'
    $taskCompiler = Get-ChildItem (Join-Path $taskRoot '.tools\compiler') -Directory | Select-Object -First 1
    $taskCompilerBin = Join-Path $taskCompiler.FullName 'bin'
    $env:PATH = "$taskCompilerBin;$(Join-Path $taskRoot '.tools\cargo\bin');$env:PATH"
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = Join-Path $taskCompilerBin 'x86_64-w64-mingw32-gcc.exe'
    $env:CC_x86_64_pc_windows_gnu = Join-Path $taskCompilerBin 'x86_64-w64-mingw32-clang.exe'
    $env:AR_x86_64_pc_windows_gnu = Join-Path $taskCompilerBin 'llvm-ar.exe'
    # Rust's GNU distribution supplies the GCC runtime libraries used by std.
    $env:RUSTFLAGS = '-Clink-self-contained=yes'
    & $taskCargo @CargoArgs
} else {
    & cargo @CargoArgs
}
exit $LASTEXITCODE
