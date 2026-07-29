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
trap {
    # Restore the shell we borrowed before the failure propagates.
    if ($me) {
        try { $me.ProcessorAffinity = $affinityWas; $me.PriorityClass = $priorWas } catch {}
    }
}
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
# Cargo's own version is not what the app reports at runtime - that comes
# from tauri.conf.json - so this drifted to 1.4.0 and nobody noticed for
# many releases. It still shows up in build output and crate metadata, and
# two version numbers disagreeing is the kind of thing that misleads exactly
# when you are trying to work out which build you are looking at.
$cargoHead = (Get-Content (Join-Path $v2 "src-tauri\Cargo.toml") -Raw) -split "\[dependencies\]" | Select-Object -First 1
if ($cargoHead -notmatch 'version\s*=\s*"([^"]+)"') { throw "Could not read the version from src-tauri/Cargo.toml" }
if ($Matches[1] -ne $Version) {
    throw "src-tauri/Cargo.toml says '$($Matches[1])' but you asked to release '$Version'. Bump it too."
}

$changelog = Get-Content (Join-Path $v2 "src\lib\changelog.ts") -Raw
if ($changelog -notmatch [regex]::Escape("version: `"$Version`"")) {
    throw "src/lib/changelog.ts has no entry for $Version - without one the update installs silently."
}

# --- Keep the machine usable -----------------------------------------------
# A release compiles the whole Rust tree in release mode and will otherwise
# take every core, leaving the desktop unresponsive for minutes.
#
# CARGO_BUILD_JOBS alone is NOT enough, and it was tried first: it caps how
# many rustc PROCESSES run at once, and nothing else. Each rustc is itself
# multi-threaded, the linker is heavily threaded and ignores -j entirely, and
# the vite build on the front half has its own workers. The result printed
# "16 of 24 cores" and still pegged all 24.
#
# ProcessorAffinity is the ceiling that actually holds: a child process
# inherits its parent's mask on Windows, so setting it here bounds cargo,
# rustc, the linker, node and everything else this script starts. Priority is
# the other half - the cores it DOES use yield to whatever is in the
# foreground, so the machine stays responsive rather than merely 30% idle.
$total = [Environment]::ProcessorCount
$jobs = [Math]::Max(1, [Math]::Floor($total * 0.7))
$me = [System.Diagnostics.Process]::GetCurrentProcess()
$priorWas = $me.PriorityClass
$affinityWas = $me.ProcessorAffinity
# Low $jobs bits set: cores 0..$jobs-1.
$me.ProcessorAffinity = [IntPtr]([int64][Math]::Pow(2, $jobs) - 1)
$me.PriorityClass = [System.Diagnostics.ProcessPriorityClass]::BelowNormal
$env:CARGO_BUILD_JOBS = $jobs
Write-Host "Building on $jobs of $total cores at below-normal priority."

# --- Gates -----------------------------------------------------------------
if (-not $SkipChecks) {
    Push-Location (Join-Path $v2 "src-tauri")
    cargo test -j $jobs -- --test-threads=$jobs
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw "cargo test failed" }
    Pop-Location
    Push-Location $v2
    npm test -- --maxWorkers=$jobs
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
# The caller's shell keeps whatever we set here, so put it back - on the
# error paths too, which is why this is a trap rather than a last line.
$me.ProcessorAffinity = $affinityWas
$me.PriorityClass = $priorWas
Write-Host "Released V2 v$Version"
