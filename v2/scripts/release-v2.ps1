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

# -Version is only the Velopack tag. The version the APP reports - in the
# title bar, in a bug report, on the bridge's /ping, and to the "What's new"
# gate - comes from tauri.conf.json, and the changelog entry is what that
# gate looks for. Both are hand-edited in a bump commit before releasing,
# and 1.15.0 shipped without one: installable, updatable, and calling
# itself 1.14.1. Refuse rather than let that happen twice.
$confPath = Join-Path $v2 "src-tauri\tauri.conf.json"
$confVersion = (Get-Content $confPath -Raw | ConvertFrom-Json).version
if ($confVersion -ne $Version) {
    throw "tauri.conf.json says '$confVersion' but you asked to release '$Version'. Bump it (and add the changelog entry) and commit first."
}
$changelog = Get-Content (Join-Path $v2 "src\lib\changelog.ts") -Raw
if ($changelog -notmatch [regex]::Escape("version: `"$Version`"")) {
    throw "src/lib/changelog.ts has no entry for $Version - without one the update installs silently."
}

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
