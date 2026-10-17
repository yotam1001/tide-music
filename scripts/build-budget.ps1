# Local development guard only: it never deletes files or runs in the player.
function Get-TideBuildOutputBytes([string]$TargetDirectory) {
    if (!(Test-Path -LiteralPath $TargetDirectory)) { return [long]0 }
    $root = Get-Item -LiteralPath $TargetDirectory -Force
    if (!$root.PSIsContainer -or ($root.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'Cannot check a linked or non-directory build output path.'
    }
    $pending = [Collections.Generic.Stack[IO.DirectoryInfo]]::new()
    $pending.Push($root)
    $bytes = [long]0
    while ($pending.Count -gt 0) {
        foreach ($entry in $pending.Pop().EnumerateFileSystemInfos()) {
            if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw 'Build output contains a linked entry; check its disk usage manually.'
            }
            if ($entry -is [IO.DirectoryInfo]) { $pending.Push($entry) }
            else { $bytes += $entry.Length }
        }
    }
    return $bytes
}

function Assert-TideBuildBudget {
    param(
        [Parameter(Mandatory)][string]$TargetDirectory,
        [long]$MaximumBytes = 8GB,
        [long]$MinimumFreeBytes = 5GB,
        # Tests supply a deterministic free-space value; callers normally omit it.
        [long]$AvailableBytes = -1
    )
    $target = [IO.Path]::GetFullPath($TargetDirectory)
    $bytes = Get-TideBuildOutputBytes $target
    if ($bytes -gt $MaximumBytes) {
        throw ('Tide build output exceeds its {0:N1} GiB budget. Review and clear generated build outputs before compiling again; no files were deleted.' -f ($MaximumBytes / 1GB))
    }
    if ($AvailableBytes -lt 0) {
        $drive = [IO.DriveInfo]::new([IO.Path]::GetPathRoot($target))
        $AvailableBytes = $drive.AvailableFreeSpace
    }
    if ($AvailableBytes -lt $MinimumFreeBytes) {
        throw ('Tide build needs at least {0:N1} GiB free. Free disk space before compiling again; no files were deleted.' -f ($MinimumFreeBytes / 1GB))
    }
}
