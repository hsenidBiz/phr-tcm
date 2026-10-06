# Auto Run more checks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Auto Run scripts can do six new things:
- send key combinations;
- drag to reorder;
- answer browser dialogs;
- check tables and grids;
- treat page errors as a check;
- check PDF downloads.

**Architecture:**
- Each feature is a new or extended `Action` in `browser/actions.rs`, wired through the existing chain: `validate` → `run` (or the runner's own handling) → report words → patterns → `describe_try` → guide → the TS describer.
- Dialogs and page errors build on the per-tab state the tabs work added in `cdp.rs`.
- Table checks read the page with one in-page function that returns headers and rows.
- PDF checks extend the downloads module.

**Tech Stack:** Rust (tokio, the CDP driver, a PDF text crate), React 19 + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-10-06-autorun-more-checks-design.md`

## Global Constraints

- **Test placement and state.** Rust tests go only in `src-tauri/tests/suite/`, with a `mod` line. Process-wide state takes a lock from `suite/serial.rs`, `activity_log` before `account_leases`. Source-reading tests normalise `\r\n`.
- **Bindings.** Never hand-edit `src/bindings.ts`. Regenerate it and restore any drift that is only line endings.
- **Old files.** Every new field is `serde(default, skip_serializing_if)`. Old scripts and run files round-trip byte-identical.
- **Wording.** Spec sentences are used verbatim. Plain sentences, with no em or en dashes. No hosts or query strings in sentences or records.
- **Wiring.** Every new action needs:
  - a `describeAction.ts` sentence (the exhaustive switch forces this);
  - `report::action_words`;
  - `patterns.rs`;
  - `ai_bridge::describe_try`;
  - a guide section with one example.
- **The guard.** The must-not-save guard and the sign-in hold rules are unchanged.
- **Working rules.** Run one test command at a time. Commit with a Bash heredoc ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, staging by name.

## Review Focus

1. **A dialog that opens while a step waits for something else.** It is answered at once, accepted unless an armed `expect_dialog` claims it. It never leaves the page stuck, and no call hangs behind it.
2. **A grid that is still loading,** or one that re-renders while it is read. The check retries and never reports a half-read table as a failure before the timeout.
3. **Page errors raised by the run's own requests,** such as a setup fixture, `api_request` or the sign-in. They are never counted.
4. **A drag whose browser hands over a native drag but the page never drops.** Interception is turned off, and the step fails with the timeout sentence, never a hang.
5. **A PDF that is encrypted, scanned or huge.** It gives a clean sentence, uses bounded memory, and never panics.

---

### Task 1: Key combinations and drag

**Files**
- `browser/actions.rs`:
  - `PressKey { key, times: Option<u8> }`. The `key` is parsed into modifiers plus a key.
  - `Drag { from, to, position: Option<DropAt>, within_ms }`, where `DropAt` is `Before`, `After` or `Onto`.
  - Refusal sentences come from spec §5.
- `browser/cdp.rs` / `browser/page.rs` (input):
  - Key events carry the CDP modifier bitmask: Alt=1, Ctrl=2, Meta=4, Shift=8. Modifier keydowns go in the order Ctrl, Alt, Shift, Meta, and keyups in reverse.
  - **Drag, mouse path:** press, 10 px nudge, 10 moves with an animation frame between each, release.
  - **Drag, native path:** `Input.setInterceptDrags { enabled: true }`, then `Input.dragIntercepted`, then `dispatchDragEvent` (`dragEnter` → `dragOver` → `drop`). Interception is disabled again on every path (Review Focus 4).
- Wiring as in the Global Constraints.
- Tests: `tests/suite/browser_input.rs` (new). Add a live fixture to `tests/suite/browser_live.rs`: one mouse-sorted list and one `draggable` list, each reordered by `drag` and by `Ctrl+ArrowUp`.

- [ ] **Step 1:** Write the failing tests:
  - parsing and refusals;
  - the bitmask on every key event;
  - `times`;
  - the mouse-path event order and positions for before, after and onto;
  - the native path, including interception off after a failure;
  - each failure sentence;
  - a drag in a second tab and in a frame;
  - an old single-key script round-trips.
- [ ] **Step 2:** Implement, then run:
  1. the focused tests;
  2. `cargo test --tests`;
  3. `npx tsc --noEmit`;
  4. `npm test`;
  5. the new live test.
- [ ] **Step 3:** Commit `feat(v2): Auto Run scripts can press key combinations and drag to reorder`.

### Task 2: Browser dialogs

**Files**
- **`cdp.rs`.** Per-tab dialog handling stops auto-accepting blindly:
  - an armed expectation, shared by the run across tabs, claims the next dialog;
  - otherwise the dialog is accepted, as today, and recorded as unexpected.
- **`actions.rs` / runner.**
  - `ExpectDialog { text, contains, answer, prompt_text, within_ms }`.
  - The runner arms every `ExpectDialog` in a step when the step starts.
  - A text mismatch still answers the dialog as asked, then fails the step.
  - The step record gains `dialog: Option<{ kind, message }>`, with the message cut to 200 characters.
- **`autorun/mod.rs`.** `CaseScript.fail_on_unexpected_dialog: bool`, defaulting to false and skipped when false.
- Wiring, and the sentences from spec §1.
- Tests: `tests/suite/autorun_dialogs.rs` (new). Add a live `confirm` to the fixture.

- [ ] **Step 1:** Write the failing tests:
  - each kind (alert, confirm, prompt, beforeunload);
  - accept, dismiss and `prompt_text`;
  - the refusals;
  - a dialog opened by a click in the same step;
  - a dialog in a second tab;
  - each failure sentence;
  - unexpected dialogs with and without the option;
  - Review Focus 1.
- [ ] **Step 2:** Implement, then run:
  1. the focused tests;
  2. `cargo test --tests`;
  3. `npx tsc --noEmit`;
  4. `npm test`;
  5. the live dialog test.
- [ ] **Step 3:** Commit `feat(v2): Auto Run scripts can check and answer browser dialogs`.

### Task 3: Table and grid checks

**Files**
- **`browser/table.rs` (new).** One in-page reader, `read_table(locator) -> TableRead { headers: Vec<String>, rows: Vec<Vec<String>> }`:
  - it handles HTML tables and ARIA grids as described in spec §2;
  - cell text is visible text, trimmed, with spaces collapsed.

  It also holds the pure helpers:
  - `find_row`;
  - `check_sorted(values, order, as)`, including the `number` and `date` readers and the ambiguity rule;
  - `count_check`.
- **`actions.rs`.**
  - New actions: `ExpectRow`, `ExpectNoRow`, `ExpectSorted` and `ExpectRowCount`.
  - They retry within the step's check timeout, like `expect_text` (Review Focus 2).
  - Sentences come from spec §2.
- Wiring.
- Tests: `tests/suite/autorun_tables.rs` (new). It holds pure tests for the helpers and fake-page tests for retries. Add an HTML table and an ARIA grid to the live fixture.

- [ ] **Step 1:** Write the failing tests:
  - HTML and ARIA reading;
  - header lookup and an unknown column, with its list;
  - contains versus exact;
  - text, number and date sorting, plus the ambiguous-date sentence and an explicit format;
  - blank cells;
  - each row-count form and its refusal when more than one is given;
  - a retry while loading.
- [ ] **Step 2:** Implement, then run:
  1. the focused tests;
  2. `cargo test --tests`;
  3. `npx tsc --noEmit`;
  4. `npm test`;
  5. the live table test.
- [ ] **Step 3:** Commit `feat(v2): Auto Run scripts can check rows, order and row counts in tables and grids`.

### Task 4: Page errors as a check

**Files**
- **`autorun/mod.rs`.**
  - `CaseScript.page_errors: Option<"fail" | "flag">`.
  - `CaseScript.ignore_page_errors: Vec<String>`, holding up to 10 entries.
  - `CaseRecord.page_errors_seen: u32`, skipped when 0.
- **Runner.**
  - Collect, per step, the script errors (`Runtime.exceptionThrown`) and the 5xx responses from every tab's page log since the previous step.
  - Exclude the run's own requests. Tag them where they are sent: `api_request`, sign-in and fixture setup (Review Focus 3).
  - Apply the ignore phrases.
  - In `fail` mode, the sentences from spec §3 apply, including `(and <n> more)`.
  - In `flag` mode, count the errors and list them in the step's log.
- **Report.** Add the `page errors seen: <n>` badge to `report.rs`, `PastRuns.tsx` and `RunReview.tsx`, using tokens.
- Wiring.
- Tests:
  - `tests/suite/autorun_page_errors.rs` (new);
  - vitest for the badge;
  - a live page that throws.

- [ ] **Step 1:** Write the failing tests:
  - a script error and a 5xx, each in fail mode and in flag mode;
  - the run's own requests excluded;
  - the ignore phrases;
  - errors between steps counted against the next step;
  - several errors in one step;
  - the flag recorded and round-tripped;
  - the default (off) unchanged;
  - the badge.
- [ ] **Step 2:** Implement, then run:
  1. the focused tests;
  2. `cargo test --tests`;
  3. `npx tsc --noEmit`;
  4. the focused vitest;
  5. ui-consistency and a11y;
  6. `npm test`;
  7. the live page-error test.
- [ ] **Step 3:** Commit `feat(v2): an Auto Run script can fail or flag on page errors`.

### Task 5: PDF downloads

**Files**
- **`Cargo.toml`.** Add a PDF text crate that can return text per page. `pdf-extract` with its per-page API is preferred. If it cannot do per-page, use `lopdf` page by page. Report which one you chose, its licence, and that it builds offline from the existing lock where possible.
- **`autorun/downloads.rs`.**
  - `ExpectDownload.pdf: Option<PdfCheck { contains, pages, on_page }>`.
  - Read the PDF from disk, refusing files over 50 MB.
  - Extract text per page and normalise it: ignore case and collapse whitespace.
  - Use the sentences from spec §4.
  - The save-time refusal `pdf checks need a name ending in .pdf`.
- **Readable script.** In `describeAction.ts`, extend the `expect_download` sentence with the PDF checks.
- **Guide.**
- **Tests.** `tests/suite/autorun_downloads.rs`, with small PDF fixtures under `tests/fixtures/`: a two-page text PDF, an encrypted PDF and an image-only PDF. Add a live PDF download to the fixture.

- [ ] **Step 1:** Write the failing tests:
  - contains;
  - each page-count form;
  - `on_page` with `1` and `-1`;
  - no such page;
  - an encrypted and an image-only file give the "could not be read" sentence;
  - the 50 MB limit (use a size check, not a real 50 MB file);
  - a PDF block on a non-PDF name is refused;
  - Review Focus 5.
- [ ] **Step 2:** Implement, then run:
  1. the focused tests;
  2. `cargo test --tests`;
  3. `npx tsc --noEmit`;
  4. `npm test`;
  5. the live PDF test.
- [ ] **Step 3:** Commit `feat(v2): Auto Run download checks can read PDFs`.
