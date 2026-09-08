# What a relink of the test binaries costs.
#
# `cargo test` links 27 separate integration binaries, and on Windows the
# heavy part of each link is writing the PDB - which is why the process
# pegging the machine during a test run is link.exe rather than rustc.
#
# This touches the lib so every test binary relinks WITHOUT a full
# recompile, then reports the wall time and how much PDB the run left
# behind. Both numbers move together, and the second is the cause.
#
#   .\scripts\link-cost.ps1 -Label "debug=0"
#
# Measured on a 24-core desktop, 27 test binaries:
#   debug = 2   115.0s   116 PDB files   5914 MB
#   debug = 0    20.6s    57 PDB files    310 MB
param([string]$Label = "run")

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$deps = Join-Path $root "src-tauri\target\debug\deps"

Push-Location (Join-Path $root "src-tauri")
try {
    # Build everything once, so what follows is a relink and not a compile.
    cargo test --no-run --quiet | Out-Null

    (Get-Item (Join-Path $root "src-tauri\src\lib.rs")).LastWriteTime = Get-Date

    $sw = [Diagnostics.Stopwatch]::StartNew()
    cargo test --no-run --quiet | Out-Null
    $sw.Stop()

    $pdbs = @(Get-ChildItem $deps -Filter *.pdb -ErrorAction SilentlyContinue)
    $mb = if ($pdbs.Count) { [math]::Round((($pdbs | Measure-Object Length -Sum).Sum / 1MB), 1) } else { 0 }
    "{0}: relink {1}s, {2} PDB files, {3} MB" -f `
        $Label, [math]::Round($sw.Elapsed.TotalSeconds, 1), $pdbs.Count, $mb
} finally {
    Pop-Location
}
