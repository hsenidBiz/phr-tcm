# Auto Run Agent Pitfalls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Remove the mistakes assistants make when driving Auto Run, recorded in the owner's `agent-pitfalls.md` (2026-10-09). The fixes:
- give full picture paths;
- replay area checks from a known start;
- stop refusing locators that are legitimately built from data, state or near-miss names;
- record a refused locator's sighting during discovery;
- make the guide plain about the rules.

**Architecture:** Changes go in four places:
- the bridge answers and `failures.rs`, for picture paths;
- `discover_area_in` and the trip to an area, for the known start;
- `autorun/seen_check.rs` and `discovery_map`, for the sighting check;
- `autorun/guide.rs`, for the wording.

**Tech Stack:** Rust (Tauri 2).

**Spec:** the owner's `C:\Users\Admin\Desktop\agent-pitfalls.md`. A copy is committed beside this plan as `docs/superpowers/specs/2026-10-09-autorun-agent-pitfalls.md`.

## Global Constraints

- The sighting check must never accept a locator that could not exist on the seen page.
  - A wildcard matches only the shape of a value that was seen (digits for digits).
  - A stripped state pseudo-class matches only when the base element was seen.
  - The new exemptions are limited to the script's own test files and the dates it picked or typed.
- The banned phrases ("read the source", "application's source", "source-derived") stay banned.
- Logs and answers carry no hosts or query strings. A picture path is a local file path.
- Rust tests are integration tests in `src-tauri/tests/suite/`, with a `mod` line and serial locks.
- No em or en dashes in code or guide text.
  - The sighting check normalises them in names it compares.
  - Tests may contain them as input data.
- Run one build or test command at a time.
- Commit with a Bash heredoc, staged by name, ending with the implementer's `Co-Authored-By`.

## Review Focus

1. A placeholder wildcard must not let an unseen attribute pass. `div[data-cycle-id="{{setup.cycle_id}}"]` passes only when a `div[data-cycle-id="<digits>"]` was seen in that area. Test: `a_placeholder_matches_only_a_seen_shape`.
2. `:not(...)` and `:has(...)` pass only when their inner part was seen too. Test: `a_not_filter_needs_its_inner_part_seen`.
3. A date exemption must not exempt arbitrary buttons. It applies only to a date the script picked or typed, or a `dd/mm/yyyy` name inside a date picker that was seen. Test: `only_a_picked_date_is_exempt`.
4. Replaying an area save from home must not run `after_sign_in` twice on a page where it already ran, since a toggle run twice closes the menu. Test: `an_area_check_starts_from_home_with_the_menu_open_once`.
5. The closest-match suggestion must never name a locator from another area. Test: `the_suggestion_comes_from_the_same_area`.

---

### Task 1: Full picture paths, and the guide's "Seeing the page"

**Files:** `ai_bridge.rs` (around line 3381), `autorun/failures.rs` (around line 215), every other place that prints a `shot-...jpg` name, and `autorun/guide.rs` ("Seeing the page").

**Interfaces:**
- Each of these answers names the picture by its absolute path under the store root's `shots` folder: a try or discovery answer, a `get_autorun_failures` line, and any other place a picture is named to an assistant.
- The guide says, in one place:
  - pictures are at that path;
  - open one directly with the file reader;
  - never search the disk for one;
  - prefer `get_autorun_page` (with a `limit`) or `probe_autorun_locator`.

- [ ] Tests:
  - `a_try_answer_names_the_picture_by_a_path_that_exists`
  - `a_failure_line_names_the_picture_by_a_path_that_exists`
  - `the_guide_says_where_pictures_are_and_never_to_search`
- [ ] Commit `fix(v2): an assistant is told where each picture is, never just its name`.

### Task 2: Area checks and trips from a known start, and a summary that drops a screen saved later

**Files:**
- `ai_bridge.rs` (`discover_area_in` and its replay)
- `autorun/nav.rs` (`go_to_module`, if the trip shares the replay)
- `mapping_summary` / `MappingRun` outcomes

**Interfaces:**
- **The area check's replay** always starts from a fresh home page with `after_sign_in` run once (`load_home`). It never starts from the current menu state.
- **A run's trip to an area** already goes home first today. It stays as it is.
  - The nav-speed plan that follows changes trips to start from the current page.
  - That plan owns the toggle guard. This task touches only the save-time check.
- **In a mapping run,** an `unreached` entry is dropped from the summary when the same screen is later saved in the run under another name. "The same screen" means the same `arrived` path.

- [ ] Tests:
  - `an_area_check_starts_from_home_with_the_menu_open_once`
  - `a_closed_menu_does_not_refuse_an_area_save`
  - `an_unreached_screen_saved_later_under_another_name_leaves_the_summary`
- [ ] Commit `fix(v2): an area is checked from a fresh home page, and a screen saved later leaves the not-reached list`.

### Task 3: The sighting check accepts data-built locators

**Files:** `autorun/seen_check.rs`, `autorun/discovery_map.rs` (seen keys), and `autorun/components.rs` (use_component inputs).

**Interfaces:** each rule applies only where noted.
1. **Placeholders as shaped wildcards.**
   - Applies to `{{fixture.*}}`, `{{setup.*}}`, `{{prefix}}` and `{{now:...}}` inside a locator's attribute value or name.
   - Each matches a seen value of the same shape:
     - all digits when the seen value was digits;
     - otherwise a non-empty run with no quote characters.
   - The text around the placeholder must match the seen value literally.
   - `use_component` inputs may carry a placeholder at save time. The resolved value is checked at run time, and a run-time failure names the input.
2. **State pseudo-classes.**
   - `:checked`, `:disabled`, `:enabled` and `:focus` are stripped before matching against the base selector's sightings.
   - `:not(X)` and `:has(X)` pass when the base was seen and X was seen in the same area.
3. **The script's own data.** These are exempt, like typed text:
   - the file names of Test files the script uploads, and names containing them;
   - their displayed sizes, in KB or MB as the app shows them;
   - a date-picker day button whose name is a date the script picked or typed;
   - a `dd/mm/yyyy` name inside a date picker that was seen.
4. **Names are normalised before comparing:**
   - whitespace collapsed;
   - case folded;
   - `—`, `–` and `-` treated as one dash.
5. **The refusal suggests the closest seen locator in the same area.**
   - The wording is: `did you mean <role> "<name>"?`
   - "Closest" means the same role first, then normalised edit distance, with a cap of 1 suggestion.

- [ ] Tests:
  - `a_placeholder_matches_only_a_seen_shape`
  - `a_component_input_placeholder_is_checked_at_run_time`
  - `a_checked_state_on_a_seen_input_passes`
  - `a_not_filter_needs_its_inner_part_seen`
  - `a_test_file_name_and_size_are_exempt`
  - `only_a_picked_date_is_exempt`
  - `dashes_spaces_and_case_do_not_refuse_a_name`
  - `the_suggestion_comes_from_the_same_area`
  - `an_unseen_locator_is_still_refused`
- [ ] Commit `fix(v2): the seen check accepts locators built from placeholders, state, the script's own files and dates, and suggests the closest seen one`.

### Task 4: Record a refused locator's sighting during discovery, and the guide's rules

**Files:** `ai_bridge.rs` (script and component save during an open discovery) and `autorun/guide.rs` ("Saving it", "Components").

**Interfaces:**
- **During an open discovery,** a script or component save refused only for unseen locators first probes each refused locator on the current page.
  - It uses the same probe as `probe_autorun_locator`.
  - It records a sighting for every locator that matches, then checks the save again once.
  - The answer says which locators it recorded.
  - Outside discovery, nothing changes.
- **The guide says plainly, before the save examples:**
  - probe every final locator exactly as written immediately before saving;
  - try a component with `use_component` + `draft` before `save_autorun_component`, and save it unchanged after that try;
  - a step is either `unchecked` (no checks) or checked, never both;
  - check state with `expect_attribute` (`checked`, `aria-checked`, `disabled`).
- **Opening a fixture draft:** the guide now says a placeholder in a locator is accepted when its shape was seen. Opening the draft by its unique name stays as a fallback.

- [ ] Tests:
  - `a_save_refused_in_discovery_records_a_locator_on_the_page_and_passes`
  - `a_save_refused_outside_discovery_records_nothing`
  - `the_guide_states_the_save_rules`
  - the banned-phrase guard still passes
- [ ] Commit `feat(v2): a save refused during discovery checks the refused locators on the page, and the guide states the save rules`.

### Final gate

- [ ] Run, one at a time: `cd src-tauri && cargo test --tests`, then `npx tsc --noEmit`, `npm test` and `npm run build`.
