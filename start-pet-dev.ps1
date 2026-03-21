param(
    [string]$DesktopPetPath = $PSScriptRoot,
    [string]$PetStoreServicePath = (Join-Path $PSScriptRoot "..\\pet-store-service"),
    [string]$BackendBind = "127.0.0.1:4317",
    [switch]$Clean,
    [switch]$BuildOnly,
    [switch]$NoBuild
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $DesktopPetPath)) {
    throw "Desktop-pet path not found: $DesktopPetPath"
}
if (-not (Test-Path -LiteralPath $PetStoreServicePath)) {
    throw "pet-store-service path not found: $PetStoreServicePath"
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "cargo not found. Install Rust and ensure cargo is in PATH."
}
if (-not (Get-Command mvn -ErrorAction SilentlyContinue)) {
    throw "mvn not found. Install Maven and ensure mvn is in PATH."
}

$backendUrl = "http://$BackendBind"

function Invoke-InDir {
    param(
        [Parameter(Mandatory = $true)][string]$Cwd,
        [Parameter(Mandatory = $true)][string]$Command
    )
    Push-Location -LiteralPath $Cwd
    try {
        & powershell -NoProfile -Command $Command
        if ($LASTEXITCODE -ne 0) {
            throw "Command failed ($LASTEXITCODE): $Command"
        }
    }
    finally {
        Pop-Location
    }
}

function Start-DevWindow {
    param(
        [Parameter(Mandatory = $true)][string]$Title,
        [Parameter(Mandatory = $true)][string]$Cwd,
        [Parameter(Mandatory = $true)][string]$Command,
        [hashtable]$Env = @{}
    )

    $envPrefix = ""
    foreach ($k in $Env.Keys) {
        $v = [string]$Env[$k]
        $escaped = $v.Replace('`', '``').Replace('"', '""')
        $envPrefix += "`$env:$k=`"$escaped`";"
    }

    $escapedTitle = $Title.Replace('`', '``').Replace("'", "''")
    $full = "$envPrefix`$host.UI.RawUI.WindowTitle = '$escapedTitle'; $Command"
    Start-Process -FilePath "powershell" -WorkingDirectory $Cwd -ArgumentList @("-NoExit", "-Command", $full) | Out-Null
}

if ($Clean) {
    Invoke-InDir -Cwd $DesktopPetPath -Command "cargo clean"
    $targetAlt = Join-Path $DesktopPetPath "target_alt"
    if (Test-Path -LiteralPath $targetAlt) {
        Remove-Item -LiteralPath $targetAlt -Recurse -Force -ErrorAction Stop
    }
    Invoke-InDir -Cwd $PetStoreServicePath -Command "mvn -DskipTests clean"
}

if (-not $NoBuild) {
    Invoke-InDir -Cwd $DesktopPetPath -Command "cargo build -p gateway-api"
    Invoke-InDir -Cwd $DesktopPetPath -Command "cargo build -p frontend"
    Invoke-InDir -Cwd $PetStoreServicePath -Command "mvn -DskipTests package"
}

if ($BuildOnly) {
    Write-Output "Build finished."
    exit 0
}

Start-DevWindow -Title "DesktopPet gateway-api" -Cwd $DesktopPetPath -Command "cargo run -p gateway-api" -Env @{ BACKEND_BIND = $BackendBind }
Start-DevWindow -Title "pet-store-service" -Cwd $PetStoreServicePath -Command "mvn -DskipTests spring-boot:run"
Start-Sleep -Seconds 2
Start-DevWindow -Title "DesktopPet frontend" -Cwd $DesktopPetPath -Command "cargo run -p frontend" -Env @{ BACKEND_BIND = $BackendBind; BACKEND_URL = $backendUrl }

Write-Output "Started: gateway-api, pet-store-service, frontend"
