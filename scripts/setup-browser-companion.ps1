#requires -Version 5.1
<#
.SYNOPSIS
Prepares optional browser native-host manifests; registration is explicit.
.DESCRIPTION
Prepare writes only a new output directory. Register changes the current user's
Tide native-host keys and writes a non-secret origin policy beside the selected
host executable. No browser profiles, account files, extensions or other host
registrations are read or modified. Unregister removes only keys still pointing
to these exact prepared manifests, and leaves files and account storage intact.
#>
[CmdletBinding()]
param(
    [ValidateSet('Prepare', 'Register', 'Unregister')]
    [string]$Mode = 'Prepare',
    [Parameter(Mandatory = $true)]
    [string]$Destination,
    [string]$HostExecutable,
    [ValidatePattern('^[a-p]{32}$')]
    [string]$ChromiumExtensionId
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$taskDirectory = [IO.Path]::GetFullPath($Destination)
$taskHostName = 'org.tidemusic.session'
$taskFirefoxPath = Join-Path $taskDirectory 'firefox-host.json'
$taskChromiumPath = Join-Path $taskDirectory 'chromium-host.json'
$taskPolicyPath = Join-Path $taskDirectory 'tide-session-host-policy.json'
function Write-HostJson([string]$Path, $Value) {
    [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
}
function Read-HostJson([string]$Path) {
    $taskFile = Get-Item -LiteralPath $Path
    if ($taskFile.Length -gt 4096 -or ($taskFile.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Invalid prepared manifest' }
    Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}
if ($Mode -eq 'Prepare') {
    if ([string]::IsNullOrWhiteSpace($HostExecutable)) { throw 'Prepare requires -HostExecutable' }
    $taskExe = (Resolve-Path -LiteralPath $HostExecutable).ProviderPath
    $taskFile = Get-Item -LiteralPath $taskExe
    if ($taskFile.PSIsContainer -or $taskFile.Name -ne 'tide-session-host.exe' -or ($taskFile.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Select the built tide-session-host.exe' }
    if (Test-Path -LiteralPath $taskDirectory) { throw 'Destination exists; choose a new directory' }
    New-Item -ItemType Directory -Path $taskDirectory | Out-Null
    $taskFirefox = @{ name=$taskHostName; description='Tide explicit session transfer'; path=$taskExe; type='stdio'; allowed_extensions=@('tide-session@tide-music') }
    Write-HostJson $taskFirefoxPath $taskFirefox
    $taskOrigins = @()
    if ($ChromiumExtensionId) {
        $taskOrigins = @("chrome-extension://$ChromiumExtensionId/")
        Write-HostJson $taskChromiumPath @{ name=$taskHostName; description='Tide explicit session transfer'; path=$taskExe; type='stdio'; allowed_origins=$taskOrigins }
    }
    Write-HostJson $taskPolicyPath @{ allowed_origins=$taskOrigins }
    'Prepared manifests only. No browser extension or native host was registered.'
    return
}

# Registration accepts only complete, consistent manifests prepared above.
$taskFirefox = Read-HostJson $taskFirefoxPath
$taskPolicy = Read-HostJson $taskPolicyPath
if ($taskFirefox.name -ne $taskHostName -or $taskFirefox.type -ne 'stdio' -or
    @($taskFirefox.allowed_extensions).Count -ne 1 -or $taskFirefox.allowed_extensions[0] -ne 'tide-session@tide-music' -or
    ![IO.Path]::IsPathRooted($taskFirefox.path) -or [IO.Path]::GetFileName($taskFirefox.path) -ne 'tide-session-host.exe') { throw 'Invalid Firefox host manifest' }
$taskEntries = @(@{key="HKCU:\Software\Mozilla\NativeMessagingHosts\$taskHostName"; manifest=$taskFirefoxPath})
if (Test-Path -LiteralPath $taskChromiumPath) {
    $taskChromium = Read-HostJson $taskChromiumPath
    if ($taskChromium.name -ne $taskHostName -or $taskChromium.type -ne 'stdio' -or $taskChromium.path -ne $taskFirefox.path -or
        @($taskChromium.allowed_origins).Count -ne 1 -or $taskChromium.allowed_origins[0] -notmatch '^chrome-extension://[a-p]{32}/$' -or
        @($taskPolicy.allowed_origins).Count -ne 1 -or $taskPolicy.allowed_origins[0] -ne $taskChromium.allowed_origins[0]) { throw 'Invalid Chromium host manifest or policy' }
    $taskEntries += @{key="HKCU:\Software\Google\Chrome\NativeMessagingHosts\$taskHostName"; manifest=$taskChromiumPath}
    $taskEntries += @{key="HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\$taskHostName"; manifest=$taskChromiumPath}
} elseif (@($taskPolicy.allowed_origins).Count -ne 0) { throw 'Unexpected Chromium origin without its manifest' }

# Preflight every key before changing any. Do not replace another registration.
foreach ($taskEntry in $taskEntries) {
    if (Test-Path -LiteralPath $taskEntry.key) {
        $taskExisting = (Get-Item -LiteralPath $taskEntry.key).GetValue('')
        if ($taskExisting -ne $taskEntry.manifest) { throw 'A different Tide native-host registration exists; it was left unchanged' }
    }
}
if ($Mode -eq 'Unregister') {
    foreach ($taskEntry in $taskEntries) {
        if (Test-Path -LiteralPath $taskEntry.key) {
            if ((Get-Item -LiteralPath $taskEntry.key).GetValue('') -ne $taskEntry.manifest) {
                throw 'The native-host registration changed during removal; it was left unchanged'
            }
            Remove-Item -LiteralPath $taskEntry.key -Force
        }
    }
    'Removed matching Tide native-host registrations. Files and saved account sessions were left intact.'
    return
}
if (!(Test-Path -LiteralPath $taskFirefox.path -PathType Leaf)) { throw 'The prepared native host is missing' }
$taskInstalledPolicy = Join-Path ([IO.Path]::GetDirectoryName($taskFirefox.path)) 'tide-session-host-policy.json'
if (Test-Path -LiteralPath $taskInstalledPolicy) {
    $taskOldPolicy = Read-HostJson $taskInstalledPolicy
    if (($taskOldPolicy | ConvertTo-Json -Compress) -ne ($taskPolicy | ConvertTo-Json -Compress)) { throw 'A different native-host policy exists; it was left unchanged' }
} else { Write-HostJson $taskInstalledPolicy $taskPolicy }
$taskCreatedEntries = @()
try {
    foreach ($taskEntry in $taskEntries) {
        # Matching existing registrations need no write. Recheck after preflight
        # and never force-create over a key claimed by another installation.
        if (Test-Path -LiteralPath $taskEntry.key) {
            if ((Get-Item -LiteralPath $taskEntry.key).GetValue('') -ne $taskEntry.manifest) {
                throw 'The native-host registration changed during setup'
            }
            continue
        }
        # Registry New-Item does not create missing ancestor keys. Create only
        # absent shared parents, without force-writing any existing key/value.
        $taskMissingParents = [Collections.Generic.List[string]]::new()
        $taskParentKey = Split-Path -Parent $taskEntry.key
        while (!(Test-Path -LiteralPath $taskParentKey)) {
            if ($taskParentKey -eq 'HKCU:\Software') { throw 'Current-user software registry is unavailable' }
            $taskMissingParents.Add($taskParentKey)
            $taskParentKey = Split-Path -Parent $taskParentKey
        }
        for ($taskParentIndex = $taskMissingParents.Count - 1; $taskParentIndex -ge 0; $taskParentIndex--) {
            $taskParentKey = $taskMissingParents[$taskParentIndex]
            if (!(Test-Path -LiteralPath $taskParentKey)) {
                try { New-Item -Path $taskParentKey | Out-Null }
                catch { if (!(Test-Path -LiteralPath $taskParentKey)) { throw } }
            }
        }
        # Shared parent keys remain on rollback/removal; another application may
        # begin using them concurrently. Only Tide's leaf key is owned here.
        New-Item -Path $taskEntry.key | Out-Null
        $taskCreatedEntries += $taskEntry
        Set-Item -LiteralPath $taskEntry.key -Value $taskEntry.manifest
    }
} catch {
    # Roll back only this attempt's keys, still empty or pointing to our own
    # manifest. Preserve pre-existing or concurrently changed registrations.
    foreach ($taskCreated in $taskCreatedEntries) {
        try {
            if (Test-Path -LiteralPath $taskCreated.key) {
                $taskValue = (Get-Item -LiteralPath $taskCreated.key).GetValue('')
                if ($null -eq $taskValue -or $taskValue -eq $taskCreated.manifest) {
                    Remove-Item -LiteralPath $taskCreated.key -Force
                }
            }
        } catch { Write-Warning 'A newly created Tide host key could not be rolled back; retry setup or use Unregister.' }
    }
    throw
}
'Registered the optional native host for the current user. Install the matching browser companion separately.'
