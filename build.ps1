# Build + package the app with PyInstaller (one-folder) and Velopack.
#
# Prerequisites (one-time):
#   - Python deps:  pip install -r requirements.txt
#   - .NET SDK + Velopack CLI:  dotnet tool install -g vpk
#   - To publish (-Upload): a GitHub token with write access to the PUBLIC
#     releases repo (azure-devops-test-case-creator-releases), via -Token or
#     the GITHUB_TOKEN env var.
#
# Usage:
#   .\build.ps1            # build installer + update packages into .\Releases
#   .\build.ps1 -Upload    # also publish a GitHub Release to the releases repo
#
# Release flow: bump VERSION in app\version.py -> .\build.ps1 -Upload.
# Installed clients pick up the new release on next launch (delta if possible).

param(
    [switch]$Upload,
    [string]$Token = $env:GITHUB_TOKEN
)

$ErrorActionPreference = "Stop"

$RepoUrl = "https://github.com/AvinAlwis/azure-devops-test-case-creator-releases"
$PackId  = "AzureDevOpsTestCaseCreator"
$MainExe = "AzureDevOpsTestCaseCreator.exe"
$Title   = "Azure DevOps Test Case Creator"

# 1. Version — single source of truth in app\version.py
$Version = (Select-String -Path "app\version.py" -Pattern 'VERSION\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
if (-not $Version) { throw "Could not read VERSION from app\version.py" }
Write-Host "Building $Title v$Version" -ForegroundColor Cyan

# 2. Build dependencies
pip install -r requirements.txt --quiet
pip install pyinstaller pillow --quiet

# 3. Icon: convert resources\icon.png -> multi-resolution .ico if needed
if ((Test-Path "resources\icon.png") -and -not (Test-Path "resources\icon.ico")) {
    python -c "from PIL import Image; Image.open('resources/icon.png').convert('RGBA').save('resources/icon.ico', sizes=[(16,16),(32,32),(48,48),(64,64),(128,128),(256,256)])"
}

# 4. PyInstaller one-folder build -> dist\AzureDevOpsTestCaseCreator\
pyinstaller --clean --noconfirm devops_test_case_creator.spec
if (-not (Test-Path "dist\$PackId\$MainExe")) { throw "PyInstaller output missing: dist\$PackId\$MainExe" }

# 5. Velopack pack -> .\Releases (Setup.exe + full/delta .nupkg packages).
#    A delta is generated automatically when a previous release is in .\Releases.
#    VPK_NO_PORTABLE skips building the portable .zip so the GitHub release
#    carries only the installer + the update packages the auto-updater needs.
#    (Deleting the zip *after* packing breaks `vpk upload`, which still expects
#    it to be present, so we never build it in the first place.)
$env:VPK_NO_PORTABLE = "true"
$packArgs = @(
    "pack",
    "--packId",      $PackId,
    "--packVersion", $Version,
    "--packDir",     "dist\$PackId",
    "--mainExe",     $MainExe,
    "--packTitle",   $Title
)
if (Test-Path "resources\icon.ico") { $packArgs += @("--icon", "resources\icon.ico") }
vpk @packArgs

# 6. Optionally publish a GitHub Release on the public releases repo
if ($Upload) {
    if (-not $Token) { throw "Set -Token or `$env:GITHUB_TOKEN to upload (needs write access to the releases repo)." }
    vpk upload github --repoUrl $RepoUrl --publish --releaseName "v$Version" --tag "v$Version" --token $Token
    Write-Host "Published v$Version to $RepoUrl" -ForegroundColor Green
} else {
    Write-Host "Built .\Releases (Setup.exe + packages). Re-run with -Upload to publish." -ForegroundColor Green
}
