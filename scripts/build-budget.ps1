# Local development guard only: it never deletes files or runs in the player.
function Assert-TideBuildBudget {
    param(
        [Parameter(Mandatory)][string]$TargetDirectory,
        [long]$MinimumFreeBytes = 5GB,
        # Tests supply a deterministic free-space value; callers normally omit it.
        [long]$AvailableBytes = -1
    )
    $target = [IO.Path]::GetFullPath($TargetDirectory)
    if ($AvailableBytes -lt 0) {
        $drive = [IO.DriveInfo]::new([IO.Path]::GetPathRoot($target))
        $AvailableBytes = $drive.AvailableFreeSpace
    }
    if ($AvailableBytes -lt $MinimumFreeBytes) {
        throw ('Tide build needs at least {0:N1} GiB free. Free disk space before compiling again; no files were deleted.' -f ($MinimumFreeBytes / 1GB))
    }
}
