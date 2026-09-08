# Run the E2E smoke against the release exe. Pure ASCII (PS 5.1).
# Prereq: npm run tauri build (or pass an exe path as the first arg).
param([string]$Exe = "")
$ErrorActionPreference = "Stop"
$v2 = Split-Path -Parent $PSScriptRoot
Push-Location $v2
if ($Exe) { node scripts/e2e-smoke.mjs $Exe } else { node scripts/e2e-smoke.mjs }
$code = $LASTEXITCODE
Pop-Location
if ($code -ne 0) { throw "e2e smoke failed with exit code $code" }
