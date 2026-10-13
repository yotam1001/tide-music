@echo off
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\install-windows.ps1" -Interactive
if errorlevel 1 echo Setup did not complete. Existing installations and saved sessions are preserved.
pause
