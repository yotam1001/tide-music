@echo off
setlocal DisableDelayedExpansion
set "tideSetupScript=%~dp0scripts\install-windows.ps1"
cd /d "%SystemRoot%" || exit /b 1
set "PSModulePath=%SystemRoot%\System32\WindowsPowerShell\v1.0\Modules"
rem Parse the complete tail before removal deletes this installed wrapper.
rem Expand the saved path once so literal exclamation marks remain intact.
setlocal EnableDelayedExpansion
(
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "!tideSetupScript!" -Mode Uninstall
set "tideSetupExit=!errorlevel!"
if not "!tideSetupExit!"=="0" echo Removal did not complete. Review the error above and retry after closing Tide.
pause
rem Exit this removal console; returning to a deleted batch file is unsafe.
exit !tideSetupExit!
)
