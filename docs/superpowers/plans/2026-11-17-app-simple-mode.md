# App Simple mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an app-wide Simple mode toggle in Settings. It hides Import, Update, Search Suites, Suite Management, API Templates and the Modules page, while every module stays on and API Templates keeps working for the assistant.

**Architecture:**
- **The setting:** a Rust app setting `app_simple_mode`, decided once at start-up (new install = on, existing = off). Turning it on switches every module on through the module tree, without counting API Templates as the person's own choice.
- **What is hidden:** one webview predicate, beside `sectionShown`, decides which screens are hidden. The sidebar, the command list, Ctrl+number, the redirect guard, Settings, the PBI notices and the tour all read it.
- **Per-environment switches:** these live in webview storage. A one-time migration marks existing environments as set by the person. From then on, while Simple mode is on, every environment with no marker has the API write switch and the Network tools on.

**Tech Stack:** Tauri 2, Rust (serde, specta), React 19 + TypeScript, vitest + Testing Library, Rust integration tests in `src-tauri/tests/suite/`.

**Spec:** `docs/superpowers/specs/2026-11-17-app-simple-mode-design.md`, including its "Corrections and calls after the builder check" and "First-run tour" sections. Read it alongside this plan.

## Global Constraints

- **CLAUDE.md rules:**
  - theme tokens only; icons from `src/lib/actionIcons.ts`;
  - `src/bindings.ts` is regenerated (`cargo test --test bindings`), never hand-edited;
  - `tests/ui-consistency.test.ts` and the docs guards are never weakened;
  - frontend tests go in `tests/` mirroring `src/`;
  - Rust tests go in `src-tauri/tests/suite/`, with a `mod` line in `suite/main.rs`.
- **Text and naming:**
  - user-facing text names no URL or file path;
  - hidden features are never named in commits, docs or the changelog. Say "the held-back module" in prose.
- **Off means unchanged:** with `app_simple_mode` off, the app is exactly today's.
- **Simple mode hides; it never switches anything off.** Turning it off leaves every module on.
- **The held-back module** (`HIDDEN_UNTIL_UNLOCKED` in `modules.rs`) is never switched on by Simple mode.
- **No assistant tool can change `app_simple_mode`.**
- **Stopping processes:** never by image name (`taskkill /IM v2.exe`); the owner's installed app is also `v2.exe`. Stop the docs dev app by its PID or by its ports (1420, 9333).
- **Off limits:** do not touch `src/lib/changelog.ts`, the versions, or `docs/backlog.html`.
- **Exact copy:**
  - **Settings switch title:** "Simple mode".
  - **Its line:** "Hides Import, Update, Search Suites, Suite Management, API Templates and Modules. Everything keeps working, and your assistant can still use API Templates."
  - **Disabled title during an unattended run:** "Wait for the Auto Run run to finish."
  - **Failure line:** "Could not switch on: <names>. Turn Simple mode off and on to try again."
  - **Guide line on each hidden screen's section:** "Hidden while Simple mode is on (Settings)."
- **Running tests:** run only the touched test files or modules plus `npx tsc --noEmit`, one command at a time. Run one full pass of each suite, at Task 5.

## Review Focus

1. **An existing install turning Simple mode on** → no API write switch or Network tools come on for its existing environments, and any environment it creates afterwards gets them on. Test in Task 2: `existing_environments_keep_their_switches_after_turning_simple_on`.
2. **A saved last screen pointing at a hidden screen** (for example, the app was closed on Import, and Simple mode was turned on in another window or by the start-up default) → the app opens on Auto Run, not on a blank page or Manual Entry. Test in Task 3: `a_remembered_hidden_screen_opens_auto_run`.
3. **A brand-new install** → it starts in Simple mode with every module on except the held-back one. The Default environment gets the API write switch and the Network tools on. Tests in Task 1 (Rust default plus modules) and Task 2 (webview switches, migration skipped on a new install).
4. **Turning Simple mode off and on again** → modules are still on after off, and nothing a person switched off comes back on. Tests in Task 1 and Task 2.
5. **Ctrl+5 and the other number shortcuts in Simple mode** → each opens the row shown at that position, never a hidden screen. Test in Task 3: `number_shortcuts_follow_the_visible_rows`.

---

### Task 1: The setting, its start-up default, and switching every module on (Rust)

**Files:**
- Modify: `src-tauri/src/app_settings.rs`. Add the fields `app_simple_mode: Option<bool>` and `switch_markers_migrated: bool`, both `#[serde(default)]`.
- Create: `src-tauri/src/simple_mode.rs`, for the pure decisions and the switch-on. Register it in `lib.rs`.
- Modify: `src-tauri/src/commands/app_settings.rs`. Add the commands `set_app_simple_mode(on: bool) -> Result<SimpleModeAnswer, String>` and `set_switch_markers_migrated() -> Result<AppSettings, String>`.
- Modify: `src-tauri/src/commands/modules.rs`. Add a flag so a switch that is not the person's own choice does not set `api_templates_set`.
- Modify: `src-tauri/src/lib.rs`, the start-up settle. Decide whether this is a new install BEFORE `cache::init` writes anything. Settle `app_simple_mode` once, and apply the switch-on for a new install.
- Modify: `src-tauri/src/ai_bridge/mod.rs` (`BridgeContext`, `/current-pbi`) and `src-tauri/src/commands/ai_bridge.rs` (`BridgeSwitches`). Carry `app_simple_mode` the way `autorun_mode` is carried.
- Test: `src-tauri/tests/suite/simple_mode.rs` (plus its `mod` line), and the existing `current_pbi` suite.

**Interfaces:**
- Produces:
  - `pub fn settle_simple_mode(saved: Option<bool>, new_install: bool) -> Option<bool>`: returns `Some(new_install)` when `saved` is `None`, and otherwise `saved` unchanged.
  - `pub fn modules_to_switch_on(on: &BTreeSet<Module>) -> Vec<Module>`: the modules missing from `on`, in tree order (parents first), never including the held-back module.
  - `#[derive(Serialize, specta::Type)] pub struct SimpleModeAnswer { pub settings: AppSettings, pub not_switched_on: Vec<String> }`. `not_switched_on` holds the module names (`Module::name`) that failed to switch on.
  - `set_app_simple_mode(true)` saves `Some(true)`, then switches each module in `modules_to_switch_on` on through the shared module path (with the not-a-person flag, so `api_templates_set` stays as it was). It also applies the Auto Run recommended features through `settle_switch` as today. A module save that fails is listed in `not_switched_on`, and the setting stays saved.
  - `set_app_simple_mode(false)` saves `Some(false)` and changes no module.
  - The start-up settle for a new install sets `Some(true)`, applies the same switch-on, and sets `switch_markers_migrated = true` (there is nothing to migrate). An existing install gets `Some(false)` and `switch_markers_migrated = false`.
  - The `BridgeContext` field `app_simple_mode: Option<bool>` is carried in `/current-pbi` when known, in the same way as `autorun_mode`. No tool writes it.

- [ ] **Step 1: Write the failing tests:**
  - `settle_simple_mode`, for all four input combinations.
  - `modules_to_switch_on`:
    - an empty set gives `[DbAccess, WritingStyle, AutoRun, ApiTemplates, Playwright]`, in parent-first order and without the held-back module;
    - a set with everything on gives `[]`.
  - Switching on against a temp data dir (the testable half, which takes the dir and settings, like `modules_set_with`):
    - every module comes on;
    - `api_templates_set` stays false;
    - the held-back module stays off even when the machine is unlocked;
    - Auto Run's `autorun_mode` becomes Simple on a machine with no Auto Run work.
  - `set_app_simple_mode(false)` after on leaves the modules on.
  - The start-up settle in a temp dir:
    - an empty dir gives `Some(true)`, `switch_markers_migrated = true`, and the modules on;
    - a dir with an existing `modules.json` or `app-settings.json` gives `Some(false)`, `switch_markers_migrated = false`, and the modules unchanged;
    - a saved value is never overwritten.
  - `/current-pbi` carries `app_simple_mode` when it has been pushed, and omits it when it is null.
- [ ] **Step 2: Run the tests to confirm they fail:** `cd src-tauri && cargo test --test suite simple_mode::`, then `cargo test --test suite current_pbi::`.
- [ ] **Step 3: Implement**, then regenerate the bindings: `cargo test --test bindings`.
- [ ] **Step 4: Run the tests to confirm they pass:** the same two commands, plus `modules::`, `app_settings::` and `autorun_recommended::`.
- [ ] **Step 5: Commit** with the message `feat(v2): app Simple mode setting, its start-up default and every module on`.

### Task 2: Per-environment switches — markers, the one-time migration, and "on while Simple" (TS)

**Files:**
- Modify: `src/lib/apiTemplates.ts`. `saveApiWrites(on)` writes the person-set marker `tcm-v2-api-writes-set:<envId>` = "1", the same pattern as `networkTools.ts`.
- Create: `src/lib/simpleModeSwitches.ts`.
- Modify: the app shell where environments and settings are already read (`src/app/` effects next to `useBridgeContextSync`). Mount `useSimpleModeSwitches()`.
- Test: `tests/lib/simpleModeSwitches.test.ts`, `tests/lib/apiTemplates.test.ts` (create it or extend it).

**Interfaces:**
- Consumes (Task 1): `AppSettings.app_simple_mode`, `AppSettings.switch_markers_migrated`, `commands.setSwitchMarkersMigrated()`.
- Produces:
  - `markAllAsSet(envIds: string[]): void`: writes both markers (API writes and Network tools) for each id, without changing the switches.
  - `turnOnUnsetSwitches(envIds: string[]): { apiWrites: number; networkTools: number }`: switches on both switches for each id that has no marker for that switch. It writes no marker, and returns how many it changed.
  - `useSimpleModeSwitches(): void`:
    - **Migration first:** when the settings have loaded and `switch_markers_migrated === false`, it calls `markAllAsSet(current env ids)` and then `commands.setSwitchMarkersMigrated()`, before anything else.
    - **Then:** whenever `app_simple_mode === true` and the environments list changes (or the mode turns on), it calls `turnOnUnsetSwitches(env ids)`.
    - **Shared cache:** it reuses the existing environments and settings query keys. No new cache.

- [ ] **Step 1: Write the failing tests:**
  - `saveApiWrites(false)` writes the marker.
  - `markAllAsSet` writes both markers and leaves the switches alone.
  - `turnOnUnsetSwitches` changes only environments without a marker, and returns the counts.
  - `existing_environments_keep_their_switches_after_turning_simple_on`:
    - settings `{ switch_markers_migrated: false, app_simple_mode: false }` with environments A and B both off → the hook migrates, calling `setSwitchMarkersMigrated` once;
    - then the mode turns on → A and B stay off;
    - then environment C appears → C gets both switches on.
  - A new install (`switch_markers_migrated: true, app_simple_mode: true`) with only Default → Default gets both switches on.
  - Simple mode off → a new environment gets nothing switched on.
  - A person switches C's API writes off while Simple mode is on → it stays off after the next list change.
- [ ] **Step 2: Run them to confirm they fail:** `npx vitest run tests/lib/simpleModeSwitches.test.ts tests/lib/apiTemplates.test.ts`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them to confirm they pass**, then run `npx tsc --noEmit`.
- [ ] **Step 5: Commit** with the message `feat(v2): Simple mode keeps the API write switch and the Network tools on for new environments, never for ones a person set`.

### Task 3: What is hidden — sidebar, command list, shortcuts, redirect, Settings card, Modules links, PBI notices (TS)

**Files:**
- Modify: `src/lib/modules.ts`. Add `SIMPLE_HIDDEN_SECTIONS`, extend `sectionShown`, and add a Simple-aware leave rule.
- Create: `src/lib/appSimpleMode.ts`. This is the hook and store for the setting, reading the existing app-settings query.
- Modify: `src/components/Sidebar.tsx` (the rows and `shortcutOrder`), `src/components/CommandPalette.tsx`, `src/app/useShellEffects.ts` (`useModuleGuards`), `src/screens/Settings.tsx` (the Modules card and `startOnModules`), `src/app/CaseScreens.tsx` (`onOpenModules`), and `src/App.tsx` (the PBI notice target, ~491).
- Test: `tests/lib/modules.test.ts`, `tests/components/Sidebar.test.tsx`, `tests/components/CommandPalette.test.tsx`, `tests/app/useShellEffects.test.tsx` (or wherever the guard is tested today; Grep for it), `tests/screens/Settings.test.tsx`, and an App-level test for the PBI notice (Grep for the existing test of `openPbiById(..., "import")`).

**Interfaces:**
- Consumes (Task 1): `AppSettings.app_simple_mode`.
- Produces:
  - `export const SIMPLE_HIDDEN_SECTIONS: ReadonlySet<string> = new Set(["import", "edit", "suites", "manage", "apitemplates"])`.
  - `sectionShown(section, modules = modulesSnapshot(), simple = appSimpleModeSnapshot()): boolean` returns false when `simple && SIMPLE_HIDDEN_SECTIONS.has(section)`; otherwise it returns today's result.
  - `leaveTarget(section, shown, hydrated, simple): Section | null` returns:
    - `"autorun"` when the section is hidden by Simple mode (hydrated, Simple on and hidden);
    - `"manual"` where today's `shouldLeaveHidden` fires;
    - null otherwise.
    
    `useModuleGuards` uses it.
  - `useAppSimpleMode(): { on: boolean; loading: boolean }`. While loading it reads as off (show everything), the same rule the Auto Run mode follows.
  - **PBI notices:** in Simple mode they call `openPbiById(id, project, "autorun")`.
  - **Modules entry points:** in Simple mode, the Settings "Modules" card is not rendered, and `onOpenModules` is not offered. The Auto Run `PlaywrightNeededDialog` hides its "open Modules" button (Playwright is on in Simple mode anyway).

- [ ] **Step 1: Write the failing tests:**
  - `sectionShown`: for each of the five sections, hidden when Simple is on and shown when off. Every other section is unchanged in both states.
  - `leaveTarget`: each branch.
  - The sidebar in Simple mode:
    - none of the five rows;
    - Manual Entry, View, Run Tests, AI Bridge, Auto Run and Playwright are present.
  - `number_shortcuts_follow_the_visible_rows`: `shortcutOrder` in Simple mode lists only the visible rows, in order.
  - The command list in Simple mode offers no entry that opens the five.
  - `a_remembered_hidden_screen_opens_auto_run`: a section state of "import" with Simple on and modules hydrated → `setSection("autorun")`.
  - The Settings page in Simple mode has no Modules card, and `startOnModules` does not open the Modules page.
  - A PBI notice in Simple mode opens Auto Run on that PBI.
  - With Simple mode off, each of the above behaves as today. Keep or add one assertion per surface.
- [ ] **Step 2: Run them to confirm they fail.** Use the touched test files, one at a time.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them to confirm they pass**, plus `npx tsc --noEmit` and `npx vitest run tests/ui-consistency.test.ts`.
- [ ] **Step 5: Commit** with the message `feat(v2): Simple mode hides Import, Update, Search Suites, Suite Management, API Templates and Modules`.

### Task 4: The Settings toggle and the tour (TS)

**Files:**
- Modify: `src/screens/Settings.tsx`. Add the toggle row on the main settings page, following the page's existing switch rows.
- Modify: `src/tour/tourScript.ts` (`tourStepsFor`) and the tour's caller (`src/app/useAppTour.ts`).
- Test: `tests/screens/Settings.test.tsx`, `tests/tour/tourScript.test.ts` (or wherever the tour is tested; Grep for it).

**Interfaces:**
- Consumes: `commands.setAppSimpleMode(on)` → `SimpleModeAnswer` (Task 1); `useAppSimpleMode()` and `SIMPLE_HIDDEN_SECTIONS` (Task 3).
- Produces:
  - **Toggle row:** titled "Simple mode", with the exact line from Global Constraints.
    - Turning it on or off calls `setAppSimpleMode` and writes the returned settings into the app-settings query.
    - When `not_switched_on` is non-empty, the row shows the exact failure line with the names joined by ", ".
    - The switch is disabled while an unattended Auto Run run is going (use the signal the Auto Run screen uses for `runGoing`), with the exact title. Use `disabled:pointer-events-auto` so the title shows.
  - **Tour:** `tourStepsFor(modules, simple = false)` also drops the stops whose `where` is `cases(s)` for an `s` in `SIMPLE_HIDDEN_SECTIONS`. The caller passes the current Simple state.

- [ ] **Step 1: Write the failing tests:**
  - The toggle:
    - it calls `set_app_simple_mode(true)`;
    - the failure line appears with names;
    - it is disabled with the title during a run;
    - turning it off calls `set_app_simple_mode(false)`.
  - `tourStepsFor(allModules, true)` contains no stop at `cases("import"|"edit"|"suites"|"manage")`.
  - `tourStepsFor(allModules, false)` equals today's list.
  - Every remaining stop's destination is a visible section.
- [ ] **Step 2: Run them to confirm they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them to confirm they pass**, plus `npx tsc --noEmit` and the ui-consistency test.
- [ ] **Step 5: Commit** with the message `feat(v2): the Simple mode toggle in Settings; the first-run tour skips hidden screens`.

### Task 5: Assistants, How To Use, and full verification

**Files:**
- Modify: `src-tauri/src/autorun/guide.rs` and `src-tauri/src/api_templates/guide.rs`, plus any other guide text that sends a person to API Templates, Import, Update, Search Suites, Suite Management or Modules. Grep the guide sources for those names.
- Modify: `docs-site/src/content/settings.ts` (a "Simple mode" section), `getting-started.ts` (name the mode; shortcut text says the numbers follow the sidebar order and gives the Simple-mode numbers), and the sections for each hidden screen (import-file, update-test-cases, search-suites, suite-management, api-templates, and the Settings Modules part). Each gets the exact "Hidden while Simple mode is on (Settings)." line.
- Test: the guide test modules (`autorun_guide`, the API Templates guide test), the docs-site tests, and a Rust test that no tool name or route sets `app_simple_mode`.

- [ ] **Step 1: Write the failing tests:**
  - Each guide that names a hidden screen contains: "If the app is in Simple mode, do that part yourself or ask the person to turn Simple mode off in Settings." Check it whitespace-normalised.
  - The docs content carries the hidden line on each listed section, and a Settings "Simple mode" section exists.
  - No bridge route or MCP tool writes `app_simple_mode`. Scan the route table and tool list the way the existing guards do.
- [ ] **Step 2: Run them to confirm they fail.**
- [ ] **Step 3: Implement.** Then the shots:
  1. Check that port 1420 is free. If the owner's `tauri dev` is running, skip the shots and report it.
  2. Run `npm run docs:dev`, then `npm run docs:shots -- --dry-run`, then `npm run docs:shots -- --only <settings-main and any shot whose sidebar or Settings changed in Simple mode>`.
  3. Run `npm run docs:build`.
  4. Stop the docs app by its PID or port.
  
  Shots stay in today's Off state unless a shot is about Simple mode. Add one new shot, `settings-simple-mode`, showing the toggle on.
- [ ] **Step 4: Full verification**, one command at a time:
  1. `npx tsc --noEmit`
  2. `npm test`
  3. `cd src-tauri && cargo test --tests -- --skip autorun_recorder::`
  4. `cargo test --test suite autorun_recorder::`
  5. `cargo test --test bindings`
  
  Report the counts. A single failure that passes when re-run alone is reported as a flake.
- [ ] **Step 5: Commit** with the message `docs(v2): Simple mode in the guides and How To Use`. Commit the shots, positions and built site with it.
