# Publish How To Use beside the app's release: zip src-tauri/help, describe it
# in how-to-use.json, upload both to the phr-tcm release for the version. The
# app downloads them from there on demand (src-tauri/src/guide.rs). Not sent
# to the legacy feed.
# Called by release-v2.ps1 right after its phr-tcm Velopack upload; can be
# re-run alone if only this upload failed:
#   powershell -NoProfile -File scripts/publish-guide.ps1 -Version X.Y.Z
# -DryRun prints the fingerprint and each step and uploads nothing.
# Outputs go in Releases/ (the Velopack output folder, git-ignored).
# Pure ASCII on purpose (PS 5.1).
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [switch]$DryRun
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot          # repo root
$site = Join-Path $root "src-tauri\help"
$out = Join-Path $root "Releases"
$zip = Join-Path $out "how-to-use.zip"
$json = Join-Path $out "how-to-use.json"
$repo = "hsenidBiz/phr-tcm"
$tag = "v$Version"

if ($Version -notmatch '^\d+\.\d+\.\d+(-beta\.\d+)?$') { throw "Version must be X.Y.Z or X.Y.Z-beta.N, got '$Version'" }
if (-not (Test-Path (Join-Path $site "index.html"))) { throw "$site has no index.html - run npm run docs:build first" }

# The same rule guide.rs applies to what it unpacks, so an unchanged guide
# keeps its fingerprint from release to release.
$fp = (& node (Join-Path $PSScriptRoot "guide-fingerprint.mjs") $site | Out-String).Trim()
if ($LASTEXITCODE -ne 0) { throw "guide fingerprint failed with exit code $LASTEXITCODE" }
if ($fp -notmatch '^[0-9a-f]{64}$') { throw "guide fingerprint was not 64 lowercase hex: '$fp'" }

$uploadArgs = @("release", "upload", $tag, "Releases/how-to-use.zip", "Releases/how-to-use.json", "--repo", $repo, "--clobber")

if ($DryRun) {
    Write-Host "Guide dry run: $tag"
    Write-Host "  fingerprint: $fp"
    Write-Host "  1. zip   $site -> $zip (Optimal, no base directory)"
    Write-Host "  2. json  $json = { fingerprint: $fp, sha256: <of the zip>, size: <bytes of the zip> } (UTF-8, no BOM)"
    Write-Host "  3. gh $($uploadArgs -join ' ')"
    Write-Host "  nothing uploaded"
    return
}

if (-not (Test-Path $out)) { New-Item -ItemType Directory -Path $out | Out-Null }
if (Test-Path $zip) { Remove-Item -LiteralPath $zip -Force }
Add-Type -AssemblyName System.IO.Compression.FileSystem
[System.IO.Compression.ZipFile]::CreateFromDirectory($site, $zip, [System.IO.Compression.CompressionLevel]::Optimal, $false)

$sha = (Get-FileHash -Algorithm SHA256 -LiteralPath $zip).Hash.ToLowerInvariant()
$size = (Get-Item -LiteralPath $zip).Length
$text = '{ "fingerprint": "' + $fp + '", "sha256": "' + $sha + '", "size": ' + $size + ' }'
[System.IO.File]::WriteAllText($json, $text, (New-Object System.Text.UTF8Encoding $false))

Push-Location $root
try {
    & gh @uploadArgs
    if ($LASTEXITCODE -ne 0) {
        throw "the guide upload failed - re-run just: gh $($uploadArgs -join ' ')"
    }
} finally {
    Pop-Location
}
Write-Host "Published How To Use for $tag (fingerprint $fp)"
