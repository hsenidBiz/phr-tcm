# One-shot release: checks -> version bump -> push source -> build -> verify -> publish.
#
# Encodes the full runbook that was previously assembled by hand each release
# (see claudedocs/reflection-notes.md #1). Two-step rule preserved: source is
# committed + pushed to the PRIVATE master BEFORE anything is published to the
# PUBLIC releases repo.
#
# Usage (from the repo root):
#   .\scripts\release.ps1                     # patch bump  (3.2.1 -> 3.2.2)
#   .\scripts\release.ps1 -Bump minor         # 3.2.1 -> 3.3.0
#   .\scripts\release.ps1 -Bump major         # 3.2.1 -> 4.0.0
#   .\scripts\release.ps1 -Version 3.5.0      # explicit version
#   .\scripts\release.ps1 -SkipChecks         # skip ruff + pytest gate
#
# Requires: gh (authenticated), vpk, python. GITHUB_TOKEN env is NOT needed -
# the token is taken from `gh auth token` in-process and never printed.

param(
    [ValidateSet("patch", "minor", "major")]
    [string]$Bump = "patch",
    [string]$Version = "",
    [switch]$SkipChecks
)

# "Continue" + explicit exit checks: PS 5.1-safe (see build.ps1 header note).
$ErrorActionPreference = "Continue"

function Assert-LastExit([string]$step) {
    if ($LASTEXITCODE -ne 0) { throw "$step failed (exit $LASTEXITCODE)" }
}
function Info([string]$msg) { Write-Host $msg -ForegroundColor Cyan }

$RepoRoot    = Split-Path -Parent $PSScriptRoot
$ReleasesUrl = "https://github.com/AvinAlwis/azure-devops-test-case-manager-releases"
Set-Location $RepoRoot

# ---------------------------------------------------------------- preflight --
Info "== Preflight =="
$branch = (git rev-parse --abbrev-ref HEAD).Trim()
Assert-LastExit "git rev-parse"
if ($branch -ne "master") { throw "Releases ship from master (currently on '$branch')." }

# Tracked changes block the release; untracked files (claudedocs/ etc.) do not.
$dirty = git status --porcelain | Where-Object { $_ -notmatch '^\?\?' }
if ($dirty) { throw "Working tree has uncommitted tracked changes:`n$($dirty -join "`n")" }

git fetch origin master --quiet
Assert-LastExit "git fetch"
$behind = (git rev-list --count "HEAD..origin/master").Trim()
if ($behind -ne "0") { throw "master is $behind commit(s) behind origin - pull first." }

$tok = (gh auth token).Trim()
if (-not $tok) { throw "gh auth token returned nothing - run 'gh auth login'." }
$null = Get-Command vpk -ErrorAction SilentlyContinue
if (-not $?) { throw "vpk not found on PATH (dotnet tool install -g vpk)." }

# ------------------------------------------------------------------- checks --
if (-not $SkipChecks) {
    Info "== Checks: ruff + pytest =="
    python -m ruff check .
    Assert-LastExit "ruff"
    python -m pytest -q
    Assert-LastExit "pytest"
} else {
    Write-Host "Skipping ruff/pytest (-SkipChecks)" -ForegroundColor Yellow
}

# ------------------------------------------------------------- version bump --
$verFile = Join-Path $RepoRoot "app\version.py"
$current = (Select-String -Path $verFile -Pattern 'VERSION\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
if (-not $current) { throw "Could not read VERSION from app\version.py" }

if ($Version) {
    if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "-Version must be X.Y.Z" }
    $new = $Version
} else {
    $p = $current.Split(".")
    switch ($Bump) {
        "major" { $new = "{0}.0.0" -f ([int]$p[0] + 1) }
        "minor" { $new = "{0}.{1}.0" -f $p[0], ([int]$p[1] + 1) }
        default { $new = "{0}.{1}.{2}" -f $p[0], $p[1], ([int]$p[2] + 1) }
    }
}
Info "== Version: $current -> $new =="
(Get-Content $verFile -Raw) -replace ('VERSION\s*=\s*"' + [regex]::Escape($current) + '"'), ('VERSION = "' + $new + '"') |
    Set-Content $verFile -Encoding utf8 -NoNewline

# --------------------------------------------- step 1: source to private master --
Info "== Commit + push source (private master) =="
git add app/version.py
Assert-LastExit "git add"
git commit -m "chore(release): bump version to $new"
Assert-LastExit "git commit"
git push origin master
Assert-LastExit "git push"

# -------------------------------------------------------- step 2a: build + pack --
Info "== Build (PyInstaller + vpk pack) =="
& (Join-Path $RepoRoot "build.ps1")   # 5.1-safe; throws on any failed step

# ------------------------------------------------------------ verify artifacts --
Info "== Verify artifacts =="
$setup = Join-Path $RepoRoot "Releases\AzureDevOpsTestCaseCreator-win-Setup.exe"
$nupkg = Get-ChildItem (Join-Path $RepoRoot "Releases") -Filter "*$new*.nupkg" -ErrorAction SilentlyContinue
if (-not (Test-Path $setup)) { throw "Setup.exe missing from .\Releases" }
if (-not $nupkg) { throw "No $new .nupkg in .\Releases - pack failed?" }

# Frozen-build QtSvg gate: the icon set is SVG; a build missing the Qt SVG
# pieces ships with blank icons (hit once - always gate before publishing).
$internal = Join-Path $RepoRoot "dist\AzureDevOpsTestCaseCreator\_internal"
$svgCount = (Get-ChildItem (Join-Path $internal "resources\icons") -Filter "*.svg" -ErrorAction SilentlyContinue).Count
foreach ($piece in @("qsvg.dll", "qsvgicon.dll", "QtSvg.pyd")) {
    if (-not (Get-ChildItem $internal -Recurse -Filter $piece -ErrorAction SilentlyContinue)) {
        throw "QtSvg gate: $piece missing from the frozen build"
    }
}
if ($svgCount -lt 40) { throw "QtSvg gate: only $svgCount SVG icons bundled (expected 40+)" }
Info "QtSvg gate OK ($svgCount icons)"

# ------------------------------------------------------------ step 2b: publish --
Info "== Publish v$new =="
vpk upload github --repoUrl $ReleasesUrl --publish --releaseName "v$new" --tag "v$new" --token $tok
Assert-LastExit "vpk upload github"

gh release view "v$new" --repo $ReleasesUrl
Assert-LastExit "gh release view"
Write-Host "`nReleased v$new (source pushed, installer published, artifacts verified)." -ForegroundColor Green
