---
name: release
description: 'Cut and publish a new release of the Test Case Manager by running scripts/release.ps1 (ruff+pytest gate, version bump, push source to private master, PyInstaller+vpk build, QtSvg gate, publish installer to the public releases repo). Use when the user types /release or says "release", "ship it", "publish a new version", or "cut a release". Optional arg — patch (default), minor, major, or an explicit X.Y.Z version.'
---

# Release

Ship a release via the one-shot `scripts/release.ps1`. It runs the whole runbook:
preflight (on master, clean tracked tree, in sync with origin, gh+vpk present) →
ruff + pytest → bump `app/version.py` → commit + push source to the **PRIVATE**
master → PyInstaller + `vpk pack` → frozen-build QtSvg gate → publish the
installer to the **PUBLIC** releases repo (token from `gh auth token`, never
printed) → `gh release view`.

## This is outward-facing and hard to reverse
It pushes to master (installed clients auto-update from it) and publishes a
public GitHub release. **Before running, confirm the version bump in one short
line** — unless the user already specified it this turn ("release minor",
"ship 3.5.0", etc.), which *is* the confirmation; then proceed without asking.

## Steps
1. Map the argument to a flag (run from the repo root):
   - none / `patch` → `.\scripts\release.ps1`
   - `minor` → `.\scripts\release.ps1 -Bump minor`
   - `major` → `.\scripts\release.ps1 -Bump major`
   - an `X.Y.Z` → `.\scripts\release.ps1 -Version X.Y.Z`
   Append `-SkipChecks` **only** if the user explicitly asks to skip ruff/pytest.
2. Run it with the **PowerShell** tool, in the **foreground** (it takes ~1–2 min:
   build + upload). Do NOT pipe it through `2>&1` / `Select-Object` — that
   re-breaks build.ps1's stderr handling under Windows PowerShell 5.1.
3. On success it prints `Released vX.Y.Z …` plus the `gh release view` output.
   Report: the shipped version, that source is on private master, and the
   release URL. Then note the two earlier changes are included if relevant.

## If it fails
The script is fail-fast (`throw` on any failed step). Surface the failing step
verbatim and STOP — do not retry blindly or work around a refusal:
- Preflight refusals ("uncommitted tracked changes", "N commits behind",
  "on branch X") mean the user must commit/pull/switch to master first — relay
  that, don't bypass.
- A mid-publish failure may leave a partial draft release. Recovery steps
  (delete the draft + stale `./Releases` artifacts, re-run) are documented in
  the distribution-velopack memory and `claudedocs/reflection-notes.md`.

## Notes
- No `GITHUB_TOKEN` env needed — release.ps1 fetches the token in-process.
- Full runbook + gotchas (Defender quarantine, VPK_NO_PORTABLE, PS 5.1 stderr):
  `scripts/release.ps1` header and the distribution-velopack memory.
