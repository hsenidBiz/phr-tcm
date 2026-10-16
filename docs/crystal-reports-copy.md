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

## Licence

The vendored crates are MPL-2.0. The licence text is
`crystal-reports/vendor/LICENSE` and the third-party notices are in
`crystal-reports/vendor/THIRD-PARTY-NOTICES.md`; both travel with the copy and
must stay with it.
