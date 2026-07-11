# Phase 2 exit notes — Test Case CRUD + import (2026-07-11)

Branch: `feat/tauri-rewrite`. Verdict: **GO** for Phase 3 planning, with the
listed deferrals carried forward.

## Delivered

- **Golden-test ports** (the v1 pytest suites translated 1:1 and green):
  - `steps_xml.rs` — build/parse of Microsoft.VSTS.TCM.Steps (ids from 2,
    placeholder step, tag-stripping round-trip semantics) + `html_to_text`.
  - `model.rs` — TestCase + every `is_valid` rule.
  - `import_parser.rs` — 9-column xlsx/csv/AI-JSON import (blocks,
    continuation rows, TestCaseID=UPDATE contract, "123.0" tolerance,
    row-numbered warnings), styled xlsx export + template via
    rust_xlsxwriter (calamine reads).
- **Writes** (client still has zero DELETE calls, guard test enforced):
  `create_test_case` ($Test Case json-patch, conditional fields),
  `update_test_case_from_model` (**blank-skip rule tested**: a blank
  imported column never wipes existing ADO data), `link_to_pbi`
  (TestedBy-Reverse).
- **submit_queue command** — serial loop ported from v1 CreationWorker:
  500 ms spacing, per-item failure isolation, creates linked to the PBI,
  updates patched in place; failed items stay in the UI queue for retry.
- **Queue UI** in Browse: manual add (one step per line, `action => expected`),
  Import file... / Save template... / Export queue... via the dialog plugin,
  per-item UPDATE badges, submit results list.
- Tests now: 43 Rust + 8 Vitest, all green.

## Gotchas added this phase

- quick-xml 0.41 surfaces `&amp;`/`&lt;` as separate `GeneralRef` events —
  they must be resolved inline or entity text silently disappears.
- The csv crate's `record.position()` points at the pre-blank-line scan
  start; v1-compatible row numbers need end-of-record position
  (`reader.position() - 1`, maxed against start).
- wiremock `path()` matches the percent-ENCODED path (`$Test%20Case`).

## Deliberate deferrals (carry to later phases)

- `ensure_requirement_suite` (board visibility via requirement-based test
  suites) — v1 runs it best-effort before creating; port in Phase 3 with the
  test-plan APIs it needs.
- Module/Preconditions field discovery (`module_ref`/`preconditions_ref`
  are plumbed through the client but the UI passes none yet — v1 discovers
  the org-specific reference names from work item type fields).
- Area/iteration pickers (create passes empty paths → project defaults).
- Streaming per-item progress events during submit (results arrive at the
  end; fine for typical queue sizes, revisit with tauri channels).
- Draft-queue persistence between launches (v1 saves the queue to
  ~/.devops_tc_creator).

## Still pending

- Live end-to-end run against real ADO (sign-in → browse → queue → create)
  — needs the user at the browser. All logic is wiremock/mockIPC-verified.
