# The one-command portability gate: copy Out/ elsewhere (excluding target/ and work/),
# run the test suite and the consumer simulation FROM THE COPY, clean up, exit nonzero on
# any failure. Proves the bundle carries everything it needs.
param()
$ErrorActionPreference = "Stop"

$root = Split-Path $PSScriptRoot -Parent          # .../Out
$copy = Join-Path $env:TEMP "rpt-out-copy"
if (Test-Path $copy) { Remove-Item -Recurse -Force $copy }

Write-Host "copying $(Split-Path $root -Leaf) -> $copy (excluding target/, work/)"
New-Item -ItemType Directory -Path $copy | Out-Null
Get-ChildItem -Force $root | Where-Object { $_.Name -notin @("target", "work") } | ForEach-Object {
    Copy-Item -Recurse -Force $_.FullName (Join-Path $copy $_.Name)
}

try {
    Push-Location $copy
    Write-Host "== cargo test -p rpt-core (in copy) =="
    cargo test --workspace --exclude rpt-reader --release
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed in copy" }

    Write-Host "== consumer simulation (in copy) =="
    cargo run -p consumer-check --release
    if ($LASTEXITCODE -ne 0) { throw "consumer-check failed in copy" }

    Write-Host "PORTABLE-OK"
    exit 0
} finally {
    Pop-Location
    Remove-Item -Recurse -Force $copy -ErrorAction SilentlyContinue
}
