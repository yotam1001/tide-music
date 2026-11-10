@echo off
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\install-grove.ps1" -Mode Uninstall
if errorlevel 1 pause
