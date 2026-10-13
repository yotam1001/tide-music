param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [int]$Seconds = 30,
    [string[]]$AppArguments = @(),
    [string]$Output = '.local/resource-samples.json',
    [switch]$RequireSuccessfulExit
)
$ErrorActionPreference = 'Stop'
$taskExe = (Resolve-Path -LiteralPath $Executable).Path
$taskParent = Split-Path -Parent $Output
if ($taskParent) { New-Item -ItemType Directory -Force -Path $taskParent | Out-Null }
$taskStart = @{ FilePath = $taskExe; PassThru = $true; WindowStyle = 'Hidden' }
if ($RequireSuccessfulExit) {
    $taskStart.RedirectStandardOutput = "$Output.stdout.log"
    $taskStart.RedirectStandardError = "$Output.stderr.log"
}
if ($AppArguments.Count -gt 0) {
    # Start-Process joins an argument array without escaping it. Encode each
    # argument for Windows command-line parsing, including spaces and quotes.
    $taskQuotedArguments = foreach ($taskArgument in $AppArguments) {
        $taskEscaped = [regex]::Replace($taskArgument, '(\\*)"', '$1$1\"')
        $taskEscaped = [regex]::Replace($taskEscaped, '(\\+)$', '$1$1')
        '"' + $taskEscaped + '"'
    }
    $taskStart.ArgumentList = $taskQuotedArguments -join ' '
}
$taskProcess = Start-Process @taskStart
# Retain the native handle while the child is alive, so its exit code remains
# available after Refresh observes exit. Record it independently of console output.
$null = $taskProcess.Handle
$taskStage = 'sampling'
try {
    $taskRows = [Collections.Generic.List[object]]::new()
    $taskCpuCounters = @{}
    $taskPreviousTime = Get-Date
    $taskObserved = @{}
    $taskSamplingRaces = 0
    try {
        for ($taskIndex = 0; $taskIndex -lt $Seconds; $taskIndex++) {
            Start-Sleep -Seconds 1
            $taskProcess.Refresh()
            if ($taskProcess.HasExited) { break }
            $taskInventory = Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId
            $taskIds = [Collections.Generic.HashSet[int]]::new()
            [void]$taskIds.Add($taskProcess.Id)
            do {
                $taskAdded = $false
                foreach ($taskItem in $taskInventory) {
                    if ($taskIds.Contains([int]$taskItem.ParentProcessId) -and $taskIds.Add([int]$taskItem.ProcessId)) { $taskAdded = $true }
                }
            } while ($taskAdded)
            $taskPrivate = 0L; $taskWorking = 0L; $taskCpu = 0.0
            foreach ($taskId in $taskIds) {
                $taskChild = Get-Process -Id $taskId -ErrorAction SilentlyContinue
                if ($taskChild) {
                    $taskCounterKey = $null
                    try {
                        # Hold each observed instance, rather than reopening its PID
                        # after exit (the PID may already identify another process).
                        $null = $taskChild.Handle
                        if ($taskChild.HasExited) { $taskChild.Dispose(); continue }
                        $taskStarted = $taskChild.StartTime.Ticks
                        $taskName = $taskChild.ProcessName
                        $taskCounterKey = "$taskId/$taskStarted"
                        if (!$taskObserved.ContainsKey($taskCounterKey)) {
                            $taskObserved[$taskCounterKey] = @{process=$taskChild;process_id=$taskId;start_ticks=$taskStarted;name=$taskName}
                        } else {
                            $taskChild.Dispose()
                            $taskChild = $taskObserved[$taskCounterKey].process
                            $taskChild.Refresh()
                        }
                        $taskPrivate += $taskChild.PrivateMemorySize64; $taskWorking += $taskChild.WorkingSet64
                        $taskLastCpu = if ($taskCpuCounters.ContainsKey($taskCounterKey)) { $taskCpuCounters[$taskCounterKey] } else { 0.0 }
                        $taskCpu += [Math]::Max(0.0, $taskChild.CPU - $taskLastCpu)
                        $taskCpuCounters[$taskCounterKey] = $taskChild.CPU
                    } catch {
                        # A short-lived helper may exit between enumeration and
                        # handle acquisition. Record the gap; it is not a full
                        # lifetime observation and must not become cleanup proof.
                        $taskSamplingRaces++
                        if ($taskChild -and ($null -eq $taskCounterKey -or !$taskObserved.ContainsKey($taskCounterKey))) { $taskChild.Dispose() }
                    }
                }
            }
            $taskNow = Get-Date
            $taskElapsed = ($taskNow - $taskPreviousTime).TotalSeconds
            # CPU percentage uses one logical core as the denominator.
            $taskCpuPercent = [Math]::Max(0.0, $taskCpu / $taskElapsed * 100)
            $taskRows.Add([pscustomobject]@{ second=$taskIndex+1; processes=$taskIds.Count; private_mib=[Math]::Round($taskPrivate/1MB,2); working_set_mib=[Math]::Round($taskWorking/1MB,2); cpu_one_core_percent=[Math]::Round($taskCpuPercent,3) })
            $taskPreviousTime = $taskNow
        }
    } finally {
        if (-not $taskProcess.HasExited) { $taskProcess.CloseMainWindow() | Out-Null; $taskProcess.WaitForExit(5000) | Out-Null }
        if (-not $taskProcess.HasExited) { $taskProcess.Kill(); $taskProcess.WaitForExit() }
    }
    $taskStage = 'writing_samples'
    $taskRows | ConvertTo-Json | Set-Content -LiteralPath $Output -Encoding utf8
    $taskRows | Measure-Object private_mib -Minimum -Maximum -Average | Select-Object Minimum,Maximum,Average
    if ($RequireSuccessfulExit) {
        $taskStage = 'checking_exit'
        $taskProcess.Refresh()
        @{exit_code=$taskProcess.ExitCode;helpers_ended=$false} | ConvertTo-Json | Set-Content -LiteralPath "$Output.exit.json" -Encoding utf8
        $taskStage = 'checking_helpers'
        $taskCleanupObservations = @()
        foreach ($taskEntry in $taskObserved.Values) {
            $taskCleanupObservations += @{process_id=$taskEntry.process_id;start_ticks=$taskEntry.start_ticks;name=$taskEntry.name;is_root=($taskEntry.process_id -eq $taskProcess.Id);has_exited=$taskEntry.process.HasExited}
        }
        # The root's exit is checked above. A retained Windows process object is
        # not evidence of a running helper; require a matching, non-exited child.
        $taskLiveHelpers = @($taskCleanupObservations | Where-Object { !$_.is_root -and !$_.has_exited })
        @{exit_code=$taskProcess.ExitCode;helpers_ended=($taskLiveHelpers.Count -eq 0);cleanup_observations=$taskCleanupObservations;sampling_races=$taskSamplingRaces} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath "$Output.exit.json" -Encoding utf8
        if ($taskProcess.ExitCode -ne 0) {
            $taskStage = 'checking_exit'
            throw 'Measured app did not finish its verification successfully'
        }
        if ($taskLiveHelpers.Count -gt 0) { throw 'An observed live playback helper remained after the app exited' }
        'PASS: verification exited successfully and observed helper processes ended'
    }

} catch {
    $taskFailure = $_
    try {
        @{stage=$taskStage;failure_type=$taskFailure.Exception.GetType().FullName;
          hresult=$taskFailure.Exception.HResult;script_line=$taskFailure.InvocationInfo.ScriptLineNumber} |
            ConvertTo-Json | Set-Content -LiteralPath "$Output.failure.json" -Encoding utf8
    } catch { }
    throw $taskFailure
} finally {
    foreach ($taskEntry in $taskObserved.Values) { $taskEntry.process.Dispose() }
    $taskProcess.Dispose()
}
