@echo off
setlocal

set "ROOT=%~dp0"

rem best-effort cleanup to avoid locked .exe / occupied ports
taskkill /IM frontend.exe /F >nul 2>nul
taskkill /IM gateway-api.exe /F >nul 2>nul

if not exist "%ComSpec%" (
  echo ComSpec not found: %ComSpec%
  pause
  exit /b 1
)

where cargo >nul 2>nul
if errorlevel 1 (
  echo cargo not found. Please install Rust and make sure cargo is in PATH.
  pause
  exit /b 1
)

if not exist "%ROOT%backend\Cargo.toml" (
  echo backend project not found: %ROOT%backend\Cargo.toml
  pause
  exit /b 1
)
if not exist "%ROOT%frontend\Cargo.toml" (
  echo frontend project not found: %ROOT%frontend\Cargo.toml
  pause
  exit /b 1
)

echo ROOT=%ROOT%
echo.
echo Starting 2 windows: BACKEND-4317 / FRONTEND
echo Note: The pet usually appears as a tray icon (bottom-right). It may not show a normal window.
echo.

echo Note: Using Windows Terminal (wt) may fail on low memory machines.
echo This script uses cmd start by default.
echo.

start "BACKEND-4317" "%ComSpec%" /v:on /k "title BACKEND-4317 ^& pushd ""%ROOT%backend"" ^& set BACKEND_BIND=127.0.0.1:4317 ^& cargo run ^& echo. ^& echo [BACKEND] Exit code: !errorlevel! ^& pause"
start "FRONTEND" "%ComSpec%" /v:on /k "title FRONTEND ^& pushd ""%ROOT%frontend"" ^& set BACKEND_URL=http://127.0.0.1:4317 ^& cargo run ^& echo. ^& echo [FRONTEND] Exit code: !errorlevel! ^& pause"

echo If a window does not show up, try "Run as administrator".
echo If it still fails, screenshot the last lines of this window.
echo.
pause

endlocal
