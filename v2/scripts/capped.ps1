# Run any command on ~70% of the machine's cores, at below-normal priority,
# so a build or a test run cannot take the desktop with it.
#
#   .\scripts\capped.ps1 cargo test
#   .\scripts\capped.ps1 npm test
#   .\scripts\capped.ps1 npm run tauri build
#
# WHY NOT just -j / --maxWorkers: those cap how many PROCESSES a tool starts
# and nothing else. Each rustc is multi-threaded, the linker is heavily
# threaded and ignores -j outright, and vite has its own workers - so a run
# capped that way still reached 100%. A processor affinity mask is a real
# ceiling, and on Windows a child inherits its parent's, so setting it once
# here bounds everything the command goes on to start.
#
# Pure ASCII on purpose (PS 5.1 reads BOM-less UTF-8 as cp1252).
# One parameter only, and it swallows everything: a second positional would
# compete with ValueFromRemainingArguments under PS 5.1 and bind the command
# name to it. The share is an env var instead - TCM_CAP=0.5 to go quieter,
# TCM_CAP=1 to use the whole machine.
param(
    [Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)]
    [string[]]$Command
)
$ErrorActionPreference = "Stop"

$fraction = 0.7
if ($env:TCM_CAP) {
    $parsed = 0.0
    if ([double]::TryParse($env:TCM_CAP, [ref]$parsed) -and $parsed -gt 0 -and $parsed -le 1) {
        $fraction = $parsed
    }
}
$total = [Environment]::ProcessorCount
$cores = [Math]::Max(1, [Math]::Min($total, [Math]::Floor($total * $fraction)))
$me = [System.Diagnostics.Process]::GetCurrentProcess()
$affinityWas = $me.ProcessorAffinity
$priorityWas = $me.PriorityClass

try {
    $me.ProcessorAffinity = [IntPtr]([int64][Math]::Pow(2, $cores) - 1)
    $me.PriorityClass = [System.Diagnostics.ProcessPriorityClass]::BelowNormal
    # Belt and braces: cargo still gets told, so it does not queue more work
    # than the cores it has been left.
    $env:CARGO_BUILD_JOBS = $cores
    Write-Host "[capped] $cores of $total cores, below-normal: $($Command -join ' ')"

    & $Command[0] @($Command[1..($Command.Length - 1)])
    $code = $LASTEXITCODE
} finally {
    # The caller's shell keeps whatever we set, so always hand it back.
    $me.ProcessorAffinity = $affinityWas
    $me.PriorityClass = $priorityWas
}

exit $code
