param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [int]$Seconds = 30,
    [string[]]$AppArguments = @(),
    [string]$Output = '.local/resource-samples.json'
)
$ErrorActionPreference = 'Stop'
$taskExe = (Resolve-Path -LiteralPath $Executable).Path
$taskStart = @{ FilePath = $taskExe; PassThru = $true; WindowStyle = 'Hidden' }
if ($AppArguments.Count -gt 0) { $taskStart.ArgumentList = $AppArguments }
$taskProcess = Start-Process @taskStart
$taskRows = [Collections.Generic.List[object]]::new()
$taskPreviousCpu = 0.0
$taskPreviousTime = Get-Date
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
            if ($taskChild) { $taskPrivate += $taskChild.PrivateMemorySize64; $taskWorking += $taskChild.WorkingSet64; $taskCpu += $taskChild.CPU }
        }
        $taskNow = Get-Date
        $taskElapsed = ($taskNow - $taskPreviousTime).TotalSeconds
        # CPU percentage uses one logical core as the denominator.
        $taskCpuPercent = [Math]::Max(0, ($taskCpu - $taskPreviousCpu) / $taskElapsed * 100)
        $taskRows.Add([pscustomobject]@{ second=$taskIndex+1; processes=$taskIds.Count; private_mib=[Math]::Round($taskPrivate/1MB,2); working_set_mib=[Math]::Round($taskWorking/1MB,2); cpu_one_core_percent=[Math]::Round($taskCpuPercent,3) })
        $taskPreviousCpu = $taskCpu; $taskPreviousTime = $taskNow
    }
} finally {
    if (-not $taskProcess.HasExited) { $taskProcess.CloseMainWindow() | Out-Null; $taskProcess.WaitForExit(5000) | Out-Null }
    if (-not $taskProcess.HasExited) { Stop-Process -Id $taskProcess.Id }
}
$taskParent = Split-Path -Parent $Output
if ($taskParent) { New-Item -ItemType Directory -Force -Path $taskParent | Out-Null }
$taskRows | ConvertTo-Json | Set-Content -LiteralPath $Output -Encoding utf8
$taskRows | Measure-Object private_mib -Minimum -Maximum -Average | Select-Object Minimum,Maximum,Average
