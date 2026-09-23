# Run order: spec order, a suggested run order, and each tester's own

Design, agreed with the owner on 2026-09-23.

## 1. The problem

Run Tests lists a PBI's test cases in the order Azure DevOps returns its test
points, which follows the order the cases were created: upload order. A tester
has no say in the order they execute in.

What the code showed before designing:

- A suite in Azure DevOps has exactly one order of its cases, stored as a
  position (`sequenceNumber`) on each case's suite entry. Suite Management
  already edits it (`reorder_suite_cases`, a PATCH to `testplan/suiteentry`)
  and reads it back sorted (`get_suite_entries`).
- Run Tests never reads that order. It uses `list_test_points`, which applies
  no sort, so reordering in Suite Management today has no effect on Run Tests.
- The optimizer already stamps two app-only orders on every case in the draft
  file: `spec_order` (the order the cases were written against the spec) and
  `tester_order` (grouped so cases sharing a setup run together). Neither
  reaches Azure DevOps and Run Tests sees neither.
- The draft file cannot be the source of a run order: the link from a PBI to
  its file is in one machine's local settings, so another tester, another
  machine or a moved file would silently fall back to upload order.

## 2. Terms

- **Spec order**: the order of the PBI suite's cases as Azure DevOps stores
  and shows it (`sequenceNumber`). One per suite, shared by everyone who opens
  the suite, including Azure DevOps' own pages and web runner.
- **Suggested run order**: the default execution order every tester starts
  from, for one PBI. Stored as a small file attached to the PBI.
- **My order**: one tester's own execution order for one suite, stored on
  their machine only.

## 3. Owner decisions

1. Spec order is what Azure DevOps shows, and it stays stable. It changes only
   in Suite Management (and once, by the app, when an upload creates cases).
   Nothing in Run Tests or the runner window changes it.
2. Execution order is not stored in the suite order: that order is global,
   last-write-wins and single, so testers reordering it would shuffle each
   other's runs and destroy the spec-order view. A second suite is not used
   either: results are recorded against the suite a case runs in, so a second
   suite would split results away from the PBI's suite.
3. A tester's own order lives on their machine.
4. Shared orders (spec order and suggested run order) are changed only in
   Suite Management. Run Tests and the runner only ever change My order.

## 4. The three orders

| Order | Stored in | Written by | Read by |
|---|---|---|---|
| Spec order | The PBI suite's `sequenceNumber` in Azure DevOps | Upload (when it creates cases); Suite Management | Run Tests ("Spec order"), Suite Management, Azure DevOps itself |
| Suggested run order | `tcm-run-order.json` attached to the PBI | Upload (when it creates cases and the file has `tester_order`); Suite Management | Run Tests ("Suggested run order", the default), Suite Management |
| My order | This machine's app settings, per suite | Run Tests reordering; the runner's "Run next…" | Run Tests ("My order"), the runner, Suite Management ("start from my order") |

### 4.1 Spec order at upload

After an upload that **created at least one case** into a PBI's requirement
suite, the app sets the suite order:

1. The uploaded cases (created and updated in this upload), sorted by
   `spec_order`; cases without one keep their position in the file.
2. Then every other case already in the suite, in its current relative order.

An upload that only updates existing cases leaves the suite order alone, so a
deliberate Suite Management arrangement survives routine edits. When there is
no suite (for example the Boards fallback could not create one), nothing is
ordered.

### 4.2 The suggested run order file

Attached to the PBI as `tcm-run-order.json` (relation type `AttachedFile`,
relation comment `Test Case Manager run order`):

```json
{
  "format": "tcm-run-order",
  "version": 1,
  "saved_by": "someone@example.com",
  "saved_at": "2026-09-23T10:15:00Z",
  "cases": [
    { "id": 157941, "group": "HRM\\Gamma Guardians" },
    { "id": 157953 }
  ]
}
```

- `saved_by` is the signed-in account, as the app shows it in the context bar.
- `group` is the case's area where known (the grouping the tree view uses).
  It is optional per case.
- Written at upload when the upload created at least one case AND the file's
  cases carry `tester_order`: the uploaded cases in `tester_order`, then the
  suite's other cases in spec order. With no `tester_order` in the file, no
  file is written and Run Tests uses spec order.
- Written from Suite Management when someone saves the suggested run order.
- Replacing: upload the new file, then one PATCH on the PBI that adds the new
  relation and removes the old one. The old file stays in Azure DevOps
  unreferenced: the app never deletes anything there.
- Reading: the newest by `saved_at` if more than one such relation exists. An
  unknown `format`/`version` or a file that does not parse is reported, not
  trusted.
- The one-time draft share (`ado_share`) ignores this file.

### 4.3 My order

Stored in the app's local settings, keyed by organisation, plan and suite
(`tcm-v2-run-order:<org>/<planId>/<suiteId>`), as a list of case ids. The
chosen view (Suggested / Spec / My order) is remembered per suite beside it.
Never written to Azure DevOps.

### 4.4 Reconciling

Every stored order (suggested or mine) is reconciled against the suite's
current cases before use: cases added since go at the end in spec order;
cases no longer in the suite are dropped. A stale order never blocks a run and
never hides a case. This is one pure function shared by every reader.

## 5. Screens

### 5.1 Run Tests

- An **Order** picker above the list: *Suggested run order* (default when the
  PBI has one), *Spec order*, *My order* (listed once this tester has one).
- Rows get a drag handle and Move up / Move down. Reordering while on
  Suggested or Spec copies that order into My order and switches to it, with
  the note "Now using your own order, on this machine."
- With grouping on, groups are the cases' areas when the order carries them,
  otherwise the existing title-based grouping, in the order of each group's
  first case. Group headers get Move group up / Move group down.
- In My order: *Reset to suggested order* (or to spec order when there is no
  suggested order).
- The runner opens with the selected cases in the order on screen.
- Filters hide rows, never reorder them. A case run on several configurations
  keeps its points together.

### 5.2 The runner window

- Keeps Prev / Next over the order it was handed.
- Adds a **Run next…** picker listing the cases not yet marked. Choosing one
  moves it directly after the current case and saves that into My order.
- The current case and cases already marked never move.
- Run Tests and the runner stay in step: each listens for the other's My
  order saves (the windows share the app's local settings) and re-sorts only
  the cases after the current one.

### 5.3 Suite Management

For a PBI's suite it shows two orders, each with the existing order editor:

- *Order in Azure DevOps* (spec order): unchanged behaviour.
- *Suggested run order* (new): starts from the saved suggested order, or from
  the spec order, or from **my order** when this machine has one for the
  suite. Saving asks for confirmation ("Every tester will see this as the
  suggested run order for this PBI"), then replaces the PBI's file. Shows who
  saved the current suggested order and when.

Static suites (no PBI) show only the Azure DevOps order.

## 6. Failures

None blocks a run or an upload.

- Run-order file cannot be read (network, permission, damaged file): Run Tests
  falls back to spec order, *Suggested run order* is greyed out, and one
  sentence says why and points at Settings → Logs.
- Suite order or run-order file cannot be written during upload: the upload
  still succeeds; the upload summary says which order was not saved; it can be
  set from Suite Management.
- No permission to edit the PBI when saving the suggested order: refused with
  the reason Azure DevOps gave.
- Two saves of the suggested order at the same moment: the later one wins;
  "saved by / when" makes that visible.
- Local settings unavailable: My order is not offered.
- User-facing errors name no URL (the app's transport sentences apply).

## 7. Invariants kept

- No DELETE call to Azure DevOps: replacing the file is an upload plus a PATCH.
- Results are recorded against the same suite's test points as today; order
  only decides which case comes next.
- The tour runs on sample data: the new calls get stand-ins in the tour
  backend.

## 8. Testing

- Rust (integration tests under `src-tauri/tests/`, unshared mock servers):
  the suite order is set at upload only when cases were created; cases outside
  the file stay after the file's cases; the run-order file is written with the
  relation replaced in one PATCH; the newest file wins; a damaged or unknown
  file is reported.
- Frontend unit tests: reconciling (added, removed, empty, duplicate ids).
- Screen tests: Run Tests (picker, reorder into My order, group moves, reset,
  fallback when the file is missing or unreadable); the runner (Run next…,
  marked cases stay put, sync with Run Tests); Suite Management (both editors,
  start from my order, confirm before save, saved-by line).
- Hand checks against a real PBI: after an upload the Azure DevOps suite shows
  spec order and the PBI has `tcm-run-order.json`; Run Tests opens in the
  suggested order; a tester's reorder does not change another tester's list or
  Azure DevOps; saving from Suite Management changes every tester's default.

## 9. Out of scope

- Per-tester orders stored in Azure DevOps or following a tester between
  machines.
- A shared home for spec order other than the suite (for example a second
  suite).
- Changing the order Azure DevOps' own web runner uses beyond the suite order.
