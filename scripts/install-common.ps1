# Shared installer helpers. Dot sourcing this file has no side effects.
Set-StrictMode -Version Latest

function Write-TideProgress([int]$Percent, [string]$Message) {
    if ($Percent -lt 0 -or $Percent -gt 100 -or $Message -match '[|\r\n]') { throw 'Invalid setup progress event.' }
    Write-Output "TIDE_PROGRESS|$Percent|$Message"
}

function Get-TideUninstallEntry {
    $keyPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\TideMusic'
    if (!(Test-Path -LiteralPath $keyPath)) { return $null }
    $key = Get-Item -LiteralPath $keyPath
    $values = @{}
    foreach ($name in $key.GetValueNames()) { $values[$name] = $key.GetValue($name) }
    if ($key.SubKeyCount -ne 0) { $values['UnexpectedChildKeys'] = $true }
    return $values
}
function New-TideUninstallEntry {
    $keyPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\TideMusic'
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
function Write-TideUninstallValue([string]$Name, $Value) {
    $kind = if ($Name -in @('NoModify','NoRepair')) { 'DWord' } else { 'String' }
    New-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\TideMusic' -Name $Name -Value $Value -PropertyType $kind -Force | Out-Null
}
function Remove-TideUninstallValue([string]$Name) {
    Remove-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\TideMusic' -Name $Name
}
function Remove-TideUninstallEntryKey {
    Remove-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\TideMusic' -Force
}
function Test-TideUninstallEntryMatch($Actual, $Expected) {
    if ($null -eq $Expected) { return $null -eq $Actual }
    if ($null -eq $Actual -or $Actual.Count -ne $Expected.Count) { return $false }
    foreach ($name in $Expected.Keys) {
        if (!$Actual.ContainsKey($name) -or $Actual[$name] -ne $Expected[$name]) { return $false }
    }
    return $true
}
function Get-TideUninstallRegistration([string]$Release, [string]$InstallRoot, [string]$Version) {
    $links = Get-TideShortcuts $Release $InstallRoot
    return @{
        TideProduct='org.tidemusic.windows.preview'
        DisplayName='Tide'; DisplayVersion=$Version; Publisher='Tide contributors'
        DisplayIcon=(Join-Path $Release 'tide-music.exe')
        InstallLocation=$InstallRoot
        UninstallString=('"{0}" {1}' -f $links['Uninstall Tide.lnk'].Target, $links['Uninstall Tide.lnk'].Arguments)
        NoModify=1; NoRepair=1
    }
}
function Set-TideUninstallRegistration($Expected, $Value) {
    if (!(Test-TideUninstallEntryMatch (Get-TideUninstallEntry) $Expected)) { throw 'A different Windows uninstall entry exists; it was left unchanged.' }
    if ($null -eq $Value) {
        if ($null -ne $Expected) { Remove-TideUninstallEntryKey }
        return
    }
    $working = @{}; $created = $false
    if ($null -ne $Expected) { foreach ($name in $Expected.Keys) { $working[$name] = $Expected[$name] } }
    try {
        if ($null -eq $Expected) { New-TideUninstallEntry; $created = $true }
        foreach ($name in @($working.Keys)) {
            if (!$Value.ContainsKey($name)) {
                if (!(Test-TideUninstallEntryMatch (Get-TideUninstallEntry) $working)) { throw 'Windows uninstall registration changed during setup.' }
                Remove-TideUninstallValue $name; $working.Remove($name)
            }
        }
        foreach ($name in $Value.Keys) {
            if (!(Test-TideUninstallEntryMatch (Get-TideUninstallEntry) $working)) { throw 'Windows uninstall registration changed during setup.' }
            Write-TideUninstallValue $name $Value[$name]
            $working[$name] = $Value[$name]
        }
    } catch {
        # Restore only the exact partial snapshot written by this attempt.
        if (($created -or $null -ne $Expected) -and (Test-TideUninstallEntryMatch (Get-TideUninstallEntry) $working)) {
            try {
                if ($created) { Remove-TideUninstallEntryKey }
                else {
                    foreach ($name in @($working.Keys)) { if (!$Expected.ContainsKey($name)) { Remove-TideUninstallValue $name } }
                    foreach ($name in $Expected.Keys) { Write-TideUninstallValue $name $Expected[$name] }
                }
            } catch { Write-Warning 'Windows uninstall registration could not be restored; retry setup.' }
        }
        throw
    }
}

function Get-TideDownload([string]$Uri, [string]$Sha256, [string]$Output) {
    if ($Uri -notmatch '^https://' -or $Sha256 -notmatch '^[a-fA-F0-9]{64}$') { throw 'A download requires HTTPS and a SHA-256 checksum.' }
    Assert-TidePlainPath $Output
    if (Test-Path -LiteralPath $Output) { throw 'Refusing to overwrite an existing download.' }
    Invoke-WebRequest -UseBasicParsing -Uri $Uri -OutFile $Output -TimeoutSec 300
    if ((Get-FileHash -LiteralPath $Output -Algorithm SHA256).Hash -ne $Sha256) { throw 'Dependency download checksum failed.' }
}
function Expand-TideArchive([string]$Archive, [string]$Destination) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    Assert-TidePlainPath $Destination
    $zip = [IO.Compression.ZipFile]::OpenRead($Archive)
    try {
        $targets = @{}
        foreach ($entry in $zip.Entries) {
            $relative = $entry.FullName.TrimEnd('/')
            if (!$relative) { continue }
            $target = Get-TideChild $Destination $relative
            if ($targets.ContainsKey($target) -or (Test-Path -LiteralPath $target)) { throw 'Archive has duplicate or existing paths.' }
            $targets[$target] = $true
        }
        foreach ($entry in $zip.Entries) {
            $relative = $entry.FullName.TrimEnd('/')
            if (!$relative) { continue }
            $target = Get-TideChild $Destination $relative
            if ($entry.FullName.EndsWith('/')) { [IO.Directory]::CreateDirectory($target) | Out-Null }
            else {
                [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target)) | Out-Null
                [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $false)
            }
        }
    } finally { $zip.Dispose() }
}

function Assert-TidePlainPath([string]$Path) {
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
function Get-TideChild([string]$Root, [string]$Relative) {
    if ($Relative -notmatch '^[A-Za-z0-9_. /-]+$' -or $Relative -match '(^|/)\.\.(/|$)' -or [IO.Path]::IsPathRooted($Relative)) {
        throw 'Invalid installation-relative path.'
    }
    $result = [IO.Path]::GetFullPath((Join-Path $Root $Relative))
    if (!$result.StartsWith($Root.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Path escapes installation.' }
    Assert-TidePlainPath $result
    return $result
}
function Write-TideJson([string]$Path, $Value) {
    [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
}
function Read-TideJson([string]$Path) {
    Assert-TidePlainPath $Path
    if ((Get-Item -LiteralPath $Path).Length -gt 4194304) { throw 'Installation record is too large.' }
    return Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}
function Get-TidePayloadNames([switch]$BrowserHost) {
    $names = @('tide-music.exe','LICENSE','THIRD_PARTY_NOTICES.md','DEPENDENCIES.md','START_HERE.md',
        'Install Tide.cmd','Uninstall Tide.cmd','docs/ACCOUNT_SETUP.md','docs/INSTALLATION.md',
        'scripts/install-windows.ps1','scripts/install-common.ps1','scripts/setup-runtime.ps1',
        'scripts/setup-provider.ps1','helpers/youtube_music.py','helpers/requirements.txt')
    if ($BrowserHost) { $names += @('tide-session-host.exe','scripts/setup-browser-companion.ps1','docs/BROWSER_COMPANION.md') }
    return $names
}
function Get-TideVerifiedPayload([string]$PackageRoot) {
    Assert-TidePlainPath $PackageRoot
    $manifest = Get-TideChild $PackageRoot 'SHA256SUMS.txt'
    $expected = @(Get-TidePayloadNames -BrowserHost:(Test-Path -LiteralPath (Join-Path $PackageRoot 'tide-session-host.exe')))
    $seen = @{}
    foreach ($line in Get-Content -LiteralPath $manifest) {
        if ($line -notmatch '^([a-f0-9]{64})  (.+)$') { throw 'Invalid package checksum manifest.' }
        $hash = $Matches[1]; $name = $Matches[2]
        if ($name -notin $expected -or $seen.ContainsKey($name)) { throw 'Unexpected or duplicate package entry.' }
        $path = Get-TideChild $PackageRoot $name
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $hash) { throw "Package checksum failed: $name" }
        $seen[$name] = $true
    }
    if ($seen.Count -ne $expected.Count) { throw 'The package is incomplete.' }
    return $expected
}
function Get-TideOwnedProcesses([string]$InstallRoot) {
    # Only process identity/path is consulted, never command lines or profiles.
    return @(Get-Process | Where-Object {
        try { $_.Path -and $_.Path.StartsWith($InstallRoot.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase) }
        catch { $false }
    })
}
function Assert-TideStopped([string]$InstallRoot) {
    if (@(Get-TideOwnedProcesses $InstallRoot).Count) {
        throw 'Close Tide and its browser companion requests, then run setup again. Running processes were left untouched.'
    }
}
function Get-TideShortcut([string]$Path) {
    if (!(Test-Path -LiteralPath $Path)) { return $null }
    Assert-TidePlainPath $Path
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($Path)
    return @{ Target=$link.TargetPath; Arguments=$link.Arguments; WorkingDirectory=$link.WorkingDirectory; Icon=$link.IconLocation }
}
function Set-TideShortcut([string]$Path, $Value) {
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($Path)
    $link.TargetPath = $Value.Target; $link.Arguments = $Value.Arguments
    $link.WorkingDirectory = $Value.WorkingDirectory; $link.IconLocation = $Value.Icon
    $link.Save()
}
function Get-TideShortcuts([string]$Release, [string]$InstallRoot) {
    $powershell = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'
    return @{
        'Tide.lnk' = @{Target=(Join-Path $Release 'tide-music.exe'); Arguments=''; WorkingDirectory=$Release; Icon=(Join-Path $Release 'tide-music.exe')}
        'Uninstall Tide.lnk' = @{Target=$powershell; Arguments=('-NoProfile -ExecutionPolicy Bypass -File "{0}" -Mode Uninstall -InstallRoot "{1}"' -f (Join-Path $Release 'scripts/install-windows.ps1'), $InstallRoot); WorkingDirectory=$InstallRoot; Icon=(Join-Path $Release 'tide-music.exe')}
    }
}
function Test-TideShortcutMatch($Actual, $Expected) {
    return $null -ne $Actual -and $Actual.Target -eq $Expected.Target -and $Actual.Arguments -eq $Expected.Arguments
}
function Get-TideInstallState([string]$InstallRoot) {
    $record = Join-Path $InstallRoot '.tide-install.json'
    Assert-TidePlainPath $InstallRoot
    if (!(Test-Path -LiteralPath $InstallRoot)) { return $null }
    if (!(Test-Path -LiteralPath $record)) { throw 'This directory is not an owned Tide installation; it was left unchanged.' }
    $state = Read-TideJson $record
    if ($state.Product -ne 'org.tidemusic.windows.preview' -or $state.Root -ne $InstallRoot -or $state.Release -notmatch '^[a-f0-9]{32}$' -or
        $state.Browser -notin @('None','Firefox','Chrome','Edge') -or ($state.ExtensionId -and $state.ExtensionId -notmatch '^[a-p]{32}$') -or
        ($state.PSObject.Properties['AppVersion'] -and $state.AppVersion -notmatch '^[0-9][A-Za-z0-9.+-]{0,63}$')) { throw 'Invalid Tide installation record.' }
    return $state
}
function Invoke-TideBrowserSetup([string]$Release, [string]$Mode, [string]$Browser, [string]$ExtensionId) {
    if ($Browser -eq 'None') { return }
    $arguments = @{Mode=$Mode; Destination=(Join-Path $Release 'native-host'); Browser=$Browser}
    if ($Mode -eq 'Prepare') {
        $arguments.HostExecutable = Join-Path $Release 'tide-session-host.exe'
        if ($ExtensionId) { $arguments.ChromiumExtensionId = $ExtensionId }
    }
    & (Join-Path $Release 'scripts/setup-browser-companion.ps1') @arguments | Out-Null
}
function Save-TideReleaseOwnership([string]$Release) {
    Assert-TidePlainPath $Release
    $files = @(Get-ChildItem -LiteralPath $Release -File -Recurse -Force | Where-Object { $_.FullName -ne (Join-Path $Release '.tide-release.json') } | ForEach-Object {
        Assert-TidePlainPath $_.FullName
        $_.FullName.Substring($Release.Length + 1).Replace('\','/')
    })
    Write-TideJson (Join-Path $Release '.tide-release.json') @{Product='org.tidemusic.windows.preview'; Files=$files}
}
function Get-TideOwnedReleasePaths([string]$Release) {
    $record = Get-TideChild $Release '.tide-release.json'
    $state = Read-TideJson $record
    if ($state.Product -ne 'org.tidemusic.windows.preview' -or @($state.Files).Count -eq 0) { throw 'Invalid owned release.' }
    $seen = @{}
    return @($state.Files | ForEach-Object {
        $path = Get-TideChild $Release $_
        if ($path -eq $record -or $seen.ContainsKey($path)) { throw 'Invalid or duplicate owned file.' }
        $seen[$path] = $true
        $path
    })
}
function Remove-TideOwnedRelease([string]$Release) {
    $record = Get-TideChild $Release '.tide-release.json'
    $directories = @{}
    # Preflight every path before removal; never recursively remove a tree.
    $paths = @(Get-TideOwnedReleasePaths $Release)
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
function Invoke-TideDependencies([string]$Release) {
    Write-TideProgress 20 'Downloading and checking playback tools'
    & (Join-Path $Release 'scripts/setup-runtime.ps1') -Destination (Join-Path $Release 'tools') | Out-Null
    Write-TideProgress 55 'Installing the private account helper'
    & (Join-Path $Release 'scripts/setup-provider.ps1') -Managed | Out-Null
}
function Install-Tide([string]$PackageRoot, [string]$InstallRoot, [string]$ShortcutRoot, [string]$Browser, [string]$ExtensionId, [string]$AppVersion = '0.1.0-preview') {
    if ($AppVersion -notmatch '^[0-9][A-Za-z0-9.+-]{0,63}$') { throw 'Invalid application version.' }
    Write-TideProgress 5 'Checking the setup package'
    $payload = @(Get-TideVerifiedPayload $PackageRoot)
    if ($Browser -notin @('None','Firefox','Chrome','Edge')) { throw 'Select a supported browser.' }
    if ($Browser -ne 'None' -and 'tide-session-host.exe' -notin $payload) { throw 'This package does not contain the browser helper.' }
    if ($Browser -in @('Chrome','Edge') -and $ExtensionId -notmatch '^[a-p]{32}$') { throw 'Enter the exact ID displayed by the installed browser extension.' }
    $old = Get-TideInstallState $InstallRoot
    Assert-TidePlainPath $ShortcutRoot
    Assert-TideStopped $InstallRoot
    $oldRelease = $null; $oldLinks = @{}
    if ($old) {
        $oldRelease = Get-TideChild $InstallRoot ('releases/' + $old.Release)
        $oldLinks = Get-TideShortcuts $oldRelease $InstallRoot
        if ($Browser -eq 'None') { $Browser = $old.Browser; $ExtensionId = $old.ExtensionId }
    }
    $oldUninstall = Get-TideUninstallEntry
    if ($null -ne $oldUninstall) {
        $oldVersion = if ($old -and $old.PSObject.Properties['AppVersion']) { $old.AppVersion } else { '0.1.0-preview' }
        if (!$old -or !(Test-TideUninstallEntryMatch $oldUninstall (Get-TideUninstallRegistration $oldRelease $InstallRoot $oldVersion))) {
            throw 'A different Windows uninstall entry exists; it was left unchanged.'
        }
    }
    foreach ($name in @('Tide.lnk','Uninstall Tide.lnk')) {
        $actual = Get-TideShortcut (Join-Path $ShortcutRoot $name)
        if ($actual -and (!$old -or !(Test-TideShortcutMatch $actual $oldLinks[$name]))) { throw 'A different Start menu shortcut exists; it was left unchanged.' }
    }
    $id = [Guid]::NewGuid().ToString('N')
    $release = Get-TideChild $InstallRoot ('releases/' + $id)
    # Serialise install/update/removal. Root creation never replaces existing files.
    [IO.Directory]::CreateDirectory($InstallRoot) | Out-Null
    $lock = [IO.File]::Open((Join-Path $InstallRoot '.tide-install.lock'), 'OpenOrCreate', 'ReadWrite', 'None')
    $oldUnregistered = $false; $newRegistered = $false; $committed = $false; $touchedLinks = @(); $pendingWritten = $false; $uninstallChanged = $false
    try {
        # Recheck after locking, including the active revision.
        if ($old) {
            $current = Get-TideInstallState $InstallRoot
            if (!$current -or $current.Release -ne $old.Release) { throw 'Installation changed; retry setup.' }
        } elseif (@(Get-ChildItem -LiteralPath $InstallRoot -Force | Where-Object Name -NE '.tide-install.lock').Count) {
            throw 'Installation changed; retry setup.'
        }
        Write-TideProgress 15 'Preparing a fresh installation'
        [IO.Directory]::CreateDirectory($release) | Out-Null
        foreach ($name in $payload) {
            $destination = Get-TideChild $release $name
            [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination)) | Out-Null
            [IO.File]::Copy((Get-TideChild $PackageRoot $name), $destination, $false)
        }
        [IO.File]::Copy((Join-Path $PackageRoot 'SHA256SUMS.txt'), (Join-Path $release 'SHA256SUMS.txt'), $false)
        Get-TideVerifiedPayload $release | Out-Null
        Invoke-TideDependencies $release
        Write-TideProgress 75 'Finishing app setup'
        Invoke-TideBrowserSetup $release 'Prepare' $Browser $ExtensionId
        Assert-TideStopped $InstallRoot
        if ($old -and $old.Browser -ne 'None') {
            Invoke-TideBrowserSetup $oldRelease 'Unregister' $old.Browser $old.ExtensionId
            $oldUnregistered = $true
        }
        Invoke-TideBrowserSetup $release 'Register' $Browser $ExtensionId
        $newRegistered = $Browser -ne 'None'
        $links = Get-TideShortcuts $release $InstallRoot
        [IO.Directory]::CreateDirectory($ShortcutRoot) | Out-Null
        foreach ($name in $links.Keys) {
            $path = Join-Path $ShortcutRoot $name
            $actual = Get-TideShortcut $path
            if ($actual -and (!$old -or !(Test-TideShortcutMatch $actual $oldLinks[$name]))) { throw 'Start menu changed during setup; retry.' }
            $touchedLinks += $name
            Set-TideShortcut $path $links[$name]
        }
        Write-TideProgress 90 'Adding Start menu and Windows uninstall entries'
        $newUninstall = Get-TideUninstallRegistration $release $InstallRoot $AppVersion
        Set-TideUninstallRegistration $oldUninstall $newUninstall
        $uninstallChanged = $true
        Save-TideReleaseOwnership $release
        $next = @{Product='org.tidemusic.windows.preview'; Root=$InstallRoot; Release=$id; Browser=$Browser; ExtensionId=$ExtensionId; AppVersion=$AppVersion}
        $pending = Join-Path $InstallRoot '.tide-install.pending.json'
        $backup = Join-Path $InstallRoot '.tide-install.previous.json'
        if ((Test-Path -LiteralPath $pending) -or (Test-Path -LiteralPath $backup)) { throw 'An interrupted installation record exists; it was preserved for review.' }
        Write-TideJson $pending $next
        $pendingWritten = $true
        $record = Join-Path $InstallRoot '.tide-install.json'
        if ($old) {
            # PowerShell coerces a null string argument to an empty path on some
            # runtimes. Use a confined explicit backup for the atomic replace.
            [IO.File]::Replace($pending, $record, $backup)
        } else { [IO.File]::Move($pending, $record) }
        $committed = $true
        if ($old) { Remove-Item -LiteralPath $backup -Force -ErrorAction Continue }
        if ($old) {
            try { Remove-TideOwnedRelease $oldRelease }
            catch { Write-Warning 'Update succeeded; some previous owned files remain for a later removal.' }
        }
        Write-TideProgress 100 'Tide is ready to open'
        Write-Output ('TIDE_RESULT|' + (Join-Path $release 'tide-music.exe'))
    } catch {
        if (!$committed) {
            if ($uninstallChanged) { Set-TideUninstallRegistration $newUninstall $oldUninstall }
            if ($pendingWritten -and (Test-Path -LiteralPath $pending)) { Remove-Item -LiteralPath $pending -Force }
            foreach ($name in $touchedLinks) {
                $path = Join-Path $ShortcutRoot $name
                $actual = Get-TideShortcut $path
                if (!$actual -or (Test-TideShortcutMatch $actual $links[$name])) {
                    if ($old) { Set-TideShortcut $path $oldLinks[$name] }
                    elseif ($actual) { Remove-Item -LiteralPath $path -Force }
                }
            }
            if ($newRegistered) { Invoke-TideBrowserSetup $release 'Unregister' $Browser $ExtensionId }
            if ($oldUnregistered) { Invoke-TideBrowserSetup $oldRelease 'Register' $old.Browser $old.ExtensionId }
            # Only files from the fresh, unexposed staging directory are owned.
            if (Test-Path -LiteralPath $release) { Save-TideReleaseOwnership $release; Remove-TideOwnedRelease $release }
        }
        throw
    } finally {
        $lock.Dispose()
        Remove-Item -LiteralPath (Join-Path $InstallRoot '.tide-install.lock') -Force
        if (!$old -and !$committed) {
            foreach ($directory in @((Join-Path $InstallRoot 'releases'), $InstallRoot)) {
                if ((Test-Path -LiteralPath $directory) -and @(Get-ChildItem -LiteralPath $directory -Force).Count -eq 0) { Remove-Item -LiteralPath $directory }
            }
        }
    }
}
function Uninstall-Tide([string]$InstallRoot, [string]$ShortcutRoot) {
    $state = Get-TideInstallState $InstallRoot
    if (!$state) { Write-Output 'Tide is not installed in this location.'; return }
    Assert-TideStopped $InstallRoot
    Assert-TidePlainPath $ShortcutRoot
    $release = Get-TideChild $InstallRoot ('releases/' + $state.Release)
    $links = Get-TideShortcuts $release $InstallRoot
    $version = if ($state.PSObject.Properties['AppVersion']) { $state.AppVersion } else { '0.1.0-preview' }
    $uninstall = Get-TideUninstallEntry
    if ($null -ne $uninstall -and !(Test-TideUninstallEntryMatch $uninstall (Get-TideUninstallRegistration $release $InstallRoot $version))) {
        throw 'The Windows uninstall entry changed; it was left unchanged.'
    }
    $lock = [IO.File]::Open((Join-Path $InstallRoot '.tide-install.lock'), 'OpenOrCreate', 'ReadWrite', 'None')
    try {
        $current = Get-TideInstallState $InstallRoot
        if ($current.Release -ne $state.Release) { throw 'Installation changed; retry removal.' }
        Assert-TideStopped $InstallRoot
        $ownedReleases = @(Get-ChildItem -LiteralPath (Join-Path $InstallRoot 'releases') -Directory | Where-Object {
            $_.Name -match '^[a-f0-9]{32}$' -and (Test-Path -LiteralPath (Join-Path $_.FullName '.tide-release.json'))
        })
        foreach ($item in $ownedReleases) { Get-TideOwnedReleasePaths $item.FullName | Out-Null }
        if (!(Test-TideUninstallEntryMatch (Get-TideUninstallEntry) $uninstall)) { throw 'Windows uninstall registration changed during removal.' }
        Invoke-TideBrowserSetup $release 'Unregister' $state.Browser $state.ExtensionId
        Set-TideUninstallRegistration $uninstall $null
        foreach ($name in $links.Keys) {
            $path = Join-Path $ShortcutRoot $name
            if (Test-TideShortcutMatch (Get-TideShortcut $path) $links[$name]) { Remove-Item -LiteralPath $path -Force }
        }
        # Previous failed cleanup releases are still strictly confined by ID and their ownership records.
        foreach ($item in $ownedReleases) { Remove-TideOwnedRelease $item.FullName }
        Remove-Item -LiteralPath (Join-Path $InstallRoot '.tide-install.json') -Force
    } finally {
        $lock.Dispose()
        Remove-Item -LiteralPath (Join-Path $InstallRoot '.tide-install.lock') -Force
    }
    foreach ($directory in @((Join-Path $InstallRoot 'releases'), $InstallRoot, $ShortcutRoot)) {
        if ((Test-Path -LiteralPath $directory) -and @(Get-ChildItem -LiteralPath $directory -Force).Count -eq 0) { Remove-Item -LiteralPath $directory }
    }
    Write-Output 'Tide removed. Saved preferences and encrypted sessions were preserved; Disconnect in Tide removes a saved session.'
}
