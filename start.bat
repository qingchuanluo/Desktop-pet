@echo off
chcp 65001 >nul
echo ========================================
echo   启动 AI 桌宠服务
echo ========================================
echo.

echo [1/1] 启动后端服务 (Rust) ...
start "Backend Service" cmd /k "cd /d %~dp0 && cargo run -p gateway-api --bin gateway-api"

echo.
echo ========================================
echo   服务启动中...
echo   - 后端: http://127.0.0.1:4317
echo ========================================
echo.
echo 按任意键退出 (服务将继续在后台运行)...
pause >nul
