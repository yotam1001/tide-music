param([Parameter(ValueFromRemainingArguments = $true)][string[]]$CargoArgs)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskCargo = Join-Path $taskRoot '.tools\cargo\bin\cargo.exe'
. (Join-Path $PSScriptRoot 'build-budget.ps1')
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
} else {
    $taskCargo = 'cargo'
}
# Formatting/inspection and explicit developer cleanup do not create build output.
$taskReadOnly = @('fmt','metadata','locate-project','help','version','--version','-V','tree','search','clean')
$taskBuild = $CargoArgs.Count -eq 0 -or $CargoArgs[0] -notin $taskReadOnly
if ($taskBuild) {
    # Inspect the same workspace/configuration that the requested command uses.
    $taskInspectArgs = @('metadata','--no-deps','--format-version','1')
    $taskExplicitTarget = $null
    for ($i = 0; $i -lt $CargoArgs.Count; $i++) {
        $arg = $CargoArgs[$i]
        if ($arg -eq '--') { break }
        if ($arg -in @('--manifest-path','--config','--target-dir')) {
            if (++$i -ge $CargoArgs.Count) { throw "Missing value for $arg" }
            if ($arg -eq '--target-dir') { $taskExplicitTarget = $CargoArgs[$i] }
            else { $taskInspectArgs += @($arg, $CargoArgs[$i]) }
        } elseif ($arg.StartsWith('--manifest-path=') -or $arg.StartsWith('--config=')) {
            $taskInspectArgs += $arg
        } elseif ($arg.StartsWith('--target-dir=')) {
            $taskExplicitTarget = $arg.Substring('--target-dir='.Length)
        }
    }
    $taskMetadata = & $taskCargo @taskInspectArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $taskTarget = ($taskMetadata | ConvertFrom-Json).target_directory
    if ($null -ne $taskExplicitTarget) {
        if ([string]::IsNullOrWhiteSpace($taskExplicitTarget)) { throw 'Empty --target-dir' }
        $taskTarget = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($taskExplicitTarget)
    }
    Assert-TideBuildBudget -TargetDirectory $taskTarget
    $env:CARGO_INCREMENTAL = '0'
}
& $taskCargo @CargoArgs
$taskResult = $LASTEXITCODE
if ($taskBuild -and $taskResult -eq 0) {
    # Also catch a successful build that grew past the budget; the next build is
    # blocked until the developer reviews its generated files.
    Assert-TideBuildBudget -TargetDirectory $taskTarget
}
exit $taskResult
