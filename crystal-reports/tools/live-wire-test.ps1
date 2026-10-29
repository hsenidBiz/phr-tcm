# live-wire-test.ps1 --- the one-command live SQL Server verification.
#
# Run this on a machine that can reach the corporate DB (VPN or office network).
# The connection string is read from the RPT_MSSQL_CONN environment variable --- never
# from the command line --- so it cannot leak into shell history or process listings.
#
# USAGE:
#   $env:RPT_MSSQL_CONN = "Server=sgdev01db02.cloud;Database=hrmmain_philippines;User Id=<user>;Password=<password>;TrustServerCertificate=True"
#   powershell -ExecutionPolicy Bypass -File tools\live-wire-test.ps1
#
# Optional:
#   $env:RPT_CYCLE_ID = "42"        # a performance cycle id for the real render (default: 1)
#
# PREREQUISITES: Rust toolchain (cargo) + this Out/ folder. The script builds what it
# needs (release CLI + the gated test binary), then runs three stages:
#     1. Network: DNS + TCP 1433 reachability
#     2. Connect: the gated smoke test (TLS login + SELECT @@VERSION)
#     3. Render: a real IndPerfOverview .rpt rendered from live data
#
param(
    [string]$Fixture = "rpt-core\tests\fixtures\PMSV10_IndPerfOverview.rpt"
)

$ErrorActionPreference = "Continue"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$fail = 0
$pass = 0
function Step($label, $ok, $detail) {
    if ($ok) { Write-Host "  [PASS] $label" -ForegroundColor Green; $script:pass++ }
    else     { Write-Host "  [FAIL] $label" -ForegroundColor Red; $script:fail++ }
    if ($detail) { Write-Host "         $detail" -ForegroundColor DarkGray }
}

Write-Host "`n=== Live-wire verification ===`n" -ForegroundColor Cyan

# ---- 0. Connection string ----
if (-not $env:RPT_MSSQL_CONN) {
    Write-Host "[ERROR] Set the connection string first:" -ForegroundColor Red
    Write-Host '  $env:RPT_MSSQL_CONN = "Server=...;Database=...;User Id=...;Password=...;TrustServerCertificate=True"'
    exit 1
}

# Parse the server + database for the network check (never print the password).
$conn = $env:RPT_MSSQL_CONN
$server = ($conn -split ';' | Where-Object { $_ -match '^(?i)server|data source|address|addr=' }) -replace '^(?i)(server|data source|address|addr)=','' -replace ',\d+$',''
$database = ($conn -split ';' | Where-Object { $_ -match '^(?i)database|initial catalog=' }) -replace '^(?i)(database|initial catalog)=',''

Write-Host "Target: $server / $database"
Write-Host "Connection string: $(($conn -replace '(?i)password=[^;]*','Password=(hidden)'))`n"

# ---- 1. Network reachability ----
Write-Host "--- Stage 1: Network ---" -ForegroundColor Yellow
try {
    $ips = [System.Net.Dns]::GetHostAddresses($server) | ForEach-Object { $_.ToString() }
    Step "DNS resolves $server" $true ($ips -join ', ')
} catch {
    Step "DNS resolves $server" $false ("$_")
}
try {
    $tcp = New-Object System.Net.Sockets.TcpClient
    $tcp.Connect($server, 1433)
    Step "TCP 1433 open" $true
    $tcp.Close()
} catch {
    Step "TCP 1433 open" $false ("$_")
}

# ---- 2. Build + gated connect test ----
Write-Host "`n--- Stage 2: Connect (TLS login + SELECT @@VERSION) ---" -ForegroundColor Yellow
Write-Host "Building the gated test (cargo test -p rpt-db-mssql --test live)...`n"
$env:RPT_MSSQL_SQL = "SELECT @@VERSION AS version"
$buildOut = cargo test -p rpt-db-mssql --test live 2>&1 | Out-String
$testOut = cargo test -p rpt-db-mssql --test live -- --ignored --nocapture 2>&1 | Out-String
if ($LASTEXITCODE -eq 0) {
    Step "Connect + login + query" $true
    $version = ($testOut | Select-String -Pattern 'row:.*' | Select-Object -First 1).Matches.Value
    if ($version) { Write-Host "         $version" -ForegroundColor DarkGray }
} else {
    Step "Connect + login + query" $false ($testOut.Trim() -split "`n" | Select-Object -Last 5)
}

# ---- 3. Real render ----
Write-Host "`n--- Stage 3: Live-data render (IndPerfOverview) ---" -ForegroundColor Yellow
Write-Host "Building the CLI (cargo build -p rpt-cli --release)...`n"
cargo build -p rpt-cli --release 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) {
    Step "CLI build" $false
    exit 1
}
Step "CLI build" $true

$cycleId = if ($env:RPT_CYCLE_ID) { $env:RPT_CYCLE_ID } else { "1" }
$env:RPT_TOOLS_DB = $conn
$out = Join-Path $env:TEMP "live-wire-render.pdf"
$renderOut = & "$root\target\release\rpt-tools.exe" render $Fixture $out --db `
    -p "@performance_cycle_id=$cycleId" `
    -p "@business_unit_level=company" `
    -p "@business_unit=" `
    -p "@emp_number_list=" `
    -p "@manager_emp_number=" 2>&1 | Out-String
if ($LASTEXITCODE -eq 0) {
    $pdfSize = (Get-Item $out -ErrorAction SilentlyContinue).Length
    Step "Render from live data" ($pdfSize -gt 1000) ("$([math]::Round($pdfSize/1KB,1)) KB PDF")
    Write-Host "         Output: $out" -ForegroundColor DarkGray
    Write-Host "         Open it to verify the report drew real data." -ForegroundColor DarkGray
} else {
    Step "Render from live data" $false ($renderOut.Trim() -split "`n" | Select-Object -Last 5)
}

# ---- Summary ----
Write-Host "`n=== Summary: $pass passed, $fail failed ===`n" -ForegroundColor $(if ($fail -eq 0) { "Green" } else { "Red" })
if ($fail -gt 0) {
    Write-Host "If DNS failed: the machine is not on the corporate network."
    Write-Host "If TCP failed: check the firewall / VPN split-tunnel."
    Write-Host "If login failed: check the credentials and the user's DB permissions."
    Write-Host "If the render returned empty: try a different cycle id (RPT_CYCLE_ID)."
    exit 1
}
Write-Host "All stages passed --- the SQL Server driver is verified end-to-end."
exit 0
