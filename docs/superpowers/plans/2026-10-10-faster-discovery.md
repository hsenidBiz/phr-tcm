# Faster Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** An assistant writes a page's scripts from one look at each screen. To get there:
- reading a screen during a discovery records what it shows;
- a refused save lists every problem, and a dry run is available;
- routine walks take one call;
- repeat jobs start from a short rule sheet.

**Architecture:**
- `ai_bridge.rs` `autorun_page` records sightings through the same path a discovery action's page read uses.
- `seen_check.rs` collects every refusal instead of returning the first. Saves gain `dry_run`.
- A new bridge route and MCP tool, `discover_autorun_actions`, loops over `discover_action_in`.
- `guide.rs` gains a "Quick rules" section, and `get_autorun_guide` gains `quick`.

**Tech Stack:** Rust (Tauri 2), with MCP tool schemas in `mcp.rs`.

**Spec:** `docs/superpowers/specs/2026-10-10-faster-discovery-design.md` (approved 2026-10-10).

## Global Constraints

- The gate does not loosen. Reading a page records only what that read returned, and only during a discovery, on the application's own origins, with a current area.
- Refusals are capped at 50 entries, followed by "and N more".
- A batch holds at most 20 actions. It stops on the first failure by default.
- No hosts or query strings in logs or answers. No HTTP DELETE. No em or en dashes. The banned phrases stay banned, and hidden features are never named.
- Rust tests are integration tests in `src-tauri/tests/suite/`, with serial locks. `src/bindings.ts` is generated only.
- Run one build or test command at a time. Commit with a Bash heredoc, staged by name, ending with the implementer's `Co-Authored-By`.

## Review Focus

1. A page read outside a discovery, or off the application's origins, records nothing. Test: `a_page_read_outside_discovery_records_nothing`.
2. A dry run never writes a script and never records a sighting, including through the record-on-page path. Test: `a_dry_run_writes_and_records_nothing`.
3. A batch action that would save on a Must not save or mapping run is blocked and counted exactly as a single action would be. Test: `a_batch_blocks_saves_like_single_actions`.
4. A refusal list keeps step order, and every entry carries a hint of the same role only. Test: `a_refusal_lists_every_unseen_locator_in_order`.
5. The quick guide never contradicts the full guide: every rule in it appears in the full guide. Test: `every_quick_rule_is_in_the_full_guide`.

---

### Task 1: Reading a screen records what it shows

**Files:** `ai_bridge.rs` (`autorun_page`) and `autorun/discovery_map.rs`, reusing the action page-read recording path.

**Interfaces:**
- During an open discovery that has a current area, the lines `get_autorun_page` returns are recorded as sightings under that area, keyed as an action read is.
- The answer ends with `Recorded N elements as seen on <area>.`
- With no current area, nothing is recorded, and the answer ends with a line saying to save or name an area first.
- A read cut short by `limit` records only the lines it returned.

- [ ] Tests:
  - `a_page_read_in_discovery_records_its_lines`
  - `a_page_read_outside_discovery_records_nothing`
  - `a_limited_read_records_only_returned_lines`
  - `a_read_with_no_area_says_so`
- [ ] Commit `feat(v2): reading a page during discovery records what it shows as seen`.

### Task 2: Every refusal at once, and a dry run

**Files:** `autorun/seen_check.rs` (collect all refusals), `ai_bridge.rs` (script and component saves, and the record-on-page pass over the whole list), `mcp.rs` (`dry_run` on both save tools).

**Interfaces:**
- The seen check returns every refusal in step and action order, each with its same-role hint, capped at 50 with "and N more".
- During a discovery, the record-on-page pass runs over the whole list. The save is then checked once more, and the answer lists what was recorded and what is still refused.
- `"dry_run": true` on `save_autorun_script` and `save_autorun_component` runs every check and answers what would be refused. It writes nothing and records nothing.

- [ ] Tests:
  - `a_refusal_lists_every_unseen_locator_in_order`
  - `the_refusal_list_is_capped_at_fifty`
  - `a_dry_run_writes_and_records_nothing`
  - `a_dry_run_that_passes_says_it_would_save`
  - `record_on_page_runs_over_every_refused_locator`
- [ ] Commit `feat(v2): a refused save names every problem at once, and a dry run checks without saving`.

### Task 3: Several actions in one call

**Files:**
- `ai_bridge.rs`: the new route `/autorun-discover-actions`, which loops over `discover_action_in` under one lock hold.
- `mcp.rs`: the new tool `discover_autorun_actions`.
- `ai_tools.rs`: the gate and the commands' mentions.
- `autorun/guide.rs`.

**Interfaces:**
- Input: `{ actions: [...], stop_on_failure?: bool = true }`. More than 20 actions is refused with a plain sentence.
- Output: one line per action that ran, ok or failed with the reason, then the page text once, after the last action that ran.
- Each action keeps its own blocking, counting, sightings and `use_component`/`draft` behaviour.

- [ ] Tests:
  - `a_batch_runs_in_order_and_answers_the_page_once`
  - `a_batch_stops_on_the_first_failure`
  - `a_batch_can_carry_on_past_a_failure`
  - `a_batch_of_more_than_twenty_is_refused`
  - `a_batch_blocks_saves_like_single_actions`
  - `a_batch_records_what_each_action_read`
- [ ] Commit `feat(v2): discover_autorun_actions runs a list of actions and answers the page once`.

### Task 4: The quick rule sheet

**Files:** `autorun/guide.rs` (a "Quick rules" section at the top, about 40 lines), the `get_autorun_guide` handler and its MCP schema (`quick: true`), and `ai_tools.rs` (the `/tcm:discover`, `/tcm:map-menus` and `/tcm:heal` bodies).

**Interfaces:**
- `quick: true` answers the Quick rules section plus the project's live sections: areas, accounts and components.
- The three commands say: read the quick guide first, and the full guide when something it does not cover comes up.

- [ ] Tests:
  - `the_quick_guide_holds_the_rules_and_the_live_sections`
  - `every_quick_rule_is_in_the_full_guide` (key phrases)
  - `the_commands_point_at_the_quick_guide`
  - the banned-phrase guard still passes
- [ ] Commit `feat(v2): a quick guide for repeat work`.

### Final gate

- [ ] Run, one at a time: `cd src-tauri && cargo test --tests`, then `cargo test --test bindings` if any schema changed, then `npx tsc --noEmit`, `npm test` and `npm run build`.
- [ ] Hand check owed: write one page's scripts with the new tools and count the refused saves.
