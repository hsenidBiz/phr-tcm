# tauri-commands — drop-in Tauri backend for the Crystal Reports editor

`src/commands.rs` holds the **portable half**: plain functions over `&EditorState` that compile
with the default (empty) feature set — outside any Tauri app, which is what the bundle's
portability gate exercises. `src/glue.rs` (feature `tauri`) adds one `#[tauri::command]`
wrapper per function; that is what your app registers.

| command (glue) | does |
|---|---|
| `open_report(path)` | open a `.rpt`, build the IR, stash both in state; returns the IR JSON |
| `get_ir()` | the current IR snapshot |
| `apply_edits(ops)` | apply native edit ops (`rpt-core::EditOp`, snake_case); returns the **refreshed IR** |
| `apply_plan(planJson)` | apply a VibeyReports LayoutPlan JSON (camelCase action names) to the open report; returns the refreshed IR |
| `save_report(path?)` | write current bytes (default: overwrite the opened file) |
| `save_as_bytes()` | the `.rpt` bytes (for Tauri's save-dialog flow) |
| `preview_thumbnail()` | page-1-only one-page PDF of the open report — the list-view thumbnail |
| `open_report_bytes(bytes)` | open a report held in memory (e.g. straight from `new_report`) — create-then-edit without temp files |
| `undo_report()` / `redo_report()` | step back / re-apply the last accepted edit batch; returns the refreshed IR |
| `find_replace(find, replace, caseInsensitive?)` | replace every matching literal text across the main report AND all subreports (one undoable batch; locked multi-run texts skipped and counted) |
| `preview_hits()` | rendered pages as selectable hit boxes (object/section/kind + true twips rectangles) — the overlay for click-to-select / drag-to-edit on the preview |
| `hit_test(page, x, y)` | the topmost hit box at a printable-relative point (server-side click lookup) |
| `diff_reports(a, b)` | JSON-path diff lines over the two reports' decoded models — the changes panel |
| `get_subreport_ir(name)` | the named embedded subreport's OWN IR (its objects/areas) — the object scope for edits with a `subreport` field |
| `get_parameters()` | the open report's declared parameters (name, kind, prompt text, pick lists) — the prompt form for previews |
| `export_pdf(opts)` | "Export PDF": the preview render with `{ title, pdfa: "1b\|2b\|3b", tagged }` |
| `preview_report()` | render the open report (unsaved edits included) to a PDF preview — `{ pdf: number[], pageCount, diagnostics: string[] }`; saved data drives the rows when the report carries it, otherwise static bands render |
| `preview_report_live(db)` | same render with LIVE SQL Server data — `db = { connectionString, params: [[name, value]…] }`; the connection string is your app's own ADO/OLE DB form, parsed once and never logged; stored-procedure tables execute fresh, so the preview reflects the database now |

## Wiring it in

1. Depend on the workspace crates from your `src-tauri`:

   ```toml
   [dependencies]
   rpt-core = { path = "../Out/rpt-core" }
   tauri-commands = { path = "../Out/tauri-commands", features = ["tauri"] }
   ```

   The vendored `rpt-reader`/`rpt-model` inherit `[workspace]` tables from `Out/Cargo.toml`;
   if you vendor `Out/` into your own workspace, copy those tables or strip the
   `.workspace = true` inheritances (see `Out/docs/integration-guide.md`).

2. Register state + the glue functions:

```rust
use tauri_commands::commands::EditorState;

tauri::Builder::default()
    .manage(EditorState::default())
    .invoke_handler(tauri::generate_handler![
        tauri_commands::glue::open_report,
        tauri_commands::glue::get_ir,
        tauri_commands::glue::apply_edits,
        tauri_commands::glue::apply_plan,
        tauri_commands::glue::save_report,
        tauri_commands::glue::save_as_bytes,
        tauri_commands::glue::preview_thumbnail,
        tauri_commands::glue::open_report_bytes,
        tauri_commands::glue::undo_report,
        tauri_commands::glue::redo_report,
        tauri_commands::glue::find_replace,
        tauri_commands::glue::preview_hits,
        tauri_commands::glue::hit_test,
        tauri_commands::glue::diff_reports,
        tauri_commands::glue::get_subreport_ir,
        tauri_commands::glue::get_parameters,
        tauri_commands::glue::export_pdf,
        tauri_commands::glue::preview_report,
        tauri_commands::glue::preview_report_live,
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
```

3. Front end:

```ts
import { invoke } from "@tauri-apps/api/core";

const ir = await invoke<ReportIr>("open_report", { path: "C:/reports/Sample.rpt" });
const fresh = await invoke<ReportIr>("apply_plan", {
  planJson: JSON.stringify({ planVersion: 1, operations: [
    { action: "setFont", target: "FtConf", fontName: "Tahoma" },
  ]}),
});
await invoke("save_report");
```

## Editing model

- Edits are batched: `apply_edits`/`apply_plan` apply ops in order; any refused op aborts the
  batch with a formatted `op #N` message (earlier ops in the batch stand — split batches if
  that matters).
- Every write goes through the verified field/writer path; a batch that returns Ok has already
  been re-opened and read back.
- Object ids (`o12`) are stable for the IR snapshot they came from and across any edit batch
  (no op adds or removes records… the structural ops do change record counts — always re-render
  from the **returned** IR and use its ids).
- Units: twips everywhere (1440 = 1 inch); font sizes in points; colours `#RRGGBB` at the
  plan-JSON boundary, `r/g/b` numbers on the native `set_text_color`.

See `docs/ir-schema.md` and `docs/vibey-operations.md` for the full op vocabulary.
