# Phase 5 notes — cutover (2026-07-12)

Branch: `feat/tauri-rewrite`. Live end-to-end validation **passed** (user ran
sign-in → orgs → PBI search → create+link → run outcomes → board; the one
live bug found was AADSTS50011, fixed by using http://localhost instead of
the literal 127.0.0.1 in the loopback redirect — Entra only allows arbitrary
ports on the localhost form).

## Delivered this phase

- **Auto-update (Velopack Rust SDK)**: `VelopackApp::build().run()` first in
  main (install/update hooks); `check_update` on launch + "Restart to
  update" banner; `apply_update` downloads and restarts. Dev builds (not
  Velopack-installed) silently report no update. Feed URL:
  `.../azure-devops-test-case-manager-v2-releases/releases/latest/download/`
  — a DEDICATED repo so v1's and v2's "latest release" never fight over the
  update feed.
- **Prefs**: last org/project/mode restored from localStorage (user-facing
  prefs live in the webview; tokens never do).
- **Release pipeline**: `v2/scripts/release-v2.ps1 -Version X.Y.Z` — gates
  (55 cargo + 14 vitest), push source FIRST, tauri build, vpk pack (app id
  `AzureDevOpsTestCaseManager.V2`), `vpk upload github` with in-process
  `gh auth token`.

## SHIPPED: v0.1.0 published 2026-07-12

https://github.com/AvinAlwis/azure-devops-test-case-manager-v2-releases/releases/tag/v0.1.0
(Setup.exe + Portable.zip + full.nupkg + releases.win.json feed).

Publish gotchas hit once, fixed for good:
- vpk pack refuses a version already present in the local Releases/ staging
  dir - pack.ps1 now cleans it first.
- GitHub cannot publish (tag) a release on an EMPTY repo ("Repository is
  empty", 422) - the releases repo needs at least one commit; a README
  initial commit fixed it. vpk leaves a draft release behind on that
  failure; delete the draft before retrying.

Install from the release's Setup.exe; subsequent releases arrive via the
in-app update banner (release-v2.ps1 -Version X.Y.Z).

## Cutover decisions on record

- **Coexistence, not replacement**: separate Velopack app id + separate
  releases repo mean v1 (v3.2.2, git-pull/Velopack) and v2 install side by
  side. v1 retirement is a later, deliberate step once v2 covers the
  deferred features (see PHASE2/3/4 notes deferral lists).
- **No v1 settings import**: v2's prefs are tiny (org/project/mode) and
  re-selected in seconds; the v1 draft-queue format is not carried over.
- **WebView2**: assumed present (Windows 11 default; ships with Edge).
  If an install ever hits a machine without it, add Velopack's WebView2
  bootstrap step then - documented, not built.
