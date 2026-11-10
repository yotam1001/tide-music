# Shared installer helpers. Dot sourcing this file has no side effects.
Set-StrictMode -Version Latest

function Write-GroveProgress([int]$Percent, [string]$Message) {
    if ($Percent -lt 0 -or $Percent -gt 100 -or $Message -match '[|\r\n]') { throw 'Invalid setup progress event.' }
    Write-Output "GROVE_PROGRESS|$Percent|$Message"
}

function Get-GroveUninstallEntry {
    $keyPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\GroveMusic'
    if (!(Test-Path -LiteralPath $keyPath)) { return $null }
    $key = Get-Item -LiteralPath $keyPath
    $values = @{}
    foreach ($name in $key.GetValueNames()) { $values[$name] = $key.GetValue($name) }
    if ($key.SubKeyCount -ne 0) { $values['UnexpectedChildKeys'] = $true }
    return $values
}
function New-GroveUninstallEntry {
    $keyPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\GroveMusic'
    $missingParents = [Collections.Generic.List[string]]::new()
    $parent = Split-Path -Parent $keyPath
    while (!(Test-Path -LiteralPath $parent)) {
        if ($parent -eq 'HKCU:\Software') { throw 'Current-user software registry is unavailable.' }
        $missingParents.Add($parent); $parent = Split-Path -Parent $parent
    }
    for ($index = $missingParents.Count - 1; $index -ge 0; $index--) {
        if (!(Test-Path -LiteralPath $missingParents[$index])) {
            try { New-Item -Path $missingParents[$index] | Out-Null }
            catch { if (!(Test-Path -LiteralPath $missingParents[$index])) { throw } }
        }
    }
    # No -Force: a leaf claimed by another installation is never replaced.
    New-Item -Path $keyPath | Out-Null
}
function Write-GroveUninstallValue([string]$Name, $Value) {
    $kind = if ($Name -in @('NoModify','NoRepair')) { 'DWord' } else { 'String' }
    New-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\GroveMusic' -Name $Name -Value $Value -PropertyType $kind -Force | Out-Null
}
function Remove-GroveUninstallValue([string]$Name) {
    Remove-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\GroveMusic' -Name $Name
}
function Remove-GroveUninstallEntryKey {
    Remove-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\GroveMusic' -Force
}
function Test-GroveUninstallEntryMatch($Actual, $Expected) {
    if ($null -eq $Expected) { return $null -eq $Actual }
    if ($null -eq $Actual -or $Actual.Count -ne $Expected.Count) { return $false }
    foreach ($name in $Expected.Keys) {
        if (!$Actual.ContainsKey($name) -or $Actual[$name] -ne $Expected[$name]) { return $false }
    }
    return $true
}
function Get-GroveUninstallRegistration([string]$Release, [string]$InstallRoot, [string]$Version) {
    $links = Get-GroveShortcuts $Release $InstallRoot
    return @{
        GroveProduct='org.grovemusic.windows.beta'
        DisplayName='Grove'; DisplayVersion=$Version; Publisher='Grove contributors'
        DisplayIcon=(Join-Path $Release 'grove-music.exe')
        InstallLocation=$InstallRoot
        UninstallString=('"{0}" {1}' -f $links['Uninstall Grove.lnk'].Target, $links['Uninstall Grove.lnk'].Arguments)
        NoModify=1; NoRepair=1
    }
}
function Set-GroveUninstallRegistration($Expected, $Value) {
    if (!(Test-GroveUninstallEntryMatch (Get-GroveUninstallEntry) $Expected)) { throw 'A different Windows uninstall entry exists; it was left unchanged.' }
    if ($null -eq $Value) {
        if ($null -ne $Expected) { Remove-GroveUninstallEntryKey }
        return
    }
    $working = @{}; $created = $false
    if ($null -ne $Expected) { foreach ($name in $Expected.Keys) { $working[$name] = $Expected[$name] } }
    try {
        if ($null -eq $Expected) { New-GroveUninstallEntry; $created = $true }
        foreach ($name in @($working.Keys)) {
            if (!$Value.ContainsKey($name)) {
                if (!(Test-GroveUninstallEntryMatch (Get-GroveUninstallEntry) $working)) { throw 'Windows uninstall registration changed during setup.' }
                Remove-GroveUninstallValue $name; $working.Remove($name)
            }
        }
        foreach ($name in $Value.Keys) {
            if (!(Test-GroveUninstallEntryMatch (Get-GroveUninstallEntry) $working)) { throw 'Windows uninstall registration changed during setup.' }
            Write-GroveUninstallValue $name $Value[$name]
            $working[$name] = $Value[$name]
        }
    } catch {
        # Restore only the exact partial snapshot written by this attempt.
        if (($created -or $null -ne $Expected) -and (Test-GroveUninstallEntryMatch (Get-GroveUninstallEntry) $working)) {
            try {
                if ($created) { Remove-GroveUninstallEntryKey }
                else {
                    foreach ($name in @($working.Keys)) { if (!$Expected.ContainsKey($name)) { Remove-GroveUninstallValue $name } }
                    foreach ($name in $Expected.Keys) { Write-GroveUninstallValue $name $Expected[$name] }
                }
            } catch { Write-Warning 'Windows uninstall registration could not be restored; retry setup.' }
        }
        throw
    }
}

function Get-GroveDownload([string]$Uri, [string]$Sha256, [string]$Output) {
    if ($Uri -notmatch '^https://' -or $Sha256 -notmatch '^[a-fA-F0-9]{64}$') { throw 'A download requires HTTPS and a SHA-256 checksum.' }
    Assert-GrovePlainPath $Output
    if (Test-Path -LiteralPath $Output) { throw 'Refusing to overwrite an existing download.' }
    Invoke-WebRequest -UseBasicParsing -Uri $Uri -OutFile $Output -TimeoutSec 300
    if ((Get-FileHash -LiteralPath $Output -Algorithm SHA256).Hash -ne $Sha256) { throw 'Dependency download checksum failed.' }
}
function Expand-GroveArchive([string]$Archive, [string]$Destination) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    Assert-GrovePlainPath $Destination
    $zip = [IO.Compression.ZipFile]::OpenRead($Archive)
    try {
        $targets = @{}
        foreach ($entry in $zip.Entries) {
            $relative = $entry.FullName.TrimEnd('/')
            if (!$relative) { continue }
            $target = Get-GroveChild $Destination $relative
            if ($targets.ContainsKey($target) -or (Test-Path -LiteralPath $target)) { throw 'Archive has duplicate or existing paths.' }
            $targets[$target] = $true
        }
        foreach ($entry in $zip.Entries) {
            $relative = $entry.FullName.TrimEnd('/')
            if (!$relative) { continue }
            $target = Get-GroveChild $Destination $relative
            if ($entry.FullName.EndsWith('/')) { [IO.Directory]::CreateDirectory($target) | Out-Null }
            else {
                [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target)) | Out-Null
                [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $false)
            }
        }
    } finally { $zip.Dispose() }
}

function Assert-GrovePlainPath([string]$Path) {
    $ancestor = [IO.Path]::GetFullPath($Path)
    while ($ancestor) {
        if (Test-Path -LiteralPath $ancestor) {
            if ((Get-Item -LiteralPath $ancestor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw 'Installation paths must not contain symbolic links or junctions.'
            }
        }
        $ancestor = [IO.Path]::GetDirectoryName($ancestor)
    }
}
function Get-GroveChild([string]$Root, [string]$Relative) {
    if ($Relative -notmatch '^[A-Za-z0-9_. /-]+$' -or $Relative -match '(^|/)\.\.(/|$)' -or [IO.Path]::IsPathRooted($Relative)) {
        throw 'Invalid installation-relative path.'
    }
    $result = [IO.Path]::GetFullPath((Join-Path $Root $Relative))
    if (!$result.StartsWith($Root.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Path escapes installation.' }
    Assert-GrovePlainPath $result
    return $result
}
function Write-GroveJson([string]$Path, $Value) {
    [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
}
function Read-GroveJson([string]$Path) {
    Assert-GrovePlainPath $Path
    if ((Get-Item -LiteralPath $Path).Length -gt 4194304) { throw 'Installation record is too large.' }
    return Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}
function Get-GrovePayloadNames([switch]$BrowserHost) {
    return @('grove-music.exe','LICENSE','THIRD_PARTY_NOTICES.md','DEPENDENCIES.md','START_HERE.md',
        'Uninstall Grove.cmd','docs/BETA.md','scripts/install-grove.ps1','scripts/grove-install-common.ps1')
}
function Get-GroveVerifiedPayload([string]$PackageRoot) {
    Assert-GrovePlainPath $PackageRoot
    $manifest = Get-GroveChild $PackageRoot 'SHA256SUMS.txt'
    $expected = @(Get-GrovePayloadNames -BrowserHost:(Test-Path -LiteralPath (Join-Path $PackageRoot 'grove-session-host.exe')))
    $seen = @{}
    foreach ($line in Get-Content -LiteralPath $manifest) {
        if ($line -notmatch '^([a-f0-9]{64})  (.+)$') { throw 'Invalid package checksum manifest.' }
        $hash = $Matches[1]; $name = $Matches[2]
        if ($name -notin $expected -or $seen.ContainsKey($name)) { throw 'Unexpected or duplicate package entry.' }
        $path = Get-GroveChild $PackageRoot $name
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $hash) { throw "Package checksum failed: $name" }
        $seen[$name] = $true
    }
    if ($seen.Count -ne $expected.Count) { throw 'The package is incomplete.' }
    return $expected
}
function Get-GroveOwnedProcesses([string]$InstallRoot) {
    # Only process identity/path is consulted, never command lines or profiles.
    return @(Get-Process | Where-Object {
        try { $_.Path -and $_.Path.StartsWith($InstallRoot.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase) }
        catch { $false }
    })
}
function Assert-GroveStopped([string]$InstallRoot) {
    if (@(Get-GroveOwnedProcesses $InstallRoot).Count) {
        throw 'Close Grove, then run setup again. Running processes were left untouched.'
    }
}
function Get-GroveShortcut([string]$Path) {
    if (!(Test-Path -LiteralPath $Path)) { return $null }
    Assert-GrovePlainPath $Path
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($Path)
    return @{ Target=$link.TargetPath; Arguments=$link.Arguments; WorkingDirectory=$link.WorkingDirectory; Icon=$link.IconLocation }
}
function Set-GroveShortcut([string]$Path, $Value) {
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($Path)
    $link.TargetPath = $Value.Target; $link.Arguments = $Value.Arguments
    $link.WorkingDirectory = $Value.WorkingDirectory; $link.IconLocation = $Value.Icon
    $link.Save()
}
function Get-GroveShortcuts([string]$Release, [string]$InstallRoot) {
    $powershell = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'
    return @{
        'Grove.lnk' = @{Target=(Join-Path $Release 'grove-music.exe'); Arguments=''; WorkingDirectory=$Release; Icon=(Join-Path $Release 'grove-music.exe')}
        'Uninstall Grove.lnk' = @{Target=$powershell; Arguments=('-NoProfile -ExecutionPolicy Bypass -File "{0}" -Mode Uninstall -InstallRoot "{1}"' -f (Join-Path $Release 'scripts/install-grove.ps1'), $InstallRoot); WorkingDirectory=$InstallRoot; Icon=(Join-Path $Release 'grove-music.exe')}
    }
}
function Test-GroveShortcutMatch($Actual, $Expected) {
    return $null -ne $Actual -and $Actual.Target -eq $Expected.Target -and $Actual.Arguments -eq $Expected.Arguments
}
function Get-GroveInstallState([string]$InstallRoot) {
    $record = Join-Path $InstallRoot '.grove-install.json'
    Assert-GrovePlainPath $InstallRoot
    if (!(Test-Path -LiteralPath $InstallRoot)) { return $null }
    if (!(Test-Path -LiteralPath $record)) { throw 'This directory is not an owned Grove installation; it was left unchanged.' }
    $state = Read-GroveJson $record
    if ($state.Product -ne 'org.grovemusic.windows.beta' -or $state.Root -ne $InstallRoot -or $state.Release -notmatch '^[a-f0-9]{32}$' -or
        $state.Browser -notin @('None','Firefox','Chrome','Edge') -or ($state.ExtensionId -and $state.ExtensionId -notmatch '^[a-p]{32}$') -or
        ($state.PSObject.Properties['AppVersion'] -and $state.AppVersion -notmatch '^[0-9][A-Za-z0-9.+-]{0,63}$')) { throw 'Invalid Grove installation record.' }
    return $state
}
function Invoke-GroveBrowserSetup([string]$Release, [string]$Mode, [string]$Browser, [string]$ExtensionId) {
    if ($Browser -ne 'None') { throw 'Grove uses browser OAuth and requires no browser extension.' }
}
function Save-GroveReleaseOwnership([string]$Release) {
    Assert-GrovePlainPath $Release
    $files = @(Get-ChildItem -LiteralPath $Release -File -Recurse -Force | Where-Object { $_.FullName -ne (Join-Path $Release '.grove-release.json') } | ForEach-Object {
        Assert-GrovePlainPath $_.FullName
        $_.FullName.Substring($Release.Length + 1).Replace('\','/')
    })
    Write-GroveJson (Join-Path $Release '.grove-release.json') @{Product='org.grovemusic.windows.beta'; Files=$files}
}
function Get-GroveOwnedReleasePaths([string]$Release) {
    $record = Get-GroveChild $Release '.grove-release.json'
    $state = Read-GroveJson $record
    if ($state.Product -ne 'org.grovemusic.windows.beta' -or @($state.Files).Count -eq 0) { throw 'Invalid owned release.' }
    $seen = @{}
    return @($state.Files | ForEach-Object {
        $path = Get-GroveChild $Release $_
        if ($path -eq $record -or $seen.ContainsKey($path)) { throw 'Invalid or duplicate owned file.' }
        $seen[$path] = $true
        $path
    })
}
function Remove-GroveOwnedRelease([string]$Release) {
    $record = Get-GroveChild $Release '.grove-release.json'
    $directories = @{}
    # Preflight every path before removal; never recursively remove a tree.
    $paths = @(Get-GroveOwnedReleasePaths $Release)
    foreach ($path in $paths) {
        $parent = [IO.Path]::GetDirectoryName($path)
        while ($parent -ne $Release) { $directories[$parent] = $true; $parent = [IO.Path]::GetDirectoryName($parent) }
    }
    foreach ($path in $paths) { if (Test-Path -LiteralPath $path -PathType Leaf) { Remove-Item -LiteralPath $path -Force } }
    Remove-Item -LiteralPath $record -Force
    foreach ($directory in ($directories.Keys | Sort-Object Length -Descending)) {
        if ((Test-Path -LiteralPath $directory) -and @(Get-ChildItem -LiteralPath $directory -Force).Count -eq 0) { Remove-Item -LiteralPath $directory }
    }
    if (@(Get-ChildItem -LiteralPath $Release -Force).Count -eq 0) { Remove-Item -LiteralPath $Release }
    else { Write-Warning 'Additional files in an old release were preserved.' }
}
function Invoke-GroveDependencies([string]$Release) {
    Write-GroveProgress 20 'Downloading verified local audio support'
    $archive = Get-GroveChild $Release 'mpv-runtime.zip'
    Get-GroveDownload 'https://github.com/mpv-player/mpv/releases/download/v0.41.0/mpv-v0.41.0-x86_64-pc-windows-msvc.zip' '4e197f729f5071c6772f35fffd96e0f36e3e8a044bd9479b136bb09b7c6a80ff' $archive
    $expanded = Get-GroveChild $Release 'runtime-source'
    Expand-GroveArchive $archive $expanded
    $mpv = @(Get-ChildItem -LiteralPath $expanded -Recurse -File -Filter mpv.exe)
    if ($mpv.Count -ne 1) { throw 'Audio runtime archive must contain one mpv.exe.' }
    $tools = Get-GroveChild $Release 'tools'
    [IO.Directory]::CreateDirectory($tools) | Out-Null
    foreach ($file in Get-ChildItem -LiteralPath $mpv[0].DirectoryName -Recurse -File) {
        $relative = $file.FullName.Substring($mpv[0].DirectoryName.Length + 1).Replace('\','/')
        $output = Get-GroveChild $tools $relative
        [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($output)) | Out-Null
        [IO.File]::Copy($file.FullName,$output,$false)
    }
    Write-GroveProgress 65 'Audio support is ready'
}
function Install-Grove([string]$PackageRoot, [string]$InstallRoot, [string]$ShortcutRoot, [string]$Browser, [string]$ExtensionId, [string]$AppVersion = '0.1.0-beta.1') {
    if ($AppVersion -notmatch '^[0-9][A-Za-z0-9.+-]{0,63}$') { throw 'Invalid application version.' }
    Write-GroveProgress 5 'Checking the setup package'
    $payload = @(Get-GroveVerifiedPayload $PackageRoot)
    if ($Browser -notin @('None','Firefox','Chrome','Edge')) { throw 'Select a supported browser.' }
    if ($Browser -ne 'None' -and 'grove-session-host.exe' -notin $payload) { throw 'This package does not contain the browser helper.' }
    if ($Browser -in @('Chrome','Edge') -and $ExtensionId -notmatch '^[a-p]{32}$') { throw 'Enter the exact ID displayed by the installed browser extension.' }
    $old = Get-GroveInstallState $InstallRoot
    Assert-GrovePlainPath $ShortcutRoot
    Assert-GroveStopped $InstallRoot
    $oldRelease = $null; $oldLinks = @{}
    if ($old) {
        $oldRelease = Get-GroveChild $InstallRoot ('releases/' + $old.Release)
        $oldLinks = Get-GroveShortcuts $oldRelease $InstallRoot
        if ($Browser -eq 'None') { $Browser = $old.Browser; $ExtensionId = $old.ExtensionId }
    }
    $oldUninstall = Get-GroveUninstallEntry
    if ($null -ne $oldUninstall) {
        $oldVersion = if ($old -and $old.PSObject.Properties['AppVersion']) { $old.AppVersion } else { '0.1.0-beta.1' }
        if (!$old -or !(Test-GroveUninstallEntryMatch $oldUninstall (Get-GroveUninstallRegistration $oldRelease $InstallRoot $oldVersion))) {
            throw 'A different Windows uninstall entry exists; it was left unchanged.'
        }
    }
    foreach ($name in @('Grove.lnk','Uninstall Grove.lnk')) {
        $actual = Get-GroveShortcut (Join-Path $ShortcutRoot $name)
        if ($actual -and (!$old -or !(Test-GroveShortcutMatch $actual $oldLinks[$name]))) { throw 'A different Start menu shortcut exists; it was left unchanged.' }
    }
    $id = [Guid]::NewGuid().ToString('N')
    $release = Get-GroveChild $InstallRoot ('releases/' + $id)
    # Serialise install/update/removal. Root creation never replaces existing files.
    [IO.Directory]::CreateDirectory($InstallRoot) | Out-Null
    $lock = [IO.File]::Open((Join-Path $InstallRoot '.grove-install.lock'), 'OpenOrCreate', 'ReadWrite', 'None')
    $oldUnregistered = $false; $newRegistered = $false; $committed = $false; $touchedLinks = @(); $pendingWritten = $false; $uninstallChanged = $false
    try {
        # Recheck after locking, including the active revision.
        if ($old) {
            $current = Get-GroveInstallState $InstallRoot
            if (!$current -or $current.Release -ne $old.Release) { throw 'Installation changed; retry setup.' }
        } elseif (@(Get-ChildItem -LiteralPath $InstallRoot -Force | Where-Object Name -NE '.grove-install.lock').Count) {
            throw 'Installation changed; retry setup.'
        }
        Write-GroveProgress 15 'Preparing a fresh installation'
        [IO.Directory]::CreateDirectory($release) | Out-Null
        foreach ($name in $payload) {
            $destination = Get-GroveChild $release $name
            [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination)) | Out-Null
            [IO.File]::Copy((Get-GroveChild $PackageRoot $name), $destination, $false)
        }
        [IO.File]::Copy((Join-Path $PackageRoot 'SHA256SUMS.txt'), (Join-Path $release 'SHA256SUMS.txt'), $false)
        Get-GroveVerifiedPayload $release | Out-Null
        Invoke-GroveDependencies $release
        Write-GroveProgress 75 'Finishing app setup'
        Invoke-GroveBrowserSetup $release 'Prepare' $Browser $ExtensionId
        Assert-GroveStopped $InstallRoot
        if ($old -and $old.Browser -ne 'None') {
            Invoke-GroveBrowserSetup $oldRelease 'Unregister' $old.Browser $old.ExtensionId
            $oldUnregistered = $true
        }
        Invoke-GroveBrowserSetup $release 'Register' $Browser $ExtensionId
        $newRegistered = $Browser -ne 'None'
        $links = Get-GroveShortcuts $release $InstallRoot
        [IO.Directory]::CreateDirectory($ShortcutRoot) | Out-Null
        foreach ($name in $links.Keys) {
            $path = Join-Path $ShortcutRoot $name
            $actual = Get-GroveShortcut $path
            if ($actual -and (!$old -or !(Test-GroveShortcutMatch $actual $oldLinks[$name]))) { throw 'Start menu changed during setup; retry.' }
            $touchedLinks += $name
            Set-GroveShortcut $path $links[$name]
        }
        Write-GroveProgress 90 'Adding Start menu and Windows uninstall entries'
        $newUninstall = Get-GroveUninstallRegistration $release $InstallRoot $AppVersion
        Set-GroveUninstallRegistration $oldUninstall $newUninstall
        $uninstallChanged = $true
        Save-GroveReleaseOwnership $release
        $next = @{Product='org.grovemusic.windows.beta'; Root=$InstallRoot; Release=$id; Browser=$Browser; ExtensionId=$ExtensionId; AppVersion=$AppVersion}
        $pending = Join-Path $InstallRoot '.grove-install.pending.json'
        $backup = Join-Path $InstallRoot '.grove-install.previous.json'
        if ((Test-Path -LiteralPath $pending) -or (Test-Path -LiteralPath $backup)) { throw 'An interrupted installation record exists; it was preserved for review.' }
        Write-GroveJson $pending $next
        $pendingWritten = $true
        $record = Join-Path $InstallRoot '.grove-install.json'
        if ($old) {
            # PowerShell coerces a null string argument to an empty path on some
            # runtimes. Use a confined explicit backup for the atomic replace.
            [IO.File]::Replace($pending, $record, $backup)
        } else { [IO.File]::Move($pending, $record) }
        $committed = $true
        if ($old) { Remove-Item -LiteralPath $backup -Force -ErrorAction Continue }
        if ($old) {
            try { Remove-GroveOwnedRelease $oldRelease }
            catch { Write-Warning 'Update succeeded; some previous owned files remain for a later removal.' }
        }
        Write-GroveProgress 100 'Grove is ready to open'
        Write-Output ('GROVE_RESULT|' + (Join-Path $release 'grove-music.exe'))
    } catch {
        if (!$committed) {
            if ($uninstallChanged) { Set-GroveUninstallRegistration $newUninstall $oldUninstall }
            if ($pendingWritten -and (Test-Path -LiteralPath $pending)) { Remove-Item -LiteralPath $pending -Force }
            foreach ($name in $touchedLinks) {
                $path = Join-Path $ShortcutRoot $name
                $actual = Get-GroveShortcut $path
                if (!$actual -or (Test-GroveShortcutMatch $actual $links[$name])) {
                    if ($old) { Set-GroveShortcut $path $oldLinks[$name] }
                    elseif ($actual) { Remove-Item -LiteralPath $path -Force }
                }
            }
            if ($newRegistered) { Invoke-GroveBrowserSetup $release 'Unregister' $Browser $ExtensionId }
            if ($oldUnregistered) { Invoke-GroveBrowserSetup $oldRelease 'Register' $old.Browser $old.ExtensionId }
            # Only files from the fresh, unexposed staging directory are owned.
            if (Test-Path -LiteralPath $release) { Save-GroveReleaseOwnership $release; Remove-GroveOwnedRelease $release }
        }
        throw
    } finally {
        $lock.Dispose()
        Remove-Item -LiteralPath (Join-Path $InstallRoot '.grove-install.lock') -Force
        if (!$old -and !$committed) {
            foreach ($directory in @((Join-Path $InstallRoot 'releases'), $InstallRoot)) {
                if ((Test-Path -LiteralPath $directory) -and @(Get-ChildItem -LiteralPath $directory -Force).Count -eq 0) { Remove-Item -LiteralPath $directory }
            }
        }
    }
}
function Uninstall-Grove([string]$InstallRoot, [string]$ShortcutRoot) {
    $state = Get-GroveInstallState $InstallRoot
    if (!$state) { Write-Output 'Grove is not installed in this location.'; return }
    Assert-GroveStopped $InstallRoot
    Assert-GrovePlainPath $ShortcutRoot
    $release = Get-GroveChild $InstallRoot ('releases/' + $state.Release)
    $links = Get-GroveShortcuts $release $InstallRoot
    $version = if ($state.PSObject.Properties['AppVersion']) { $state.AppVersion } else { '0.1.0-beta.1' }
    $uninstall = Get-GroveUninstallEntry
    if ($null -ne $uninstall -and !(Test-GroveUninstallEntryMatch $uninstall (Get-GroveUninstallRegistration $release $InstallRoot $version))) {
        throw 'The Windows uninstall entry changed; it was left unchanged.'
    }
    $lock = [IO.File]::Open((Join-Path $InstallRoot '.grove-install.lock'), 'OpenOrCreate', 'ReadWrite', 'None')
    try {
        $current = Get-GroveInstallState $InstallRoot
        if ($current.Release -ne $state.Release) { throw 'Installation changed; retry removal.' }
        Assert-GroveStopped $InstallRoot
        $ownedReleases = @(Get-ChildItem -LiteralPath (Join-Path $InstallRoot 'releases') -Directory | Where-Object {
            $_.Name -match '^[a-f0-9]{32}$' -and (Test-Path -LiteralPath (Join-Path $_.FullName '.grove-release.json'))
        })
        foreach ($item in $ownedReleases) { Get-GroveOwnedReleasePaths $item.FullName | Out-Null }
        if (!(Test-GroveUninstallEntryMatch (Get-GroveUninstallEntry) $uninstall)) { throw 'Windows uninstall registration changed during removal.' }
        Invoke-GroveBrowserSetup $release 'Unregister' $state.Browser $state.ExtensionId
        Set-GroveUninstallRegistration $uninstall $null
        foreach ($name in $links.Keys) {
            $path = Join-Path $ShortcutRoot $name
            if (Test-GroveShortcutMatch (Get-GroveShortcut $path) $links[$name]) { Remove-Item -LiteralPath $path -Force }
        }
        # Previous failed cleanup releases are still strictly confined by ID and their ownership records.
        foreach ($item in $ownedReleases) { Remove-GroveOwnedRelease $item.FullName }
        Remove-Item -LiteralPath (Join-Path $InstallRoot '.grove-install.json') -Force
    } finally {
        $lock.Dispose()
        Remove-Item -LiteralPath (Join-Path $InstallRoot '.grove-install.lock') -Force
    }
    foreach ($directory in @((Join-Path $InstallRoot 'releases'), $InstallRoot, $ShortcutRoot)) {
        if ((Test-Path -LiteralPath $directory) -and @(Get-ChildItem -LiteralPath $directory -Force).Count -eq 0) { Remove-Item -LiteralPath $directory }
    }
    Write-Output 'Grove removed. Saved preferences and Spotify approvals were preserved; Disconnect in Grove removes its saved approvals.'
}
