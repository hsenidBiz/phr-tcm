# Backlog

The working list for this bundle. Status legend: ☐ todo · ◐ in progress · ☑ done (kept one
section down for a record of what shipped and when). This folder
(`CrystalReports ReImplementation/Out`) is the **canonical source** — fixes land here first.

## Now

- ☐ **2. Live-wire verification** — READY TO RUN on a networked machine:
  copy this `Out/` folder (or just `tools/live-wire-test.ps1` + the source) to a machine
  that can reach the DB, then:
  ```
  $env:RPT_MSSQL_CONN = "Server=...;Database=...;User Id=...;Password=...;TrustServerCertificate=True"
  powershell -ExecutionPolicy Bypass -File tools\live-wire-test.ps1
  ```
  Three stages (network / connect+login / live-data render) with a clear PASS/FAIL
  summary and troubleshooting hints. Optional `$env:RPT_CYCLE_ID = "42"` picks the
  performance cycle. The connection string never touches the command line.

- ☑ **1. `addGroup` area-clone corruption** — FIXED 2026-10-22: the footer selection took
  the model's `.next_back()` (OUTERMOST footer, since footers list innermost-first) instead
  of `.next()`; the clone split the outer pair's `[footer][group def]` adjacency. All three
  fixtures PASS; regression test in followups (validation-log §26). — on reports whose innermost group header
  carries a chart or a subreport placeholder (World Sales Report, IndPerfOverview family),
  `addGroup` splices a malformed area clone: the flat record walk desyncs ~20 bytes after a
  2-byte record at the clone's tail, and the validator's `stream_parsed` / `group_structure`
  fire (probe evidence in `docs/validation-log.md` §23). Steps: reproduce → bisect the three
  splices (group def / footer bracket / header bracket) → fix the insertion → the three
  fixtures PASS 0/0 → corpus sweep + full gates.

## Next

- ☑ **3. Thumbnail** — v1 SHIPPED 2026-10-22: `preview_thumbnail` (page-1 one-page PDF)
  through the whole stack. The embedded-DIB variant is deferred to the list below.
- ☑ **4. Orientation / paper-size** — SHIPPED 2026-10-22: minimal `0x0007` DEVMODE
  synthesis (`pagesetup.rs`); the blank and every PMSV10 report are now editable
  (validation-log §27).
- ☑ **5. Folder consolidation** — the original `Desktop\CrystalReports\Out` archived by
  rename; this folder is canonical.

- ☐ **3b. Embedded-DIB thumbnail** (the in-file `PID_THUMBNAIL` variant) (`regenerate_thumbnail`) — render page 1 through the
  bundled renderer → DIB → write back into the report's stored thumbnail record (the reader
  already parses it; catalog engine-notes.md candidate 9 called the write side "trivial once
  a renderer exists" — it now exists). Library op + Tauri command; TCM's file browser shows
  live thumbnails.

## Candidates (proposed 2026-10-22 — pick the next batch)

Editor-integration polish (all small, high day-to-day value for TCM):

- ☑ **A. Parameter-prompt surface** — SHIPPED 2026-10-22: `get_parameters` (name, kind,
  prompt text, flags, pick lists) through the whole stack; validation-log §28.
- ☑ **C. Undo/redo** — SHIPPED 2026-10-22: `undo_report`/`redo_report` (50-step byte
  history, redo cleared by new edits, reset on open); §28.
- ☑ **D. Open-from-bytes** — SHIPPED 2026-10-22: `open_report_bytes`; §28.
- ☑ **H. Atomic saves** — SHIPPED 2026-10-22: sibling `.tmp` + rename; §28.
- ☑ **E. PDF export options** — SHIPPED 2026-10-22: `export_pdf` (title, PDF/A 1b/2b/3b,
  tagging); a-levels refused pending a language option; §28.

Larger tracks:

- ☑ **B. Subreport internals editing** — SHIPPED 2026-10-22: the lift-into-a-mini-report
  seam (`subedit.rs`) — a subdocument IS a complete report, so the ENTIRE op vocabulary
  applies inside every subreport unchanged (`get_subreport_ir`, `EditOp::InSubreport`, the
  plan `subreport` key). Validation-log §29.
- ☐ **F. Multi-run text editing** — DEMOTED 2026-10-22: a probe of every fixture found ZERO
  multi-run text objects in this report family (all `text.editable == true`); only worth
  building if a future report needs it.
- ☐ **G. Embedded-DIB thumbnail (3b detail)** — rasterize page 1 (own software rasterizer
  over the page draw-ops + bundled fonts, or a PDF-raster crate) into the
  SummaryInformation `PID_THUMBNAIL` DIB so Windows/Explorer and Crystal's own dialogs show
  it. Effort M; only worth it if TCM's browser wants in-file thumbnails rather than the
  PDF-thumbnail command.

## Candidates (round 2, proposed 2026-10-22 — probe-grounded)

- ☑ **I. Section visibility-formula editing** — SHIPPED 2026-10-22: `setSectionVisibility`
  (0x00ff slot → global 0x76 ordinal → the setFormula rebuild). The probe found ALL SIX
  IndDetailedEval subreports carry one. §30.
- ☑ **J. PDF/A level-A + PDF/UA** — SHIPPED 2026-10-22: `lang` unlocks 1a/2a/3a; `ua` also
  requires `title`. §30.
- ☑ **K. Locale option** — SHIPPED 2026-10-22: `locale` on the export options
  (en/de/fr/es/it roots; unknown refused). §30.
- ☑ **L. Diff command** — SHIPPED 2026-10-22: `diff_reports` (JSON-path lines over the
  decoded models; clean on identical/re-encoded). §30.
- ☑ **M. Walkthrough example** — SHIPPED 2026-10-22:
  `cargo run -p tauri-commands --example walkthrough`. §30.

## Round-3 (shipped 2026-10-22 — validation-log §31)

- ☑ **N. Render-geometry bridge** — `preview_hits`/`hit_test`: rendered pages as per-object
  hit boxes (true rectangles, instance-grouped); subreport clicks route to subreport
  scopes (code-verified attribution). The overlay surface for click-to-select /
  drag-to-edit on the preview.
- ☑ **O. Find & replace** — across the main report and every subreport, one undoable
  batch; locked multi-run texts skipped and counted.
- ☑ **P. Batch apply** — `rpt-tools apply-all <dir> <plan.json>`: per-file isolation,
  `.bak` backups, no-op detection.
- ☑ **Q. True rendered heights** — ride on N's hit boxes.

## Tier-A property batch (shipped 2026-10-22 — §32)

- ☑ 11 format-leaf ops: textRotation, verticalAlignment, keepTogether, wordWrap, maxLines,
  textInterpretation, indents, readingOrder, booleanFormat, sectionFlags (4 flags),
  groupSelection — all on the plan vocabulary + TS.
- ☐ Date/time format value edits; paper source/duplex; picture crop; removeSort;
  removeGuideline — each needs a wire pass (documented in §32).

## Tier-B batch (shipped 2026-10-22 — §33)

- ☑ `addSummary` (grand totals; group subtotals deferred), `setParameter` (prompt text;
  the optional leaf refuses when the record doesn't carry it), `removeSort`.

## Deferred (with reasons)

- **removeGroup** — the reverse of addGroup (area-pair splice-out + renumber of later
  groups' `#N` identities + sort removal). Structural; needs the area-removal unit probed.
- **Table-link editing** (QESession `0x0a`) — their reports carry one link but never edit it.
- **RTF text interpretation** — zero corpus usage; the HTML interpreter covers the used case.
- **Per-row subreport link re-execution** — layout-engine change (ScopeData is scope-keyed,
  not row-keyed). Only worth building if the live preview (item 2) shows their procs return
  empty rows on NULL parameters; otherwise the current one-fetch-per-scope + in-memory
  link filter is correct.
- **Formula-slot exclusivity validator check** (wave-2) — blocked on per-slot static-default
  semantics for the ~80-member slot family; without it the check false-positives.
- **Charts / cross-tab / alert / OLAP editing** (engine-notes candidates 6–8, 11) — none of
  this workspace's reports use them.
- **Parameter `Pm-` PromptManager entry** — a synthesized `Pm-` parameter is raw-only (the
  model surfaces it via the GUID-less orphan path); a first-class PromptManager MetaObject
  would make it editable in the designer's prompt UI. Low priority: links work end-to-end.

## Shipped (record)

- 2026-10-22 — **Backlog batch**: addGroup corruption fix (§26), orientation/paper DEVMODE
  synthesis + thumbnail command (§27), folder consolidation, BACKLOG.md itself.

- 2026-10-22 — **HTML text interpretation** (`TextFormat::HTMLText`) — markup renders as
  styled text instead of literal tags (VENDORED.md #12; validation-log §25).
- 2026-10-22 — **Empty-data preview semantics** — no-data notice first + unknown-name noise
  filtered; group bands no longer print over an empty dataset (VENDORED.md #11;
  validation-log §24). WSR golden pin re-based onto a fixed render clock (root cause: the
  report renders as-of-dependent content, so wall-clock pins drift with the calendar).
- 2026-10-22 — **Live SQL Server data** (`rpt-db-mssql`): stored-proc reports over TDS,
  ADO connection strings, parameter coercion, connection cache; gated live smoke shipped
  (wire verification = backlog item 2).
- 2026-10-16 — **Follow-up batch**: setFormula / setRecordSelection (the format stores no
  tokens), Group #N Order mint + addGroup numbering fix, Pm- parameter creation,
  addRunningTotal + `{#name}` placement, connection reuse, wave-2 summary-op check
  (validation-log §23).
- 2026-10-16 — **Preview rendering** (vendored render stack; validation-log §21) and
  **live-DB track** (validation-log §22).
- Earlier — 38 layout ops (29 Vibey + Tier 0 + sections), create-from-scratch, load-path
  validation (13 checks), corpus 263/263, special-field wire table verification, sub-side
  link binding. See `docs/validation-log.md` §§1–20.
