# Auto Run Menu Map Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** A read-only discovery "mapping run" in which the assistant walks the named modules' menus and saves every screen as an area marked as made by mapping, with a summary at the end.

**Architecture:**
- A mapping run is a discovery session with `mapping` state. The session's save guard blocks every write.
- `ModulePath` records who made it.
- `save_autorun_area` stores and updates mapping-made areas and refuses person-made ones.
- `end_autorun_discovery` returns and keeps a summary, which the Discovery dialog shows.
- A `/tcm:map-menus` command drives it.

**Tech Stack:** Rust (Tauri 2), React 19 + TS, vitest.

**Spec:** `docs/superpowers/specs/2026-10-09-autorun-menu-map-design.md`

## Global Constraints

- **Read only.** A mapping run never sends a save request. Every write the page attempts is blocked by the existing save guard (`browser/save_guard.rs`, `SAVE_METHODS`, the same machinery as Must not save) and counted.
- **Existing areas.** Areas saved before this change, and areas saved by ordinary discovery, are `MadeBy::Person` and never changed by mapping.
- **Cap and wording.** The screen cap is **150** areas per mapping run. The refusal text is "This mapping run has saved 150 screens; end it and start another for the rest."
- **No secrets, hosts or query strings** in summaries or logs. Paths are menu click names only. No HTTP DELETE.
- **Test layout.** Rust tests are integration tests in `src-tauri/tests/suite/`, with a `mod` line. A test touching process-wide state takes its lock from `suite/serial.rs`. `src/bindings.ts` is generated only.
- **No em or en dashes.** Run one build or test command at a time.
- **Commits.** Bash heredoc, staged by name, ending with the implementer's `Co-Authored-By` line.
- **Gate.** The new tool fields and the command ride the Auto Run gate.

## Review Focus

1. A page that saves on load (an auto-save, or a form posting on navigation) during a mapping run must be blocked, and the run must carry on. Task 1 test: `a_save_the_page_sends_during_mapping_is_blocked_and_counted`.
2. An area file written before this change (no `made_by`) must load as Person and be refused for update. Task 2 test: `an_old_area_loads_as_made_by_a_person`.
3. Mapping twice in a row with no menu change: everything is unchanged, nothing is rewritten, and no duplicates appear. Task 2 test: `mapping_the_same_screen_twice_is_unchanged`.
4. The browser closes or the session ends mid-run: the summary still records what was done. Task 3 test: `a_mapping_run_closed_mid_way_keeps_its_summary`.
5. A screen name that differs from an existing area only by case or spacing counts as the same area (`nav::module_key`). Task 2 test: `a_screen_named_like_an_existing_area_matches_it`.

---

### Task 1: The mapping run and its write block

**Files:**
- `commands/autorun.rs` (`DiscoveryState`)
- `ai_bridge.rs` (`/autorun-discover-start` and `discover_action_in`)
- the save guard wiring used for Must not save (find how `no_save` arms `save_guard` on a session)

**Interfaces:**
- `DiscoveryState` gains `pub mapping: Option<MappingRun>`, where:
  - `pub struct MappingRun { pub modules: Vec<String>, pub started_at: u64, pub added: Vec<String>, pub updated: Vec<(String, String, String)>, pub unchanged: Vec<String>, pub unreached: Vec<(String, String)>, pub blocked_writes: u32 }`;
  - `updated` is (name, old path, new path) and `unreached` is (name, reason).
- `POST /autorun-discover-start` accepts `mapping: bool` and `modules: [String]`.
  - `mapping` true needs at least one module, else 400 "Name at least one module to map."
  - In a mapping run, the session's save guard is armed so every write is blocked.
  - Every blocked write bumps `blocked_writes` and is logged as `method path` only.
- `discover_action_in` reports blocked writes in its response (`blocked: n`) for a mapping run.

- [ ] Tests:
  - `a_mapping_run_needs_a_module`
  - `a_save_the_page_sends_during_mapping_is_blocked_and_counted`
  - `ordinary_discovery_still_allows_saves`
- [ ] Implement. Run `cargo test --test suite autorun_discovery::`, then `save_guard::`, then `cargo test --tests`.
- [ ] Commit `feat(v2): a discovery mapping run blocks every save the page tries`

### Task 2: Who made each area, and saving in a mapping run

**Files:**
- `autorun/nav.rs` (`ModulePath`)
- `ai_bridge.rs` (`/autorun-discover-area`)

**Interfaces:**
- `#[derive(Default)] pub enum MadeBy { #[default] Person, Mapping }`, serialised as `"person"` / `"mapping"`.
- `ModulePath` gains `#[serde(default)] pub made_by: MadeBy`.
- `/autorun-discover-area` in a mapping run:
  - **Over the cap:** with 150 already saved this run (`added.len() + updated.len()`), it refuses with the cap sentence.
  - **New name:** replay check, then save as `Mapping`, then push to `added`.
  - **Existing `Mapping` area:** replay check. If the clicks or `arrived` changed, save the new path and push to `updated`. Otherwise push to `unchanged`, with no write.
  - **Existing `Person` area:** refuse, as today, and push to `unchanged`, noted as recorded by a person.
  - **Replay fails:** push to `unreached` with the reason, refuse as today, and save nothing.
- Outside a mapping run, behaviour is unchanged and saves as `Person`.
- Names match with `nav::module_key`.

- [ ] Tests:
  - `an_old_area_loads_as_made_by_a_person`
  - `a_mapping_save_is_made_by_mapping`
  - `a_mapping_area_whose_path_changed_is_updated`
  - `mapping_the_same_screen_twice_is_unchanged`
  - `a_person_area_is_never_changed_by_mapping`
  - `a_screen_named_like_an_existing_area_matches_it`
  - `the_cap_refuses_the_151st_screen`
- [ ] Implement. Run `autorun_discovery::`, then `autorun_nav::`, then `cargo test --tests`.
- [ ] Commit `feat(v2): areas record who made them, and a mapping run adds or updates only its own`

### Task 3: The summary, kept and shown

**Files:**
- `ai_bridge.rs` (`/autorun-discover-end`, and `auto_run_close_browser` and `end_discovery` paths)
- a small store at `autorun/mapping_summary.rs` (`projects/<slug>-mapping.json`, an atomic write)
- `commands/autorun.rs` (a new command)
- `src/screens/AutoRun/DiscoveryDialog.tsx`

**Interfaces:**
- `pub struct MappingSummary { pub ran_at: u64, pub modules: Vec<String>, pub added, pub updated, pub unchanged, pub unreached, pub blocked_writes }`, exported with u64 as f64.
- **Ending a mapping run** (end route, Close browser, End discovery, or the browser dying) saves the summary and logs one INFO line with counts and names only.
- **The end route** returns `{summary}` for a mapping run.
- **Command:** `auto_run_load_mapping_summary(organization, project) -> Result<Option<MappingSummary>, String>`. Register it and regenerate the bindings.
- **Discovery dialog:** a "Last menu mapping" section showing the date, the modules, and collapsible lists Added / Updated (old → new) / Unchanged / Could not reach, plus the blocked-save count. It shows nothing when there is no summary.

- [ ] Tests:
  - Rust:
    - `ending_a_mapping_run_saves_and_returns_its_summary`
    - `a_mapping_run_closed_mid_way_keeps_its_summary`
    - `the_summary_names_no_address`
  - Vitest:
    - `the_discovery_dialog_shows_the_last_mapping`
    - `no_mapping_section_without_a_summary`
- [ ] Implement. Run the focused suites, then `cargo test --tests`, `cargo test --test bindings`, `npx tsc --noEmit` and `npm test`.
- [ ] Commit `feat(v2): a mapping run's summary is kept and shown in the Discovery dialog`

### Task 4: The command, the tool fields and the guide

**Files:**
- `mcp.rs` (the `start_autorun_discovery` schema)
- `ai_tools.rs` (`COMMANDS`: the new `/tcm:map-menus`)
- `src/lib/mcpTools.ts`, if commands or tools are mirrored there
- `autorun/guide.rs`, a "Mapping the menus" paragraph

**Interfaces:**
- `start_autorun_discovery` documents `mapping` and `modules`.
- **`/tcm:map-menus` body:**
  1. Ask which modules and which account if not given.
  2. Start a mapping run.
  3. For each module: open it from the main menu; walk every sub-menu entry, admin side then self-service; `save_autorun_area` each screen as "Module / Menu path", after checking the live areas list; go home between branches.
  4. Never fill in or submit anything.
  5. Stop on session expiry, an unexpected page or the cap.
  6. End the run and report the summary.
- **The guide** says mapping is read-only, saves only its own areas, and that `/tcm:discover` uses the areas it made.
- The banned-phrase guard still passes.

- [ ] Tests:
  - the command exists and its body names the steps and "never fill in or submit";
  - the tool schema has `mapping` and `modules`;
  - the guide paragraph is present.
- [ ] Run `tcm_mcp::`, then `ai_tools::`, then `autorun_guide::`, then the vitest for `mcpTools`.
- [ ] Commit `feat(v2): /tcm:map-menus walks the named modules' menus and saves every screen as an area`

### Task 5: The How To Use guide

- [ ] In `docs-site/src/content/auto-run.ts`, add one sentence to the Discovery entry about menu mapping and the "Last menu mapping" section.
- [ ] Run `npm run docs:build`, then `npx vitest run docs-site`.
- [ ] No screenshot retake unless a positions test requires it.
- [ ] Commit `docs(v2): the guide covers mapping the menus`

### Final gate

- [ ] Run, one at a time: `cd src-tauri && cargo test --tests`, then `npx tsc --noEmit`, `npm test` and `npm run build`.
- [ ] Hand check owed: one real mapping run of one module.
