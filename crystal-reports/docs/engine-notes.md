# Engine-notes — Crystal behaviors not yet re-implemented

A working list of engine behaviors this bundle does NOT yet re-implement, with the
observations that pin each one. The bundle itself is a from-scratch re-implementation of
what Crystal Reports does with `.rpt` files; nothing here derives from Crystal's code.

Compiled 2026-10-15, expanded the same day. The notes record observable engine behavior
(strings, limits, and table contents the engine itself exposes); the addresses below are file
observations, kept so a future verification pass can find the same behavior quickly.

Effort: S ≤ a day, M ≤ a week, L multi-week. All addresses are file offsets in
the engine's designer binary unless another is named.

---

## Tier 0 — implementable directly from known behavior — **IMPLEMENTED 2026-10-15**

All five shipped as operations on the same plan vocabulary (`addPicture`, `setZOrder`,
`setHyperlink`/`setTooltip`/`setCssClass`, `addGuideline`, `copyObject`) with tests in
`rpt-core/tests/portability.rs` — see `docs/vibey-operations.md` "Tier 0 extensions" for the
mechanisms and documented assumptions. One evidence correction from implementation: static
pictures link via `Embedding N/CONTENTS` storages (not zlibBLOB, which only caches blob-DB
fields), and the guide region's wrapper nesting is irregular across files — the shipped ops
make no framing assumptions about it.

## Tier 1 — remaining candidates, by value per effort

### 1. Load-path validation invariants — **IMPLEMENTED 2026-10-16** (`rpt-tools check`; `docs/validate.md`; 13 first-wave checks, wave-2 targets ranked). Originally rated S, highest leverage —

*(2026-10-16 postscript: the special-field wire table itself was later verified against the
engine's own special-field table — finding and fixing a one-place rotation in our earlier
reconstruction; see validation-log §20.)*

**What it closes.** A "will the real designer accept this file?" checker — the offline
stand-in for the blocked designer smoke test. Directly hardens everything shipped: the five
reconstructed special-field codes (validation-log §12), the structural clone shapes, and every
future op below.

**Evidence.** The engine's load-path checks name the invariants: the
special-var range gates `specialVarType >= RDSVPrintDate && specialVarType <= RDSVPageNofM`
and `RDSVFirst/LastSpecialVarType`, the subreport-link geometry
(`subDocument != mainDocument`, `subreportLinkN < getNPromptingVarLinkItems()`
cluster), the formula-syntax gate (`pSelectionFormula->getSyntax() == RDFCrystalSyntaxFormula`),
and the run-record guard `RDSpecialVarFieldDefinition`. A systematic pass over
The engine's own load-path checks yield the contract.
load-path contract; each was confirmed against the engine's observable behavior.

### 2. `Group #N Order` formula minting for `addGroup` — **IMPLEMENTED 2026-10-16** (`addGroup` mints the wrapper + re-numbers the clone's identity fields; see vibey-operations.md "Follow-up batch"). Originally rated S —

**What it closes.** Full addGroup parity: the engine mints a `Group #N Order` formula per
group; our clone-based addGroup carries the condition field directly but does not mint the
named formula, so references to `Group #2 Order` after our edit would miss.

**Evidence.** The name is format-assembled (no literal `Group #%d Order` in the binary).
The wire shape is fully documented upstream (`0x77→0x76` + `NamedValue 0x71` + the `0x29`
sort with `field_kind=1`); donor-clone a `Group #1 Order` wrapper from any grouped fixture,
patch name + pool index, insert beside the group definition — the same splice-and-rebind
pattern as addField. Further engine verification is only needed if the donor shortcut fails.

### 3. Running-total placement (`addRunningTotal`) — **IMPLEMENTED 2026-10-16** (0x0080 run synthesis from the pinned donor layout + `{#name}` placement through addField; field-based conditions v1). Originally rated S–M —

**What it closes.** Placing a running total with evaluate/reset conditions.

**Evidence.** The engine's running-total definition carries the eval/reset semantics
named in adjacent asserts (`actualEvalType != RDPrintTimeFieldType || incident != next`) —
exactly the condition records an op would synthesize. The model side already decodes running
totals (`RunningTotal` ×11 strings; `FieldRefKind::RunningTotal` pool 9 in our IR), and the
condition formulas reuse `0x0079`/NamedValue records we already splice.

### 4. Section insert/delete beyond group creation — **IMPLEMENTED 2026-10-16** (`addSection`/`removeSection`; see vibey-operations.md). Originally rated M —

**What it closes.** `addSection`/`removeSection` — multiple sections in one area. Vibey
measured that the SDK cannot do this ("adding a group is the ONLY way this tool can create a
section"), but the designer's UI can, so the engine implements it natively.

**Evidence.** No `InsertSection`-style strings survive (the command is UI-driven); the
targets are the section-controller code paths reached from the section context-menu commands,
plus the `0x8c` Section / band-marker writer. Cross-reference from the band-marker records
(`0x8d`–`0x99` writers) inward. Your `_shell_*.rpt` files simulate extra sections manually —
they are the natural validation fixtures.

### 5. Sub-side subreport-link binding — **FULLY IMPLEMENTED 2026-10-16** (create-or-extend of the Record Selection formula PLUS Pm- parameter creation — the last follow-up closed the same day). Originally rated M —

**What it closes.** Our `setSubreportLink` writes only the main-report half (`0x106`); the
engine also synthesizes a formula record inside the *subreport's* Contents binding the linked
parameter to its subreport field. Without it, `SubreportFieldName` reads empty.

**Evidence.** The engine writes both halves at link time (its writer-side
checks mirror candidate 1). Upstream already decodes
the read side (`subreport_link_bindings()` in `build_model/subreport.rs`) — the wire shape is
known; the work is synthesis (a `0x77/0x76` formula whose body is `{subreport.field} =
{?param}`), which overlaps the formula token vocabulary but needs only one comparison token
pair. The S-grade shortcut — donor-cloning an existing sub-side `0x76` from a linked
subreport somewhere in the 263-file corpus — should be tried first; no fixture currently has
links (companylogo has none).

### 6. Chart data-binding edits — **effort M**

**What it closes.** Retitle/rebind chart series without the designer.

**Evidence.** `RDChartObject` ×7 with the chart analyzer source path
(`\crystalreports\cpp\src\designer\views\AnalyzerControl.cpp`); upstream decodes chart
definitions (0x011c analytic header) but not the designer's edit surface.

### 7. Cross-tab grid editing — **effort M–L**

**What it closes.** Re-binding cross-tab row/column/summary fields.

**Evidence.** The richest writer cluster found: `RDCrossTab` ×30, `RDCrossTabObject` /
`RDCrossTabValueGridDefinition`, including the grid/suppress option records.
Upstream decodes the stored bindings (`attach_grid_bindings`) — the edit surface is the gap.

### 8. Alerts — **effort S, low editor value**

**Evidence.** Alert messages carry `AlertMessage`/`ConditionFormula` records
(shared with conditional formats). A small undecoded family upstream ("alerts…
recognized but not decoded"); worth extracting only if your reports use them.

### 9. Thumbnail regeneration (preview refresh) — **effort S, blocked on a renderer**

**Evidence.** The engine's thumbnail cache stores a DIB; the reader already parses it.
The write side is trivial once a page bitmap exists (the reader already parses the DIB);
rendering is the whole engine, so this waits on vendoring upstream's `rpt-render`.

### 10. Formula tokenization — **RESOLVED BY EVIDENCE 2026-10-16: there are no tokens.** Every 0x76 across the corpus is source text + refs + kind words; `setFormula` shipped as a source rewrite. The "effort L" rating rested on a wrong premise.

**Evidence.** Compilation is not in the designer (`BasicSyntax` ×6 etc. are the
syntax-selection surface only); the token tables live compiled-in in the formula/UFL natives
with no plaintext anchors. **Recommended path: vendor `rpt-formula` from upstream rpt-rs and
write the encoder against its token model**, validating round-trip on corpus formulas.
(Historical note: this candidate closed itself — the format stores no tokens; setFormula shipped 2026-10-16.)

### 11. OLAP grids and maps — **effort L, format extension**

**Evidence.** `RDOlapGrid` ×39 (the largest cluster in the binary; first with
`RDAnalysisObject`/`olapGridObject` vocabulary), `RDMapObject` ×4. These are the
two record families upstream explicitly does not decode "for want of a report that uses one" —
extracting them is new format archaeology, justified only when your editor needs to touch
such objects. Neither exists in any fixture or corpus file here.

---

## Priority order

1. **Tier 0 batch** (`addPicture`, z-order, hyperlink/tooltip ops, guides) — pure op-writing
   on proven machinery; days, no binary work.
2. **Candidate 1** (load-path validation) — best value-per-effort; hardens all shipped work
   including the reconstructed special-field codes.
3. **Candidate 4** (section insert/delete) — the last structural gap Vibey itself measured as
   impossible via the SDK.
4. **Candidate 2** (Group #N Order) and **3** (running totals) — S-grade parity completions.
5. **Candidate 5** (sub-side link) — corpus donor hunt first, synthesis second.
6. Candidates 6–8 as chart/cross-tab/alert needs arise; 9–11 deferred (renderer /
   rpt-formula vendoring / format-extension decisions).


## Post-script — create-from-scratch (2026-10-16): embedded specimen shipped, synthesis recipe parked

`rpt_core::blank::create_report()` + `rpt-tools new` ship the from-scratch capability via an
embedded engine-authored blank (`rpt-core/src/blank.rpt`) + seed donors (`seed.rpt`). Pure
synthesis (phase B) remains a documented follow-up with all three gaps identified and small:
(1) Contents stream-header synthesis (extend `encode_contents` past its retained-prefix design —
`codec/mod.rs:127-156`); (2) the 64-byte blank QESession (`qe_cfb_encrypt` + the QENG layout at
`codec/mod.rs:170-179` + the on-disk specimen `rpt-rs/tests/fixtures/reports/typography/*.rpt`);
(3) container creation (`cfb::CompoundFile::create` — proven in
`rpt-rs/crates/rpt-reader/tests/diagnose.rs:13-22`; root CLSID is all-zeros, cfb's default).
Phase B's test would diff its output against the embedded golden blank — the specimen converts
synthesis from guesswork into "match the known-good skeleton".
