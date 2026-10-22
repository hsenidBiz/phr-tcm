# Ship a release: gates, push source, build, pack with Velopack, publish to
# the company repository's GitHub Releases (https://github.com/hsenidBiz/phr-tcm).
# Pure ASCII on purpose (PS 5.1).
# Token comes from gh auth token in-process and is never printed.
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [switch]$SkipChecks,
    [switch]$DryRun,
    [string]$PlaywrightClone
)
$ErrorActionPreference = "Stop"
trap {
    # Restore the shell we borrowed before the failure propagates.
    if ($me) {
        try { $me.ProcessorAffinity = $affinityWas; $me.PriorityClass = $priorWas } catch {}
    }
}
$v2 = Split-Path -Parent $PSScriptRoot            # repo root (the app used to live in v2/)
$repoUrl = "https://github.com/hsenidBiz/phr-tcm"

if ($Version -notmatch '^\d+\.\d+\.\d+(-beta\.\d+)?$') { throw "Version must be X.Y.Z or X.Y.Z-beta.N, got '$Version'" }
$IsBeta = $Version -match '-beta\.\d+$'


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

# A beta is published as a GitHub PRERELEASE on the same feed: installs
# read prereleases only when Settings > Download beta builds is on, so
# nothing on the stable line ever moves to a beta.
$uploadArgs = @("upload", "github", "--repoUrl", $repoUrl, "--publish", "--releaseName", "v$Version", "--tag", "v$Version")
if ($IsBeta) { $uploadArgs += "--pre" }
# Velopack packs only target\release\v2.exe. A beta skips Tauri's own
# installers (the MSI one refuses a non-numeric prerelease label).
$buildArgs = @("run", "tauri", "build")
if ($IsBeta) { $buildArgs += @("--", "--no-bundle") }
# The Playwright repo's rule files are compiled into the app from
# src-tauri/pw-rules (see snapshot-pw-rules.ps1). -PlaywrightClone overrides
# the clone set on the Playwright tab.
$snapshotArgs = @{}
if ($PlaywrightClone) { $snapshotArgs["Clone"] = $PlaywrightClone }
if ($DryRun) {
    Write-Host "Dry run: $Version (beta: $IsBeta)"
    Write-Host "  scripts/snapshot-pw-rules.ps1 (refuses a missing or dirty Playwright repo)"
    & (Join-Path $PSScriptRoot "snapshot-pw-rules.ps1") @snapshotArgs -Check
    Write-Host "  npm $($buildArgs -join ' ')"
    Write-Host "  scripts/pack.ps1 -Version $Version -DeltaFrom $repoUrl (downloads the previous release, packs full + delta)"
    Write-Host "  vpk $($uploadArgs -join ' ') --token *** --outputDir Releases"
    Write-Host "  then scripts/publish-guide.ps1 -Version ${Version} - zip src-tauri/help, write how-to-use.json, gh release upload to phr-tcm"
    return
}

# --- Playwright rules snapshot -----------------------------------------------
# Before the gates and the push, whatever -SkipChecks says: the app compiles
# src-tauri/pw-rules in, so a stale or uncommitted snapshot must stop the
# release here, before anything is built, pushed or published. The script
# itself refuses a missing or dirty Playwright repo.
& (Join-Path $PSScriptRoot "snapshot-pw-rules.ps1") @snapshotArgs
$snapshotChanged = @(git -C $v2 status --porcelain -- src-tauri/pw-rules)
if ($snapshotChanged.Count -gt 0) {
    throw "The Playwright rules snapshot changed: review src-tauri/pw-rules, commit it, and run the release again."
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

# --- Build cache -------------------------------------------------------------
# Cargo never deletes what it built, and src-tauri\target once reached 600 GB.
# Trim it before the gates build anything (see trim-cargo-cache.ps1). It only
# ever removes cache, and does nothing while another cargo is running.
& (Join-Path $PSScriptRoot "trim-cargo-cache.ps1")

# --- Gates -----------------------------------------------------------------
if (-not $SkipChecks) {
    Push-Location (Join-Path $v2 "src-tauri")
    cargo test -j $jobs -- --test-threads=$jobs
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw "cargo test failed" }
    Pop-Location
    Push-Location $v2
    npm test -- --maxWorkers=$jobs
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw "npm test failed" }
    # The PRODUCTION build, before the push - not just as a side effect of
    # `tauri build` afterwards. tsc and vitest never compile the CSS, so a
    # broken stylesheet passes both and only dies in the bundler. That is
    # exactly what happened on 1.17.8: a stray */ closed a comment early,
    # every test was green, the source went to master, and the build failed
    # a step later - leaving master briefly unbuildable, which is the one
    # thing the source-first rule exists to prevent.
    npm run build
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw "production build failed - nothing pushed" }
    Pop-Location
}

# --- Source first, then build -----------------------------------------
Push-Location $v2
# Both repositories carry the same source: the company one (origin) and
# the personal one. A release is never published from source only one has.
foreach ($remote in @("origin", "personal")) {
    git push $remote HEAD
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw "git push $remote failed - source must be pushed to both repositories before publishing" }
}

& npm @buildArgs
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "tauri build failed" }
Pop-Location

# --- Pack ------------------------------------------------------------------
# -DeltaFrom: updates download only what changed since the release
# installs are on now (see pack.ps1); the upload below publishes the delta
# with the full package.
& (Join-Path $PSScriptRoot "pack.ps1") -Version $Version -DeltaFrom $repoUrl

# --- Publish ---------------------------------------------------------------
# phr-tcm is the feed. A failed upload is fatal: half-published is the state
# that strands people.
$token = (gh auth token | Out-String).Trim()
if (-not $token) { throw "gh auth token returned nothing - run gh auth login" }
& vpk @uploadArgs --token $token --outputDir (Join-Path $v2 "Releases")
if ($LASTEXITCODE -ne 0) { throw "vpk upload failed with exit code $LASTEXITCODE" }
# How To Use is downloaded by the app from THIS release. Failure is fatal: an app without its guide asset shows
# "not available for this version". Re-run just: scripts/publish-guide.ps1
& (Join-Path $PSScriptRoot "publish-guide.ps1") -Version $Version
gh release view "v$Version" --repo hsenidBiz/phr-tcm
# The caller's shell keeps whatever we set here, so put it back - on the
# error paths too, which is why this is a trap rather than a last line.
$me.ProcessorAffinity = $affinityWas
$me.PriorityClass = $priorWas
Write-Host "Released V2 v$Version"
