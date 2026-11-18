# The assistant can look up bugs (`get_bugs`): short design

Approved by the owner 2026-11-18 ("Sure, go ahead").

## Intent
Owner, 2026-11-18: "Can you also modify TCM so that its able to lookup bugs? Just like it can retrieve test cases?"

Today the assistant reads test cases through `get_test_cases` and `get_suite_test_cases`, but it cannot read a single bug. TCM can file bugs (Run Tests, Auto Run) but never reads them back. So an assistant cannot do things like:
- see why a case failed before;
- check what is already reported against a PBI;
- read a bug someone names.

## Decided by the owner
- **Assistant only:** one new MCP tool beside `get_test_cases`. No new screen.
- **Three ways to find bugs** (combinable, like `get_test_cases`):
  - `pbi_id`: bugs linked to the PBI, plus bugs linked to any of the PBI's test cases (that is how Run Tests files them);
  - `bug_ids`: specific bugs by id (up to 200);
  - `case_id`: bugs linked to one test case.

## Approaches
**A. Same pipeline as test cases (chosen).**
- `mcp.rs` defines the `get_bugs` tool.
- A read-only bridge route `GET /bugs?...` serves it.
- `ado/` endpoints use a WIQL query (POST, already allowed) plus a batch read of fields and relations.
- The same paging and caps as `get_test_cases` apply.
- One audited path, with the existing caches and errors.

**B. A generic "read any work item" tool.** Rejected for now: it is broader than asked, would need its own field rules, and would make the tool list harder to understand.

**C. Fold bugs into `get_test_cases`.** Rejected: it mixes two shapes, and the import JSON shape is test-case specific.

## Shape of the answer (each bug)
- **Identity:** `id`, `title`, `state`, `reason`.
- **Triage:** `severity`, `priority`, `assigned_to` (display name), `tags`.
- **Placement:** `area_path`, `iteration_path`.
- **Dates:** `created`, `changed`.
- **`repro_steps`:** HTML turned into plain text.
- **`system_info`:** plain text, when present.
- **Links:** `linked_test_cases` (ids), `linked_pbis` (ids).
- **`titles_only`:** returns only `id`, `title` and `state`, capped at 200, for duplicate checks.

## Rules
- **Paging:** `limit` defaults to 5 and is capped at 20; `offset` pages through.
- **Read only:** no DELETE, no writes.
- **Project scope:** follows the current project, like `get_test_cases`.
- **Not a bug:** an id that is not a Bug comes back under `not_a_bug`.
- **Wrong PBI:** an id outside the PBI comes back under `not_on_pbi`, as with test cases.
- **Errors:** they name no URL, using the existing `network_error` sentences.
- **Gating:** the tool follows the same module and disabled-tool gating as the other test-case tools.
- **Assistant guidance:** the guides tell the assistant when to use it:
  - before filing a bug, to check for duplicates;
  - when a case has failed before;
  - when the person names a bug.

## Left to the builder
- The exact WIQL.
- How to reuse the existing work-item batch read.
- Cache keys, in `cache/keys.rs`, with a short time-to-live.
