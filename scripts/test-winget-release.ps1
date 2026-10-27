# Actual release installation test. Run ONLY on a disposable Windows CI runner.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows') {
    throw 'This test is restricted to a disposable Windows GitHub Actions runner.'
}
$taskKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\TideMusic'
$taskInstallRoot = Join-Path $env:LOCALAPPDATA 'Programs/Tide'
if ((Test-Path -LiteralPath $taskKey) -or (Test-Path -LiteralPath $taskInstallRoot)) {
    throw 'Runner already has Tide; refusing to change an existing installation.'
}
$taskManifest = Join-Path $PSScriptRoot '../packaging/winget/0.4.5'
$taskLogs = Join-Path $PSScriptRoot '../.local/winget-smoke'
[IO.Directory]::CreateDirectory($taskLogs) | Out-Null
$taskInstallerManifest = Get-Content (Join-Path $taskManifest 'TideMusic.Tide.installer.yaml') -Raw
$taskUrl = [regex]::Match($taskInstallerManifest, '(?m)^  InstallerUrl: (https://[^\r\n]+)$').Groups[1].Value
$taskExpectedHash = [regex]::Match($taskInstallerManifest, '(?m)^  InstallerSha256: ([A-Fa-f0-9]{64})$').Groups[1].Value
if (!$taskUrl -or !$taskExpectedHash) { throw 'Manifest URL or hash is missing.' }
$taskInstaller = Join-Path $taskLogs 'TideSetup-winget.exe'
Invoke-WebRequest -Uri $taskUrl -OutFile $taskInstaller -TimeoutSec 120
if ((Get-FileHash -LiteralPath $taskInstaller -Algorithm SHA256).Hash -ne $taskExpectedHash) {
    throw 'Public installer hash does not match the manifest.'
}
winget validate --manifest $taskManifest --disable-interactivity
if ($LASTEXITCODE -ne 0) { throw 'Manifest validation failed.' }
winget settings --enable LocalManifestFiles
if ($LASTEXITCODE -ne 0) { throw 'Could not enable local manifests on the test runner.' }
winget install --manifest $taskManifest --silent --accept-package-agreements --accept-source-agreements --disable-interactivity --verbose-logs
if ($LASTEXITCODE -ne 0) { throw "WinGet installation failed: $LASTEXITCODE" }
$taskEntry = Get-ItemProperty -LiteralPath $taskKey
if ($taskEntry.DisplayName -ne 'Tide' -or $taskEntry.Publisher -ne 'Tide contributors' -or $taskEntry.DisplayVersion -ne '0.4.5') {
    throw 'Windows Apps metadata differs from the package manifest.'
}
$taskEntry | Select-Object DisplayName, Publisher, DisplayVersion, InstallLocation | ConvertTo-Json | Set-Content (Join-Path $taskLogs 'installed-metadata.json')
$taskState = Get-Content (Join-Path $taskInstallRoot '.tide-install.json') -Raw | ConvertFrom-Json
$taskRelease = Join-Path $taskInstallRoot ('releases/' + $taskState.Release)
foreach ($taskRelative in @('tide-music.exe', 'tools/mpv.exe', 'tools/yt-dlp.exe', 'tools/deno.exe', '.tools/provider-venv/Scripts/python.exe')) {
    if (!(Test-Path -LiteralPath (Join-Path $taskRelease $taskRelative))) { throw "Missing installed dependency: $taskRelative" }
}
& (Join-Path $taskRelease 'tools/mpv.exe') --version
if ($LASTEXITCODE -ne 0) { throw 'mpv cannot start.' }
& (Join-Path $taskRelease 'tools/yt-dlp.exe') --version
if ($LASTEXITCODE -ne 0) { throw 'yt-dlp cannot start.' }
& (Join-Path $taskRelease '.tools/provider-venv/Scripts/python.exe') -I -c 'import ytmusicapi, requests; assert callable(ytmusicapi.YTMusic)'
if ($LASTEXITCODE -ne 0) { throw 'Managed Python dependencies cannot load.' }
winget list --name Tide --exact --accept-source-agreements --disable-interactivity | Tee-Object (Join-Path $taskLogs 'winget-list.txt')
if ($LASTEXITCODE -ne 0) { throw 'WinGet did not detect the installed application.' }
# The package is not yet in the public catalog, so WinGet's list ID may be its
# ARP key (TideMusic). Name, publisher and version are asserted directly above.
$taskRepeat = Start-Process -FilePath $taskInstaller -ArgumentList '--silent' -WindowStyle Hidden -PassThru -Wait
if ($taskRepeat.ExitCode -ne 0) { throw "Repeat installation failed: $($taskRepeat.ExitCode)" }
winget uninstall --name Tide --exact --silent --accept-source-agreements --disable-interactivity --verbose-logs
if ($LASTEXITCODE -ne 0) { throw "WinGet uninstall failed: $LASTEXITCODE" }
if (Test-Path -LiteralPath $taskKey) { throw 'Windows Apps entry remains after uninstall.' }
if (Test-Path -LiteralPath (Join-Path $taskInstallRoot '.tide-install.json')) { throw 'Installation record remains after uninstall.' }
'PASS: public hash, schema, silent install, dependency startup, Apps metadata, WinGet detection, repeat install, and silent uninstall' |
    Tee-Object (Join-Path $taskLogs 'result.txt')
