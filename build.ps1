# Compile l'interface puis l'exécutable, et copie le résultat dans app\mercure.exe
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path

Push-Location "$root\frontend"
npm ci
npm test
npm run build
Pop-Location

Push-Location "$root\backend"
cargo test
cargo build --release
Pop-Location

New-Item -ItemType Directory -Force -Path "$root\app" | Out-Null
Copy-Item "$root\backend\target\release\mercure.exe" "$root\app\mercure.exe" -Force
Write-Host "OK : $root\app\mercure.exe"
