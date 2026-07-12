# One-shot release for the Tauri V2 app. Pure ASCII on purpose (PS 5.1).
#
# Flow: gates (cargo test + vitest) -> push source branch -> tauri build ->
# vpk pack -> publish to the DEDICATED v2 releases repo (never v1's):
#   https://github.com/AvinAlwis/azure-devops-test-case-manager-v2-releases
# Token comes from gh auth token in-process and is never printed.
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [switch]$SkipChecks
)
$ErrorActionPreference = "Stop"
$v2 = Split-Path -Parent $PSScriptRoot            # v2/
$repoUrl = "https://github.com/AvinAlwis/azure-devops-test-case-manager-v2-releases"

if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must be X.Y.Z, got '$Version'" }

# --- Gates -----------------------------------------------------------------
if (-not $SkipChecks) {
    Push-Location (Join-Path $v2 "src-tauri")
    cargo test
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw "cargo test failed" }
    Pop-Location
    Push-Location $v2
    npm test
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw "npm test failed" }
    Pop-Location
}

# --- Source first (private repo), then build -------------------------------
Push-Location $v2
git push origin HEAD
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "git push failed - source must be pushed before publishing" }

npm run tauri build
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "tauri build failed" }
Pop-Location

# --- Pack ------------------------------------------------------------------
& (Join-Path $PSScriptRoot "pack.ps1") -Version $Version

# --- Publish ---------------------------------------------------------------
$token = (gh auth token | Out-String).Trim()
if (-not $token) { throw "gh auth token returned nothing - run gh auth login" }
vpk upload github --repoUrl $repoUrl --publish --releaseName "v$Version" --tag "v$Version" --token $token --outputDir (Join-Path $v2 "Releases")
if ($LASTEXITCODE -ne 0) { throw "vpk upload failed with exit code $LASTEXITCODE" }

gh release view "v$Version" --repo AvinAlwis/azure-devops-test-case-manager-v2-releases
Write-Host "Released V2 v$Version"
