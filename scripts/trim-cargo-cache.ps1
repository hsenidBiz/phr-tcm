# Keep src-tauri\target from growing without bound.
#
# Cargo never deletes what it built. Every test run leaves incremental
# session folders behind, and every dependency or toolchain change leaves the
# old compiled copies in deps\. Over a few weeks of test runs that reached
# 600 GB. Everything here is build cache: Cargo rebuilds whatever is missing,
# so the only cost of trimming is a slower next build.
#
# What it removes:
# - target\gate: a second whole build cache that old plans created with
#   CARGO_TARGET_DIR=target/gate. One target folder is enough.
# - incremental session folders not touched for -Days days, in every profile.
# - the whole incremental folder of a profile still above -MaxIncrementalGB
#   after that.
# - files in deps\ and build\ not touched for -StaleDays days (old dependency
#   versions and test binaries no build has used since).
#
# It never touches target\release\v2.exe or anything a build is using right
# now: run it when no cargo or rustc process is running (it checks).
#
#   .\scripts\trim-cargo-cache.ps1            # trim
#   .\scripts\trim-cargo-cache.ps1 -WhatIf    # say what it would remove

[CmdletBinding()]
param(
    [int]$Days = 3,
    [int]$StaleDays = 14,
    [int]$MaxIncrementalGB = 40,
    [switch]$WhatIf
)

$ErrorActionPreference = "Stop"
$target = Join-Path (Split-Path -Parent $PSScriptRoot) "src-tauri\target"
if (-not (Test-Path $target)) { Write-Host "No target folder - nothing to trim."; return }

if (Get-Process cargo, rustc -ErrorAction SilentlyContinue) {
    Write-Host "A cargo or rustc process is running - not trimming the build cache now."
    return
}

function Get-SizeBytes([string]$path) {
    if (-not (Test-Path $path)) { return 0 }
    $sum = (Get-ChildItem $path -Recurse -File -Force -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
    if ($null -eq $sum) { 0 } else { $sum }
}

function Remove-Tree([string]$path) {
    if ($WhatIf) { Write-Host "would remove $path"; return }
    # rd copes with very deep trees and long paths better than Remove-Item.
    cmd /c rd /s /q "`"$path`"" 2>$null
}

$before = Get-SizeBytes $target
$now = Get-Date

# 1. The second build cache.
$gate = Join-Path $target "gate"
if (Test-Path $gate) { Remove-Tree $gate }

# 2. Incremental folders, per profile (debug, release, and any other).
foreach ($profile in Get-ChildItem $target -Directory -ErrorAction SilentlyContinue) {
    $inc = Join-Path $profile.FullName "incremental"
    if (-not (Test-Path $inc)) { continue }
    foreach ($session in Get-ChildItem $inc -Directory -ErrorAction SilentlyContinue) {
        if (($now - $session.LastWriteTime).TotalDays -gt $Days) { Remove-Tree $session.FullName }
    }
    if ((Get-SizeBytes $inc) -gt ($MaxIncrementalGB * 1GB)) { Remove-Tree $inc }
}

# 3. Superseded compiled copies. deps\ and build\ name everything
#    <crate>-<hash>; a changed dependency, feature set or toolchain leaves the
#    old hash behind for good. A file's write time is when it was BUILT, not
#    when it was last used (a dependency built once is reused for weeks), so
#    age alone would throw away copies still in use. Only a hash whose crate
#    has a NEWER hash, and which itself has not been rebuilt for -StaleDays
#    days, is removed. A copy still in use that goes anyway is just rebuilt.
foreach ($profile in Get-ChildItem $target -Directory -ErrorAction SilentlyContinue) {
    foreach ($sub in "deps", "build") {
        $dir = Join-Path $profile.FullName $sub
        if (-not (Test-Path $dir)) { continue }
        $copies = Get-ChildItem $dir -Force -ErrorAction SilentlyContinue |
            Where-Object { $_.BaseName -match '^(.+)-([0-9a-f]{16})$' } |
            ForEach-Object {
                $null = $_.BaseName -match '^(.+)-([0-9a-f]{16})$'
                [pscustomobject]@{ File = $_; Crate = $Matches[1]; Hash = $Matches[2] }
            }
        foreach ($crate in $copies | Group-Object Crate) {
            $hashes = $crate.Group | Group-Object Hash | ForEach-Object {
                [pscustomobject]@{
                    Items  = @($_.Group | ForEach-Object { $_.File })
                    # Sort, not Measure-Object: PowerShell 5.1 cannot take the maximum of a date.
                    Newest = (@($_.Group | ForEach-Object { $_.File }) | Sort-Object LastWriteTime | Select-Object -Last 1).LastWriteTime
                }
            } | Sort-Object Newest -Descending
            if ($hashes.Count -lt 2) { continue }
            foreach ($old in $hashes | Select-Object -Skip 1) {
                if (($now - $old.Newest).TotalDays -le $StaleDays) { continue }
                foreach ($item in $old.Items) {
                    if ($item.PSIsContainer) { Remove-Tree $item.FullName }
                    elseif ($WhatIf) { Write-Host "would remove $($item.FullName)" }
                    else { Remove-Item $item.FullName -Force -ErrorAction SilentlyContinue }
                }
            }
        }
    }
}

$after = Get-SizeBytes $target
$freed = [math]::Round(($before - $after) / 1GB, 1)
$left = [math]::Round($after / 1GB, 1)
if ($WhatIf) { Write-Host ("Build cache is {0} GB (dry run, nothing removed)." -f [math]::Round($before / 1GB, 1)) }
else { Write-Host ("Build cache trimmed: freed {0} GB, {1} GB left." -f $freed, $left) }
