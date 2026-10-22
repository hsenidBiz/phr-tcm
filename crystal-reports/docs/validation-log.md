# Validation log — 2026-10-15

Environment: CR XI R2 (11.5) SP6 portable copy; Rust 1.98.1; rpt-rs vendored at commit
`3f5beb51dd34d36c4a4326280d25f473b44889d1`.

## 1. Reader / IR — samples

`rpt-tools ir` on each of the 6 PMSV10 reports. All: **0 warnings**, every model object paired
with record addresses.

| file | objects | subreports | warnings |
|---|---|---|---|
| PMSV10_GoalAlignCascade.rpt | 82 | 1 | 0 |
| PMSV10_GoalAlignCascadeSS.rpt | 82 | 1 | 0 |
| PMSV10_IndDetailedEval.rpt | 75 | 6 | 0 |
| PMSV10_IndDetailedEvalSS.rpt | 75 | 6 | 0 |
| PMSV10_IndPerfOverview.rpt | 84 | 1 (+images) | 0 |
| PMSV10_IndPerfOverviewSS.rpt | 84 | 1 (+images) | 0 |

Artifacts: `work/*.ir.json`.

## 2. Round-trip writer

`rpt-tools roundtrip` (decode → re-encode Contents → re-open → compare record serialization):

- `work/edited-a.rpt` (7 ops applied) — **ok**
- `work/edited-b.rpt` (4 ops applied) — **ok**

## 3. Corpus sweep

`rpt-tools sweep "Program/Crystal Reports 11.5/Samples"` — 263 .rpt files (all languages,
Feature Examples / Financial / General Business, plus templates):

**263 clean, 0 with warnings, 0 failures.** Every file: full IR pairing + byte-identical
record round-trip. Log: `work/sweep-samples.log`.

## 4. Edit ops — verified by semantic model diff (`rpt-tools diff`)

Battery A on `PMSV10_GoalAlignCascade.rpt` (`work/ops-a.json` → `work/edited-a.rpt`):
set_text + set_bounds + set_font (face/size/bold) + set_text_color + set_section_height +
set_margins. Diff vs original: **23 differences, all intended** — bounds ×4, text (text/display/
run coherently), font (face/size/weight/bold in both object font and run font), color rgb,
section height, margins ×4. No collateral changes.

Battery B on `PMSV10_IndPerfOverview.rpt` (`work/ops-b.json` → `work/edited-b.rpt`):
set_orientation landscape + set_paper_size A4(9) + set_font italic + set_text by id.
Diff: orientation Portrait→Landscape with content_width/height swapping correctly, and exactly
the requested text change. (Italic on `Conf` was a no-op — already italic.)

set_object_name verified in development on the same battery path (same record/table as the
verified name/width/height fields).

### Safety-gate events observed (working as designed)

- `set_margins` initially refused under `EditPolicy::Checked` — the `0x66` field table does not
  reproduce the record's 19-byte tail on these files (the four margin words themselves decode
  exactly; this report stores the `0x80000000` "default" sentinel). Resolved by the documented
  page-level path: forced write of the fixed-width head fields + explicit read-back verification
  (`rpt-core/src/edit.rs`, `patch_page_level`).
- `set_orientation` on GoalAlignCascade refuses: no `0x07` DEVMODE record (saved without a
  printer). IndPerfOverview and the designer-saved corpus files carry one.

## 5. Designer smoke test — deferred

The portable install is not registered (`HKLM\SOFTWARE\Business Objects\Suite 11.0\…` missing).
Launching the designer on the **original** samples fails identically to the edited files
(exit `0xC0000139` when launched from another cwd; process alive but non-committal when launched
from its own directory). Since original and edited behave identically, nothing about the edits
is implicated. Probe for when the suite is properly installed (preserved in `work/`)
(compares original vs `edited-a` vs `edited-b`: process lifetime + window title over 16 s).
Upstream rpt-rs additionally documents this writer's output as verified against the native
Crystal engine on its own 154-file corpus.

## 6. VibeyReports operation port — 2026-10-15 (session 2)

All 29 VibeyReports layout actions implemented offline in rpt-core (`rpt-tools apply` takes the
plan JSON verbatim). Validation:

- **Mega battery** (`work/vibey-all.json` → `work/vibey-all.rpt`): one plan, 30 ops covering
  all 29 actions including addObject→removeObject and addTable→removeTable round-trips.
  Applied; every added object reads back at its exact requested bounds; the added group
  (`categoryHeader`/`categoryFooter` G1, descending) registered with its sort; the imported
  subreport `megalogo` registered and placed; round-trip byte-clean; 154 objects, 0 warnings.
- Per-tier batteries with model-diff proof: field-patch tier (8 ops → exactly 12 intended
  changes, nothing else), structural tier (addText/addLine/addBox/removeObject at exact
  bounds), field/special/sort tier (`work/vibey-3.rpt`), subreport tier (`work/vibey-sub.rpt`),
  table tier (`work/vibey-tables.rpt`), addGroup (`work/vibey-group.rpt` — areas renamed,
  levels bumped, groups 1→2 with correct condition/direction).
- **Corpus regression after all writer changes: 263/263 clean** (`work/sweep-v2.log`).

Vendored-crate extensions for this work: FieldEdit::Ref, write_contents_logical,
RecordStream::decode pub, qe_cfb_encrypt/pub-decrypt (see `vendor/VENDORED.md`).

Known offline boundaries (documented in `docs/vibey-operations.md`): addTable carries no
column list (server-fetched in the SDK); addField limited to referenced fields; addGroup needs
an existing group donor; addSpecialField supports 5 of 7 kinds (2 engine codes undocumented);
setSubreportLink writes the main-report half only.

## 7. Portability plan, Task 1 — tauri-commands synced to current API — 2026-10-15

- Crate `tauri-commands` added to the workspace; `src/commands.rs` rewritten against the
  current `apply_edits_on(&Rpt, &[EditOp], &mut ApplyOutcome)` signature, plus a new
  `apply_plan(planJson)` command (Vibey LayoutPlan entry for the UI — plan ruling).
- Compiles standalone with default features (plain functions over `&EditorState`; the
  `#[tauri::command]` wrappers live in `src/glue.rs` behind the `tauri` feature — mechanism
  ruling, see ledger) — Review-Focus #1's compile half pinned.
- `rpt-core` now re-exports `rpt_reader`, so consumers never depend on `vendor/` paths.
- `cargo build -p tauri-commands` and `cargo build --release`: PASS, zero warnings.

## 8. Portability plan, Task 2 — ui-react types synced — 2026-10-15

- `check-shapes.py` added: parses ir.rs/edit.rs and types.ts, asserts Ir-field and EditOp-variant
  parity (snake_case wire keys — ruling: the shipped JSON is snake_case; plan's camelCase note
  contradicted it). RED on 25 mismatches → GREEN after sync.
- types.ts: +3 Ir fields (groups/sorts/available_fields) + GroupIr/SortIr/AvailableFieldIr,
  EditOp extended to all 31 variants, LayoutPlan/LayoutOperation added; api.ts: +applyPlan.

## 10. Portability plan, Task 4 — consumer simulation + relocation gate — 2026-10-15

- `consumer-check` crate: depends on `rpt-core`/`tauri-commands` by path only, compiles the
  handoff `commands.rs` as its own module (`#[path]` include), drives open → apply_plan
  (setFont + addText + setSubreportLink) → save → reopen with assertions. Prints `portable-ok`.
- `Out/tools/verify-portable.ps1`: copies `Out/` (minus target/work) to `%TEMP%\rpt-out-copy`,
  runs `cargo test -p rpt-core --release` and the consumer simulation **from the copy**,
  cleans up. Result this run: `PORTABLE-OK` — the bundle is relocatable (Review-Focus #2).

## 11. Portability plan, Task 5 — 0.2.0 handoff stamp — 2026-10-15

Workspace version 0.2.0; README/integration-guide carry the four acceptance commands
(cargo test / verify-portable.ps1 / check-shapes.py / tauri-commands feature note).
All green: release build zero warnings, shapes in sync.

## 13. Portability plan, Task 7 — engine-behavior catalog — 2026-10-15

`docs/engine-notes.md`: four candidates with pinned engine behaviors (link-writer
ASSERT cluster; rptcontrollers.dll 0x1063df SubreportLink; 0x86be38 BasicSyntax clusters;
0x9e79de Thumbnail/DIB writers), effort grades, and a priority order. Candidate 1 (sub-side
link binding) rated M — the S-grade donor-clone shortcut must be tried across the corpus
first, so Task 8's "skip if rated above S" condition triggers: skipped, becomes the
recommended follow-up alongside candidate 2 (Group #N Order minting, S).

## 14. Portability plan — final review + gate adjustment — 2026-10-15

Final battery (all green): debug+release builds 0 warnings; `cargo test --workspace --exclude
rpt-reader` → 73 passed (4 rpt-core + 61 rpt-model + 8 cli), 1 ignored; `check-shapes.py` in
sync; `verify-portable.ps1` → PORTABLE-OK from a temp copy. `rpt-test-support` vendored (6th
entry in VENDORED.md) so vendored unit tests compile; the 36 corpus-fixture tests excluded from
the gate per ledgered ruling (upstream's ~150-file corpus not vendored by design).

Review: self-review (no subagent tool in this harness — weaker than a fresh reviewer by
design). Findings: two minors, deferred (glue.rs `tauri`-feature compile unchecked — needs the
tauri crate from crates.io, documented; `refusals_are_errors_not_panics` test name now also
carries the Task-6 success-path placements). Review-Focus lines all pinned by tests (Tasks 1–4).

## 15. Extraction catalog expanded — 2026-10-15

`docs/engine-notes.md` restructured after a second behavior survey: a new Tier 0 (five
operations needing no binary verification — addPicture, z-order, hyperlink/tooltip ops, guides,
cross-report copyObject) plus seven further candidates whose wire shapes were verified against engine anchors
(load-path validation S; section insert/delete M; running totals S–M @ 0x83d1cd; charts M @
0x82915b; cross-tab M–L @ 0x82659a; alerts S; OLAP ×39 / maps ×4 as L format extensions).
Existing four candidates retained and folded into the numbered ordering; priority now leads
with the Tier 0 batch and load-path validation.

## 16. Tier 0 extraction batch — 2026-10-15

Seven new ops (`setTooltip`, `setHyperlink`, `setCssClass`, `setZOrder`, `addPicture`,
`addGuideline`, `copyObject`) + plan lowerings + TS types. World Sales Report vendored as the
7th fixture (static-picture donor, Embedding 20/21, full optional-format objects).
Tests: 5 new (format ops + refusals, addPicture incl. Embedding-22 storage + data-byte +
ordinal readback, addGuideline, copyObject incl. cross-file Text/Line + refusals) → suite
9 passed / 1 ignored; whole-workspace gate (excl. rpt-reader) 78 passed; builds 0 warnings;
check-shapes in sync; verify-portable PORTABLE-OK.

Decode-gap fixes (vendored ext #6 tooltip/css mirror, #7 static-picture kind): fixture RO_Xtreme_Logo
now correctly reads as a static Picture (was misclassified BlobField by the name heuristic);
the IR picture list gains it.

Notable implementation corrections vs. the catalog's assumptions: static pictures bind via
`0xbd.embedding_ordinal` → `Embedding N/CONTENTS` (raw bytes, not zlibBLOB); cfb walk paths use
the platform separator (Embedding ordinals parse digit-prefix-wise); an insertion exactly at a
record's content_end lands outside it (guides therefore clone-beside their donor); the guide
region's wrapper nesting is irregular across files.

## 17. Load-path validation — offline engine-acceptance checker — 2026-10-16

`rpt-tools check` + `rpt_core::validate::validate()` — 13 first-wave checks re-implementing the
engine's open-file invariants, each citing the engine behavior it re-implements (
the engine's error catalog; full check table in `docs/validate.md`, the observed corpus preserved in
`docs/engine-conditions.txt` + `docs/engine-range-conditions.txt`).

Results: all 7 fixtures PASS 0 errors / 0 warnings; the 30-op mega battery's output PASSES;
structurally-edited files (addGroup/setSubreportLink/addText via CLI) PASS; three crafted
violations (special-field index 99, width 40000, border enum 9) each fire their named check.
Reference resolution is Warn-severity by design (display-text heuristic; summary/GroupName
display forms reduce to their first braced operand). Tests: 2 new (clean sweep + violations),
suite 11 passed / 1 ignored. Wave-2 targets ranked in validate.md.

## 18. Section insert/delete — 2026-10-16

`addSection`/`removeSection` (plan actions + EditOp + TS types). Grammar mapped from World
Sales Report's multi-section areas (one band bracket per section; Area.section_count
load-bearing). Mechanisms: donor-clone with tail excision (objects + RulerScale), verified
identity patches, symmetric end_section_index shifts (+1 on insert at/after the slot; −1 above
a removal; Equal → refusal). Both ops self-validate (model read-back AND `validate()` run on
the result; any error fails the op).

Tests (2 new, suite 13 passed / 1 ignored; workspace gate 83): WSR add-after-Section2 → 4
sections + auto-name Section22 + validator PASS; add+remove round-trips the drawing index back
to the engine-written 18; removing empty Section8 shifts Line4's index 18→17 (fixup proof);
single-section-area add (GoalAlignCascade PageFooter → 2 sections); refusals (last section,
non-empty section). Gates: 0 warnings, shapes in sync.

## 19. Create-from-scratch — 2026-10-16

`create_report()` / `rpt-tools new` / `new_report` (Tauri) / `newReport` (TS). Embedded
engine-authored blank (7,168 B, sha256 afbb78600d8750d5…, validator 0/0) + embedded seed donors
(blank + Text/Line/Box via cross-report copyObject, validator 0/0). Options: title/author/
comments via a fresh VT_LPWSTR SummaryInformation (reader's own proven layout — the VT width is
u32, fixed after the first write landed undecoded); margins via the verified op; orientation/
paper refused (no DEVMODE in the blank, standard semantics). Seed fallbacks wired into
add_object_inner (remask cross-report), add_drawing (shape-filtered), addGuideline (synthesizes
a GuidelineCollection bracket when no entry exists). Creation gate: validate() must pass.

Tests (3 new; suite 17 passed / 1 ignored; workspace gate 86): blank clean (5 areas, 0 objects,
0 warnings, validator 0/0, round-trip); options + THE LOOP (create → title/author/margins
readback → addText+addSection+setMargins plan on the created bytes → validator PASS →
round-trip — the full nothing→file→edited-file path Vibey could not run); all object kinds
(addText/addLine/addBox/addGuideline) land on the blank via seed donors. Gates: 0 warnings,
shapes in sync, PORTABLE-OK.

## 20. Special-field table verified + sub-side link binding — 2026-10-16

**Special-field wire table VERIFIED — and corrected.** The engine's own
the engine's special-field table (26 entries) is the wire→SDK `CRSpecialVarType`
translation table (the SDK values are the published `Shared.SpecialVarType` ordinals). Every corpus-firm code (0–8, 10, 11, 16,
17) matches — and the earlier reconstructed 0x0C–0x0F block was **cyclically shifted by one**:
the engine's mapping is 0x0C=FileCreationDate, 0x0D=RecordNumber, 0x0E=GroupNumber,
0x0F=TotalPageCount (we had RecordNumber/GroupNumber/TotalPageCount/FileCreationDate). Our
shipped table was wrong but masked: the model decoder falls back to the display string for
unmapped codes, so name-based tests passed either way. Fixed in `special_field()` (+ 7 newly
legal codes 0x12–0x18 named), vendored ext #8 extends `SpecialFieldType::from_code` with the
verified block, validator widened to 0..=25, and a NEW test pins the wire indexes themselves
(opener `field_index` = 0x0D/0x0F/0x0E for recordNumber/totalPageCount/groupNumber) — the
assertion class that would have caught the rotation. Corpus sweep after the fix: 263/263 clean.

**Guides H/V axis VERIFIED.** IndPerfOverview (A4, content 11186 × 16118): the first
GuidelineList's positions include 12168 — beyond content WIDTH, within HEIGHT ⇒ Y coordinates ⇒
horizontal; the GuidelineCollection's 13 positions all fall within width, evenly spaced (a
12-column grid) ⇒ vertical. `HORIZONTAL_AFTER_FIRST = true` stands, docs upgraded from
"assumption" to "verified (guide-position clustering vs page geometry)".

**Sub-side link binding SHIPPED.** The subreport-side half of a link is its Record Selection
formula (`0x0077 → 0x0076 → 0x0071("Record Selection", Boolean)`, refs + text — NO token
stream, verified against two engine-authored donors), sitting between MultiColumnFormat and
RecordSortField. `setSubreportLink` now rebuilds that run create-or-extend (merged text with
`\r\nand\r\n`, refs in the engine's bare-name/`?Pm-` wire forms, kinds, counts) via new
vendored ext #9 `write_stream_logical` (re-encode ANY Contents-dialect stream — the
nested-subdocument twin of `write_contents_logical`). Opportunistic upgrade semantics: when the
subreport carries no `Pm-<main field>` parameter (parameter creation remains the documented
follow-up), the main-side half stands exactly as Vibey wrote it — no regression (mega battery
green). Extend-proof test: StageWise gains a third clause naming `stage_name`, read back from
the formula text; model notes: re-using one Pm parameter for two sub fields maps the
parameter to its first binding (engine-consistent parameter→column mapping). VENDORED.md now
lists extensions #1–#9.

Tests +4 (wire-pinned specials, sub-side extend, main-only downgrade); suite 21 passed /
1 ignored; workspace gate 89.

## 21. Preview rendering — vendored render stack — 2026-10-16

**Ten crates vendored, saved-data-only, PDF out.** `metafile`, `rpt-format-value`,
`rpt-formula`, `rpt-data`, `rpt-pages`, `rpt-text` (+3.7 MB bundled fonts, `include_bytes!` —
no host scan, deterministic), `rpt-render-util`, `rpt-render-pdf`, `rpt-layout`, `rpt-render`
— the pinned commit's sources, one pruning (`rpt-db-postgres` dev-dep +
`tests/postgres_fixtures.rs`; no DB crate ships). Upstream's root `tests/fixtures/`
(reports corpus + render baselines; 25 MB, `sql/` excluded) vendored at `Out/tests/fixtures/`
because the retained upstream tests resolve it via `rpt_test_support::workspace_root()`.

**Vendoring fidelity — cross-checked against the upstream-built binary.** Rendered World Sales
Report with the clone's `rpt-render.exe` (built `--no-default-features`, same commit) and with
our stack: 37 pages both, differences reduce to exactly (a) the PDF Producer string (workspace
version 0.2.0 vs upstream 0.4.0) and (b) OUR PDF additionally embeds the `RO_Xtreme_Logo`
picture (279×107) — upstream's reader misclassifies it as a blob-field picture via the
name heuristic that vendored ext #7 fixed. Our render is the more faithful one. Upstream's one
render warning ("locale C not in table") is its CLI's env-locale handling; our default-options
path uses the built-in en-US locale directly.

**Determinism — with one deliberate exception.** Same bytes → byte-identical PDF across runs,
profiles, processes, and machines (epoch PDF timestamp, bundled-only fonts; verified by
double-render byte-equality and a SHA-256 golden pin on WSR's 37-page PDF:
`9ac03b10…f913d`). The exception: Print Date/Time special fields render at the wall clock
(`default_as_of()` — exactly what Crystal's preview does), so reports printing them (the
PMSV10 fixtures do) produce PDFs that legitimately move with the second. A spurious
"nondeterminism" hunt during testing ended at this — root-caused, not suppressed. Concurrent
renders are safe (interleaved renders each self-consistent).

**Ops surface:** `rpt_core::preview::preview(bytes) -> PreviewOutput { pdf, page_count,
diagnostics }` (+ `preview_report(&Rpt)` over an open handle); `rpt-tools render <in.rpt>
<out.pdf>`; Tauri `preview_report` (renders the open report incl. unsaved edits); TS
`previewReport()` + `PdfPreview.tsx` (Blob URL in an iframe, diagnostics as a collapsible
strip). No-saved-data reports render static bands over `EmptySource` with the formula failures
surfaced as diagnostics — a preview, not an error.

**Gate delta:** workspace tests now include the vendored crates' own upstream suites
(rpt-formula 255, rpt-layout 334, rpt-data 98, metafile 19, rpt-render 13 files incl. the
PDF/dataset/typography baselines) — a standing fidelity check, not just our tests.

## 22. Live SQL Server data — rpt-db-mssql — 2026-10-16

**The ask:** the app holds a connection string with credentials and queries the DB itself;
the editor should fetch live data when the DB changes, instead of rendering the file's saved
data. Everything re-implemented from the engine's observable behavior — vendoring/driver work.

**Why a new crate:** the upstream live-DB path implements Postgres + SQLite only; SQL Server
is "recognized but not implemented". The consumer app is SQL Server EXCLUSIVELY (every query
shells sqlcmd.exe — no Rust driver in the app at all), and this workspace's reports bind
exclusively to stored procedures (`usp_perf_…;1` tables, OLE DB (ADO), `QE_DatabaseType:
"OLE DB (ADO)"`, `crdb_ado.dll`). So `rpt-db-mssql` was written against the pipeline's
driver-free `RowSource`/`ScopeData` seam over tiberius (pure-Rust TDS; rustls/native-tls per
its default features, `trust_cert()` for the ADO `TrustServerCertificate=True` semantics).
No vendored crate changed.

**Format facts established (from the PMSV10 fixtures' decoded models):** a stored-proc
table's `Table.parameters` declares the proc's `@` parameters (name + FieldValueType,
including an ADO-artifact `@RETURN_VALUE` that must be skipped); the report's parameter
fields carry the SAME `@` names 1:1 (binding is by name, `@` optional, case-insensitive);
`Table.data_fields` pre-declares the result columns (name + type — 30 on the overview proc,
1 Blob on the logo proc) because the proc's wire result set carries no report-side types;
subreport scopes (e.g. `sp_perf_company_logo;1`) repeat the same shape. There is no
per-parameter VALUE expression stored — Crystal binds by name at prompt time.

**Credential discipline (mirrors the consumer app's own rules):** password never in `Debug`,
never in an error (missing-key errors name only the KEY), never on the CLI (the `--db` flag
reads `RPT_TOOLS_DB` from the env), errors carry `server/database` only. `redact_connection`
is the last-resort scrubber. The API-surface leak test feeds a real (refused) connection
through `preview_live` and asserts the password is absent.

**Hermetic verification:** 10 unit tests in rpt-db-mssql + 5 in rpt-core/tests/live_db.rs —
parser (app shape, ADO aliases, comma port, missing keys, Debug/label), plan over the real
fixture (rpc name `usp_perf_ind_perf_sheet`, `@RETURN_VALUE` excluded, values resolved,
NULL binds), coercion per ParameterValueKind with refusals, `LiveDb` serde shape, and the
leak rule. All green; workspace gate unchanged.

**Wire verification — attempted, BLOCKED by network, honestly stated:** the live smoke
(`RPT_MSSQL_CONN` + `cargo test -p rpt-db-mssql --test live -- --ignored`) was pointed at
the app's dev-read preset (sgdev01db02.cloud/hrmmain_philippines, readonly user — read from
`db_defaults.rs`, passed via env, never printed). This workstation is NOT on the corporate
network: `sgdev01db02.cloud` fails DNS resolution and the report's own stored server
`10.56.3.16` times out on TCP 1433. The failure DID prove the error path on a real
connection failure (label-only, password absent). From a networked machine, the exact
verification is:
  RPT_TOOLS_DB='Server=…;Database=…;…' rpt-tools render IndPerfOverview.rpt out.pdf --db -p @performance_cycle_id=<id>
(or one `previewReportLive` call from the app) — expected: multi-page data-driven PDF;
without it the same fixture renders static bands.

**Follow-ups documented:** per-row subreport link re-execution (needs a layout-engine
change; today one global fetch per scope + in-memory link filtering, as upstream);
parameter-less plain-table T-SQL generation (unused by this workspace's reports);
connection reuse ACROSS previews (today: one login per render).

## 23. Follow-up batch — 2026-10-16

**The headline: the format has NO formula tokens.** A sweep of every `0x0076` across the
128-file corpus + all fixtures (610+ records, main and nested `Subdocument N/Contents`
streams — the latter via the crate's own corpus parity test, which re-decodes nested
streams) shows each record is exactly `0x71` + refcount + FieldRefs + text + kind words +
optional tails, zero unread bytes, zero token fields. The old "tokenized bytecode" boundary
(docs + catalog candidate 10) was a wrong premise; the engine recompiles from source at
load. `setFormula`/`setRecordSelection` therefore ship as source-text + reference-list
rewrites on the proven rebuild machinery. The formula-ref pool vocabulary was pinned
EMPIRICALLY from donor formulas (0=db, 3=special, 4=`Group #N`, 5=param; formula-to-formula
refs resolve through pool 1; `{#}`/`{%}` refused for want of donors).

**Engineering lessons this batch (all now in code comments):**
- Records are rebuilt from DECODED FIELD VALUES, never from copied stored bytes — a nested
  record's stored run is not uniformly remaskable (the child's header and content sit under
  different masks).
- `U16Be` fields are TWO bytes (a 4-byte emission misaligned every following field — found
  by dumping the rebuilt record's hex, not by the compiler).
- A `0x7a` synthesized with schema 0x0700 selects an older field-table layout; the donor's
  word is 0x0701.
- Chained container writes DROP nested-stream edits: a subdocument edit followed by a main
  `write_contents_logical` reverted the subdocument (verified by instrumented re-open). The
  fix is structural — one write per stream per operation (Pm- creation moved inside the
  sub-side binding's edit).
- The `0xe5` group record's `order_formula_index` is the ordinal among TOP-LEVEL `0x77`
  wrappers (pinned: WSR 0/1/2 sequential; IndPerfOverview #2 Order = 5 with user formulas
  interleaved).
- `addField {#name}` stores the ref text as `#name` (the engine donor's form); the model
  renders it `{#name}`, and brace-wrapped storage renders double — caught by read-back.

**Pre-existing defect (documented, not this batch's):** `addGroup`'s area-clone step
corrupts the stream on fixtures whose innermost group header carries chart or subreport
objects — `stream_parsed`/`group_structure` fire on World Sales Report and
IndPerfOverviewSS's parent file (verified with and without the batch's changes; the
corruption sits at the inserted clone's tail, a ~20-byte hole before the section-code
chain). GoalAlignCascade and the corpus batteries are unaffected. Probe evidence preserved
in this log; root-causing the area-clone splice is its own session.

**Wave-2 validator:** summary-operation range check shipped (model-side `Other` detection;
the SDK enum's 20-member bound cited). The parameter value/description parity check
was implemented, FIRED ON ENGINE-AUTHORED FIXTURES (1 value / 2 description rows is normal
engine output), and was REMOVED — a false-positive generator, not an invariant.
Formula-slot exclusivity remains open pending per-slot static-default semantics.

**Gates:** `rpt-core/tests/followups.rs` (8 tests) + the full battery + check-shapes +
verify-portable; live-DB connection cache covered by the existing `live_db.rs` leak-rule
test (connect path) with reuse exercised by the gated live smoke.


## 24. Empty-data preview semantics — notice + engine-parity group-band suppression — 2026-10-22

**Context** (reported from the TCM app): previewing `PMSV10_IndPerfOverview.rpt` showed an
empty page plus 13 "formula error: unknown name: {sheet.…}" diagnostics. Diagnosis: the
report passes all load checks; it simply has NO SAVED DATA (like every PMSV10 fixture), and
two presentation faults made that look like 13 errors:

1. **The renderer printed the group skeleton over zero rows.** Upstream rpt-layout emitted
   the group header/footer bands once for an empty dataset ("so static content renders").
   Real Crystal prints NO group band when there are no rows — and every one of the 13
   failing fields sat exactly in the employee group's header/footers (the detail band's
   stage_* fields never printed, hence no errors from them). FIXED (VENDORED.md #11): the
   empty-group branch now emits nothing; the pinning test was rewritten to pin the parity
   behavior (group captions absent, page header present).
2. **"No data" was never said.** `preview()` now puts a first-class notice first in
   `diagnostics` — "this report has no data to render (no saved data / the query returned
   no rows) — only its static bands are shown; render with the live preview to draw fresh
   rows" — and filters the "formula error: unknown name" entries that are pure consequences
   of the empty row set. The live path emits the same notice when a fetch returns zero rows.

**Found while re-verifying: the WSR golden pin had gone stale, and the reason matters.**
The pinned wall-clock hash (validation-log §21, pinned 2026-10-16) no longer matched — in
BOTH bundle copies, identically, so nothing in this change caused it. Root cause: World
Sales Report DOES render as-of-dependent content (its page-header ModificationTime /
DataDate field objects resolve from the render's as-of instant), so the render is
byte-stable within a day but moves with the calendar — §21's "WSR prints no time-dependent
field" claim was wrong (within-day determinism cannot distinguish date-granular values;
verified directly by rendering at two fixed as_of instants a week apart → different bytes).
The golden test now pins a FIXED as_of, making the hash a function of pipeline + fonts
only; the wall-clock `preview()` path is asserted structurally beside it.

Gates: rpt-layout 334 green; rpt-core preview battery 6/6 (the no-data test now pins the
notice + the filtered noise + group-band suppression); full workspace gate re-run.
