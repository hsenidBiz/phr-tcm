# Auto Run Simple mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Auto Run a Simple / Advanced mode (Simple by default when Auto Run is switched on, recommended features switched on with it), reword the divergence ruling buttons, and tidy the open script card.

**Architecture:**
- One saved setting, `autorun_mode`, lives in Rust app settings. Each advanced part of the Auto Run screen reads it through one hook and hides itself in Simple, unless one shared "blocks a run" function says it is needed.
- Switching Auto Run on in `modules_set` applies the recommended features on a machine that has never used Auto Run. It reports back what it turned on, so the webview can switch on the Network tools, which live in webview storage, and show the note.
- A new per-step script field, `by_auto_run`, feeds one shared rule (Rust and TS, one fixture). That rule filters the unchecked expectations everywhere.

**Tech Stack:** Tauri 2, Rust (serde, specta), React 19 + TypeScript, vitest + Testing Library, Rust integration tests in `src-tauri/tests/suite/`.

**Spec:** `docs/superpowers/specs/2026-11-17-autorun-simple-mode-design.md` (approved by the owner, 17 November 2026). Read it alongside this plan.

## Global Constraints

- **Repository rules from `CLAUDE.md`:**
  - theme tokens only, never a hardcoded colour;
  - icons from `src/lib/actionIcons.ts`;
  - `src/bindings.ts` is regenerated with `cargo test --test bindings`, never hand-edited;
  - `tests/ui-consistency.test.ts` is never weakened;
  - frontend tests go in `tests/` mirroring `src/`; Rust tests go in `src-tauri/tests/suite/` with a `mod` line in `suite/main.rs`.
- User-facing text names no URL or file path.
- Hidden features are never named in commits, docs or the changelog.
- Do not touch `src/lib/changelog.ts`, versions or `docs/backlog.html`; the coordinator owns them.
- **No regression for Advanced:** with `autorun_mode = advanced`, the Auto Run screen is exactly today's, apart from the both-modes changes (the ruling words and the script card).
- Simple hides; it never switches a feature off. Assistants keep every tool.
- **Exact copy (from the spec):**
  - **Mode switch labels:** "Simple" and "Advanced".
  - **Blocker note:** "Shown because a run needs it. Ask the assistant to sort it out, or switch to Advanced."
  - **Modules note:** "Also turned on API Templates, the Network tools and videos of failed cases. You can switch each off." Shown only when something was turned on; name only what was.
  - **Read-only viewer footer:** "Switch to Advanced to edit."
  - **Ruling question:** "The case and the app disagree at step N. Which is right?"
  - **Ruling buttons:** "The app is wrong" (`case_stands`), "The app is right, update the case" (`rewrite`), "I fixed the data" (`data_fixed`, data divergences only).
  - **Confirm sentences:**
    - "The script keeps the case's value at step N, so the run fails there until the app is fixed."
    - "Azure DevOps opens on the case so you can update it; the case waits until its script is repaired to match."
    - "You changed the environment's data to match the case; the case runs again."
  - **Script card:** "Done by Auto Run" and "Assistant notes".
- Run only the touched test files and modules, plus `npx tsc --noEmit`, one command at a time. There is one full run of each suite, at Task 8.

## Review Focus

1. **An existing install with Auto Run already on, updating to this version** → it lands in Advanced and nothing is switched on. Test in Task 1: `an_install_with_auto_run_on_settles_to_advanced_and_gets_nothing_switched_on`.
2. **A person who turns Auto Run off and on again after switching a recommended feature off** → that feature stays off. Test in Task 1: `a_person_set_switch_survives_auto_run_off_and_on`.
3. **A Simple-mode person while an assistant's discovery holds the browser** → the Discovery row with End discovery shows. When the discovery ends, the row hides again. Test in Task 3: `discovery_row_shows_in_simple_only_while_a_discovery_holds_the_browser`.
4. **`by_auto_run` on a clean-up step, or after a step that has actions** → the save is refused, naming the step. Tests in Task 6.
5. **The assistant echoing a ruling back to the person** (the bell, the "Ruled by the assistant" line, guide text) → it uses the new words. Test in Task 5: `no_person_facing_text_uses_the_old_ruling_labels`.

---

### Task 1: Mode setting, person-set records and the recommended features (Rust)

**Files:**
- Modify: `src-tauri/src/app_settings.rs` (new fields and defaults)
- Modify: `src-tauri/src/commands/app_settings.rs` (`set_autorun_failed_video` records person-set; new `set_autorun_mode`)
- Create: `src-tauri/src/autorun/recommended.rs` (pure decisions); register it in `src-tauri/src/autorun/mod.rs`
- Modify: `src-tauri/src/commands/modules.rs` (`modules_set` applies the recommended features and returns what it did)
- Modify: the app start-up path that loads settings and modules (where `modules::initial` is applied) to settle the mode for an existing install, once
- Test: `src-tauri/tests/suite/autorun_recommended.rs` (+ a `mod` line in `suite/main.rs`)

**Interfaces:**
- Produces:
  - `#[serde(rename_all = "snake_case")] pub enum AutorunMode { Simple, Advanced }` (specta::Type), in `app_settings.rs`.
  - `AppSettings` gains:
    - `autorun_mode: Option<AutorunMode>` (`None` = never picked);
    - `autorun_failed_video_set: bool`;
    - `api_templates_set: bool`.
    
    All `#[serde(default)]`.
  - `pub fn settle_mode_on_upgrade(mode: Option<AutorunMode>, auto_run_on: bool) -> Option<AutorunMode>`, in `recommended.rs`. When `mode` is `None` and Auto Run is on, it returns `Some(Advanced)`; otherwise it returns `mode` unchanged.
  - `pub struct Recommended { pub api_templates: bool, pub video: bool, pub network_tools: bool, pub mode_simple: bool }` (serde + specta). Each field says whether that item was turned on now.
  - `pub fn recommended_on_switch(auto_run_used_before: bool, s: &AppSettings) -> Recommended`, in `recommended.rs`:
    - all fields are false when `auto_run_used_before`;
    - otherwise: `api_templates = !s.api_templates_set`, `video = !s.autorun_failed_video_set && !s.autorun_failed_video`, `network_tools = true` (the webview applies it per never-set environment), `mode_simple = s.autorun_mode.is_none()`.
  - `pub fn auto_run_used_before(data_dir: &Path) -> bool`: true when the `autorun` folder exists in the app data folder.
  - The `modules_set` return type changes from `Vec<Module>` to `pub struct ModulesAnswer { pub on: Vec<Module>, pub recommended: Option<Recommended> }`. `recommended` is `Some` only when this call turned Auto Run from off to on.
  - New command `set_autorun_mode(mode: AutorunMode) -> Result<AppSettings, String>`.
  - `set_autorun_failed_video` also sets `autorun_failed_video_set = true`.
  - `modules_set` called with `ApiTemplates` sets `api_templates_set = true` whether it is switching on or off.

- [ ] **Step 1: Write the failing tests** in `autorun_recommended.rs`:
  - `settle_mode_on_upgrade(None, true) == Some(Advanced)`;
  - `settle_mode_on_upgrade(None, false) == None`;
  - `settle_mode_on_upgrade(Some(Simple), true) == Some(Simple)`;
  - `an_install_with_auto_run_on_settles_to_advanced_and_gets_nothing_switched_on`: `recommended_on_switch(true, &defaults)` has every field false;
  - `a_machine_that_never_used_auto_run_gets_everything`: `recommended_on_switch(false, &AppSettings::default())` has all four fields true;
  - `a_person_set_switch_survives_auto_run_off_and_on`: with `api_templates_set = true` and `autorun_failed_video_set = true`, both fields are false;
  - `a_picked_mode_is_never_overwritten`: with `autorun_mode = Some(Advanced)`, `mode_simple` is false;
  - `auto_run_used_before` on a temp dir, with and without an `autorun` subfolder;
  - `modules_set_with` (extended to take the settings and data dir; keep it the testable half):
    - switching Auto Run on, on a fresh temp dir, turns on `ApiTemplates`, sets `autorun_failed_video`, and sets `autorun_mode = Simple`;
    - on a dir with `autorun/`, none of that happens;
    - `recommended` is `None` when Auto Run was already on.
- [ ] **Step 2: Run them and confirm they fail:** `cd src-tauri && cargo test --test suite autorun_recommended::`. Expected: they fail to compile (the names don't exist yet).
- [ ] **Step 3: Implement** the fields, `recommended.rs`, the `modules_set` / `modules_set_with` change and the two setters.
  - In `modules_set`, read `auto_run_used_before` BEFORE applying the switch.
  - Save the settings once after applying.
  - Call `settle_mode_on_upgrade` once at start-up, after modules load. Persist only when it changed.
- [ ] **Step 4: Run the tests and regenerate the bindings:** `cargo test --test suite autorun_recommended::`, then `cargo test --test bindings`, then `cargo test --test suite app_settings::`. Expected: all pass, and `src/bindings.ts` updated.
- [ ] **Step 5: Fix the TS callers** of `modulesSet`, which now returns `{ on, recommended }` (`src/screens/Modules/index.tsx` and any other caller found by Grep), keeping today's behaviour. Run `npx tsc --noEmit` and `npx vitest run tests/screens/Modules`. Expected: clean, and the tests pass.
- [ ] **Step 6: Commit** with message `feat(v2): Auto Run mode setting and recommended features on first switch-on`.

### Task 2: Mode hook, header switch, Network tools on switch-on, Modules note (TS)

**Files:**
- Create: `src/screens/AutoRun/useAutoRunMode.ts`
- Modify: `src/screens/AutoRun/index.tsx` (the header switch)
- Modify: `src/lib/networkTools.ts` (person-set marker and switch-on helper)
- Modify: `src/lib/activeEnv.ts` (add `envKeyFor(base: string, envId: string): string` if no equivalent exists, and make `envScopedKey` use it)
- Modify: `src/screens/Modules/index.tsx` (apply the Network tools and show the note)
- Test: `tests/screens/AutoRun/useAutoRunMode.test.ts`, `tests/lib/networkTools.test.ts` (create or extend), `tests/screens/Modules/index.test.tsx`, `tests/screens/AutoRun/index.test.tsx`

**Interfaces:**
- Consumes (Task 1): `commands.setAutorunMode`, `AppSettings.autorun_mode`, and `modulesSet → { on, recommended }`.
- Produces:
  - `useAutoRunMode(): { mode: "simple" | "advanced"; setMode(m: "simple" | "advanced"): Promise<void> }`. The settings value `null` reads as `"advanced"` here: Task 1 settles every install with Auto Run on, so `null` while Auto Run is on means a test or an edge case, and Advanced is the safe default.
  - `isSimple(): boolean` convenience is optional; the components take the hook.
  - `networkTools.ts`: `saveNetworkTools(on)` also writes a person-set marker key, `tcm-v2-network-tools-set:<envId>` = "1".
  - New `turnOnNetworkToolsWhereUnset(envIds: string[]): number` sets the switch on for each environment that has no marker and returns how many it changed. It does not write the marker; that's a default, not a person's choice.

- [ ] **Step 1: Write the failing tests:**
  - **`useAutoRunMode`:** reads `null` as advanced; `setMode("simple")` calls `commands.setAutorunMode("simple")`.
  - **networkTools:**
    - an environment with the marker is untouched by `turnOnNetworkToolsWhereUnset`;
    - one without gets "1";
    - `saveNetworkTools(false)` writes the marker, so a later `turnOnNetworkToolsWhereUnset` leaves it off.
  - **Modules page** (with the command mocked):
    - switching Auto Run on with `recommended = { api_templates: true, video: true, network_tools: true, mode_simple: true }` calls `turnOnNetworkToolsWhereUnset` with the environments' ids and shows the note naming all three;
    - with `api_templates: false`, the note leaves "API Templates" out;
    - with all false, there is no note.
  - **Auto Run header:**
    - a segmented switch with "Simple" and "Advanced", where clicking "Advanced" calls `setAutorunMode("advanced")`;
    - it is disabled, with a title, while an unattended run is going (use the existing "run is going" signal the screen already reads).
- [ ] **Step 2: Run them and confirm they fail:** `npx vitest run tests/screens/AutoRun/useAutoRunMode.test.ts tests/lib/networkTools.test.ts tests/screens/Modules/index.test.tsx tests/screens/AutoRun/index.test.tsx`.
- [ ] **Step 3: Implement.**
  - The environments' ids come from the environments list the app already queries; reuse its cache key, never a new cache.
  - The note sits inline under the Auto Run row and builds its list from the `recommended` fields.
  - The header switch follows an existing segmented control in the app (Grep for one, for example the Settings theme or the gallery tabs) and uses `Button` or `role="radiogroup"` like it.
- [ ] **Step 4: Run them and confirm they pass** (same command), then run `npx tsc --noEmit` and `npx vitest run tests/ui-consistency.test.ts`.
- [ ] **Step 5: Commit** with message `feat(v2): Simple and Advanced switch in Auto Run; Network tools on when Auto Run is first switched on`.

### Task 3: Simple hides setup rows; one "blocks a run" rule (TS)

**Files:**
- Create: `src/screens/AutoRun/blocksRun.ts`
- Modify: `src/screens/AutoRun/SetupPanel.tsx` (the summary lines and the full rows)
- Test: `tests/screens/AutoRun/blocksRun.test.ts`, `tests/screens/AutoRun/SetupPanel.test.tsx` (create; mount the panel the way `SetupFixtures.test.tsx` does)

**Interfaces:**
- Consumes (Task 2): `useAutoRunMode()`.
- Produces:
  - `export type SetupRowName = "Environment" | "Site address" | "Sign-in" | "Accounts" | "Areas" | "Discovery" | "Components" | "Test files" | "Fixtures" | "Database"`.
  - `export const SIMPLE_HIDDEN: ReadonlySet<SetupRowName> = new Set(["Discovery", "Components", "Fixtures"])`. The Sign-in row stays, but in Simple it hides its recipe editor, Record sign-in and recipe details.
  - `export function blockingRows(input: { essentialMissing: boolean; signIn: "saved" | "builtin" | "none" | null; discovering: boolean; fixturesFailed: number; fixturesFailedHere: number }): Set<SetupRowName>`:
    - `"Discovery"` when `discovering`;
    - `"Fixtures"` when either fixtures count is above 0;
    - `"Sign-in"` (with its details) when `signIn === "none"`.
    
    It is the one function the header reasons and Simple's rule both read.
  - `export function shownInSimple(row: SetupRowName, blocking: Set<SetupRowName>): boolean` returns true when the row is not in `SIMPLE_HIDDEN`, or when it is in `blocking`.

- [ ] **Step 1: Write the failing tests:**
  - `blockingRows`, each case above;
  - `shownInSimple` for every row name, with an empty blocking set and with Discovery blocking;
  - the panel in Simple: no Discovery, Components or Fixtures rows, and no recipe editor or Record sign-in buttons;
  - the same panel in Advanced shows all of them (the today's-screen check);
  - `discovery_row_shows_in_simple_only_while_a_discovery_holds_the_browser`: the row shows, with End discovery and the exact blocker note, while `useDiscoveryActive` is true, and is gone when it turns false;
  - a failed fixture shows the Fixtures row with the note in Simple.
- [ ] **Step 2: Run them and confirm they fail:** `npx vitest run tests/screens/AutoRun/blocksRun.test.ts tests/screens/AutoRun/SetupPanel.test.tsx`.
- [ ] **Step 3: Implement.** Gate each row's markup and summary line with `mode === "advanced" || shownInSimple(name, blocking)`. A row shown only because it blocks renders the exact note as one muted line under it. Build `attentionReasons` from the same inputs `blockingRows` reads; its wording is unchanged.
- [ ] **Step 4: Run them and confirm they pass**, plus the existing `tests/screens/AutoRun/SetupFixtures.test.tsx`, `tests/screens/AutoRun/index.test.tsx` and `npx tsc --noEmit`.
- [ ] **Step 5: Commit** with message `feat(v2): Simple mode hides advanced setup rows unless one stops a run`.

### Task 4: Simple hides run-side controls; read-only View script (TS)

**Files:**
- Modify: `src/screens/AutoRun/PastRuns.tsx`, `src/screens/AutoRun/RunReview.tsx` (Replay to step)
- Modify: `src/screens/AutoRun/index.tsx` (Import scripts)
- Modify: `src/screens/AutoRun/ScriptEditor.tsx` (a `readOnly` prop)
- Modify: `src/screens/AutoRun/CaseCard.tsx` (passes `readOnly` in Simple)
- Test: the matching files in `tests/screens/AutoRun/` (`PastRuns`, `RunReview`, `index`, `ScriptEditor`, `CaseCard`)

**Interfaces:**
- Consumes (Task 2): `useAutoRunMode()`.
- Produces: `ScriptEditor` takes `readOnly?: boolean`. When true:
  - the fields render as text, not inputs;
  - no "Edit script", no Copy case, no Save;
  - the footer has Close and the muted text "Switch to Advanced to edit."

- [ ] **Step 1: Inventory.** Before the tests, list every control on the Auto Run screen and its dialogs that the spec does not name, with a proposed Simple side (shown or hidden), in the task report. Stop and report it as `NEEDS_CONTEXT`; the coordinator rules on the list, then this task continues.
- [ ] **Step 2: Write the failing tests:**
  - in Simple: no "Replay to step" in Past runs or the run review, no Import scripts, and View script opens with no inputs, no Edit script and no Copy case, and shows the footer text;
  - in Advanced: each one is present, as today;
  - the run window, the network-wait panel, Execution order, Raise bug, video and the Assistants card are present in Simple;
  - plus tests for each control the coordinator's ruling hides.
- [ ] **Step 3: Run them and confirm they fail:** `npx vitest run tests/screens/AutoRun/PastRuns.test.tsx tests/screens/AutoRun/RunReview.test.tsx tests/screens/AutoRun/index.test.tsx tests/screens/AutoRun/ScriptEditor.test.tsx tests/screens/AutoRun/CaseCard.test.tsx`.
- [ ] **Step 4: Implement** the gates and `readOnly`.
- [ ] **Step 5: Run them and confirm they pass** (same command), then `npx tsc --noEmit`.
- [ ] **Step 6: Commit** with message `feat(v2): Simple mode hides replay, import and script editing; View script is read-only`.

### Task 5: Ruling wording (TS and person-facing Rust text)

**Files:**
- Modify: `src/screens/AutoRun/DivergenceMark.tsx`
- Modify: the `Term` explanation for "ruling" (find it with Grep for `name="ruling"`, or in the terms list `src/lib/terms.ts`, or whatever file `Term` reads)
- Modify: Rust text a person sees quoted:
  - bell notices about rulings;
  - the "Ruled by the assistant" sentence, if built in Rust;
  - the ruling tool's description and guide lines that quote button labels (`src-tauri/src/autorun/divergences.rs`, `src-tauri/src/autorun/guide.rs`, `src-tauri/src/ai_bridge/` routes);
  - the How To Use content that names the buttons (`docs-site/src/content/auto-run.ts`).
- Test: `tests/screens/AutoRun/DivergenceMark.test.tsx`, plus a Rust guard test `src-tauri/tests/suite/ruling_words.rs` (+ `mod` line)

**Interfaces:** The `Ruling` values (`case_stands`, `data_fixed`, `rewrite`) and every command stay unchanged.

- [ ] **Step 1: Write the failing tests.**
  - **DivergenceMark** (for an open `data` divergence at step 3 on case #12):
    - the question line "The case and the app disagree at step 3. Which is right?";
    - buttons named "The app is wrong", "The app is right, update the case" and "I fixed the data";
    - accessible names "The app is wrong for #12 step 3", "The app is right, update case #12 for step 3" and "I fixed the data for #12 step 3";
    - each confirm sentence verbatim, with N = 3;
    - Confirm sends `case_stands`, `rewrite` or `data_fixed` as before;
    - an `app` divergence shows no question line and no buttons, as today.
  - **Rust `no_person_facing_text_uses_the_old_ruling_labels`:** scans `src/`, `src-tauri/src/` and `docs-site/src/content/` for "The case is right", "Rewrite the case" and "Data fixed". It finds none, except in a comment or in an internal tool name, and lists the allowed files explicitly if any exist.
- [ ] **Step 2: Run them and confirm they fail:** `npx vitest run tests/screens/AutoRun/DivergenceMark.test.tsx`, then `cd src-tauri && cargo test --test suite ruling_words::`.
- [ ] **Step 3: Implement** the wording.
- [ ] **Step 4: Run them and confirm they pass** (same commands), plus `cargo test --test suite divergence` (every divergence module), `npx tsc --noEmit`, and `npm run docs:build` if the docs content changed. Expected: the docs guard passes.
- [ ] **Step 5: Commit** with message `feat(v2): divergence rulings ask whether the case or the app is right`.

### Task 6: `by_auto_run` steps, the save check, and the filtered unchecked expectations (Rust and TS mirror)

**Files:**
- Modify: `src-tauri/src/autorun/mod.rs` (`StepScript` gains `#[serde(default, skip_serializing_if = "std::ops::Not::not")] pub by_auto_run: bool`)
- Create: `src-tauri/src/autorun/by_auto_run.rs` (the shared rule); register it in `mod.rs`
- Modify: `src-tauri/src/autorun/floor.rs` (the save-check rules)
- Modify: `src-tauri/src/autorun/unchecked.rs` (`expectations` filters)
- Modify: the fixture `src-tauri/tests/fixtures/unchecked_expectations.json`, plus a new fixture `src-tauri/tests/fixtures/by_auto_run.json`
- Modify: `src/screens/AutoRun/unchecked.ts` (TS mirror); create `src/screens/AutoRun/byAutoRun.ts`
- Modify: `src-tauri/src/autorun/guide.rs` (document the field)
- Test: `src-tauri/tests/suite/autorun_by_auto_run.rs` (+ `mod` line), the existing floor and unchecked suites, `tests/screens/AutoRun/unchecked.test.ts`, `tests/screens/AutoRun/byAutoRun.test.ts`, `tests/screens/AutoRun/floor.test.ts`

**Interfaces:**
- Produces:
  - `pub fn done_by_auto_run(script: &CaseScript) -> BTreeSet<i32>`: the step numbers Auto Run performs itself.
    - A step counts when it has `by_auto_run: true` and sits in the leading run.
    - If no step in the script sets the field (an older script), the fallback is the leading run of case steps from step 1 with zero actions and a whole-step `Unchecked::Step` sentence.
    - Clean-up steps never count.
  - The TS mirror is `doneByAutoRun(script: CaseScript): Set<number>` in `byAutoRun.ts`. Both are held to `by_auto_run.json`, whose rows are `{ name, script, steps: number[] }`.
  - `expectations(script)` leaves out every step in `done_by_auto_run(script)`, and so does the TS `uncheckedExpectations`. Every consumer then agrees: the card, the passed count, the report's Unchecked lines, counts and Insights total, and the Playwright export's unchecked comments.
  - **Save-check rules** in `check_floor`:
    - a step with `by_auto_run: true` needs no `unchecked` sentence or check;
    - `"step {n} is marked as done by Auto Run but has actions - drop one or the other"`;
    - `"step {n} is marked as done by Auto Run but is not one of the opening steps"` (any `by_auto_run` step after a step without it, or a clean-up step).
    
    The TS mirror `floor.ts` gets the same rules, if it mirrors floor.rs today.

- [ ] **Step 1: Write the failing tests.**
  - **`by_auto_run.json` rows:**
    - steps 1–3 marked;
    - steps 1 and 3 marked (only 1 counts);
    - no field, with a fallback of 1–2 (zero actions plus a sentence) and step 3 with actions;
    - no field and step 1 with actions (empty);
    - a clean-up step marked (excluded).
  - **The floor tests:** each new sentence verbatim; a marked step with no `unchecked` saves.
  - **The unchecked fixture:** a row where steps 1–3 are done by Auto Run and step 6 has a list entry gives only step 6's line and "1 expectation unchecked".
  - **The report:** the existing report test module asserts a passed case's "N expectations unchecked" uses the filtered count (add one case).
  - **The Playwright export:** its translate test asserts no unchecked comment for a done-by-Auto-Run step.
- [ ] **Step 2: Run them and confirm they fail:** `cd src-tauri && cargo test --test suite autorun_by_auto_run::`, then `cargo test --test suite unchecked`, then `cargo test --test suite floor`, then `npx vitest run tests/screens/AutoRun/byAutoRun.test.ts tests/screens/AutoRun/unchecked.test.ts tests/screens/AutoRun/floor.test.ts`.
- [ ] **Step 3: Implement.** Document in `guide.rs` when to set `by_auto_run`: the step's whole expected result is launching the app, signing in, or reaching the area, which Auto Run does before step 1. Regenerate the bindings.
- [ ] **Step 4: Run them and confirm they pass** (same commands), plus `cargo test --test suite autorun_report`, the Playwright export module (`pw_export`), `cargo test --test bindings` and `npx tsc --noEmit`.
- [ ] **Step 5: Commit** with message `feat(v2): steps done by Auto Run, and unchecked expectations that leave them out`.

### Task 7: The open script card (TS)

**Files:**
- Modify: `src/screens/AutoRun/CaseCard.tsx`
- Test: `tests/screens/AutoRun/CaseCard.test.tsx`

**Interfaces:**
- Consumes (Task 6): `doneByAutoRun`, and the filtered `uncheckedExpectations`.
- Produces:
  - `scriptHeaderLine(script: CaseScript): string`, of the form `"Script · 8 steps · runs as automation · Self Service › Insights › AI Insights"`. It leaves out a part with no value.
  - `scriptFacts` splits into `actionFacts(script)` and `assistantNotes(script)`:
    - `actionFacts`: Must not save, Needs records, Preconditions, Assumed values, Invented values;
    - `assistantNotes`: Changes, Needs unchanged, Last repair, Checks given up, Page errors, Ignored page errors, Unexpected dialogs.

- [ ] **Step 1: Write the failing tests:**
  - the header line, with and without account and area;
  - the action facts as short lines;
  - "Assistant notes" as a closed `<details>`, with its count in the summary and its items inside;
  - steps 1–3 show "Done by Auto Run" and no "0 actions";
  - "Not checked" lists only step 6 for the screenshot's script (build it from the owner's example: steps 1–3 with whole-step sentences, step 6 a list entry);
  - a passed case's count reads "1 expectation unchecked".
  
  Update the existing CaseCard tests where the facts list moved, keeping each assertion's intent.
- [ ] **Step 2: Run them and confirm they fail:** `npx vitest run tests/screens/AutoRun/CaseCard.test.tsx`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them and confirm they pass**, then `npx tsc --noEmit` and `npx vitest run tests/ui-consistency.test.ts`.
- [ ] **Step 5: Commit** with message `feat(v2): a shorter script card: one header line, assistant notes folded away, steps done by Auto Run`.

### Task 8: Assistants, How To Use, and full verification

**Files:**
- Modify: `src-tauri/src/autorun/guide.rs` (Simple mode advice)
- Modify: the bridge context, or a status answer, that carries the mode (`src-tauri/src/ai_bridge/mod.rs` `BridgeContext` and its push in `src/app/useBridgeContextSync.ts`)
- Modify: `docs-site/src/content/auto-run.ts` (a Simple and Advanced section) and the guide shots
- Test: the bridge context test that lists the context fields; the docs guard

**Interfaces:**
- Produces: the `BridgeContext` field `autorun_mode: Option<AutorunMode>`. An assistant reads it; no tool sets it.

- [ ] **Step 1: Write the failing test.** The bridge status answer includes `autorun_mode` when the webview pushed it. Find the existing context-field test with Grep for `network_tools` in `src-tauri/tests/suite/`.
- [ ] **Step 2: Run it and confirm it fails.**
- [ ] **Step 3: Implement.**
  - **Guide text:** "The person may be in Simple mode: do not send them to Discovery, Components, Fixtures or the sign-in recipe; offer to do it, or ask them to switch to Advanced."
  - **The How To Use section:** what Simple shows and hides, the switch, and the blocker note.
  - **Shots:** first check whether port 1420 is in use (the owner's `tauri dev`). If it is, skip the shots and report it. Otherwise run `npm run docs:dev`, `npm run docs:shots -- --dry-run`, `npm run docs:shots -- --only <the Auto Run shots that changed>`, then `npm run docs:build`.
- [ ] **Step 4: Full verification**, one command at a time:
  1. `npx tsc --noEmit`
  2. `npm test`
  3. `cd src-tauri && cargo test --tests -- --skip autorun_recorder::`
  4. `cargo test --test suite autorun_recorder::`
  5. `cargo test --test bindings`
  
  Expected: all pass. A timing test that fails once is re-run alone, and the result is reported.
- [ ] **Step 5: Commit** with message `docs(v2): Simple mode in the guides; assistants can see the mode`. Include the shots and the built help site with it.
