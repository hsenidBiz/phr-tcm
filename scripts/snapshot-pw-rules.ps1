# Snapshot the team's Playwright repo rule files into src-tauri/pw-rules/, the
# copy the app compiles in and falls back to when the clone is not there.
# Pure ASCII on purpose (PS 5.1).
#
# Refuses, writing nothing, when no clone is set, when a required file is
# missing from it, or when git says the rule files have uncommitted changes.
# The only git command run in the clone is the read-only
#   git --no-optional-locks -C <clone> status --porcelain -- <rule paths>
# Nothing is ever written to the clone.
#
#   -Clone  the Playwright repo folder. Default: playwright_clone from the
#           app's app-settings.json.
#   -Out    where the snapshot goes. Default: src-tauri\pw-rules in this repo.
#   -Check  write nothing; say whether the snapshot is current, or what differs.
#   -Registry  the Rust file that lists the shipped skills (SNAPSHOT). Default:
#           src-tauri\src\autorun\pw_rules.rs in this repo.
#
# TCM's own generated agent files (.claude/agents/tcm-*.md, written there when
# the AI Bridge is registered in the clone) are not rule files: they never
# count as uncommitted changes and are never copied.
param(
    [string]$Clone,
    [string]$Out,
    [string]$Registry,
    [switch]$Check
)
$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

function Resolve-Full([string]$p) {
    return $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($p)
}

# --- Where the clone and the output are --------------------------------------
if (-not $Clone) {
    $settings = Join-Path $env:APPDATA "com.avinalwis.testcasemanager.v2\app-settings.json"
    if (Test-Path -LiteralPath $settings -PathType Leaf) {
        try { $Clone = [string](Get-Content -LiteralPath $settings -Raw | ConvertFrom-Json).playwright_clone } catch { $Clone = "" }
    }
}
if ($Clone) { $Clone = $Clone.Trim() }
if (-not $Clone) {
    throw "No Playwright repo is set on the Playwright tab: set it there, or pass -Clone."
}
$Clone = Resolve-Full $Clone
if (-not (Test-Path -LiteralPath $Clone -PathType Container)) {
    throw "The Playwright repo folder is not there: $Clone. Set it on the Playwright tab, or pass -Clone."
}
if (-not $Out) { $Out = Join-Path $repoRoot "src-tauri\pw-rules" }
$Out = Resolve-Full $Out
if (-not $Registry) { $Registry = Join-Path $repoRoot "src-tauri\src\autorun\pw_rules.rs" }

# --- The files, as the repo lays them out -> where the snapshot keeps them ----
$agents = @(
    "playwright-test-planner.md",
    "playwright-test-generator.md",
    "playwright-test-healer.md",
    "spec-editor.md",
    "test-refactorer.md",
    "doc-drift-auditor.md"
)
$required = @()
foreach ($a in $agents) { $required += ".claude/agents/$a" }
$required += ".claude/commands/gen-test.md"
$required += "CLAUDE.md"

$missing = @()
foreach ($r in $required) {
    if (-not (Test-Path -LiteralPath (Join-Path $Clone $r) -PathType Leaf)) { $missing += $r }
}
if ($missing.Count -gt 0) {
    throw "The Playwright repo is missing required files: $($missing -join ', ')."
}

# --- Uncommitted changes to the rule files are refused ------------------------
# TCM's own tcm-*.md agent files are left out of the same one git command.
$watched = @(".claude/agents", ".claude/commands/gen-test.md", ".claude/skills", "CLAUDE.md", ':(exclude).claude/agents/tcm-*.md')
$gitLines = @()
$gitExit = $null
try {
    $gitLines = @(git --no-optional-locks -C $Clone status --porcelain -- @watched)
    $gitExit = $LASTEXITCODE
} catch {
    $gitExit = "git did not run"
}
if ($gitExit -ne 0) {
    throw "The Playwright repo's rule files could not be checked with git: $gitExit."
}
$dirty = @($gitLines | ForEach-Object { "$_".Trim() } | Where-Object { $_ })
if ($dirty.Count -gt 0) {
    throw "The Playwright repo has uncommitted changes to its rule files: $($dirty -join '; '). Commit them in the repo first."
}

# --- Read everything first, so a failure leaves nothing half-written ---------
# key: path relative to $Out with forward slashes; value: the clone file's bytes.
$want = [ordered]@{}
foreach ($a in $agents) {
    $want["agents/$a"] = [System.IO.File]::ReadAllBytes((Join-Path $Clone ".claude/agents/$a"))
}
$want["commands/gen-test.md"] = [System.IO.File]::ReadAllBytes((Join-Path $Clone ".claude/commands/gen-test.md"))
$skillsDir = Join-Path $Clone ".claude/skills"
if (Test-Path -LiteralPath $skillsDir -PathType Container) {
    foreach ($d in (Get-ChildItem -LiteralPath $skillsDir -Directory | Sort-Object Name)) {
        $f = Join-Path $d.FullName "SKILL.md"
        if (Test-Path -LiteralPath $f -PathType Leaf) {
            $want["skills/$($d.Name)/SKILL.md"] = [System.IO.File]::ReadAllBytes($f)
        }
    }
}
# The snapshot folder holds no file named CLAUDE.md and no .claude folder, so
# Claude Code working in this repository never loads them as its own.
$want["CLAUDE.repo.md"] = [System.IO.File]::ReadAllBytes((Join-Path $Clone "CLAUDE.md"))

# --- What is in $Out now ------------------------------------------------------
$have = @{}
if (Test-Path -LiteralPath $Out -PathType Container) {
    $prefix = $Out.TrimEnd('\', '/') + "\"
    foreach ($f in (Get-ChildItem -LiteralPath $Out -Recurse -File -Force)) {
        $have[$f.FullName.Substring($prefix.Length).Replace('\', '/')] = $f.FullName
    }
}

$changed = @()
foreach ($k in $want.Keys) {
    if (-not $have.ContainsKey($k)) { $changed += "missing from the snapshot: $k"; continue }
    $now = [System.IO.File]::ReadAllBytes($have[$k])
    if (-not [System.Linq.Enumerable]::SequenceEqual([byte[]]$now, [byte[]]$want[$k])) { $changed += "differs: $k" }
}
foreach ($k in ($have.Keys | Sort-Object)) {
    if (-not $want.Contains($k)) { $changed += "no longer in the repo: $k" }
}

# The skills the app lists are named in pw_rules.rs (SNAPSHOT); a skill added to
# or removed from the repo needs a code change there, so say so now and not as a
# failing cargo test later.
$skillChange = ""
if (Test-Path -LiteralPath $Registry -PathType Leaf) {
    $listed = @([regex]::Matches([System.IO.File]::ReadAllText($Registry), '"\.claude/skills/([A-Za-z0-9_-]+)/SKILL\.md"') | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique)
    $inRepo = @($want.Keys | Where-Object { $_ -like "skills/*/SKILL.md" } | ForEach-Object { $_.Split('/')[1] } | Sort-Object -Unique)
    $added = @($inRepo | Where-Object { $listed -notcontains $_ })
    $removed = @($listed | Where-Object { $inRepo -notcontains $_ })
    if ($added.Count -gt 0 -or $removed.Count -gt 0) {
        $parts = @()
        if ($added.Count -gt 0) { $parts += "added: $($added -join ', ')" }
        if ($removed.Count -gt 0) { $parts += "removed: $($removed -join ', ')" }
        $skillChange = "The Playwright repo's skills changed ($($parts -join '; ')). Add or remove them in SNAPSHOT in src-tauri/src/autorun/pw_rules.rs, then run cargo test --test suite autorun_pw_rules:: before releasing."
    }
}

if ($Check) {
    if ($skillChange) { Write-Host $skillChange }
    if ($changed.Count -eq 0) {
        Write-Host "The Playwright rules snapshot is current."
    } else {
        Write-Host "The Playwright rules snapshot differs from the repo:"
        foreach ($c in $changed) { Write-Host "  $c" }
    }
    return
}

# --- Write ---------------------------------------------------------------------
foreach ($k in $want.Keys) {
    $dest = Join-Path $Out ($k.Replace('/', '\'))
    $now = $null
    if ($have.ContainsKey($k)) { $now = [System.IO.File]::ReadAllBytes($have[$k]) }
    if ($null -ne $now -and [System.Linq.Enumerable]::SequenceEqual([byte[]]$now, [byte[]]$want[$k])) { continue }
    $dir = Split-Path -Parent $dest
    if (-not (Test-Path -LiteralPath $dir -PathType Container)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    [System.IO.File]::WriteAllBytes($dest, [byte[]]$want[$k])
}
foreach ($k in ($have.Keys | Sort-Object)) {
    if (-not $want.Contains($k)) { Remove-Item -LiteralPath $have[$k] -Force }
}
# Folders the deletions emptied (deepest first); $Out itself stays.
if (Test-Path -LiteralPath $Out -PathType Container) {
    $dirs = @(Get-ChildItem -LiteralPath $Out -Recurse -Directory -Force | Sort-Object { $_.FullName.Length } -Descending)
    foreach ($d in $dirs) {
        if (@(Get-ChildItem -LiteralPath $d.FullName -Force).Count -eq 0) { Remove-Item -LiteralPath $d.FullName -Force }
    }
}
if ($changed.Count -eq 0) {
    Write-Host "The Playwright rules snapshot was already current."
} else {
    Write-Host "The Playwright rules snapshot was updated:"
    foreach ($c in $changed) { Write-Host "  $c" }
}
# The snapshot is written; the release stops here so the code can follow it.
if ($skillChange) { throw $skillChange }
