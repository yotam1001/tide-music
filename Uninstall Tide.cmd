@echo off
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\install-windows.ps1" -Mode Uninstall
if errorlevel 1 echo Removal did not complete. Review the error above and retry after closing Tide.
pause
