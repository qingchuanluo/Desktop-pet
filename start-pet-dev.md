## 启动脚本

文件：
- `start-pet-dev.ps1`
- `start-pet-dev.bat`

默认行为：先编译（Rust + Java），然后分别打开 3 个窗口启动：
- Desktop-pet：`gateway-api`
- pet-store-service：Spring Boot
- Desktop-pet：`frontend`

推荐运行（双击）：
- `start-pet-dev.bat`

运行（PowerShell）：

```powershell
powershell -ExecutionPolicy Bypass -File ".\\start-pet-dev.ps1"
```

常用参数：

```powershell
# 先清理，再编译，再启动
powershell -ExecutionPolicy Bypass -File ".\\start-pet-dev.ps1" -Clean

# 只编译（不启动）
powershell -ExecutionPolicy Bypass -File ".\\start-pet-dev.ps1" -BuildOnly

# 跳过编译（只启动）
powershell -ExecutionPolicy Bypass -File ".\\start-pet-dev.ps1" -NoBuild

# 改后端监听地址（gateway-api / frontend）
powershell -ExecutionPolicy Bypass -File ".\\start-pet-dev.ps1" -BackendBind "127.0.0.1:4317"
```

如果你是在 IDE 的内置终端里运行，可能会因为沙箱/权限导致桌宠窗口不弹出来。推荐直接在系统的 PowerShell/Windows Terminal 里运行，或双击 bat。

## 清理编译产物

Rust（Desktop-pet）：

```powershell
cd ".\\"
cargo clean
```

如果你项目里有额外的 `target_alt\\`：

```powershell
Remove-Item -LiteralPath ".\\target_alt" -Recurse -Force
```

Java / Maven（pet-store-service）：

```powershell
cd "..\\pet-store-service"
mvn -DskipTests clean
```
