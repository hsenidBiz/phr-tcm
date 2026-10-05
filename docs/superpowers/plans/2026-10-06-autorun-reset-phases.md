# Auto Run reset phases Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Scripts can name the shared state they change or need unchanged. Auto Run works out an order and its reset points, keeps its own order per PBI, and pauses a run where the environment must be reset.

**Architecture:**
- **The planner:** a pure Rust module (`autorun/plan.rs`) turns case ids, their scripts' marks and an optional saved order into an order and phases. Rust owns it: the webview asks for the plan to show it, and the unattended run recomputes it from the same inputs at start, so the two always agree.
- **The saved order:** stored in the Auto Run store. The assistant sets it with a new MCP tool.
- **Pausing:** the unattended run pauses at a boundary on an event plus an answer command, the same pattern as part 4's replay ask. The supervised pane pauses in the webview.

**Tech Stack:** Rust (serde, tokio, the AI bridge and MCP), React 19 + TypeScript, vitest.

**Spec:** docs/superpowers/specs/2026-10-05-autorun-reset-phases-design.md

## Global Constraints

- Rust tests only in `src-tauri/tests/suite/`, with a `mod` line in `suite/main.rs`. Process-wide state takes a lock from `suite/serial.rs`.
- Never hand-edit `src/bindings.ts`. Regenerate it with `cd src-tauri && cargo test --test bindings`, and restore line-ending-only drift.
- Colours from tokens, icons from `src/lib/actionIcons.ts`. `ui-consistency` and `a11y` stay as they are.
- Plain sentences, no em dashes. Every sentence in the spec is verbatim.
- Old scripts and old run files load unchanged: every new field is `serde(default, skip_serializing_if)`.
- One test command at a time. Commit with a Bash heredoc ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Stage by name.
- Source-reading tests normalise `\r\n` to `\n` (this checkout has CRLF files).

## Review Focus

1. **Names that differ only in case or spacing** (`Cycle Published` against ` cycle published `) are one name everywhere: in the plan, in the editor's chips and in the reset panel.
2. **A saved order that names cases no longer in the PBI, or misses new ones.** Stale ids are dropped. Selected cases missing from the order go at the end in list order. Nothing crashes.
3. **A run of only marked cases, all needing the same name, with no changer.** One phase and no reset.
4. **The app closed while paused at a reset.** The run is recorded as stopped at the reset point, with the rest `not run: the run stopped at a reset point`. It is never left "running".
5. **A selection with marks but where the order already satisfies them.** No reset is shown and no extra UI appears, as with no marks.

---

### Task 1: The marks on a script

**Files:**
- `src-tauri/src/autorun/mod.rs`: `CaseScript.changes: Vec<String>` and `CaseScript.needs_unchanged: Vec<String>`, each with `serde(default, skip_serializing_if = "Vec::is_empty")`.
- A new `src-tauri/src/autorun/marks.rs`:
  - `pub fn normalise(name: &str) -> String`: trimmed, inner whitespace collapsed to one space, lowercased. It is the comparison key.
  - `pub fn check_marks(changes: &[String], needs: &[String]) -> Result<(), Vec<String>>`, giving the spec §1 refusal sentences verbatim.
- Save-time validation: the same place preconditions are validated (`nav::check_project_rules` or the shared save path), so the editor, the assistant's save and import all refuse.
- `src-tauri/src/autorun/guide.rs`: the spec §1 guide text.
- `src/screens/AutoRun/ScriptEditor.tsx`: "Changes" and "Needs unchanged" rows under the steps.
  - Chips, each with a remove button whose accessible name is `Remove change <name>` or `Remove needs unchanged <name>`.
  - A text box to add a name, with Enter or an Add button.
  - Saved with the script.
- Tests:
  - `tests/suite/autorun_marks.rs` (new);
  - the existing script round-trip test, now proving an old file loads unchanged;
  - `autorun_guide.rs`;
  - `ScriptEditor.test.tsx`.

- [ ] **Step 1:** Write failing tests.
  - `normalise` cases: Review Focus 1.
  - Every refusal sentence.
  - The same name in both lists is allowed.
  - The import, editor and assistant save paths each refuse an invalid mark.
  - An old script file round-trips byte-identical.
  - The guide contains the rule and the publish example.
  - The editor rows add, remove and save.
- [ ] **Step 2:** Implement and regenerate the bindings.
- [ ] **Step 3:** Run the focused tests, `cargo test --tests`, `npx tsc --noEmit` and `npm test`, one at a time. Commit `feat(v2): Auto Run scripts can name the shared state they change or need unchanged`.

### Task 2: The planner, the saved order and the assistant's tool

**Files:**
- A new `src-tauri/src/autorun/plan.rs`:
  - `pub struct Marks { pub changes: Vec<String>, pub needs: Vec<String> }`, with normalised keys plus display spellings.
  - `pub struct Reset { pub before_case_id: i32, pub names: Vec<String>, pub changed_by: Vec<(String, Vec<i32>)> }`. The names are display spellings, and `changed_by` maps each name to its changer ids.
  - `pub struct Plan { pub order: Vec<i32>, pub phases: Vec<Vec<i32>>, pub resets: Vec<Reset> }`.
  - `pub fn suggest(cases_in_list_order: &[(i32, Marks)]) -> Vec<i32>`: spec §2.1, with the cycle broken as late as possible in list order.
  - `pub fn phases(order: &[i32], marks: &HashMap<i32, Marks>) -> Plan`: spec §2.2.
  - `pub fn plan_for(selected_in_list_order: &[(i32, Marks)], saved: Option<&[i32]>) -> (Plan, Option<(usize, usize)>)`. It uses the saved order (stale ids dropped, missing ids appended in list order, Review Focus 2), else the suggestion. The `Option` is `(saved resets, suggested resets)` when the saved order needs more.
- `src-tauri/src/autorun/store.rs`: `load_order(root, pbi_id) -> Option<Vec<i32>>`, `save_order(root, pbi_id, &[i32])` and `clear_order(root, pbi_id)`, at `orders/<pbi>.json`.
- `src-tauri/src/commands/autorun.rs`:
  - `auto_run_plan(organization, project, pbi_id, case_ids_in_list_order) -> PlanView`. `PlanView` holds the order, phases, resets with case titles if known (titles may come from the caller) and the reset counts.
  - `auto_run_save_order(pbi_id, case_ids)`.
  - `auto_run_clear_order(pbi_id)`.
- AI bridge `POST /autorun-order` and the MCP tool `set_autorun_order { pbi_id, case_ids }`:
  - it is in the Auto Run tool set (Rust `ai_tools.rs` and its TypeScript mirror; a test keeps them in sync);
  - it refuses `case <id> is not in PBI <pbi>` (the bridge knows the PBI's cases from the same source `get_autorun_failures` or the case list uses);
  - it warns for cases with no script.
- Tests: `tests/suite/autorun_plan.rs` (new), the store tests, the bridge and MCP tests.

**Interfaces:**
- Consumes: `marks::normalise` from Task 1.
- Produces: `plan_for`, `Plan`, `Reset`, `PlanView`, the commands and `load_order`, all used by Tasks 3 and 4.

- [ ] **Step 1:** Write failing planner tests for every spec §5 case:
  - no marks;
  - one name with needers before and after;
  - needs and changes the same name;
  - two names;
  - a cycle;
  - Review Focus 2, 3 and 5;
  - a saved order that needs extra resets, with the counts.

  Also write the store round-trip, the tool's refusals and warnings, and the tool's gating.
- [ ] **Step 2:** Implement and regenerate the bindings.
- [ ] **Step 3:** Run the focused tests, `cargo test --tests`, `npx tsc --noEmit` and `npm test`, one at a time. Commit `feat(v2): Auto Run works out an order and its reset points, and keeps its own order per PBI`.

### Task 3: The Execution order dialog and the plan before a run

**Files:**
- A new `src/screens/AutoRun/ExecutionOrderDialog.tsx`, opened from the Test cases tab's More menu as `Execution order`.
  - It lists the selected cases (or all scripted cases when none are selected) in the planned order.
  - Up/down buttons, plus drag if the app's existing ordering dialog (Run Tests' Set execution order) has a drag pattern to reuse.
  - Reset points show between rows as `Reset: revert "<name>"`.
  - The spec's count sentence, when the saved order needs more resets.
  - Buttons: Save order, Use suggested order and Cancel.
- `src/screens/AutoRun/index.tsx`: Run selected and Run unattended use the planned order (`auto_run_plan`) instead of list order.
- `src/screens/AutoRun/ReplayPane.tsx`: the run dialog's plan, per spec §2.4. It shows phases and reset lines only when there is more than one phase.
- Tests: vitest for the dialog, the order used by both run buttons, and the run dialog's plan.

**Interfaces:**
- Consumes: `auto_run_plan`, `auto_run_save_order` and `auto_run_clear_order` from Task 2.

- [ ] **Step 1:** Write failing vitest tests:
  - the dialog renders the order and reset lines;
  - up/down reorders;
  - Save calls `save_order`;
  - Use suggested calls `clear_order`;
  - the count sentence shows;
  - both run buttons pass the planned order;
  - the run dialog shows phases only when there are more than one (Review Focus 5).
- [ ] **Step 2:** Implement.
- [ ] **Step 3:** Run `npx tsc --noEmit`, the focused files, ui-consistency and a11y, then `npm test`, one at a time. Commit `feat(v2): Auto Run shows its execution order and reset points, and you can change the order`.

### Task 4: Pausing at a reset point

**Files:**
- `src-tauri/src/autorun/mod.rs`: `LocalRun.resets: Vec<ResetRecord>` with `ResetRecord { before_case_id, names, changed_by, waited_ms, outcome }`, where `outcome` is `"continued"` or `"stopped"`. Serde default, skipped when empty.
- `src-tauri/src/commands/autorun_replay.rs` and `autorun/replay.rs`:
  - the unattended run recomputes the plan at start (`plan_for` with the PBI's saved order and the scripts on disk);
  - at each boundary, after the previous case's browser closed, it emits `autorun-reset-needed { run_id, before_case_id, names, changed_by, remaining }` and waits.
- A new `src-tauri/src/autorun/reset_wait.rs`: one waiting reset per run, a `oneshot`, and no timeout. The answer command is `auto_run_answer_reset(run_id, continue_run: bool)`.
  - **Stop:** records the remaining cases with `not run: the run stopped at a reset point`, and the run ends.
  - **The run's Stop button while paused:** counts as Stop.
  - **App exit while paused:** answers Stop through the exit hook, so the run is saved as stopped (Review Focus 4).
- `src-tauri/src/autorun/report.rs`: a `Reset: revert "<name>" - continued|stopped` line between the cases.
- `src/screens/AutoRun/ReplayPane.tsx`: the `Reset needed` panel (spec §3) with Continue and Stop, listening for the event.
- `src/screens/AutoRun/PastRuns.tsx` and `RunReview.tsx`: the reset line between cases.
- `src/screens/AutoRun/RunPane.tsx` (supervised): between cases, when the plan has a boundary before the next case, the same panel shows before that case's browser is used. It is webview-only and needs no Rust wait.
- Tests:
  - Rust, with fake browsers:
    - a pause at a boundary;
    - Continue runs the next phase;
    - Stop records the rest;
    - the run's Stop button while paused;
    - app exit while paused;
    - the `resets` record;
    - old run files load;
    - the report line.
  - vitest: both panels, Past runs and the review.

**Interfaces:**
- Consumes: `plan_for` and `load_order` from Task 2, the planned order from Task 3, and the existing replay events.

- [ ] **Step 1:** Write the failing tests named above.
- [ ] **Step 2:** Implement and regenerate the bindings.
- [ ] **Step 3:** Run the focused tests, `cargo test --tests`, `npx tsc --noEmit` and `npm test`, one at a time. Commit `feat(v2): an Auto Run run pauses where the environment must be reset, and carries on when you say`.
