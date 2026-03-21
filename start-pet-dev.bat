@echo off
setlocal

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0start-pet-dev.ps1" %*
if errorlevel 1 (
  echo.
  echo Script failed. Press any key to close...
  pause >nul
)
