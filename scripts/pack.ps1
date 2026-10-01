# Pack the Tauri v2 build with Velopack. LOCAL ONLY - never publishes.
# Pure ASCII on purpose (PS 5.1 reads BOM-less UTF-8 as cp1252).
# -DeltaFrom <repo url> also makes a delta package: the release installs
# already have is downloaded first, and vpk pack diffs against it. Without
# it (or when that download fails) only the full package is made, which
# every install can still update from.
param(
    [string]$Version = "0.0.1",
    [string]$DeltaFrom = ""
)
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

# --- Delta base ------------------------------------------------------------
# A delta is only made against a full package found in Releases/, and only
# helps an install that is ON that version - Velopack falls back to the
# full package for everyone else. So the base is the release installs on
# this line are on now:
# - a stable: the newest stable (stable installs never see a beta);
# - a beta: the release published last, stable or beta - `vpk download
#   --pre` alone would take the newest PRERELEASE, which can be older than
#   the newest stable that beta installs have already moved to.
if ($DeltaFrom) {
    $isBeta = $Version -match '-beta\.\d+$'
    $pre = $false
    if ($isBeta) {
        $slug = $DeltaFrom -replace '^https://github\.com/', ''
        try {
            $last = gh release list -R $slug --limit 1 --json isPrerelease | ConvertFrom-Json
            $pre = [bool]($last -and $last[0].isPrerelease)
        } catch {
            Write-Warning "Could not tell whether the last release was a beta - diffing against the newest stable."
        }
    }
    $downloadArgs = @("download", "github", "--repoUrl", $DeltaFrom, "--outputDir", $out)
    if ($pre) { $downloadArgs += "--pre" }
    & vpk @downloadArgs
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "Could not download the previous release - packing the full package only, so updates download it in full."
        if (Test-Path $out) { Remove-Item -Recurse -Force $out }
    }
}

# packId is the app identity - never change it (existing installs update by
# it). packTitle is the human-readable name used for shortcuts / Add-Remove.
vpk pack --packId "AzureDevOpsTestCaseManager.V2" --packTitle "Test Case Manager" --packVersion $Version --packDir $stage --mainExe $exe.Name --outputDir $out --delta BestSize
if ($LASTEXITCODE -ne 0) { throw "vpk pack failed with exit code $LASTEXITCODE" }
Write-Host "Packed v$Version to Releases/"
$delta = Get-ChildItem $out -Filter "*-$Version-delta.nupkg" -ErrorAction SilentlyContinue
if ($delta) { Write-Host ("Delta package: {0:N1} MB" -f ($delta.Length / 1MB)) }
