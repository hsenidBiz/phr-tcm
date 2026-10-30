# VibeyReports layout operations — offline Rust implementation

All **29 layout actions** of the VibeyReports contract
(`VibeyReports.Contracts.LayoutActions`) are implemented in `rpt-core`, driven by the same
plan JSON the C# worker takes — camelCase, same action names, same required-field rules.
MCP layer deliberately not ported (user decision).

```
rpt-tools apply <in.rpt> <plan.json> <out.rpt>
```

```json
{"planVersion": 1, "operations": [{"action": "setFont", "target": "FtConf", "fontName": "Tahoma"}]}
```

The native op vocabulary (`rpt-core::edit::EditOp`) is a superset: the same operations accept
either vocabulary — `rpt-tools edit` takes snake_case native ops, `rpt-tools apply` takes the
Vibey plan shape.

## Operation status

| action | mechanism | notes |
|---|---|---|
| move, resize | verified field patch (`0x9e`/`0xbe`) | line/box endpoints kept in step (`0xa9` right/bottom) |
| setFont / setFontSize / setBold | verified field patch (`0x8`) | weight ≥ 700 reads as bold; size stored in points + twips |
| setAlignment | `0xc0` (authoritative) + `0xfc` | Left/Right/Centre/Justified |
| setTextColor / setFillColor / setLineColor / setSectionBackground | colorref word patches (`0x100`/`0xec`/`0xfe`) | `#RRGGBB`; wire form `[0,B,G,R]` BE |
| setSectionBreak | `0xfe` flags | new_page_before / new_page_after |
| setCanGrow / setSuppress | `0xfc` (object) / `0xfe` (section) | suppress = visible==0; suppressIfBlank section-only |
| setBorder | `0xec` edge styles + colour | none/single/double/dashed/dotted = 0–4 |
| setNumberFormat | `0xf0` gate + both `0xf8` slots | clears use_system_defaults (the SDK gate); rounding = 11 − dp |
| addText / addLine / addBox | donor-clone run splice | line = `0xaa` wrapper, box = `0xac`; anchor after last object or section-code records; `object_count` fixed |
| addField | donor-clone + verified ref rebind (`FieldEdit::Ref`) | pool 0, index = ordinal among DB field defs; the field must already be referenced by the report |
| addSpecialField | donor-clone + ref rebind | full engine-verified 18-code table (the engine's own special-field table): codes 0x00–0x11 + 0x12–0x18 (HPageNumber…ContentLocale); wire-pinning tests assert the codes themselves, not just names |
| removeObject | run splice-out + count fixup | any object kind; verification = model no longer lists it |
| addGroup | full-footprint clone | group def `0xe5` + GH/GF area brackets `0x8a→0x8b` + its sort; renamed per GroupSectionNaming (`{alnum}Header/Footer`, `…Section1`), group level bumped in every `0x9b`; donor group required |
| addSort | clone or synthesize `0x29` | first sort synthesized from scratch (placed after the last group def) |
| addSubreport | CFB storage + placeholder clone | imports the .rpt under `Subdocument N` (cfb crate), names it via its root `document_name`, clones a `0xa3` placeholder with `subdocument_index` patched; a donor subreport object in the target is required |
| setSubreportLink | synthesized `0x106` sibling **+ sub-side Record Selection binding** | parameter index resolved from the subdoc's `0x7a` records; both pool handles resolved; the subreport's `0x77→0x76→0x71("Record Selection")` formula run is created-or-extended with `{sub.field}={?Pm-main.field}` lines joined by `\r\nand\r\n` (refs: db kind 0 + Pm- param kind 5). Opportunistic: without a `Pm-` parameter on the subreport the main-side half stands alone (parameter creation is the documented follow-up) |
| removeTable | QESession splice-out | `0x3 QeTable` under its `0x2 QeConnection`; count fixed; Contents untouched (placed fields keep dangling refs, exactly the SDK's measured behaviour) |
| addTable | QESession donor clone | connection cloned from the donor table (Vibey's own semantics — no credentials anywhere); new identity; **column list stripped** — the SDK fetches real columns from the live server, which an offline editor cannot do |
| setTableLocation | QESession rename | name + qualified_name repointed, alias untouched |

## Offline differences from the SDK worker (all deliberate, all verified)

1. **No database contact, ever.** `addTable`/`setTableLocation` in the SDK verify against the
   live server (Vibey's own notes: both throw "Logon failed" without `VIBEY_DB_PASSWORD`).
   The offline implementation writes the metadata only; the engine verifies on the next
   open/refresh. `addTable` therefore carries no column list.
2. **`addField`'s allowlist is the report's own referenced fields** (`ir.available_fields`),
   not a live `DataFields` enumeration — an unreferenced column has no pool index offline.
3. **`addGroup` needs an existing group to clone** (every report in this workspace qualifies).
4. `addSpecialField` carries the full engine-verified 18-code table (see the operation row);
   unmapped wire codes are refused.
5. `setSubreportLink` writes BOTH halves (main-side `0x106` + sub-side Record Selection
   formula); it needs a `Pm-` parameter already on the subreport — the SDK would create one,
   parameter synthesis is the documented follow-up.

## Validation (this workspace, PMSV10_GoalAlignCascade unless noted)

- **Mega battery**: one plan, 30 ops covering all 29 actions (including addObject→removeObject
  and addTable→removeTable round-trips) — applied, model-verified, byte-clean round-trip,
  0 IR warnings. `Out/work/vibey-all.rpt`.
- Per-tier batteries with `rpt-tools diff` proof: 8 field-patch ops → exactly the 12 intended
  changes; structural tier → objects at exact bounds; subreport tier → links/subreports
  registered; table tier → rename/clone/removal verified against the model.
- **Corpus regression after all writer changes: 263/263 sample reports clean** (full IR pairing
  + byte-identical record round-trip; `Out/work/sweep-v2.log`).

## The first Vendored extensions (see `vendor/VENDORED.md` for the full numbered list)

1. `FieldEdit::Ref` — verified field-reference rebinds (`addField`/`addSpecialField`).
2. `Rpt::write_contents_logical` — the structural splice seam.
3. `RecordStream::decode` public — walking subdocument streams.
4. `qe_cfb_encrypt` + `RecordStream::qe_cfb_decrypt` public — the QESession re-encoder.

## Tier 0 extensions (2026-10-15 extraction batch)

Seven further operations on the same plan vocabulary — beyond the Vibey contract, named in its
style. All pure op-writing on the proven machinery — re-implementing what the designer's own save path produces; no binary verification required.

| action | mechanism | notes |
|---|---|---|
| `setTooltip` | `0xfc.tool_tip_text` (unconditional field) | empty string clears |
| `setHyperlink` | `0xfc.hyperlink_text` + `hyperlink_type` | kind: website/email/html/none/reportPart/anotherObject (RAS ordinals, 6 = none) |
| `setCssClass` | `0xfc.css_class` (optional field) | clean refusal when the record doesn't carry it |
| `setZOrder` | run move to the section's object-span end (front) / start (back) | record order IS the layering — no z field exists anywhere; object_count unchanged |
| `addPicture` | fresh `Embedding N` storage (donor storage copied whole, CONTENTS = the image bytes) + donor-cloned picture run with `0xbd.embedding_ordinal` rebound | static pictures only; needs an in-report static-picture donor (chart graphic bases and blob-field pictures are excluded by their wrapper tags) |
| `addGuideline` | clones an existing `0x10c` entry, inserts the copy beside it, patches `position` | the guide region's wrapper nesting is irregular across files, so the op makes no framing assumptions about it; H/V axis mapping (first entry = horizontal) is VERIFIED — guide positions cluster against page geometry (A4 fixture: first list holds a position beyond content width but within height, i.e. Y coords) |
| `copyObject` | cross-report run clone (uniform outer-mask re-mask) into the target section | v1 scope: single-run Text, Line, Box — no FieldRef handles in their core records; embedded-field text and pool-bound kinds are refused |

New plan-JSON fields: `kind`, `z`, `axis`, `imagePath`, `fromReport`.

Two decode gaps fixed in the vendored crate to support these ops (VENDORED.md #6/#7): the
model now mirrors the record's tooltip/css_class, and static pictures keep their kind when
their name doesn't start with "Picture" (the old heuristic reclassified e.g. `RO_Xtreme_Logo`
as a blob-field object).

## Section insert/delete (2026-10-16)

The structural gap the SDK measured as impossible (`ISCRReportDefController` has no section
Add/Remove — only group creation mints sections). Each section is one self-contained band
bracket `BandOpen → 0x8c → 0x9d chain → 0xff/0xfe → [objects] → BandEnd`; the Area record's
`section_count` is load-bearing (the engine loops on it rather than scanning).

| action | mechanism | notes |
|---|---|---|
| `addSection` {target=area, section=after?, newName?, heightTwips?} | donor-clone of a sibling's whole bracket (band tags automatically correct — same area), with everything between the format records and the BandEnd excised (objects + any RulerScale); verified-patch name/height/`object_count=0`; `section_count +1`; **+1 shift of `end_section_index` on drawings at/after the insertion slot** | name defaults to the engine's global `SectionN` counter + 1 (gaps are legal — the engine never renumbers); height defaults to the donor's; the donor's 0xff condition-formula slots carry over (shared-pool refs) |
| `removeSection` {target=area?, section} | refusals first (last section in area; non-empty — engine path requires an empty section; bracket hosts the area's RulerScale; a drawing's `end_section_index` names it), then bracket delete, `section_count −1`, **−1 shift of drawing indices above** | `end_section_index` (I16Be, on `0xa9` drawings, report-global canonical 0-based) is the only stored section reference anywhere in the format — verified by a full field-table sweep |

Both ops verify via the model AND run the load-path validator on their own result (a refusal
from either fails the op — no invalid file is ever returned).

## Creating reports from scratch (2026-10-16)

The last SDK restriction gone: Vibey always started from an existing .rpt. `create_report()`
(library + `rpt-tools new` CLI + `new_report` Tauri command) issues a blank file from nothing:

- The blank is an **engine-authored specimen embedded as bytes** (`src/blank.rpt`, ~7 KB: CR
  11.0, 0 objects, the standard five areas, empty query session) — `VibeyReports/out/reports/_shell_logo.rpt`
  with its one residual QESession table removed via the normal `remove_table` op; it passes the
  load-path validator 0/0 and round-trips. Chosen over synthesis: the one untested surface of
  from-scratch is real-engine acceptance, and an authentic engine-authored blank answers it
  better than re-deriving the save path would (pure synthesis is documented as a
  follow-up in `docs/engine-notes.md`).
- Options: `title`/`author`/`comments` (fresh minimal SummaryInformation property set —
  VT_LPWSTR, the reader's own proven layout), `margins` (verified op), `orientation`/`paper_size`
  (refused with the standard DEVMODE explanation — the blank carries none).
- **Seed donors:** a fresh blank has no objects, so addText/addLine/addBox would have no donor.
  An embedded `src/seed.rpt` (blank + one Text/Line/Box, built by the editor's own cross-report
  copyObject) is the fallback — runs re-masked across reports by the copyObject rule. Guides on
  a guide-less report synthesize a fresh GuidelineCollection bracket.
- Creation gate: the result must pass the load-path validator or creation fails.
- CLI: `rpt-tools new out.rpt [--title T] [--author A] [--orientation o] [--paper N]`.

## Preview rendering (2026-10-16)

The editor can now show what a report *looks like*, not just where its objects sit:
`preview()` (library + `rpt-tools render` CLI + `preview_report` Tauri command) renders the
open report — unsaved edits included — to a real, data-driven PDF.

- **Vendored render stack** (VENDORED.md #10): ten crates from the same pinned rpt-rs commit —
  `rpt-render` (facade), `rpt-render-pdf` (krilla writer), `rpt-render-util`, `rpt-layout`,
  `rpt-data`, `rpt-formula`, `rpt-format-value`, `rpt-text` (+ 3.7 MB bundled Liberation /
  DejaVu fonts, embedded via `include_bytes!`), `rpt-pages`, `metafile`. The DB crates stay out:
  **saved data only, zero-config** — `RenderSource::Saved` uses the report's own saved records
  (saved-batch filtering, no re-run of record selection); no saved data → `EmptySource` →
  static bands render (title, headers/footers, unbound text). A preview, not an error.
- **PDF is the only output backend** (upstream removed raster/SVG/HTML in 0.4.0 — "the one the
  project will build on"). The webview displays the bytes natively: Blob URL in an `<iframe>`
  (`PdfPreview.tsx`), zoom/scroll free. PNG thumbnails remain a follow-up (pdf.js on the TS
  side would do it without touching Rust).
- **Fonts**: bundled-only by default — `Arial`→Liberation Sans, `Times New Roman`→Liberation
  Serif, `Courier New`→Liberation Mono (metric-compatible), DejaVu Sans as per-glyph symbol
  fallback. No host font scan → identical output on every machine.
- **Determinism**: byte-reproducible for any report that prints no time-dependent special
  field — Print Date/Time render at the wall clock (`default_as_of()`), exactly like
  Crystal's own preview. Tests pin this on World Sales Report (time-free): render twice →
  byte-identical; one text edit → different bytes; **SHA-256 golden pin** on its 37-page
  PDF (`tests/preview.rs`) — a hash change is a review event, never an accident.
- **Vendoring fidelity, cross-checked** against the upstream-built `rpt-render` binary on the
  WSR fixture: byte differences reduce to (a) the Producer version string and (b) **our PDF
  additionally embeds the `RO_Xtreme_Logo` picture** — upstream's reader misclassifies it as a
  blob-field picture (the name heuristic VENDORED.md #7 fixed); our render is the more
  faithful one.
- **Diagnostics**: render-level problems (an object kind the renderer doesn't draw, a formula
  failing over the empty row set) come back as strings in `PreviewOutput.diagnostics` instead
  of failing — the CLI prints them as `note:` lines, the TS component shows a collapsible
  amber strip. WSR renders clean; the six PMSV10 fixtures report their byline-formula
  failures over the empty row set, which is exactly right.
- Tests: `rpt-core/tests/preview.rs` — all 7 fixtures → structurally valid PDFs (header, EOF,
  page-count vs object graph), WSR 37 pages with text-showing operators + the logo image
  embedded, PMSV10 static-band degradation, from-scratch blank previews, determinism +
  edit sensitivity, golden hash.

## Live SQL Server data (2026-10-16)

The preview can now fetch FRESH rows from your database instead of the file's saved data:
pass your app's own connection string and the render runs the report's stored procedures
live. `preview_report_live` (Tauri; renders the open report incl. unsaved edits),
`rpt_core::preview::preview_live[_report]` (library), `rpt-tools render <in> <out.pdf> --db
-p Name=Value` (CLI — the connection string comes from the `RPT_TOOLS_DB` env var, never the
command line, so the password cannot appear in `ps` or shell history).

- **New crate `rpt-db-mssql` — OUR code, not vendored.** Upstream ships Postgres/SQLite
  drivers only ("mssql recognized but not implemented"), and this project's reports are all
  SQL Server: every table is a stored procedure (`usp_…;1`, OLE DB (ADO), one proc per
  report scope). The crate implements the pipeline's driver-free seam
  (`rpt_data::RowSource` / `ScopeData`) over tiberius — the pure-Rust TDS driver; no OLE DB
  provider, no ODBC install, no sqlcmd. NOTHING in the vendored crates changed for this.
- **Connection strings: your app's ADO/OLE DB keyword form**
  (`Server=;Database=;User Id=;Password=;TrustServerCertificate=True`), same key aliases as
  the app's own parser (`Data Source`/`Initial Catalog`/`UID`/`PWD`, `,port` server suffix).
  The string is parsed once, carried in a `Debug`-hidden struct, never logged, and never
  echoed in an error — a missing key errors naming ONLY the key; connection errors carry the
  `server/database` label.
- **How a scope fetches:** the table's `;1` marker is stripped and the proc executes as an
  RPC with typed, bound parameters (`EXEC` + `@P1` placeholders — no SQL text is ever built
  from user input); SQL-command tables run verbatim; plain joined tables are refused for v1
  (none of this workspace's reports use them). The report's pre-declared `data_fields`
  (name + type — the format stores them, the proc's result set doesn't) define the output
  columns; result columns match by name (case-insensitive), unmatched ones render blank with
  a diagnostic note. Every cell arrives as text (or bytes for Blob fields like
  `HIE_LOGO_IMAGE`) and is re-typed by the pipeline — the same everything-is-text design as
  the upstream drivers.
- **Parameters:** report parameter fields carry the proc's `@` names 1:1 (measured on the
  PMSV10 fixtures: `@performance_cycle_id` etc.). The caller supplies `name=value` text,
  coerced against the report's declared kinds (number/boolean/date/time/datetime rules as
  upstream's CLI); unsupplied proc parameters bind NULL. Multi-value string arrays join
  with `,` (the `@emp_number_list` convention).
- **One connection per render**: main scope and every subreport scope share a single TDS
  login; subreport scopes are fetched once and memoized (identical scopes replay cached
  rows); a subreport fetch failure degrades to that scope's saved data with a diagnostic —
  same semantics as the saved-data path. Per-row subreport link filtering still happens in
  memory after the fetch (the upstream drivers' semantics; per-row re-execution would need a
  layout-engine change and is a documented follow-up).
- **Verification state:** hermetic tests cover the parser (aliases, comma port, missing-key
  messages, Debug/label redaction), the fetch plan over the real PMSV10 fixture (proc name,
  `@RETURN_VALUE` excluded, bind resolution, NULL binds), parameter coercion (each kind,
  bad-value refusals), and the end-to-end API-surface leak rule (a failed connect names the
  target, never the password). The wire itself is gated behind `RPT_MSSQL_CONN`:
  `rpt-db-mssql/tests/live.rs` (parse → connect/TLS/login → `SELECT @@VERSION` → optional
  raw SQL). NOTE: this workstation is not on the corporate network — `sgdev01db02.cloud`
  fails DNS and `10.56.3.16` times out — so the final end-to-end against the real server
  must be run from the app's environment (one command; see validation-log §22).

## Follow-up batch: formulas, group-order mint, Pm- creation, running totals, connection reuse (2026-10-16)

Six follow-ups in one pass, each on the plan vocabulary + native ops (`rpt-tools apply`/`edit`).

- **`setFormula {target, source}` + `setRecordSelection {source}` — the "tokenized bytecode"
  boundary was WRONG.** Sweeping every `0x0076` formula record across the 128-file corpus and
  all fixtures (610+ records, main and nested streams): the format stores formulas as `0x71`
  name + reference list + source text + kind words — **no token bytecode anywhere**; the
  engine recompiles from source at load (the vendored model's own docs say so). Editing is
  the proven rebuild path: the `0x71` child and the tail after `text` are re-emitted from
  their decoded field values (copied stored bytes carry position masks a fresh build cannot
  reuse), refs + text rebuilt, one splice. **References are DERIVED from the new source**
  (`{alias.field}`→pool 0, `{?name}`→param pool 5 written `?name`, `{@name}`→formula pool 1,
  `Group #N`→pool 4, special display names→pool 3 — the vocabulary pinned empirically from
  donor formulas); `{#…}` and `{%…}` refuse naming the reference (no wire-kind donor
  evidence; fails closed). Known staleness, documented: the `0x71`'s stored result
  type/length are engine-recomputed facts.
- **`addGroup` parity: `Group #N Order` minting + a real numbering fix.** The clone used to
  carry the donor's `Group #(N-1) Name`/`@Group #… Order` fields and order-formula handle —
  a report gaining its second group carried two clashing "#1" identities. Now the clone's
  four identity fields are re-numbered and the new group's `Group #N Order` wrapper is
  MINTED (rebuilt fresh from the innermost donor's decoded fields, body
  `if IsNull ({field})\r\nthen 2\r\nelse 2`), inserted after the LAST top-level formula
  wrapper — its pool ordinal (the wrapper count, the index space `order_formula_index` and
  kind-1 refs address, verified against two fixtures' stored indexes) shifts nothing.
  Pre-existing defect documented (NOT this batch's): the area-clone step corrupts the stream
  on fixtures whose innermost group header carries charts or subreport placeholders
  (World Sales Report, IndPerfOverview — the validator's `stream_parsed`/`group_structure`
  fire; GoalAlignCascade and the corpus files the batteries use are unaffected).
- **`setSubreportLink` now CREATES the `Pm-` parameter** (the long-standing follow-up): when
  the subreport lacks it, a `0x7a` (schema 0x0701 — the donor's word; 0x0700 selects an
  older table layout) is synthesized from the field table's required prefix — name record
  typed like the main field, continuing `parameter_index`, empty bound-field handle — and
  spliced before `FieldManagerEnd 0x70`, INSIDE the same subdocument edit that writes the
  binding (chained container writes were observed to drop nested-stream edits; one write,
  no drop). The binding's kind-5 parameter ordinal now reads the subdocument's raw `0x7a`
  records (the engine's pool) rather than the decoded model — a synthesized parameter is
  model-visible only through the GUID-less orphan path until a PromptManager entry joins to
  it. The no-`Pm-` degradation path is gone; failures propagate.
- **`addRunningTotal {newName, fieldRef, operation, evaluateField?, resetField?}`** —
  synthesizes the `0x0080[0x007e[0x0071]]` run from the pinned donor layout (every field +
  value quoted from `China Orders, with running totals.rpt`), spliced among the field
  definitions, verified by model read-back. Field-based evaluate/reset conditions in v1
  (kind 0/1; formula conditions ride setFormula later). The result always reports Number
  (the engine widens Currency). Placement rides `addField` with a **`{#name}` reference →
  pool 9** (`FieldRefKind::RunningTotal`), the stored ref text `#name` — the engine donor's
  exact form.
- **Connection reuse across live previews**: `rpt_db_mssql::connect_cached` (process-global,
  keyed by a hash of the full config — the password never stored as text) behind
  `Arc<Mutex<TdsClient>>` (tiberius `Client` is auto-Send+Sync); a stale-connection
  `Connect`-class failure evicts + reconnects ONCE; statement errors keep the connection.
  One login per server per process, not per preview.
- **Wave-2 validation (the file-checkable subset)**: summary-operation range check shipped
  (a summary/running-total decoding as `Other` = an out-of-enum operation word → Error).
  The parameter value/description parity idea was INVESTIGATED AND REJECTED: engine-authored
  fixtures themselves carry mismatched counts (1 value / 2 rows; 0/1) — the block is not the
  invariant the engine condition implies. Formula-slot exclusivity stays open: the ~80-member
  slot family needs per-slot static-default semantics before a check can avoid false
  positives.
- Tests: `rpt-core/tests/followups.rs` — 8 tests (exact-diff formula rewrites, refusal
  cases, the mint's read-back, Pm- creation end-to-end, RT mint + `{#}` placement + refusal,
  fixture cleanliness under the new check).


## Empty-data preview semantics (2026-10-22)

A report with no data to render (no saved data in the file, or a live fetch returning zero
rows) now previews as: static bands only, NO group header/footer bands (engine parity —
Crystal prints no group band over zero rows; the previous skeleton emission produced the
13 "unknown name" errors TCM saw), and a single first-class diagnostic up front —
"this report has no data to render … only its static bands are shown; render with the live
preview to draw fresh rows" — with the empty-row unknown-name formula errors filtered out.
The UI can key its empty-state screen on that notice's prefix. (Layout change documented as
VENDORED.md #11; the WSR golden pin was re-based onto a fixed render clock the same day —
see validation-log §24.)


## HTML text interpretation (2026-10-22)

Fields whose string format is `HTMLText` (stored `0x00fa` byte 15) now render their value
as markup — the PMSV10 `pad_*` formulas' deliberate HTML (wrappers, `<br>`, entities, the
small-font spacer span) renders as styled text instead of printing the tags literally
(VENDORED.md #12; see validation-log §25 for the subset and the line-font rule).
