# Polish & parity closure exit notes (v0.7.0, 2026-07-12)

Final feature iteration before the v1.0.0 retirement sign-off.

## Delivered

- **Area/iteration on create**: classification trees ported from v1
  (paths built from node NAMES, never the `path` field with its extra
  \Area segment - golden tested; discovery failure -> empty). New cases
  default to the PBI's OWN area/iteration ("Same as PBI" - an improvement
  over both v0.x's project-default and v1's config-screen value), with
  review-gate pickers to override per batch.
- **Recently-used PBIs**: last 8 per org/project, shown when the PBI
  search input gets focus while empty.
- **Keyboard shortcuts**: Ctrl+1..5 sidebar tabs, Ctrl+Shift+M Work
  Manager toggle (v1's binding), Ctrl+K palette (existing).
- **Empty states**: board zero-item guidance per scope.
- **E2E smoke on the real binary**: `scripts/e2e-smoke.ps1` launches the
  packed exe with WebView2's CDP port
  (WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS), connects with playwright-core
  connectOverCDP, asserts the shell + sign-in render, exits clean.
  VERIFIED PASSING against the release exe. Run manually after builds;
  deliberately not inside release gates (keeps them fast, needs a display).
- Tests: 66 cargo + 37 vitest + 1 E2E smoke.

## Roadmap state

All feature iterations complete. Remaining: **v1.0.0 retirement** -
line-by-line parity checklist vs v1 (shipped / consciously dropped /
deferred with reason - the deferral lists live in the ITERATION/PHASE
notes), user migration, v1 archive, repo promotion decision.
