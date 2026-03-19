# AI 桌宠 - 一键启动脚本
# 使用方法: 右键 -> "使用 PowerShell 运行"

$ErrorActionPreference = "Continue"

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  启动 AI 桌宠服务" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# 检查 Python
try {
    $pythonVersion = python --version 2>&1
    Write-Host "[检查] Python: $pythonVersion" -ForegroundColor Green
} catch {
    Write-Host "[错误] 未找到 Python，请先安装 Python 3.8+" -ForegroundColor Red
    Read-Host "按回车退出"
    exit 1
}

# 检查 Python 依赖
Write-Host "[检查] 安装 Python 依赖..." -ForegroundColor Yellow
pip install -q torch soundfile numpy flask 2>$null
if ($LASTEXITCODE -eq 0) {
    Write-Host "[OK] 依赖已安装" -ForegroundColor Green
} else {
    Write-Host "[警告] 依赖安装有警告，但继续尝试启动..." -ForegroundColor Yellow
}

# 检查 Rust
$cargoPath = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargoPath) {
    Write-Host "[错误] 未找到 Cargo，请先安装 Rust" -ForegroundColor Red
    Read-Host "按回车退出"
    exit 1
}
Write-Host "[检查] Cargo: 已安装" -ForegroundColor Green

Write-Host ""
Write-Host "----------------------------------------" -ForegroundColor Cyan
Write-Host "  启动服务..." -ForegroundColor Cyan
Write-Host "----------------------------------------" -ForegroundColor Cyan

# 启动 TTS 服务
Write-Host "[1/2] 启动 TTS 服务 (端口 5000)..." -ForegroundColor Yellow
Start-Process powershell -ArgumentList "-NoExit", "-Command", "cd '$PWD'; python tts_service.py" -WindowStyle Normal -PassThru | Out-Null
Write-Host "      TTS 已启动 (新窗口)" -ForegroundColor Gray

# 等待一下让 TTS 启动
Start-Sleep -Seconds 2

# 启动后端服务
Write-Host "[2/2] 启动后端服务 (端口 4317)..." -ForegroundColor Yellow
Start-Process powershell -ArgumentList "-NoExit", "-Command", "cd '$PWD'; cargo run -p gateway-api --bin gateway-api" -WindowStyle Normal -PassThru | Out-Null
Write-Host "      后端已启动 (新窗口)" -ForegroundColor Gray

Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  服务已启动！" -ForegroundColor Green
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  - TTS 服务: http://localhost:5000" -ForegroundColor White
Write-Host "  - 后端 API: http://127.0.0.1:4317" -ForegroundColor White
Write-Host ""
Write-Host "  关闭此窗口不会停止服务" -ForegroundColor Gray
Write-Host "  如需停止服务，请关闭打开的命令行窗口" -ForegroundColor Gray
Write-Host ""

# 提示用户
Read-Host "按回车退出 (服务继续在后台运行)"
