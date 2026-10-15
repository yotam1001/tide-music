@echo off
setlocal
cd /d "%SystemRoot%" || exit /b 1
set "PSModulePath=%SystemRoot%\System32\WindowsPowerShell\v1.0\Modules"
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\install-windows.ps1" -Mode Uninstall
set "tideSetupExit=%errorlevel%"
if not "%tideSetupExit%"=="0" echo Removal did not complete. Review the error above and retry after closing Tide.
pause
exit /b %tideSetupExit%
