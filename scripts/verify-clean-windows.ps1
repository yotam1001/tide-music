#requires -Version 7.0
<#
.SYNOPSIS
Opt-in real installation probe for a disposable GitHub-hosted Windows runner.
.DESCRIPTION
Installs, updates, launches and uninstalls the supplied verified preview package.
Uses real current-user Windows integration with a synthetic Chrome extension ID;
it never opens a browser or imports an account. This does not test GUI clicks,
Chrome extension installation or authenticated session transfer. Evidence remains
in RUNNER_TEMP even after a failure. Never run this on a personal Windows machine.
#>
[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$PackageRoot)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# All device mutations, including scratch creation, must follow these checks.
if ($env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted') {
    throw 'This opt-in probe requires a disposable GitHub-hosted Actions runner.'
}
if (![OperatingSystem]::IsWindows() -or ![Environment]::Is64BitProcess -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne [Runtime.InteropServices.Architecture]::X64) {
    throw 'This probe requires a Windows x64 process and operating system.'
}
if (!$env:RUNNER_TEMP -or !$env:LOCALAPPDATA) { throw 'Required hosted-runner directories are unavailable.' }

$taskPackage = [IO.Path]::GetFullPath($PackageRoot)
$taskInstaller = Join-Path $taskPackage 'scripts/install-windows.ps1'
$taskCommon = Join-Path $taskPackage 'scripts/install-common.ps1'
foreach ($required in @($taskInstaller, $taskCommon, (Join-Path $taskPackage 'tide-session-host.exe'))) {
    if (!(Test-Path -LiteralPath $required -PathType Leaf)) { throw 'A current preview package including its native host is required.' }
}
. $taskCommon
Assert-TidePlainPath $taskPackage
Get-TideVerifiedPayload $taskPackage | Out-Null

$taskInstallRoot = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Programs/Tide'))
$taskShortcutRoot = Join-Path ([Environment]::GetFolderPath('Programs')) 'Tide'
$taskHostSuffixes = @(
    'Software\Mozilla\NativeMessagingHosts\org.tidemusic.session',
    'Software\Google\Chrome\NativeMessagingHosts\org.tidemusic.session',
    'Software\Chromium\NativeMessagingHosts\org.tidemusic.session',
    'Software\Microsoft\Edge\NativeMessagingHosts\org.tidemusic.session'
)
$taskUninstallSuffix = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\TideMusic'
function Test-CleanRegistryKey([Microsoft.Win32.RegistryHive]$Hive, [Microsoft.Win32.RegistryView]$View, [string]$Subkey) {
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($Hive, $View)
    try {
        $key = $base.OpenSubKey($Subkey)
        if ($null -eq $key) { return $false }
        $key.Dispose()
        return $true
    } finally { $base.Dispose() }
}
foreach ($path in @($taskInstallRoot, $taskShortcutRoot)) {
    Assert-TidePlainPath $path
    if (Test-Path -LiteralPath $path) { throw 'Existing Tide installation or Start-menu folder detected; no changes made.' }
}
foreach ($hive in @([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryHive]::LocalMachine)) {
    foreach ($view in @([Microsoft.Win32.RegistryView]::Registry64, [Microsoft.Win32.RegistryView]::Registry32)) {
        foreach ($subkey in @($taskUninstallSuffix) + $taskHostSuffixes) {
            if (Test-CleanRegistryKey $hive $view $subkey) {
                throw 'Existing Tide uninstall or native-host registration detected; no changes made.'
            }
        }
    }
}

# Retain only synthetic account-free fixtures and diagnostic evidence. No recursive
# deletion is performed by this probe; removal uses the package ownership logic.
Assert-TidePlainPath $env:RUNNER_TEMP
$taskEvidence = Join-Path ([IO.Path]::GetFullPath($env:RUNNER_TEMP)) ('tide-clean-windows-' + [Guid]::NewGuid().ToString('N'))
if (Test-Path -LiteralPath $taskEvidence) { throw 'Probe evidence destination already exists.' }
[IO.Directory]::CreateDirectory($taskEvidence) | Out-Null
$taskIsolatedProfile = Join-Path $taskEvidence 'appdata'
$taskSentinelDirectory = Join-Path $taskIsolatedProfile 'TideMusic'
[IO.Directory]::CreateDirectory($taskSentinelDirectory) | Out-Null
$taskSettings = Join-Path $taskSentinelDirectory 'settings.json'
$taskSentinel = '{"queue":{"tracks":[],"current":null},"light_mode":false,"compact_mode":false,"autoplay":false,"volume":0,"repeat":false}'
[IO.File]::WriteAllText($taskSettings, $taskSentinel, [Text.UTF8Encoding]::new($false))
$taskExpectedSettingsHash = (Get-FileHash -LiteralPath $taskSettings -Algorithm SHA256).Hash
$taskExtensionId = 'abcdefghijklmnopabcdefghijklmnop' # Deliberately synthetic: not a browser installation.
$taskExpectedOrigin = 'chrome-extension://' + $taskExtensionId + '/'
$taskPowerShell = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'

function Invoke-CleanChild([string]$Executable, [string[]]$Arguments, [string]$Label, [int]$TimeoutSeconds = 1800) {
    if ($Label -notmatch '^[a-z0-9-]+$') { throw 'Invalid diagnostic label.' }
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $Executable
    $info.WorkingDirectory = $taskPackage
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    foreach ($argument in $Arguments) { $info.ArgumentList.Add($argument) }
    foreach ($name in @($info.Environment.Keys)) {
        if ($name -like 'TIDE_*' -or $name -like 'PYTHON*' -or $name -like 'SLINT_*') { $info.Environment.Remove($name) | Out-Null }
    }
    $info.Environment['APPDATA'] = $taskIsolatedProfile
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $info
    try {
        if (!$process.Start()) { throw 'Probe child could not start.' }
        $outTask = $process.StandardOutput.ReadToEndAsync()
        $errTask = $process.StandardError.ReadToEndAsync()
        $finished = $process.WaitForExit($TimeoutSeconds * 1000)
        if (!$finished) {
            # Only the exact child created above and its descendants are stopped.
            $process.Kill($true)
            if (!$process.WaitForExit(15000)) { throw 'Timed-out probe child could not be stopped.' }
        }
        if (![Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($outTask, $errTask), 15000)) {
            throw 'Probe child output did not finish within its bound.'
        }
        $stdout = $outTask.GetAwaiter().GetResult()
        $stderr = $errTask.GetAwaiter().GetResult()
        [IO.File]::WriteAllText((Join-Path $taskEvidence ($Label + '.stdout.log')), $stdout)
        [IO.File]::WriteAllText((Join-Path $taskEvidence ($Label + '.stderr.log')), $stderr)
        if (!$finished) { throw 'Probe child exceeded its time limit; evidence preserved.' }
        if ($process.ExitCode -ne 0) { throw 'Probe child failed; evidence preserved.' }
        Write-Host ('PASS ' + $Label)
        return $stdout
    } finally { $process.Dispose() }
}
function Assert-CleanSentinel {
    if (!(Test-Path -LiteralPath $taskSettings -PathType Leaf) -or
        (Get-FileHash -LiteralPath $taskSettings -Algorithm SHA256).Hash -ne $taskExpectedSettingsHash) {
        throw 'Installation, update or removal changed the synthetic saved settings.'
    }
}
function Assert-CleanInstallation([string]$Version) {
    $state = Get-TideInstallState $taskInstallRoot
    if (!$state -or $state.AppVersion -ne $Version -or $state.Browser -ne 'Chrome' -or $state.ExtensionId -ne $taskExtensionId) {
        throw 'Installed ownership record differs from the requested probe installation.'
    }
    $release = Get-TideChild $taskInstallRoot ('releases/' + $state.Release)
    foreach ($relative in @('tide-music.exe', 'tide-session-host.exe', 'tools/mpv.exe', 'tools/yt-dlp.exe',
        'tools/deno.exe', '.tools/provider-venv/Scripts/python.exe', 'helpers/youtube_music.py', '.tide-release.json')) {
        if (!(Test-Path -LiteralPath (Get-TideChild $release $relative) -PathType Leaf)) { throw 'Installed runtime payload is incomplete.' }
    }
    $entry = Get-TideUninstallEntry
    if (!(Test-TideUninstallEntryMatch $entry (Get-TideUninstallRegistration $release $taskInstallRoot $Version))) {
        throw 'Real Windows uninstall registration does not match the installed release.'
    }
    $expectedLinks = Get-TideShortcuts $release $taskInstallRoot
    foreach ($name in $expectedLinks.Keys) {
        if (!(Test-TideShortcutMatch (Get-TideShortcut (Join-Path $taskShortcutRoot $name)) $expectedLinks[$name])) {
            throw 'Real Start-menu shortcut does not match the installed release.'
        }
    }
    $chromeKey = Get-Item -LiteralPath ('HKCU:\' + $taskHostSuffixes[1])
    try { $manifestPath = $chromeKey.GetValue('') } finally { $chromeKey.Dispose() }
    $expectedManifestPath = Get-TideChild $release 'native-host/chromium-host.json'
    if ($manifestPath -ne $expectedManifestPath) { throw 'Chrome registration does not point to the owned manifest.' }
    $manifest = Read-TideJson $expectedManifestPath
    if ($manifest.name -ne 'org.tidemusic.session' -or $manifest.type -ne 'stdio' -or
        $manifest.path -ne (Join-Path $release 'tide-session-host.exe') -or
        @($manifest.allowed_origins).Count -ne 1 -or $manifest.allowed_origins[0] -cne $taskExpectedOrigin) {
        throw 'Chrome manifest does not strictly allow the synthetic probe extension.'
    }
    $policy = Read-TideJson (Join-Path $release 'tide-session-host-policy.json')
    if (@($policy.allowed_origins).Count -ne 1 -or $policy.allowed_origins[0] -cne $taskExpectedOrigin) {
        throw 'Installed receiver policy differs from the synthetic probe origin.'
    }
    foreach ($subkey in @($taskHostSuffixes[0], $taskHostSuffixes[2], $taskHostSuffixes[3])) {
        if (Test-CleanRegistryKey CurrentUser Registry64 $subkey) { throw 'Setup registered a browser other than Chrome.' }
    }
    Assert-CleanSentinel
    return $release
}

try {
    $installArgs = @('-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $taskInstaller,
        '-Mode', 'Install', '-InstallRoot', $taskInstallRoot, '-Browser', 'Chrome', '-ChromiumExtensionId', $taskExtensionId,
        '-AppVersion', '0.1.0-clean-probe')
    Invoke-CleanChild $taskPowerShell $installArgs 'fresh-install' | Out-Null
    $taskFirstRelease = Assert-CleanInstallation '0.1.0-clean-probe'
    Invoke-CleanChild (Join-Path $taskFirstRelease 'tools/mpv.exe') @('--version') 'mpv-version' 30 | Out-Null
    Invoke-CleanChild (Join-Path $taskFirstRelease 'tools/yt-dlp.exe') @('--version') 'yt-dlp-version' 30 | Out-Null
    Invoke-CleanChild (Join-Path $taskFirstRelease 'tools/deno.exe') @('--version') 'deno-version' 30 | Out-Null
    $taskPythonArgs = @('-I', '-c', 'import ytmusicapi, requests, certifi; assert callable(ytmusicapi.YTMusic)')
    Invoke-CleanChild (Join-Path $taskFirstRelease '.tools/provider-venv/Scripts/python.exe') $taskPythonArgs 'private-python' 30 | Out-Null

    # Omitting Browser on the second install must retain the managed Chrome choice.
    $updateArgs = @('-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $taskInstaller,
        '-Mode', 'Install', '-InstallRoot', $taskInstallRoot, '-AppVersion', '0.1.1-clean-probe')
    Invoke-CleanChild $taskPowerShell $updateArgs 'update' | Out-Null
    $taskUpdatedRelease = Assert-CleanInstallation '0.1.1-clean-probe'
    if ($taskUpdatedRelease -eq $taskFirstRelease -or (Test-Path -LiteralPath $taskFirstRelease)) {
        throw 'Update did not replace and remove the previous owned release.'
    }

    # Ninety seconds of mono 16-bit PCM zeros; no internet audio or credentials.
    $taskWave = Join-Path $taskEvidence 'silence.wav'
    $writer = [IO.BinaryWriter]::new([IO.File]::Create($taskWave))
    try {
        $dataSize = 8000 * 2 * 90
        $writer.Write([Text.Encoding]::ASCII.GetBytes('RIFF')); $writer.Write([int](36 + $dataSize))
        $writer.Write([Text.Encoding]::ASCII.GetBytes('WAVEfmt ')); $writer.Write([int]16)
        $writer.Write([int16]1); $writer.Write([int16]1); $writer.Write([int]8000)
        $writer.Write([int]16000); $writer.Write([int16]2); $writer.Write([int16]16)
        $writer.Write([Text.Encoding]::ASCII.GetBytes('data')); $writer.Write([int]$dataSize)
        $writer.Write([byte[]]::new($dataSize))
    } finally { $writer.Dispose() }
    $taskScreenshots = Join-Path $taskEvidence 'screenshots'
    $taskSmokeArgs = @('--smoke-ui', $taskScreenshots, '--smoke-populated', '--fixture-audio', $taskWave)
    $taskSmokeOutput = Invoke-CleanChild (Join-Path $taskUpdatedRelease 'tide-music.exe') $taskSmokeArgs 'installed-app-ui' 90
    if ($taskSmokeOutput -notmatch 'PASS: native song/playlist play preserves browsing') {
        throw 'Installed app did not report the expected local-audio UI regression.'
    }
    foreach ($name in @('full.png', 'light.png', 'restored.png')) {
        $image = Join-Path $taskScreenshots $name
        if (!(Test-Path -LiteralPath $image -PathType Leaf) -or (Get-Item -LiteralPath $image).Length -eq 0) {
            throw 'Installed UI probe did not produce expected screenshot evidence.'
        }
    }
    Assert-CleanSentinel

    Invoke-CleanChild $taskPowerShell @('-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
        '-File', $taskInstaller, '-Mode', 'Uninstall', '-InstallRoot', $taskInstallRoot) 'uninstall' 120 | Out-Null
    foreach ($path in @($taskInstallRoot, $taskShortcutRoot)) {
        if (Test-Path -LiteralPath $path) { throw 'Uninstall left owned installation or Start-menu files.' }
    }
    foreach ($hive in @([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryHive]::LocalMachine)) {
        foreach ($view in @([Microsoft.Win32.RegistryView]::Registry64, [Microsoft.Win32.RegistryView]::Registry32)) {
            foreach ($subkey in @($taskUninstallSuffix) + $taskHostSuffixes) {
                if (Test-CleanRegistryKey $hive $view $subkey) { throw 'Uninstall left a Tide registry integration entry.' }
            }
        }
    }
    Assert-CleanSentinel
    [IO.File]::WriteAllText((Join-Path $taskEvidence 'result.txt'), 'PASS real clean Windows installation, update, runtimes, synthetic Chrome registration, installed local-audio UI and removal. No browser or authenticated import tested.')
    Write-Host 'PASS clean Windows integration; this does not validate GUI clicks or authenticated Chrome import.'
} catch {
    [IO.File]::WriteAllText((Join-Path $taskEvidence 'result.txt'), 'FAIL clean Windows integration. Owned failure evidence retained; no automatic cleanup was attempted.')
    Write-Warning 'Clean Windows probe failed. Evidence and any partial owned installation remain on the disposable runner.'
    throw
} finally {
    Write-Host ('Evidence directory: ' + $taskEvidence)
    if ($env:GITHUB_OUTPUT) {
        Add-Content -LiteralPath $env:GITHUB_OUTPUT -Value ('evidence-directory=' + $taskEvidence) -Encoding utf8
    }
}
