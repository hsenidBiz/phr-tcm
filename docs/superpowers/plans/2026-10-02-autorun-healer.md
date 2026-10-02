# Auto Run healer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `/tcm:heal` command that walks an assistant through failing Auto Run cases, plus a suspected-defect mark for "the script is right, the application is wrong".

**Architecture:** The mark is an optional field on `CaseScript`, changed only through one store function (`store::set_suspected_defect`). Every other save keeps what is on disk. A new bridge route and MCP tool set it, after checks against the case's newest run. A new IPC command clears it. Unattended proposals and both kinds of recorded run read it and clear it. The command is a `COMMANDS` entry whose tool is an Auto Run tool, so it is gated for free. The webview shows a badge with a Clear button on the case row.

**Tech Stack:** Rust (Tauri 2, serde, specta), React 19 + TypeScript, vitest.

**Spec:** docs/superpowers/specs/2026-10-02-autorun-healer-design.md

## Global Constraints

- Auto Run stays gated exactly as today. Nothing is named in the changelog, help site or README, and nothing mentions a secret unlock.
- A mark never changes a script's `steps`, `repairs` or `last_repair`, never runs anything, and reaches Azure DevOps only through a reason a person sends.
- Note limits: not blank, at most 300 characters. It is stored after `api_checks::shown_body`'s address and token scrub.
- No HTTP DELETE anywhere. Clearing a mark is a local file write.
- Rust tests go only under `src-tauri/tests/suite/` (one binary, a `mod` line in `suite/main.rs`). Never hand-edit `src/bindings.ts`: regenerate it with `cd src-tauri && cargo test --test bindings`.
- Colours come from tokens and icons from `src/lib/actionIcons.ts`. `src/ui-consistency.test.ts` and `src/a11y.test.tsx` pass unchanged.
- User-facing text: plain sentences, no em dashes.
- Run one test command at a time. Commit with a Bash heredoc ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. A save from the editor, an import or an assistant repair of ANOTHER step must not drop a mark. Only a repair that changes the marked step drops it (Task 1).
2. A webview copy that is stale (the mark was cleared by a run while the editor was open) must not bring the mark back. Saves keep the disk's mark and ignore the incoming one (Task 1).
3. A mark on a case whose newest run stopped for sign-in, the browser, or a person's Blocked must be refused (Task 1).
4. A run in which the case fails before reaching the marked step keeps the mark. Only a recorded pass of the marked step clears it (Task 2).
5. The note can carry an address, a query or a token pasted from a page. They must be gone before it is stored (Task 1).

---

### Task 1: The mark, its tool and its keeping rules

**Files:**
- Modify: `src-tauri/src/autorun/mod.rs` (`SuspectedDefect`, field on `CaseScript`)
- Create: `src-tauri/src/autorun/defects.rs` (pure checks + note scrub)
- Modify: `src-tauri/src/autorun/store.rs` (`set_suspected_defect`; saves keep the disk's mark)
- Modify: `src-tauri/src/ai_bridge.rs` (route `POST /autorun-defect`; drop the mark when a repair changes the marked step)
- Modify: `src-tauri/src/mcp.rs` (tool `mark_autorun_suspected_defect`), `src-tauri/src/ai_tools.rs` (`DEV_ONLY_TOOLS`)
- Modify: `src-tauri/src/commands/autorun.rs` + `src-tauri/src/lib.rs` (`auto_run_clear_suspected_defect`, gated like the other Auto Run commands)
- Test: `src-tauri/tests/suite/autorun_defects.rs` (new module), plus updates to any test that counts tools

**Interfaces:**
- Produces:
  - `pub struct SuspectedDefect { pub step_number: i32, pub note: String, pub marked_at: String }`, with Serialize, Deserialize, specta::Type, Clone, PartialEq, Debug.
  - On `CaseScript`: `#[serde(default, skip_serializing_if = "Option::is_none")] pub suspected_defect: Option<SuspectedDefect>`.
  - `defects::check_mark(run: Option<&LocalRun>, script: Option<&CaseScript>, case_id: i32, step_number: i32, note: &str, now_ms: u64) -> Result<SuspectedDefect, String>`.
  - `store::set_suspected_defect(root: &Path, case_id: i32, mark: Option<SuspectedDefect>) -> Result<(), String>`, an atomic write of that one script.
  - IPC `auto_run_clear_suspected_defect(case_id: i32) -> Result<(), String>`.

- [ ] **Step 1: Write the failing tests** in `autorun_defects.rs`. Use the suite's existing helpers for a temp root, a script and a run (see `autorun_quirks.rs`).
  - `check_mark` refuses with these sentences:
    - no script: `case 501 has no script to mark`
    - a step not in the script: `step 9 is not in the script`
    - a step that did not fail in the newest run: the same sentence `quirks::source_from_run` gives
    - a STOP case (sign-in failed, browser stopped, verdict `Blocked`, decided by `failures::stop_reason`): `a STOP failure is not an application defect`
    - a blank note: `a suspected defect needs a note saying what the application did`
    - 301 characters: `the note is longer than 300 characters`
  - It accepts a failed application step and returns the mark with `marked_at` = now.
  - The note `saved at https://app.example/x?token=abc with Bearer eyJa.b.c` is stored with no `://`, no `token=abc` and no `eyJ`.
  - `set_suspected_defect` writes the mark and leaves `steps`, `repairs` and `last_repair` byte-for-byte unchanged. A second mark replaces the first, and `None` removes it.
  - `save_scripts_atomically` keeps the disk's mark when the incoming script has `None`, and also when it has a different mark (the disk wins). A brand new script saves with none.
  - The bridge route (call the route function the way the other autorun route tests do) answers 200 with the stored mark, and 400 with each refusal sentence. It is refused where Auto Run is not offered, as `record_autorun_quirk` is.
  - An assistant repair declaring step 2 keeps a mark on step 3. One declaring step 3 removes it.
  - `auto_run_clear_suspected_defect`'s pure half removes the mark.
- [ ] **Step 2:** Run `cd src-tauri && cargo test --test suite autorun_defects::`. Expected: FAIL (it does not compile, or the assertions fail).
- [ ] **Step 3: Implement.**
  - `check_mark` reuses `quirks::source_from_run` for the newest-run step check and `failures::stop_reason` for STOP. Run the scrub with `api_checks::shown_body` first, then measure the 300 limit on the trimmed original.
  - `store::save_scripts_atomically` copies each script's on-disk `suspected_defect` into the one being saved, under the store's existing lock.
  - In `ai_bridge.rs`, after the save, call `set_suspected_defect(None)` for each case whose declared `edits.steps` include the marked step. The save's answer line says `suspected defect at step N cleared - the step was repaired`.
  - The route follows `autorun_quirk`'s shape: body `{ case_id, step_number, note }`. It finds the newest run of the case the same way `autorun_quirk` does.
  - In `mcp.rs`, describe the tool in one sentence that names when to use it: the application, not the script, is wrong. Add `call("POST", "/autorun-defect", …)`.
  - Add the tool to `DEV_ONLY_TOOLS`.
- [ ] **Step 4:** Run `cd src-tauri && cargo test --test suite autorun_defects::`, then `cargo test --test bindings` (it regenerates the bindings; restore the file if the diff is line endings only). Expected: PASS.
- [ ] **Step 5:** Commit: `feat(v2): Auto Run cases can carry a suspected application defect`.

### Task 2: Runs read and clear the mark

**Files:**
- Modify: `src-tauri/src/autorun/replay.rs` (`propose`; after each unattended case record)
- Modify: `src-tauri/src/autorun/quirks.rs` or `commands/autorun.rs` (`count_saved_run` path, for supervised runs)
- Modify: `src-tauri/src/autorun/failures.rs` (`get_autorun_failures` names the mark)
- Test: `src-tauri/tests/suite/autorun_defects.rs`, plus any replay/proposal test module that pins reasons

**Interfaces:**
- Consumes: `SuspectedDefect`, `store::set_suspected_defect` (Task 1).
- Produces: `defects::passed(script: &CaseScript, steps: &[StepRecord]) -> bool` (the marked step is present and every one of its outcomes passed).

- [ ] **Step 1: Write the failing tests.**
  - `propose` with a mark on step 3 and a failure at step 3 gives verdict `Failed`. Its reason starts `Suspected application defect at step 3: <note>`, followed by the usual `step 3: …` sentence.
  - A failure at step 2 gives the ordinary reason.
  - An unattended run whose record passes step 3 clears the mark on disk, and its case `reason` ends with `The suspected defect at step 3 did not happen this time - the mark was cleared.`
  - A run that stops before step 3 keeps the mark.
  - `count_saved_run` on a supervised run that passed step 3 clears the mark. It leaves the run file unchanged (a person's run carries no machine reason).
  - `get_autorun_failures` text for a marked case contains `suspected defect at step 3: <note>`.
  - `publish::comment_for` on such a case carries the defect reason.
- [ ] **Step 2:** Run `cd src-tauri && cargo test --test suite autorun_defects::`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - `propose` already receives the script, so prefix the Failed reason when the failing step is the marked one.
  - In the replay loop, before `run.cases.push(record)`, if the case's script has a mark and `defects::passed`, clear the mark and append the sentence to `record.reason`.
  - Next to `record_run_evidence` in `count_saved_run`, clear the marks the saved run passed. Never fail a run because a mark cannot be written: log it with `applog::warn`.
- [ ] **Step 4:** Run `cd src-tauri && cargo test --test suite autorun_defects::`, then the replay and failures modules. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(v2): Auto Run runs label and clear suspected defects`.

### Task 3: `/tcm:heal` and the guide

**Files:**
- Modify: `src-tauri/src/ai_tools.rs` (`COMMANDS`)
- Modify: `src-tauri/src/autorun/guide.rs` (a subsection in "Repairing a script that failed")
- Test: `src-tauri/tests/suite/ai_tools.rs` (or the module that tests `COMMANDS`), `src-tauri/tests/suite/autorun_guide.rs`

**Interfaces:**
- Consumes: the tool name `mark_autorun_suspected_defect` (Task 1).

- [ ] **Step 1: Write the failing tests.**
  - `COMMANDS` has stem `heal`, tool `get_autorun_failures`, desc `Diagnose and repair failing Auto Run cases, one at a time`, and hint `[case ids, or blank for every failed case in the newest run]`.
  - `command_files_in(dir, &effective_disabled_for(&[], false))` has no `heal.md`. With `true`, it has it.
  - The body names each of `get_autorun_guide`, `get_autorun_failures`, `get_autorun_page`, `probe_autorun_locator`, `try_autorun_action`, `save_autorun_script` and `mark_autorun_suspected_defect`. Each is in the bridge's tool list. The body has no em dash.
  - The guide has `### When the application is wrong`, which names `mark_autorun_suspected_defect` and says the script is not changed.
- [ ] **Step 2:** Run `cd src-tauri && cargo test --test suite ai_tools::` and `autorun_guide::`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - The body is the spec's §3 routine, steps 1-8, written as the other command bodies are (short lines, `$ARGUMENTS` in the first line).
  - The guide subsection says:
    - When to choose a mark: the page reached the right place with the right data and did something other than the case's expected result.
    - What the mark does: the script is not changed, it is not a repair, later runs label a failure at that step, and a pass clears it.
    - What it is not: a way around a check the assistant could not make pass.
- [ ] **Step 4:** Run the same two commands. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(v2): /tcm:heal walks an assistant through failing Auto Run cases`.

### Task 4: The badge and Clear on the case row

**Files:**
- Modify: `src/screens/AutoRun/index.tsx` (case rows), or a small new component beside it if the row grows
- Test: `src/screens/AutoRun/index.test.tsx` (or the existing Auto Run screen test file)

**Interfaces:**
- Consumes: `CaseScript.suspected_defect` and `commands.autoRunClearSuspectedDefect(caseId)` from the regenerated bindings (Task 1).

- [ ] **Step 1: Write the failing tests.**
  - A case whose script has a mark shows the text `Suspected defect`. The badge's accessible description contains `step 3` and the note.
  - The button `Clear suspected defect for #501` opens an inline confirm (`Keep` / `Clear`). Confirming calls `auto_run_clear_suspected_defect` with `501` and the badge goes away. Keep calls nothing.
  - A case without a mark shows no badge.
- [ ] **Step 2:** Run `npx vitest run src/screens/AutoRun`. Expected: FAIL.
- [ ] **Step 3: Implement.** Use a warning-token badge like the row's other badges, an icon from `actionIcons`, and an inline confirm like the Remove confirm on the database list. After clearing, invalidate the scripts query the row reads.
- [ ] **Step 4:** Run `npx vitest run src/screens/AutoRun`, then `npm test` and `cd src-tauri && cargo test --tests`, one at a time. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(v2): Auto Run case rows show a suspected defect and can clear it`.
