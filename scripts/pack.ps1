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
#
# The base is found by its tag and fetched with gh, not `vpk download`: vpk
# reads only the newest page of releases, so a stable after a run of betas
# found "no releases" and 2.1.1 went out with no delta. vpk pack needs the
# base's full package plus a releases.win.json naming it.
if ($DeltaFrom) {
    $isBeta = $Version -match '-beta\.\d+$'
    $slug = $DeltaFrom -replace '^https://github\.com/', ''
    $base = $null
    try {
        $list = gh release list -R $slug --limit 100 --exclude-drafts --json tagName,isPrerelease | ConvertFrom-Json
        if ($LASTEXITCODE -ne 0) { throw "gh release list failed" }
        $base = $list | Where-Object { $_.tagName -ne "v$Version" -and ($isBeta -or -not $_.isPrerelease) } | Select-Object -First 1
    } catch {
        Write-Warning "Could not list the releases: $_"
    }
    $fetched = $false
    if ($base) {
        $tag = $base.tagName
        New-Item -ItemType Directory -Force $out | Out-Null
        gh release download $tag -R $slug -p "*-full.nupkg" -p "releases.win.json" -D $out --clobber
        $full = Get-ChildItem $out -Filter "*-full.nupkg" -ErrorAction SilentlyContinue | Select-Object -First 1
        $feedPath = Join-Path $out "releases.win.json"
        if ($LASTEXITCODE -eq 0 -and $full -and (Test-Path $feedPath)) {
            # Keep only the full package's entry: the base's own delta is not
            # downloaded, and a feed naming a missing file is not one to diff
            # against. Written without a BOM - vpk reads it as JSON.
            $feed = Get-Content $feedPath -Raw | ConvertFrom-Json
            $feed.Assets = @($feed.Assets | Where-Object { $_.Type -eq "Full" })
            if ($feed.Assets.Count -eq 1) {
                [IO.File]::WriteAllText($feedPath, ($feed | ConvertTo-Json -Depth 10), (New-Object Text.UTF8Encoding $false))
                Write-Host "Delta base: $tag ($($full.Name))"
                $fetched = $true
            }
        }
    }
    if (-not $fetched) {
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
elseif ($DeltaFrom) {
    # vpk download can find nothing and still exit 0, so say it here: the
    # release will go out with a full package only.
    Write-Warning "No delta package was made - updates to v$Version will download the full package."
}
