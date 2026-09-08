# Pack the Tauri v2 build with Velopack. LOCAL ONLY - never publishes.
# Pure ASCII on purpose (PS 5.1 reads BOM-less UTF-8 as cp1252).
param([string]$Version = "0.0.1")
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot          # repo root
$exeDir = Join-Path $root "src-tauri\target\release"
$exe = Get-ChildItem $exeDir -Filter "*.exe" | Where-Object { $_.Name -notmatch "setup" } | Select-Object -First 1
if (-not $exe) { throw "No release exe found in $exeDir - run 'npm run tauri build' first." }
# Releases/ is a local staging dir for upload - stale packs make vpk refuse
# to build the same version again, so start clean every time.
$out = Join-Path $root "Releases"
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
$stage = Join-Path $env:TEMP "tcm-v2-pack"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item $exe.FullName $stage
# packId is the app identity - never change it (existing installs update by
# it). packTitle is the human-readable name used for shortcuts / Add-Remove.
vpk pack --packId "AzureDevOpsTestCaseManager.V2" --packTitle "Test Case Manager" --packVersion $Version --packDir $stage --mainExe $exe.Name --outputDir (Join-Path $root "Releases")
if ($LASTEXITCODE -ne 0) { throw "vpk pack failed with exit code $LASTEXITCODE" }
Write-Host "Packed v$Version to Releases/"
