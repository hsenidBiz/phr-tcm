# One-shot release for the Tauri V2 app. Pure ASCII on purpose (PS 5.1).
#
# Flow: gates (cargo test + vitest + production build) -> push source branch -> tauri build ->
# vpk pack -> publish to the DEDICATED v2 releases repo (never v1's):
#   https://github.com/AvinAlwis/azure-devops-test-case-manager-v2-releases
# Token comes from gh auth token in-process and is never printed.
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [switch]$SkipChecks,
    # The DevOps branch the release is pushed to. Anything but main is a
    # rehearsal: the app only reads main unless TCM_UPDATE_BRANCH says
    # otherwise.
    [string]$DevOpsBranch = "main",
    # Rehearsals only: publish to the DevOps branch and NOT to GitHub.
    # Refused on main - a real release goes to both, or to neither.
    [switch]$SkipGitHub,
    # Rehearsals only: do not push the source branch. A rehearsal bumps the
    # version locally and reverts it afterwards; pushing that bump to
    # origin/master would be a lie about what was released.
    [switch]$SkipSourcePush
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

if ($SkipGitHub -and $DevOpsBranch -eq "main") {
    throw "-SkipGitHub is for rehearsals on a throwaway branch. A release to main goes to DevOps AND GitHub."
}
if ($SkipSourcePush -and $DevOpsBranch -eq "main") {
    throw "-SkipSourcePush is for rehearsals on a throwaway branch. A real release publishes the source it was built from."
}

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

# --- Source first (private repo), then build -------------------------------
Push-Location $v2
if ($SkipSourcePush) {
    Write-Host "Skipped the source push (-SkipSourcePush): rehearsal on $DevOpsBranch."
} else {
    git push origin HEAD
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw "git push failed - source must be pushed before publishing" }
}

npm run tauri build
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "tauri build failed" }
Pop-Location

# --- Pack ------------------------------------------------------------------
& (Join-Path $PSScriptRoot "pack.ps1") -Version $Version

# --- Publish: DevOps first, then GitHub ------------------------------------
# The app reads DevOps first and falls back to GitHub, and both are given
# every release: a fallback one version behind is worse than none, because
# it would offer a user without DevOps access a version they already have
# while the real one exists. DevOps goes first so that the failure that can
# leave the two apart (below) is the one with the clearer recovery.
$devopsRepo = "https://dev.azure.com/PeoplesHR/HRM/_git/PHR-TCM"
$clone = Join-Path $env:TEMP "phr-tcm-release"
if (Test-Path $clone) { Remove-Item -Recurse -Force $clone }
git clone --quiet --depth 1 --branch $DevOpsBranch $devopsRepo $clone
if ($LASTEXITCODE -ne 0) {
    # A branch that does not exist yet (a fresh rehearsal branch): start from
    # an empty clone. main always exists.
    if ($DevOpsBranch -eq "main") { throw "could not clone $devopsRepo" }
    # A failed clone can leave the directory behind, and the retry would then
    # die on "destination path already exists" rather than on anything real.
    if (Test-Path $clone) { Remove-Item -Recurse -Force $clone }
    git clone --quiet --depth 1 $devopsRepo $clone
    if ($LASTEXITCODE -ne 0) { throw "could not clone $devopsRepo" }
    Get-ChildItem $clone -Force | Where-Object { $_.Name -ne ".git" } | Remove-Item -Recurse -Force
}

node (Join-Path $PSScriptRoot "prune-releases.mjs") --repo $clone --pack (Join-Path $v2 "Releases") --keep 5
if ($LASTEXITCODE -ne 0) { throw "prune-releases failed with exit code $LASTEXITCODE" }
Copy-Item (Join-Path $PSScriptRoot "phr-tcm-README.md") (Join-Path $clone "README.md") -Force

# One orphan commit, force-pushed: the repo is a shelf, not a history, and
# a git history of 30 MB packages is the one thing that would make it grow.
Push-Location $clone
git checkout --quiet --orphan "release-$Version"
git add -A
git commit --quiet -m "release $Version"
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "git commit in the releases clone failed" }
git push --force --quiet origin "HEAD:$DevOpsBranch"
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "push to $devopsRepo ($DevOpsBranch) failed - nothing published anywhere" }
Pop-Location
Write-Host "Published v$Version to DevOps ($DevOpsBranch)"

if ($SkipGitHub) {
    Write-Host "Skipped GitHub (-SkipGitHub): this was a rehearsal on $DevOpsBranch."
} else {
    $token = (gh auth token | Out-String).Trim()
    if (-not $token) { throw "DevOps has v$Version but GitHub does not: gh auth token returned nothing - run gh auth login, then re-run only: vpk upload github --repoUrl $repoUrl --publish --releaseName v$Version --tag v$Version --outputDir v2\Releases" }
    vpk upload github --repoUrl $repoUrl --publish --releaseName "v$Version" --tag "v$Version" --token $token --outputDir (Join-Path $v2 "Releases")
    if ($LASTEXITCODE -ne 0) { throw "DevOps has v$Version but GitHub does not (vpk upload exit $LASTEXITCODE). The fallback is now a version behind - re-run only the vpk upload github step above until it succeeds." }
    gh release view "v$Version" --repo AvinAlwis/azure-devops-test-case-manager-v2-releases
}
# The caller's shell keeps whatever we set here, so put it back - on the
# error paths too, which is why this is a trap rather than a last line.
$me.ProcessorAffinity = $affinityWas
$me.PriorityClass = $priorWas
Write-Host "Released V2 v$Version"
