# Integration guide — wiring the editor into your Rust/Tauri/Tailwind V4 app

## What's in `Out/`

| path | what |
|---|---|
| `vendor/` | rpt-rs crates (reader/writer pair + the 10-crate render stack incl. bundled fonts), MPL-2.0, pinned commit — see `vendor/VENDORED.md` |
| `tests/fixtures/` | report corpus + render baselines the vendored crates' tests resolve (upstream's own layout) |
| `rpt-core/` | the library: `open` → IR (`crpt-ir/1`), typed edit ops, save, create-from-scratch, validate, PDF preview (saved + live) |
| `rpt-db-mssql/` | live SQL Server preview driver (this project's own code, not vendored): stored-proc reports over tiberius (pure-Rust TDS), your app's ADO connection string in, fresh rows out |
| `rpt-cli/` | `rpt-tools` binary: `ir` / `edit` / `apply` / `diff` / `roundtrip` / `sweep` / `check` / `new` / `render` |
| `tauri-commands/` | `commands.rs` — drop-in `#[tauri::command]` module + README |
| `ui-react/` | React + TS + Tailwind components (`ReportEditor` composition, `LayoutPreview`, `PdfPreview`) + README |
| `docs/` | this guide, `format-notes.md`, `ir-schema.md`, `validate.md`, `vibey-operations.md`, `validation-log.md` |
| `work/` | validation artifacts (IR dumps, sweep log, edited samples, ops files) |

## Backend wiring

1. Make `Out/` crates reachable from your app. Simplest: add to your `src-tauri/Cargo.toml`

   ```toml
   [dependencies]
   rpt-core = { path = "../Out/rpt-core" }
   # transitively brings the vendored reader/model + the render stack (preview); keep Out/
   # in place, or move vendor/ + rpt-core/ into your repo — MPL-2.0 requires the vendored
   # files to keep their license notices (vendor/LICENSE, THIRD-PARTY-NOTICES.md), and the
   # bundled fonts in vendor/rpt-text/fonts/ must travel with their licence texts.
   ```

   All vendored crates must be workspace members or standalone: they inherit `[workspace]`
   tables from `Out/Cargo.toml`. If you vendor them into your own workspace, copy the
   `[workspace.package]`/`[workspace.dependencies]`/`[workspace.lints]` tables from
   `Out/Cargo.toml` (the dependency table now includes the render pins: fontdb, ttf-parser,
   cosmic-text, krilla, harfrust, similar) or strip the `.workspace = true` inheritances.

2. Copy `tauri-commands/commands.rs` into `src-tauri/src/`, register per its README
   (`.manage(EditorState::default())` + `generate_handler![…]`).

3. Front end: copy `ui-react/src/` and render `<ReportEditor />` (see `ui-react/README.md`).

## The one data-flow rule

The backend is the single source of truth. `apply_edits` returns the **refreshed IR**; the UI
replaces its state with it wholesale. Never mutate IR client-side, never reuse ids across an
error (a refused batch may have applied its earlier ops — re-request `get_ir`).

## Acceptance commands (0.2.0)

- `cargo test --workspace --exclude rpt-reader` — the handoff contract as code (~990 tests:
  our batteries in `rpt-core/tests/` over the vendored fixtures, plus the vendored crates'
  own upstream suites — formula, layout, data, PDF/dataset/typography baselines).
- `powershell Out/tools/verify-portable.ps1` — relocation gate: builds and tests a copy of
  `Out/` from a temp directory.
- `python Out/ui-react/check-shapes.py` — TS/Rust shape sync for the UI contract.
- `tauri-commands` is a workspace crate: default features compile it outside Tauri (plain
  functions over `&EditorState`); your app enables `features = ["tauri"]` and registers
  `tauri_commands::glue::*`.

## CLI (scripting / CI)

```
rpt-tools ir        <in.rpt> [out.json]      # the readable JSON IR
rpt-tools edit      <in.rpt> <ops.json> <out.rpt>
rpt-tools apply     <in.rpt> <plan.json> <out.rpt>   # VibeyReports plan JSON
rpt-tools diff      <a.rpt> <b.rpt>          # semantic model diff, JSON-path lines
rpt-tools roundtrip <in.rpt>                 # decode→re-encode→reopen→compare records
rpt-tools sweep     <dir>                    # ir+roundtrip over every .rpt under dir
rpt-tools check     <in.rpt>                 # offline load-path validation
rpt-tools new       <out.rpt> [--title T] …  # create a blank .rpt from scratch
rpt-tools render    <in.rpt> <out.pdf>       # PDF preview (saved data when present)
rpt-tools render    <in.rpt> <out.pdf> --db -p Name=Value   # …or live SQL Server data
                                                     # (connection string from RPT_TOOLS_DB env)
```

Build: `cargo build --release` in `Out/` → `Out/target/release/rpt-tools.exe`.

## Validation status (see docs/validation-log.md)

- 7 vendored fixtures → IR with **0 pairing warnings** (exact object counts pinned); World
  Sales Report renders a 37-page data-driven PDF preview (golden-hash pinned).
- 263/263 designer sample reports: clean IR + **byte-identical record round-trip**.
- All 29 Vibey layout actions + Tier 0 + sections + from-scratch + sub-side link binding,
  verified by model diff — exactly the intended changes, nothing else.
- Preview rendering cross-checked against the upstream-built `rpt-render` binary (validation
  log §21).
- Designer smoke test deferred (portable install not registered — the original files fail to
  launch it identically to edited ones; probe script preserved in `work/`).

## When you need engine verification

The bundle is a re-implementation of what Crystal Reports does, not a derivative of its
code. Verify against the installed engine when:

- **A patched file the real designer rejects**: verify the behavior against the engine's load path. Anchors that find it
  fast: the fixed AES key bytes (`11 dd 18 96 bd 4a 15 cd bf f2 54 35 03 e6 76 0f`), the UTF-16
  stream name `Contents`, record type constants (`0x66` page setup, `0x8c` section, `0x9e` name).
- **Undecoded record families** (maps, OLAP, Flash, charts' finer detail): find the consumer via
  record-type constants or the `CHART N`/`Embedding N` stream names.
- **Writer invariants** beyond the verified path (e.g. what forces a DEVMODE record to be written
  when saving with a printer).

Everything already observed is documented in `docs/` — prefer those notes over re-deriving.

## Known v1 boundaries

- Multi-run text objects (embedded field references) are read-only (`text.editable == false`).
- Orientation/paper-size edits need the page-DEVMODE record (report saved with a printer).
- Subreport *internals* are not editable (the subreport placeholder object itself is).
- ~~Formula bodies are not editable~~ **Editable since 2026-10-16** — `setFormula`/`setRecordSelection` (the format stores source text, not tokens; refs auto-derive from the new source).
- Password-protected reports unsupported. Older (pre-XI) schema versions untested.
