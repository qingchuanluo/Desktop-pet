@echo off
setlocal

powershell -NoProfile -Command "Get-CimInstance Win32_Process | Where-Object { $_.Name -ieq 'python.exe' -and $_.CommandLine -match 'tts_service(_v2)?\\.py' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }"

taskkill /IM gateway-api.exe /F >nul 2>nul
taskkill /IM frontend.exe /F >nul 2>nul

endlocal
