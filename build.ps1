# Omni full build pipeline
# Usage: .\build.ps1

$ErrorActionPreference = "Stop"

Write-Host "=== Step 1: Build Rust (release) ===" -ForegroundColor Cyan
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "Rust build failed" }

Write-Host "=== Step 2: Generate TypeScript types (ts-rs) ===" -ForegroundColor Cyan
cargo ts-rs export --output-directory desktop/src/generated
if ($LASTEXITCODE -ne 0) { throw "TypeScript generation failed" }

Write-Host "=== Step 3: Build Nextron app ===" -ForegroundColor Cyan
Push-Location desktop
npm run build
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "Nextron build failed" }
Pop-Location

Write-Host "=== Step 4: Package installer (NSIS) ===" -ForegroundColor Cyan
if (-not (Test-Path "dist")) { New-Item -ItemType Directory -Path "dist" }
Push-Location installer
makensis installer.nsi
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "NSIS packaging failed" }
Pop-Location

Write-Host ""
Write-Host "=== Build complete ===" -ForegroundColor Green
Write-Host "Installer: dist\OmniSetup.exe"
