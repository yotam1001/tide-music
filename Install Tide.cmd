@echo off
setlocal
set "PSModulePath=%SystemRoot%\System32\WindowsPowerShell\v1.0\Modules"
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\install-windows.ps1" -Interactive
set "tideSetupExit=%errorlevel%"
if not "%tideSetupExit%"=="0" echo Setup did not complete. Existing installations and saved sessions are preserved.
pause
exit /b %tideSetupExit%
