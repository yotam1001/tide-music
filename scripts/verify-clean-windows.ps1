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
param([Parameter(Mandatory = $true)][string]$PackageRoot,
      [Parameter(Mandatory = $true)][string]$SetupExecutable,
      [string]$PreviousPackageRoot)
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
$taskSetup = [IO.Path]::GetFullPath($SetupExecutable)
if (!(Test-Path -LiteralPath $taskSetup -PathType Leaf)) { throw 'The verified graphical setup is required.' }
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
$taskSentinel = '{"queue":{"tracks":[],"current":null},"light_mode":false,"compact_mode":false,"autoplay":false,"check_updates":false,"history_feedback":true,"discord_presence":true,"volume":0,"repeat":false}'
[IO.File]::WriteAllText($taskSettings, $taskSentinel, [Text.UTF8Encoding]::new($false))
$taskExpectedSettingsHash = (Get-FileHash -LiteralPath $taskSettings -Algorithm SHA256).Hash
$taskExtensionId = 'abcdefghijklmnopabcdefghijklmnop' # Deliberately synthetic: not a browser installation.
$taskExpectedOrigin = 'chrome-extension://' + $taskExtensionId + '/'
$taskPowerShell = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'

function Invoke-CleanChild([string]$Executable, [string[]]$Arguments, [string]$Label, [int]$TimeoutSeconds = 1800, [string]$RawArguments = '', [string]$WorkingDirectory = $taskPackage) {
    if ($Label -notmatch '^[a-z0-9-]+$') { throw 'Invalid diagnostic label.' }
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $Executable
    $info.WorkingDirectory = $WorkingDirectory
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    if ($RawArguments) {
        if ($Arguments.Count) { throw 'Choose either structured or registered arguments.' }
        $info.Arguments = $RawArguments
    } else { foreach ($argument in $Arguments) { $info.ArgumentList.Add($argument) } }
    foreach ($name in @($info.Environment.Keys)) {
        if ($name -like 'TIDE_*' -or $name -like 'PYTHON*' -or $name -like 'SLINT_*') { $info.Environment.Remove($name) | Out-Null }
    }
    $info.Environment['APPDATA'] = $taskIsolatedProfile
    if ([IO.Path]::GetFileName($Executable) -ieq 'powershell.exe') {
        $info.Environment['PSModulePath'] = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/Modules'
    }
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
    $taskBaselineInstaller = $taskInstaller
    $taskBaselineVersion = '0.1.0-clean-probe'
    if ($PreviousPackageRoot) {
        $taskBaselinePackage = [IO.Path]::GetFullPath($PreviousPackageRoot)
        Assert-TidePlainPath $taskBaselinePackage
        Get-TideVerifiedPayload $taskBaselinePackage | Out-Null
        $taskBaselineInstaller = Join-Path $taskBaselinePackage 'scripts/install-windows.ps1'
        $taskBaselineVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo((Join-Path $taskBaselinePackage 'tide-music.exe')).ProductVersion
        if ($taskBaselineVersion -notmatch '^\d+\.\d+\.\d+$') { throw 'Baseline has no stable product version.' }
    }
    $installArgs = @('-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $taskBaselineInstaller,
        '-Mode', 'Install', '-InstallRoot', $taskInstallRoot, '-Browser', 'Chrome', '-ChromiumExtensionId', $taskExtensionId,
        '-AppVersion', $taskBaselineVersion)
    Invoke-CleanChild $taskPowerShell $installArgs 'fresh-install' | Out-Null
    $taskFirstRelease = Assert-CleanInstallation $taskBaselineVersion
    Invoke-CleanChild (Join-Path $taskFirstRelease 'tools/mpv.exe') @('--version') 'mpv-version' 30 | Out-Null
    Invoke-CleanChild (Join-Path $taskFirstRelease 'tools/yt-dlp.exe') @('--version') 'yt-dlp-version' 30 | Out-Null
    Invoke-CleanChild (Join-Path $taskFirstRelease 'tools/deno.exe') @('--version') 'deno-version' 30 | Out-Null
    $taskPythonArgs = @('-I', '-c', 'import ytmusicapi, requests, certifi; assert callable(ytmusicapi.YTMusic)')
    Invoke-CleanChild (Join-Path $taskFirstRelease '.tools/provider-venv/Scripts/python.exe') $taskPythonArgs 'private-python' 30 | Out-Null

    # Exercise the actual graphical setup's automatic update handoff. The old
    # synthetic player exits normally; setup must wait, retain Chrome and reopen
    # the new installed player. These are disposable-runner processes only.
    $taskVersionAssembly = [Reflection.Assembly]::LoadFile($taskSetup)
    $taskVersionStream = $taskVersionAssembly.GetManifestResourceStream('Tide.Version')
    if ($null -eq $taskVersionStream) { throw 'Setup has no embedded app version.' }
    $taskVersionReader = [IO.StreamReader]::new($taskVersionStream)
    try { $taskUpdatedVersion = $taskVersionReader.ReadToEnd().Trim() } finally { $taskVersionReader.Dispose() }
    if ($PreviousPackageRoot -and [version]$taskUpdatedVersion -le [version]$taskBaselineVersion) { throw 'Upgrade candidate must be newer than the public stable baseline.' }
    Write-Host ('Upgrade under test: ' + $taskBaselineVersion + ' -> ' + $taskUpdatedVersion)
    # Reproduce the three-field conflict seen in a real in-app update. A
    # removed Tide release is still named by the registered old icon/uninstall
    # command, while the active installation is newer and valid. Setup must
    # confirm readiness without changing it, then repair it only after COMMIT.
    $taskRetainedRelease = Get-TideChild $taskInstallRoot ('releases/' + ('b' * 32))
    if (Test-Path -LiteralPath $taskRetainedRelease) { throw 'Retained-release fixture already exists.' }
    $taskOriginalEntry = Get-TideUninstallEntry
    $taskStaleEntry = Get-TideUninstallRegistration $taskRetainedRelease $taskInstallRoot '0.1.0-older-probe'
    Set-TideUninstallRegistration $taskOriginalEntry $taskStaleEntry
    if ((Get-TideUninstallConflictMessage $taskStaleEntry $taskOriginalEntry) -notmatch 'DisplayIcon, DisplayVersion, UninstallString') {
        throw 'Real handoff fixture did not reproduce the expected conflict.'
    }
    $taskPreviousAppdata = $env:APPDATA
    try {
        $env:APPDATA = $taskIsolatedProfile
        $taskParent = Start-Process -FilePath (Join-Path $taskFirstRelease 'tide-music.exe') `
            -ArgumentList @('--smoke-ui', ('"' + (Join-Path $taskEvidence 'before-update') + '"')) `
            -WorkingDirectory $env:RUNNER_TEMP -PassThru `
            -RedirectStandardOutput (Join-Path $taskEvidence 'update-parent.stdout.log') `
            -RedirectStandardError (Join-Path $taskEvidence 'update-parent.stderr.log')
        $taskSetupInfo = [Diagnostics.ProcessStartInfo]::new()
        $taskSetupInfo.FileName = $taskSetup
        $taskSetupInfo.WorkingDirectory = $env:RUNNER_TEMP
        $taskSetupInfo.UseShellExecute = $false
        $taskSetupInfo.CreateNoWindow = $true
        $taskSetupInfo.RedirectStandardInput = $true
        $taskSetupInfo.RedirectStandardOutput = $true
        $taskSetupInfo.RedirectStandardError = $true
        foreach ($argument in @('--update-from', [string]$taskParent.Id, '--confirm-ready')) {
            $taskSetupInfo.ArgumentList.Add($argument)
        }
        $taskSetupInfo.Environment['APPDATA'] = $taskIsolatedProfile
        $taskSetupProcess = [Diagnostics.Process]::new()
        $taskSetupProcess.StartInfo = $taskSetupInfo
        if (!$taskSetupProcess.Start()) { throw 'Setup handoff process did not start.' }
        $taskSetupError = $taskSetupProcess.StandardError.ReadToEndAsync()
        $taskReady = $taskSetupProcess.StandardOutput.ReadLineAsync()
        if (!$taskReady.Wait(30000) -or $taskReady.Result -cne ('TIDE_UPDATE_READY|' + $taskParent.Id + '|' + $taskUpdatedVersion)) {
            $taskSetupProcess.StandardInput.Close()
            throw 'Actual setup did not confirm its version and parent before restart.'
        }
        if ($taskSetupProcess.HasExited) { throw 'Actual setup exited before commit.' }
        if (!(Test-TideUninstallEntryMatch (Get-TideUninstallEntry) $taskStaleEntry)) {
            throw 'Setup changed the registration before restart was committed.'
        }
        # Keep a second, real Tide window alive through COMMIT. It has no timed
        # exit: the installer must close it normally, rather than fail its guard.
        # This GUI fixture must be visible on the disposable runner so Windows
        # can deliver the same CloseMainWindow request as a real player window.
        $taskSecondPlayer = Start-Process -FilePath (Join-Path $taskFirstRelease 'tide-music.exe') `
            -ArgumentList @('--benchmark-mode', 'compact') -WorkingDirectory $env:RUNNER_TEMP `
            -PassThru `
            -RedirectStandardOutput (Join-Path $taskEvidence 'second-player.stdout.log') `
            -RedirectStandardError (Join-Path $taskEvidence 'second-player.stderr.log')
        if (!$taskSecondPlayer.WaitForInputIdle(15000)) { throw 'Second Tide window did not initialize.' }
        if ($taskSecondPlayer.HasExited) { throw 'Second Tide window exited before update commit.' }
        $taskSetupOutput = $taskSetupProcess.StandardOutput.ReadToEndAsync()
        $taskCommitBytes = [Text.Encoding]::ASCII.GetBytes("COMMIT`n")
        $taskSetupProcess.StandardInput.BaseStream.Write($taskCommitBytes, 0, $taskCommitBytes.Length)
        $taskSetupProcess.StandardInput.BaseStream.Flush()
        $taskSetupProcess.StandardInput.Close()
        Write-Host 'PASS actual packaged setup readiness and explicit commit'
    } finally { $env:APPDATA = $taskPreviousAppdata }
    if (!$taskSetupProcess.HasExited) {
        try { $null = $taskSetupProcess.WaitForInputIdle(15000) } catch { if (!$taskSetupProcess.HasExited) { throw } }
        $taskSetupProcess.Refresh()
        if (!$taskSetupProcess.HasExited -and $taskSetupProcess.MainWindowHandle -ne 0) { throw 'Automatic update opened a separate setup window.' }
    }
    if (!$taskSetupProcess.WaitForExit(600000) -or $taskSetupProcess.ExitCode -ne 0) { throw 'Automatic setup handoff did not finish.' }
    if (!$taskSetupOutput.Wait(15000) -or !$taskSetupError.Wait(15000)) {
        throw 'Reopened Tide inherited the setup confirmation pipes; they must close when setup exits.'
    }
    [IO.File]::WriteAllText((Join-Path $taskEvidence 'setup.stdout.log'), $taskReady.Result + [Environment]::NewLine + $taskSetupOutput.Result)
    [IO.File]::WriteAllText((Join-Path $taskEvidence 'setup.stderr.log'), $taskSetupError.Result)
    if (!$taskParent.HasExited -or $taskParent.ExitCode -ne 0) { throw 'The previous player did not close normally.' }
    if (!$taskSecondPlayer.HasExited -or $taskSecondPlayer.ExitCode -ne 0) { throw 'Update did not gracefully close the second real Tide window.' }
    Write-Host 'PASS committed update gracefully closed a second real Tide instance without force termination'

    $taskUpdatedRelease = Assert-CleanInstallation $taskUpdatedVersion
    if ($taskUpdatedRelease -eq $taskFirstRelease -or (Test-Path -LiteralPath $taskFirstRelease)) {
        throw 'Update did not replace and remove the previous owned release.'
    }
    $taskRestarted = @(Get-Process -Name tide-music -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq (Join-Path $taskUpdatedRelease 'tide-music.exe') })
    if ($taskRestarted.Count -ne 1) { throw 'Setup did not reopen exactly one updated player.' }
    $taskRestartDeadline = [DateTime]::UtcNow.AddSeconds(30)
    while (!$taskRestarted[0].HasExited -and $taskRestarted[0].MainWindowHandle -eq 0 -and [DateTime]::UtcNow -lt $taskRestartDeadline) {
        Start-Sleep -Milliseconds 200; $taskRestarted[0].Refresh()
    }
    if ($taskRestarted[0].HasExited -or !$taskRestarted[0].CloseMainWindow() -or !$taskRestarted[0].WaitForExit(15000)) {
        throw 'The restarted player did not show and close normally.'
    }
    Assert-CleanSentinel
    Write-Host 'PASS actual graphical setup waited, installed, retained Chrome and restarted Tide'
    # Exercise the exact installed production binary, not a separate feature
    # build, before claiming that the stable installer actually includes Jams.
    $taskJamOutput = Invoke-CleanChild (Join-Path $taskUpdatedRelease 'tide-music.exe') @('--smoke-jam-ownership') 'installed-jam-ownership' 60
    if ($taskJamOutput -notmatch 'PASS offline Jam ownership:') {
        throw 'Installed player did not include the verified Jam controls.'
    }
    Assert-CleanSentinel

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

    # Reuse the PCM header with a three-second payload for transition coverage.
    $taskShortWave = Join-Path $taskEvidence 'transition-fixture.wav'
    $taskShortBytes = [IO.File]::ReadAllBytes($taskWave)[0..48043]
    [BitConverter]::GetBytes([int]48036).CopyTo($taskShortBytes, 4)
    [BitConverter]::GetBytes([int]48000).CopyTo($taskShortBytes, 40)
    [IO.File]::WriteAllBytes($taskShortWave, [byte[]]$taskShortBytes)
    $taskPerformance = Join-Path $taskEvidence 'playback-performance.json'
    $taskResources = Join-Path $taskEvidence 'resource-samples.json'
    & (Join-Path $PSScriptRoot 'measure-process.ps1') -Executable (Join-Path $taskUpdatedRelease 'tide-music.exe') `
        -Seconds 90 -AppArguments @('--smoke-performance', $taskPerformance, $taskShortWave) `
        -Output $taskResources -RequireSuccessfulExit
    & python (Join-Path $PSScriptRoot 'check-performance.py') $taskResources $taskPerformance (Join-Path $taskEvidence 'performance-summary.json')
    if ($LASTEXITCODE -ne 0) { throw 'Installed local performance benchmark failed.' }
    Assert-CleanSentinel

    # Exercise the actual registered command, including the harder legacy case
    # where the invoking process starts inside the installation being removed.
    $taskUninstallCommand = (Get-TideUninstallEntry).UninstallString
    if ($taskUninstallCommand -notmatch '^"([^"]+)" (.+)$') { throw 'Registered uninstall command cannot be parsed.' }
    $taskUninstallExecutable = $Matches[1]; $taskUninstallArguments = $Matches[2]
    if ($taskUninstallExecutable -ne $taskPowerShell) { throw 'Registered uninstaller is not the expected Windows PowerShell.' }
    Invoke-CleanChild -Executable $taskUninstallExecutable -Arguments @() -RawArguments $taskUninstallArguments `
        -WorkingDirectory $taskInstallRoot -Label 'registered-uninstall' -TimeoutSeconds 120 | Out-Null
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
    [IO.File]::WriteAllText((Join-Path $taskEvidence 'result.txt'), 'PASS real clean Windows installation, background automatic update/restart, runtimes, synthetic Chrome registration, installed local-audio UI and removal. No browser or authenticated import tested.')
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
