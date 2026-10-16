# The Crystal Reports copy

`crystal-reports/` is a copy of the .rpt re-implementation bundle. TCM builds
against it by path (`rpt-core` and `rpt-db-mssql` in `src-tauri/Cargo.toml`);
it is not a fork, and nothing in it is ours to change.

- **Source:** `C:\Users\Admin\Desktop\Main Menu\Software\Personal Projects\CrystalReports ReImplementation\Out`
- **Copied:** 2026-10-16
- **Source commit:** none - the source folder is not a git repository, so
  there is no commit to record. The copy is identified by the date above.

## What was left out

| Left out | Why |
|---|---|
| `target/` | Build output of the source folder. TCM builds into `src-tauri/target` only. |
| `work/` | Scratch space of the source folder, not part of the bundle. |
| `.git/` | A nested repository must not live inside this one. (None existed in the source at copy time.) |

`crystal-reports/target/` is in `.gitignore` in case anything ever builds there.
Nothing should: build from `src-tauri`.

## The rule

The copy is refreshed only on the owner's word, by copying again from the
source folder. No file inside `crystal-reports/` is ever edited here - a fix
belongs in the source folder and arrives with the next refresh.

## Refresh checklist

Before a refreshed copy ships, check each line below against the new code.

- **Subreports are not fetched live.** The vendored `preview_live_report`
  (`crystal-reports/rpt-core/src/preview.rs`, where it builds
  `RenderOptions`) never passes `RenderOptions.scope`, so only the main
  report's data is fetched from the database and subreports render from
  their saved data. TCM's check (`live_target` in
  `src-tauri/src/crystal/live.rs`) looks only at the main report's first
  plan, which today covers everything that runs. If a refreshed copy wires
  `scope`, `MssqlScope::rows_for` would run a subreport's procedure or SQL
  command with no check at all: TCM's check must then cover every
  subreport before the refresh ships.

## Licence

The vendored crates are MPL-2.0. The licence text is
`crystal-reports/vendor/LICENSE` and the third-party notices are in
`crystal-reports/vendor/THIRD-PARTY-NOTICES.md`; both travel with the copy and
must stay with it.
