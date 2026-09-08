# Sample total CPU while something else runs, so a claim about "70%" is a
# measurement rather than an assertion.
#
#   .\scripts\cpu-sample.ps1 -OutFile $env:TEMP\cpu.txt        # start
#   ... run the thing ...
#   .\scripts\cpu-sample.ps1 -OutFile $env:TEMP\cpu.txt -Report # summarise
param(
    [Parameter(Mandatory = $true)][string]$OutFile,
    [switch]$Report,
    [int]$IntervalMs = 900
)

if ($Report) {
    $s = Get-Content $OutFile | ForEach-Object { [double]$_ }
    if ($s.Count -eq 0) { "no samples"; exit }
    "samples = $($s.Count)"
    "avg     = $([math]::Round(($s | Measure-Object -Average).Average, 1))%"
    "max     = $([math]::Round(($s | Measure-Object -Maximum).Maximum, 1))%"
    "over_90 = $(($s | Where-Object { $_ -gt 90 }).Count)"
    "over_75 = $(($s | Where-Object { $_ -gt 75 }).Count)"
    exit
}

while ($true) {
    $c = (Get-Counter '\Processor(_Total)\% Processor Time' -ErrorAction SilentlyContinue).CounterSamples[0].CookedValue
    if ($null -ne $c) { Add-Content -Path $OutFile -Value ([math]::Round($c, 1)) }
    Start-Sleep -Milliseconds $IntervalMs
}
