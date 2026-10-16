# Load-path validation — the offline engine-acceptance checker

`rpt-tools check <file.rpt>` (library: `rpt_core::validate::validate(&Rpt) -> Report`) runs the
real engine's open-file invariants against a report — the offline stand-in for the blocked
designer smoke test. Our writer and reader share the same format assumptions; this checker is
the independent second opinion that surfaces a shared blind spot before a user hits
"cannot open file".

**Exit codes:** 0 = no Error findings; 1 = at least one Error; 2 = usage/open failure.
Output lines: `[ERR]|[WARN] check: message (engine: <the engine's own assert / PE error string>)`.

**Severity policy** (documented judgment): **Error** = the PE-open rejection class — the
designer/print-engine error catalog's "Invalid …" strings; **Warn** = engine-tolerant but
suspect, or a check that is heuristic offline (display-text reference resolution — the engine
resolves pool handles, not strings). Asserts are debug-build expressions: they name the
invariants, but release tolerances may differ, so ambiguous cases downgrade to Warn.

## First-wave checks (implemented 2026-10-16)

| check | severity | what it verifies | engine anchor |
|---|---|---|---|
| `version_gate` | Err/Warn | authoring major ≤ 11 (this engine); letter 0; ≥3 | the engine's version gate ("newer version than expected") |
| `stream_parsed` | Error | Contents decodes fully (no desync/truncation) | the engine's record-framing contract |
| `special_fields` | Error | every pool-3 reference index ∈ 0..=25 | the engine's 26-entry special-field table ("Invalid special var type") |
| `areas_sections` | Error | every area has ≥1 section | the engine's area/section pairing |
| `group_structure` | Error | group-header levels dense 0..n; header/footer area counts == group count | the engine's group-level pairing |
| `object_bounds` | Error/Warn | left/top/width/height within Int16; non-negative; page overhang → Warn | the engine's bounds limits ("Invalid section height") |
| `border_enums` | Error | 4 edges ∈ 0..=4; fill_style in range | the engine's style enums ("Invalid line style") |
| `value_types` | Error | every field value type in the engine's union | the engine's value-type union ("Invalid value type") |
| `references` | Warn | data sources / embedded fields / sort+group fields resolve against the pools (display-text heuristic; summary/GroupName forms reduce to their first operand) | PE "Invalid field name/number" |
| `subreports` | Error/Warn | placeholder ↔ subreport pairing (Err); link field/parameter resolution (Warn) | the engine's link geometry ("Invalid subreport link number") |
| `table_aliases` | Error | aliases unique; table count cap | the engine's alias lookup |
| `page_setup` | Error | margins in 0..=14400; content dims non-negative | PE "Invalid page margins" |
| `saved_data` | Error | has_saved_data ⟺ decoded rows | the engine's saved-records validity flag |

**Validated clean:** all 7 fixtures PASS 0 errors / 0 warnings; the 30-op mega battery's
output passes; structurally-edited files (addGroup + setSubreportLink + addText) PASS.

## Wave 2 (engine-assert-derived, not yet implemented)

The ranked next targets:

1. **Formula-slot exclusivity** — the format-slot family (~80 engine conditions):
   each format slot must carry a static enum XOR a conditional formula, never both.
2. **Saved-data record arithmetic** — `nBytesRead == nConstantDatabaseRecordLength +
   nConstantSpecialVariablesRecordLength` (0x597680), `m_nDatabaseMemoFields ==
   databaseMemoValues.size()` (0x5fcc84).
3. **Parameter sanity** — `numParameterValues >= 1`, description-count parity, range-condition
   enum (the engine's parameter checks).
4. **Structural IsKindOf lattice** — ~1,100 tag-under-parent-tag checks; our locate grammar
   already covers the load-bearing subset, so this is defense-in-depth.
5. **Summary-operation range** — `summaryOperation` bounds (0x5a5c20) once the exact numeric
   range is pinned (needs one verification pass over the referencing function's behavior).

See also `docs/engine-notes.md` (candidate 1 now IMPLEMENTED) and
`docs/validation-log.md` §17.
