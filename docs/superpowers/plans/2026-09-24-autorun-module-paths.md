# Auto Run: Module Paths, No Direct Addresses and an Account for the Run Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An unattended run of a module's cases goes the way a tester goes (sign in, click through the menu to the case's module, then run the steps), the menu path is recorded by clicking in a real browser, a per-project switch stops scripts from opening pages by address, and a run can sign in as one chosen account for scripts that name none.

**Architecture:** A new per-project file `projects/<slug>.nav.json` (module `autorun/nav.rs`) holds recorded paths and the `direct_urls` switch, and owns every decision about them as pure functions (matching, the blocked sentences, where a page is, the address rule, the guide section). The replay engine gains a module step between sign-in and step 1 (`MODULE_STEP = -1`) that reuses the runner's own `click` through `execute_in`; `run_step` gains a routed variant so a mid-script `sign_in` goes back to the module. A recorder (`autorun/recorder.rs`) turns clicks reported by a CDP binding into locators (accessibility role and name first, on-the-spot hints second), and its commands (`commands/autorun_record.rs`) save a path only after it has replayed in a fresh signed-in browser. The frontend gets a Module paths dialog and a Sign in as select on the unattended run panel.

**Tech Stack:** Rust (Tauri 2, tauri-specta commands and events, tokio, serde_json), Chrome DevTools Protocol (`Runtime.addBinding`, `Runtime.bindingCalled`, `Page.addScriptToEvaluateOnNewDocument`, `Accessibility.getPartialAXTree`), React 19 + TypeScript, TanStack Query, Tailwind 4 tokens, vitest.

**Spec:** `docs/superpowers/specs/2026-09-24-autorun-module-paths-design.md` (binding; read it before any task).

**Decisions this plan makes that the spec leaves open (a reviewer should check them):**
- Recorded role and text locators carry `"exact": true`. A contains-match on `link "Leave"` would also match `link "Apply Leave"` in the same menu, and the runner's click refuses two matches. The saved JSON therefore has `"exact": true` beside each `role`/`name`, which the spec's example omits.
- The run-time module line is a `StepRecord` with `step_number: -1` (`replay::MODULE_STEP`); the review labels it "Module" and its outcome reads `Go to Leave`.
- A `sign_in` in the middle of a script keeps one outcome per action: its outcome becomes `"<sign-in detail>; then Go to Leave"` (or the "Could not reach module" sentence, failed), and the rest of that step is not run when the module is not reached.
- The navigate refusal message on save is prefixed with the case: `case 7: this project does not allow opening pages by address: ... (step 2).` The spec's sentence is kept whole after the prefix, so a bundle says which case it means.
- A module path whose home page did not load says `the home page did not load` (not the navigate's own words, which name the address).

## Global Constraints

- **Branch:** `feat/autorun-module-paths`. Stay on it. No release, no version bump, no `changelog.ts` entry in this plan.
- **Rust tests live only in `src-tauri/tests/`.** Never a `#[cfg(test)]` module inside `src/`. Shared fakes are in `src-tauri/tests/common/mod.rs` (`ScriptedDriver`, `FakePage`, `stateful_app`, and after Task 2 `menu_app`); reuse them, never copy one.
- **One build or test command at a time** (shared machine). Before ANY cargo command, run the dev-app check with the PowerShell tool: `Get-Process v2, cargo -ErrorAction SilentlyContinue | Select-Object Name, Path`. If a `cargo` is listed, wait for it. Never kill a `v2` process; if one's `Path` is under `target\gate`, ask the controller.
- **Rust commands** run from `src-tauri/` as `CARGO_TARGET_DIR=target/gate cargo test --test <name>` (Bash tool). **Frontend:** `npx vitest run --exclude "**/.claude/**" <file>` and `npx tsc --noEmit` from the repo root. Use the Grep and Read tools for searching (bash `grep` hangs here).
- **`src/bindings.ts` is generated** by `CARGO_TARGET_DIR=target/gate cargo test --test bindings`; never hand-edit it. If it shows as modified but `git diff --ignore-all-space --ignore-cr-at-eol -- src/bindings.ts` is empty, run `git checkout -- src/bindings.ts` and do not commit it.
- **No new crates and no new npm packages.**
- **No HTTP DELETE to Azure DevOps**, and nothing in this plan calls Azure DevOps at all. Nav files, recordings and paths stay on this machine.
- **User-facing errors name no URL** except where the spec's own sentence does. None of the new sentences does; paths such as `/hr/leave/apply` are allowed (the spec shows them).
- **The spec's sentences, verbatim** (only `X`, `N`, `<locator>`, `<reason>` vary):
  - `This case has no Module - set one in Azure DevOps, or record a path for it.`
  - `No menu path recorded for module "X" - record one in Auto Run, Module paths.`
  - `Choose an account when starting the run, or set Runs as on the script.`
  - `Could not reach module "X": click N, <locator> - <reason>.`
  - `this project does not allow opening pages by address: a run starts on the case's module screen - use clicks instead of "navigate" (step N).`
  - Dialog failure form: `click N, <locator>: <reason>` (spec example: `click 2, link "Apply Leave": no visible match`).
- **Stored file:** `<autorun root>/projects/<slug>.nav.json`, beside the recipe's `<slug>.json`, slug from `recipe::project_slug`. `direct_urls` absent means `true`. No file, or an empty `modules` list, runs exactly as today.
- **UI words:** switch "Scripts may open pages by address"; select "Sign in as" with default option "Each script's own account"; buttons "Module paths", "Record a module…", "Re-record", "Try", "Remove" (Remove asks first).
- **UI conventions:** dialogs are the shared `Modal`; dropdowns are the themed `Select` (or the shared `Combobox` where free text is allowed); switches are the shared `Switch`; button icons come from `src/lib/actionIcons.ts` as `<IconX aria-hidden />`; colours are theme tokens only; `src/ui-consistency.test.ts` is never weakened; no em dashes in any text a user or an assistant reads.
- **Auto Run** stays a tab whose tools are shown in development builds. No new assistant tool (spec §8).
- **Passwords** never appear in a log, an event, a run file, a nav file or a recorder event; recorder events carry locator words only.
- **Commits:** Bash heredoc, `git commit -q -F - <<'EOF' ... EOF`, confirm with `git log -1`. The trailer names the model that writes the commit (the plan shows `Claude Opus 5.5`; write your own model's name if it differs). Keep each edited file's existing line endings (they are CRLF).

## Review Focus

1. **The person closes the recording browser window instead of pressing Stop.** Expected: the recording ends, the dialog says nothing was saved, and the recorder slot is freed so a new recording or a run can start. Tests: Task 5 `a_closed_recording_browser_ends_the_recording_and_saves_nothing`, Task 7 "closing the recording browser ends the recording and frees it".
2. **A module is recorded again, or recorded as " leave " when "Leave" exists.** Expected: the new path replaces the old one; it is neither refused as a duplicate nor kept twice. Test: Task 1 `recording_a_module_again_replaces_its_path_whatever_its_case`.
3. **The account picked for runs was removed from the Accounts list since.** Expected: the select shows "Each script's own account" and the run starts with no run account, silently, instead of being refused. Test: Task 3 "an account removed since it was picked falls back to each script's own, silently".
4. **The nav file was hand-edited into something unreadable.** Expected: the run refuses before any browser opens and says the module paths file is not readable, rather than running every case from the home page on a guess. Test: Task 2 `an_unreadable_module_paths_file_stops_the_run_before_any_browser_opens`.
5. **The application changes its address a moment after the last menu click (routes asynchronously).** Expected: the arrival check keeps looking for up to the navigation time and passes. Test: Task 2 `an_address_that_changes_a_moment_after_the_last_click_still_counts`.

---

## File Structure

| File | Responsibility |
| --- | --- |
| `src-tauri/src/autorun/nav.rs` (create) | The nav file: types, load/save beside the recipe, duplicate refusal, module matching, the §5 blocked sentences, `path_of`/`same_page`, the dialog view; later the route (`go_home`, `go_to_module`, `check_path`), the address rule and the guide section. |
| `src-tauri/src/autorun/recorder.rs` (create) | The click listener JS, `arm`, `next_click`, `locate` (AX tree, then hints), the `capture` loop and `finish`. Pure locator rules are plain functions. |
| `src-tauri/src/autorun/replay.rs` (replace) | `CaseToRun`, `run_cases` (module and run account per case, blocked-before-start rows), `run_case_as` (sign-in, module step, steps), `MODULE_STEP`, `propose` blocking on route problems. `run_selection` and `run_case` stay as thin wrappers. |
| `src-tauri/src/autorun/runner.rs` (replace) | `run_step_routed` (a `sign_in` goes back to the module), later the saved-`navigate` refusal. `run_step` delegates with no route. |
| `src-tauri/src/autorun/failures.rs` (modify) | Module line and stop reason for an assistant's failure report. |
| `src-tauri/src/autorun/accounts.rs` (modify) | `account_for_run`. |
| `src-tauri/src/autorun/mod.rs` (modify) | `pub mod nav; pub mod recorder;` |
| `src-tauri/src/commands/autorun.rs` (modify) | Nav commands; save/import take organization and project and apply the address rule; the supervised browser refuses while recording. |
| `src-tauri/src/commands/autorun_replay.rs` (modify) | `ReplayCase.module`, `auto_run_replay(account)`, `open_real`, refusal while recording. |
| `src-tauri/src/commands/autorun_record.rs` (create) | Record start/stop/cancel, Try, the one-at-a-time claim and the exclusion checks. |
| `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/events.rs` (modify) | Registration; `RecordingEvent`; the `module` phase in `ReplayProgress`'s doc. |
| `src-tauri/src/ai_bridge.rs` (modify) | Address rule on `/autorun-script`; guide section on `/autorun-guide`. |
| `src-tauri/tests/autorun_nav.rs`, `autorun_recorder.rs` (create) | Tests for the two new modules and the recorder commands' exclusion. |
| `src-tauri/tests/common/mod.rs`, `autorun_replay.rs`, `autorun_runner.rs`, `autorun_failures.rs`, `autorun_accounts.rs`, `autorun_commands.rs`, `autorun_bridge.rs`, `browser_live.rs` (modify) | Fakes and tests. |
| `src/screens/AutoRun/ModulePathsDialog.tsx` (create) + test | The list, Re-record/Try/Remove, the switch, the recording flow. |
| `src/screens/AutoRun/ReplayPane.tsx` (replace) + test | Sign in as, remembered per org/project; module per case; the `module` phase. |
| `src/screens/AutoRun/RunReview.tsx` + test | `stepLabel` ("Sign in", "Module", "Step N"). |
| `src/screens/AutoRun/ScriptEditor.tsx` + test, `index.tsx` + test, `src/screens/AutoRun.test.tsx` | New command arguments; the Module paths button. |
| `src/lib/actionIcons.ts` (modify) | `IconModulePaths`. |
| `src/bindings.ts` (generated) | Regenerated in Tasks 1, 3, 4 and 6. |

## Tasks

1. The nav file
2. The module step in a run
3. An account for the run (Rust and the Sign in as select)
4. Scripts may not open pages by address
5. The recorder core
6. The recorder commands and a real-browser test
7. The Module paths dialog

---

### Task 1: The nav file

**Files:**
- Create: `src-tauri/src/autorun/nav.rs`
- Modify: `src-tauri/src/autorun/mod.rs` (add `pub mod nav;` between `pub mod guide;` and `pub mod publish;`)
- Modify: `src-tauri/src/commands/autorun.rs` (three commands after `auto_run_save_quirks`)
- Modify: `src-tauri/src/lib.rs` (register them after `autorun::auto_run_save_quirks,`)
- Test: `src-tauri/tests/autorun_nav.rs` (create)
- Generated: `src/bindings.ts`

**Interfaces:**
- Consumes: `recipe::{origin_of, project_slug}`, `browser::locator::Target`.
- Produces (in `v2_lib::autorun::nav`):
  - `pub struct ModulePath { pub module: String, pub clicks: Vec<Target>, pub arrived: String, pub recorded: String }` (serde)
  - `pub struct NavFile { pub direct_urls: bool, pub modules: Vec<ModulePath> }` (serde; `Default` is `direct_urls: true`, no modules)
  - `pub struct ModuleView { pub module: String, pub clicks: Vec<String>, pub arrived: String, pub recorded: String }` and `pub struct NavView { pub direct_urls: bool, pub modules: Vec<ModuleView> }` (specta, Serialize)
  - `pub fn view(&NavFile) -> NavView`
  - `pub const NO_MODULE: &str`, `pub const NO_ACCOUNT: &str`, `pub fn no_path(module: &str) -> String`
  - `pub fn module_key(&str) -> String`, `pub fn nav_path(root: &Path, org: &str, project: &str) -> PathBuf`
  - `pub fn load_nav(root, org, project) -> Result<NavFile, String>`, `pub fn validate(&NavFile) -> Result<(), String>`, `pub fn save_nav(root, org, project, &NavFile) -> Result<(), String>`
  - `pub fn find_path<'a>(&'a NavFile, module: &str) -> Option<&'a ModulePath>`
  - `pub fn put_path(root, org, project, ModulePath) -> Result<NavFile, String>`, `pub fn remove_path(root, org, project, module: &str) -> Result<NavFile, String>`, `pub fn set_direct_urls(root, org, project, allowed: bool) -> Result<NavFile, String>`
  - `pub fn route_for<'a>(&'a NavFile, module: Option<&str>, account: Option<&str>) -> Result<Option<&'a ModulePath>, String>`
  - `pub fn path_of(href: &str) -> String`, `pub fn same_page(href: &str, start_url: &str) -> bool`
- Produces (commands; TypeScript names in brackets): `auto_run_load_nav(organization, project) -> Result<NavView, String>` [`autoRunLoadNav`], `auto_run_set_direct_urls(organization, project, allowed) -> Result<NavView, String>` [`autoRunSetDirectUrls`], `auto_run_remove_module_path(organization, project, module) -> Result<NavView, String>` [`autoRunRemoveModulePath`].

- [ ] **Step 1: Write the failing tests** in `src-tauri/tests/autorun_nav.rs`:

```rust
//! Module paths: the per-project file an unattended run reads to get from
//! the home page to a case's module screen, and the rules around it.

use serde_json::json;
use v2_lib::autorun::nav::{
    find_path, load_nav, module_key, nav_path, no_path, path_of, put_path, remove_path, route_for, same_page,
    save_nav, set_direct_urls, view, ModulePath, NavFile, NO_ACCOUNT, NO_MODULE,
};
use v2_lib::autorun::recipe::project_slug;

fn path(module: &str, arrived: &str) -> ModulePath {
    serde_json::from_value(json!({
        "module": module,
        "clicks": [
            { "role": "link", "name": "Leave", "exact": true },
            { "role": "link", "name": "Apply Leave", "exact": true }
        ],
        "arrived": arrived,
        "recorded": "2026-09-24T10:00:00Z"
    }))
    .unwrap()
}

fn with(modules: Vec<ModulePath>) -> NavFile {
    NavFile { direct_urls: true, modules }
}

#[test]
fn the_blocked_sentences_are_the_designs_own_words() {
    assert_eq!(NO_MODULE, "This case has no Module - set one in Azure DevOps, or record a path for it.");
    assert_eq!(NO_ACCOUNT, "Choose an account when starting the run, or set Runs as on the script.");
    assert_eq!(
        no_path(" Payroll "),
        "No menu path recorded for module \"Payroll\" - record one in Auto Run, Module paths."
    );
}

#[test]
fn no_file_and_a_file_without_the_switch_both_allow_addresses() {
    let dir = tempfile::tempdir().unwrap();
    let nav = load_nav(dir.path(), "Acme", "Web").unwrap();
    assert!(nav.direct_urls);
    assert!(nav.modules.is_empty());
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    std::fs::write(nav_path(dir.path(), "Acme", "Web"), "\u{feff}{ \"modules\": [] }").unwrap();
    assert!(load_nav(dir.path(), "Acme", "Web").unwrap().direct_urls, "absent means true");
}

#[test]
fn the_file_sits_beside_the_sign_in_recipe_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let nav = NavFile { direct_urls: false, modules: vec![path("Leave", "/hr/leave/apply")] };
    save_nav(dir.path(), "Acme", "Web", &nav).unwrap();
    let expected = dir.path().join("projects").join(format!("{}.nav.json", project_slug("Acme", "Web")));
    assert_eq!(nav_path(dir.path(), "Acme", "Web"), expected);
    assert!(expected.is_file());
    assert!(!dir.path().join("projects").join(format!("{}.nav.json.tmp", project_slug("Acme", "Web"))).exists());
    assert_eq!(load_nav(dir.path(), "Acme", "Web").unwrap(), nav);
}

#[test]
fn an_unreadable_file_says_so() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    std::fs::write(nav_path(dir.path(), "Acme", "Web"), "{ not json").unwrap();
    let err = load_nav(dir.path(), "Acme", "Web").unwrap_err();
    assert!(err.starts_with("the module paths file is not readable"), "{err}");
}

#[test]
fn two_paths_for_the_same_module_are_refused_and_nothing_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let err = save_nav(dir.path(), "Acme", "Web", &with(vec![path("Leave", "/a"), path("  leave ", "/b")])).unwrap_err();
    assert_eq!(err, "module \"leave\" has two paths - keep one");
    assert!(!nav_path(dir.path(), "Acme", "Web").exists());
}

#[test]
fn a_path_needs_a_name_clicks_and_an_address_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut no_clicks = path("Leave", "/hr/leave/apply");
    no_clicks.clicks.clear();
    assert_eq!(
        save_nav(dir.path(), "Acme", "Web", &with(vec![no_clicks])).unwrap_err(),
        "module \"Leave\" has no clicks - record it again"
    );
    assert!(save_nav(dir.path(), "Acme", "Web", &with(vec![path("Leave", "hr/leave")])).is_err());
    assert!(save_nav(dir.path(), "Acme", "Web", &with(vec![path("  ", "/x")])).is_err());
    assert_eq!(
        save_nav(dir.path(), " ", "Web", &with(vec![])).unwrap_err(),
        "pick an organization and a project first"
    );
}

#[test]
fn a_module_matches_trimmed_and_ignoring_case() {
    let nav = with(vec![path("Leave", "/hr/leave/apply")]);
    assert!(find_path(&nav, "  LEAVE ").is_some());
    assert!(find_path(&nav, "Leav").is_none());
    assert!(find_path(&nav, "   ").is_none());
    assert_eq!(module_key(" Apply Leave "), "apply leave");
}

/// Review focus 2: Re-record, or the same module typed in another case,
/// replaces the path rather than being refused or kept twice.
#[test]
fn recording_a_module_again_replaces_its_path_whatever_its_case() {
    let dir = tempfile::tempdir().unwrap();
    put_path(dir.path(), "Acme", "Web", path("Leave", "/hr/leave/old")).unwrap();
    let nav = put_path(dir.path(), "Acme", "Web", path(" leave ", "/hr/leave/apply")).unwrap();
    assert_eq!(nav.modules.len(), 1);
    assert_eq!(nav.modules[0].module, "leave");
    assert_eq!(nav.modules[0].arrived, "/hr/leave/apply");
    assert_eq!(load_nav(dir.path(), "Acme", "Web").unwrap(), nav);
}

#[test]
fn removing_a_path_and_turning_the_switch_change_only_their_own_part() {
    let dir = tempfile::tempdir().unwrap();
    put_path(dir.path(), "Acme", "Web", path("Leave", "/hr/leave/apply")).unwrap();
    put_path(dir.path(), "Acme", "Web", path("Payroll", "/hr/payroll")).unwrap();
    let nav = set_direct_urls(dir.path(), "Acme", "Web", false).unwrap();
    assert!(!nav.direct_urls);
    assert_eq!(nav.modules.len(), 2);
    let nav = remove_path(dir.path(), "Acme", "Web", "LEAVE").unwrap();
    assert!(!nav.direct_urls, "removing a path leaves the switch alone");
    assert_eq!(nav.modules.iter().map(|m| m.module.as_str()).collect::<Vec<_>>(), vec!["Payroll"]);
    let again = remove_path(dir.path(), "Acme", "Web", "Leave").unwrap();
    assert_eq!(again, nav, "removing what is not there changes nothing");
}

#[test]
fn a_project_with_no_paths_needs_neither_a_module_nor_an_account() {
    assert_eq!(route_for(&NavFile::default(), None, None), Ok(None));
    assert_eq!(route_for(&NavFile::default(), Some("Leave"), Some("hr.admin")), Ok(None));
}

#[test]
fn with_paths_a_case_needs_its_module_a_path_for_it_and_an_account_in_that_order() {
    let nav = with(vec![path("Leave", "/hr/leave/apply")]);
    assert_eq!(route_for(&nav, None, None), Err(NO_MODULE.to_string()));
    assert_eq!(route_for(&nav, Some("  "), Some("hr.admin")), Err(NO_MODULE.to_string()));
    assert_eq!(route_for(&nav, Some("Payroll"), None), Err(no_path("Payroll")));
    assert_eq!(route_for(&nav, Some("leave"), None), Err(NO_ACCOUNT.to_string()));
    assert_eq!(route_for(&nav, Some("leave"), Some(" ")), Err(NO_ACCOUNT.to_string()));
    assert_eq!(route_for(&nav, Some(" Leave "), Some("hr.admin")).unwrap().map(|p| p.arrived.as_str()), Some("/hr/leave/apply"));
}

#[test]
fn only_the_path_of_an_address_counts_as_where_a_page_is() {
    assert_eq!(path_of("https://hr.example.internal/hr/leave/apply?tab=2#top"), "/hr/leave/apply");
    assert_eq!(path_of("https://hr.example.internal"), "/");
    assert_eq!(path_of("https://hr.example.internal/"), "/");
    assert_eq!(path_of("file:///C:/app/menu.html"), "/C:/app/menu.html");
    assert_eq!(path_of("/hr/x?y=1"), "/hr/x");
    let home = "https://hr.example.internal/hr/home/index";
    assert!(same_page("https://HR.example.internal/hr/home/index?from=login", home));
    assert!(!same_page("https://hr.example.internal/hr/welcome", home));
    assert!(!same_page("https://other.example/hr/home/index", home));
}

#[test]
fn the_dialog_view_reads_every_click_in_words() {
    let v = view(&with(vec![path("Leave", "/hr/leave/apply")]));
    assert!(v.direct_urls);
    assert_eq!(v.modules[0].module, "Leave");
    assert_eq!(v.modules[0].clicks, vec!["link \"Leave\"".to_string(), "link \"Apply Leave\"".to_string()]);
    assert_eq!(v.modules[0].arrived, "/hr/leave/apply");
}
```

- [ ] **Step 2: Run to see it fail** (dev-app check first). From `src-tauri/`: `CARGO_TARGET_DIR=target/gate cargo test --test autorun_nav`. Expected: compile error, `could not find nav in autorun`.

- [ ] **Step 3: Implement.** Create `src-tauri/src/autorun/nav.rs`:

```rust
//! Module paths: how an unattended run gets from the application's home
//! page to a case's module screen, and the per-project switch that says
//! whether scripts may open pages by address.
//!
//! One file per project, beside the sign-in recipe:
//! `<autorun root>/projects/<slug>.nav.json`. It is kept on this machine
//! only and never sent to Azure DevOps. A project with no file, or with an
//! empty `modules` list, runs exactly as it did before module paths
//! existed.

use super::recipe::{origin_of, project_slug};
use crate::browser::locator::Target;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn yes() -> bool {
    true
}

/// One module's recorded way in from the home page.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModulePath {
    /// Compared with a test case's Module field, trimmed and ignoring case.
    pub module: String,
    /// In order, the locators a run clicks: the same `Target` every script
    /// click uses.
    pub clicks: Vec<Target>,
    /// The path part of the address the recording ended on, with no query
    /// or fragment: `/hr/leave/apply`.
    pub arrived: String,
    /// When it was recorded, UTC, `YYYY-MM-DDTHH:MM:SSZ`.
    pub recorded: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NavFile {
    /// Whether a script may open a page by address. Absent means yes, so a
    /// project changes nothing until someone turns it off.
    #[serde(default = "yes")]
    pub direct_urls: bool,
    #[serde(default)]
    pub modules: Vec<ModulePath>,
}

impl Default for NavFile {
    fn default() -> Self {
        NavFile { direct_urls: true, modules: vec![] }
    }
}

/// A recorded module as the Module paths dialog shows it. Every click is
/// already in words (`link "Leave"`), so the webview never keeps a second
/// copy of how a locator reads.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct ModuleView {
    pub module: String,
    pub clicks: Vec<String>,
    pub arrived: String,
    pub recorded: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct NavView {
    pub direct_urls: bool,
    pub modules: Vec<ModuleView>,
}

pub fn view(nav: &NavFile) -> NavView {
    NavView {
        direct_urls: nav.direct_urls,
        modules: nav
            .modules
            .iter()
            .map(|m| ModuleView {
                module: m.module.clone(),
                clicks: m.clicks.iter().map(Target::describe).collect(),
                arrived: m.arrived.clone(),
                recorded: m.recorded.clone(),
            })
            .collect(),
    }
}

/// Why a case in a project WITH paths is not run (design §5).
pub const NO_MODULE: &str = "This case has no Module - set one in Azure DevOps, or record a path for it.";
pub const NO_ACCOUNT: &str = "Choose an account when starting the run, or set Runs as on the script.";
const NO_PATH_START: &str = "No menu path recorded for module \"";

pub fn no_path(module: &str) -> String {
    format!("{NO_PATH_START}{}\" - record one in Auto Run, Module paths.", module.trim())
}

/// How two module names are compared: trimmed, case ignored.
pub fn module_key(module: &str) -> String {
    module.trim().to_lowercase()
}

pub fn nav_path(root: &Path, org: &str, project: &str) -> PathBuf {
    root.join("projects").join(format!("{}.nav.json", project_slug(org, project)))
}

pub fn load_nav(root: &Path, org: &str, project: &str) -> Result<NavFile, String> {
    match std::fs::read_to_string(nav_path(root, org, project)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            serde_json::from_str(s).map_err(|e| format!("the module paths file is not readable: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(NavFile::default()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn validate(nav: &NavFile) -> Result<(), String> {
    let mut seen = HashSet::new();
    for m in &nav.modules {
        let name = m.module.trim();
        if name.is_empty() {
            return Err("a module path needs the module's name".to_string());
        }
        if !seen.insert(module_key(name)) {
            return Err(format!("module \"{name}\" has two paths - keep one"));
        }
        if m.clicks.is_empty() {
            return Err(format!("module \"{name}\" has no clicks - record it again"));
        }
        for (i, click) in m.clicks.iter().enumerate() {
            click.validate().map_err(|e| format!("module \"{name}\", click {}: {e}", i + 1))?;
        }
        if !m.arrived.starts_with('/') {
            return Err(format!(
                "module \"{name}\": where it ends must be an address path such as /hr/leave/apply"
            ));
        }
    }
    Ok(())
}

/// Replaces the whole file. Validated first, then written to a temporary
/// file and renamed, so a reader never sees half a file and a refused save
/// leaves the earlier one exactly as it was.
pub fn save_nav(root: &Path, org: &str, project: &str, nav: &NavFile) -> Result<(), String> {
    validate(nav)?;
    if org.trim().is_empty() || project.trim().is_empty() {
        return Err("pick an organization and a project first".to_string());
    }
    let path = nav_path(root, org, project);
    std::fs::create_dir_all(path.parent().expect("projects folder")).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(nav).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    Ok(())
}

pub fn find_path<'a>(nav: &'a NavFile, module: &str) -> Option<&'a ModulePath> {
    let key = module_key(module);
    if key.is_empty() {
        return None;
    }
    nav.modules.iter().find(|m| module_key(&m.module) == key)
}

/// Add a path, or replace the one already recorded for the same module
/// (Re-record). The module name is stored trimmed.
pub fn put_path(root: &Path, org: &str, project: &str, mut path: ModulePath) -> Result<NavFile, String> {
    path.module = path.module.trim().to_string();
    let mut nav = load_nav(root, org, project)?;
    let key = module_key(&path.module);
    match nav.modules.iter_mut().find(|m| module_key(&m.module) == key) {
        Some(slot) => *slot = path,
        None => nav.modules.push(path),
    }
    save_nav(root, org, project, &nav)?;
    Ok(nav)
}

pub fn remove_path(root: &Path, org: &str, project: &str, module: &str) -> Result<NavFile, String> {
    let mut nav = load_nav(root, org, project)?;
    let key = module_key(module);
    nav.modules.retain(|m| module_key(&m.module) != key);
    save_nav(root, org, project, &nav)?;
    Ok(nav)
}

pub fn set_direct_urls(root: &Path, org: &str, project: &str, allowed: bool) -> Result<NavFile, String> {
    let mut nav = load_nav(root, org, project)?;
    nav.direct_urls = allowed;
    save_nav(root, org, project, &nav)?;
    Ok(nav)
}

/// Where a case should be taken after sign-in, or why it cannot run.
/// `Ok(None)`: the project has no paths, and the case runs as it always
/// has. `module` is the case's Module field; `account` is the account that
/// applies to it (the script's own, else the run's). Checked in the
/// design's order: the Module, then a path for it, then an account.
pub fn route_for<'a>(
    nav: &'a NavFile,
    module: Option<&str>,
    account: Option<&str>,
) -> Result<Option<&'a ModulePath>, String> {
    if nav.modules.is_empty() {
        return Ok(None);
    }
    let module = module.map(str::trim).unwrap_or("");
    if module.is_empty() {
        return Err(NO_MODULE.to_string());
    }
    let path = find_path(nav, module).ok_or_else(|| no_path(module))?;
    if account.map_or(true, |a| a.trim().is_empty()) {
        return Err(NO_ACCOUNT.to_string());
    }
    Ok(Some(path))
}

/// The path part of an address: no scheme or host, no query, no fragment.
/// `/` when there is nothing after the host.
pub fn path_of(href: &str) -> String {
    let no_fragment = href.split('#').next().unwrap_or("");
    let no_query = no_fragment.split('?').next().unwrap_or("");
    let rest = match no_query.find("://") {
        Some(i) => &no_query[i + 3..],
        None => no_query,
    };
    match rest.find('/') {
        Some(i) => rest[i..].to_string(),
        None => "/".to_string(),
    }
}

/// Is the page at `href` the recipe's home page? Same origin and same path;
/// a query or fragment does not make it another page.
pub fn same_page(href: &str, start_url: &str) -> bool {
    origin_of(href).is_some() && origin_of(href) == origin_of(start_url) && path_of(href) == path_of(start_url)
}
```

In `src-tauri/src/autorun/mod.rs` add `pub mod nav;` after `pub mod guide;`.

In `src-tauri/src/commands/autorun.rs`, after `auto_run_save_quirks`, add:

```rust
/// The project's module paths and its address switch, as the Module paths
/// dialog shows them. A project with no file reads as no paths, switch on.
#[tauri::command]
#[specta::specta]
pub fn auto_run_load_nav(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<crate::autorun::nav::NavView, String> {
    let nav = crate::autorun::nav::load_nav(&root(&app)?, &organization, &project)?;
    Ok(crate::autorun::nav::view(&nav))
}

/// "Scripts may open pages by address", saved the moment it is flipped.
#[tauri::command]
#[specta::specta]
pub fn auto_run_set_direct_urls(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    allowed: bool,
) -> Result<crate::autorun::nav::NavView, String> {
    let nav = crate::autorun::nav::set_direct_urls(&root(&app)?, &organization, &project, allowed)?;
    crate::applog::info(format!(
        "Auto-run: scripts may open pages by address: {}",
        if allowed { "on" } else { "off" }
    ));
    Ok(crate::autorun::nav::view(&nav))
}

/// Forget one module's path. The dialog asks first.
#[tauri::command]
#[specta::specta]
pub fn auto_run_remove_module_path(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    module: String,
) -> Result<crate::autorun::nav::NavView, String> {
    let nav = crate::autorun::nav::remove_path(&root(&app)?, &organization, &project, &module)?;
    crate::applog::info("Auto-run module path removed");
    Ok(crate::autorun::nav::view(&nav))
}
```

In `src-tauri/src/lib.rs`, in `collect_commands![`, directly after `autorun::auto_run_save_quirks,` add:

```rust
            autorun::auto_run_load_nav,
            autorun::auto_run_set_direct_urls,
            autorun::auto_run_remove_module_path,
```

- [ ] **Step 4: Run** (one at a time, dev-app check before each cargo command): `CARGO_TARGET_DIR=target/gate cargo test --test autorun_nav` (all pass), `CARGO_TARGET_DIR=target/gate cargo test --test autorun_recipe` (unchanged, pass), `CARGO_TARGET_DIR=target/gate cargo test --test bindings` (regenerates `src/bindings.ts` with `autoRunLoadNav`, `autoRunSetDirectUrls`, `autoRunRemoveModulePath`, `NavView`, `ModuleView`), then from the repo root `npx tsc --noEmit` (clean).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/autorun/nav.rs src-tauri/src/autorun/mod.rs src-tauri/src/commands/autorun.rs src-tauri/src/lib.rs src-tauri/tests/autorun_nav.rs src/bindings.ts
git commit -q -F - <<'EOF'
feat(v2): a per-project module paths file beside the sign-in recipe

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 2: The module step in a run

**Files:**
- Modify: `src-tauri/src/autorun/nav.rs` (route types and the trip to a module)
- Replace: `src-tauri/src/autorun/replay.rs`
- Replace: `src-tauri/src/autorun/runner.rs`
- Modify: `src-tauri/src/autorun/failures.rs` (`stop_reason`, `describe_step`)
- Modify: `src-tauri/src/events.rs` (`ReplayProgress` doc)
- Modify: `src-tauri/tests/common/mod.rs` (`menu_app`, `menu_recipe`, `MenuApp`)
- Test: `src-tauri/tests/autorun_replay.rs`, `src-tauri/tests/autorun_failures.rs`, `src-tauri/tests/autorun_nav.rs`

**Interfaces:**
- Consumes: Task 1's `NavFile`, `ModulePath`, `load_nav`, `route_for`, `path_of`, `same_page`, `NO_MODULE`, `NO_ACCOUNT`, `no_path`; `runner::picture`, `signin::{prepare, sign_in}`, `recipe::load_recipe`, `actions::{execute_in, failed_by, Action, ActionOutcome, Policy}`.
- Produces (in `nav`):
  - `pub struct Route { pub start_url: String, pub origins: Vec<String>, pub path: ModulePath }`, `impl Route { pub fn new(recipe: &SignInRecipe, path: ModulePath) -> Route }`
  - `pub enum Where { Home, Click { n: usize, locator: String } }`, `pub struct PathFailure { pub at: Where, pub reason: String, pub harness: bool }` with `pub fn for_run(&self, module: &str) -> String` and `pub fn for_dialog(&self) -> String`
  - `pub async fn go_home<D: Driver>(d, start_url: &str, origins: &[String], timing: &Timing) -> ActionOutcome`
  - `pub async fn go_to_module<D: Driver>(d, route: &Route, timing: &Timing) -> Result<String, PathFailure>` (Ok carries the arrived path)
  - `pub fn reached(module: &str, result: Result<String, PathFailure>) -> ActionOutcome` (passed detail `Go to <module>`)
  - `pub const UNREACHED_PREFIX: &str = "Could not reach module \""`, `pub fn is_route_problem(detail: &str) -> bool`, `pub fn is_setup_problem(reason: &str) -> bool`
- Produces (in `replay`): `pub const MODULE_STEP: i32 = -1`, `pub struct CaseToRun { pub case_id: i32, pub title: String, pub module: Option<String> }`, `pub async fn run_cases<B: Browsers>(browsers, root, organization, project, run: &mut LocalRun, cases: &[CaseToRun], run_account: Option<&str>, timing, cancel, progress) -> Result<(), String>`, `pub async fn run_case_as<D: Driver>(d, root, organization, project, script, account: Option<&str>, route: Option<&Route>, timing, cancel, on_step) -> CaseRecord`. `run_selection` and `run_case` keep their signatures.
- Produces (in `runner`): `pub async fn run_step_routed<D: Driver>(d, root, organization, project, step, timing, account: &mut Option<String>, route: Option<&Route>) -> Result<Vec<ActionOutcome>, String>`.
- Produces (progress): phase `"module"` with `step_number: -1`.
- Produces (tests/common): `pub fn menu_recipe() -> SignInRecipe`, `pub struct MenuApp { pub log: Arc<Mutex<Vec<String>>>, pub path: Arc<Mutex<String>> }`, `pub fn menu_app(entries: &[(&'static str, &'static str, &'static str)], landing: &'static str, lag: usize) -> (ScriptedDriver, MenuApp)`.

- [ ] **Step 1: Add the fake.** In `src-tauri/tests/common/mod.rs`, change `use std::sync::Arc;` to `use std::sync::{Arc, Mutex};` (the `PROBE_JS`, `HAS_FOCUS_JS`, `VISIBLE_JS`, `HIGHLIGHT_JS` and `CHECK_TEXT_JS` imports it needs are already there), and append:

```rust
/// The sign-in recipe `menu_app` answers to: one css click, then a marker
/// that is always there. Its home page is the HR application's.
pub fn menu_recipe() -> SignInRecipe {
    serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/hr/home/index",
        "steps": [ { "kind": "click", "selector": { "css": "#go" } } ],
        "signed_in": { "css": "#marker" }
    }))
    .unwrap()
}

/// What `menu_app` saw, in order: `navigate <path>`, `click <name or
/// css>`, `check <value>`; and where its page is now.
pub struct MenuApp {
    pub log: Arc<Mutex<Vec<String>>>,
    pub path: Arc<Mutex<String>>,
}

/// An application with a click-only menu. Each entry is (role, accessible
/// name, the path a click on it lands on). Every css locator is found, so
/// `menu_recipe` always signs in; its `#go` click lands on `landing`. A
/// menu click's new path shows only after `lag` more reads of the address,
/// the way an application that routes asynchronously behaves. `check_text`
/// passes for the value "yes" only.
pub fn menu_app(
    entries: &[(&'static str, &'static str, &'static str)],
    landing: &'static str,
    lag: usize,
) -> (ScriptedDriver, MenuApp) {
    let app = MenuApp { log: Arc::new(Mutex::new(vec![])), path: Arc::new(Mutex::new("/".to_string())) };
    let (log, path) = (app.log.clone(), app.path.clone());
    let entries: Vec<(String, String, String)> =
        entries.iter().map(|(r, n, p)| (r.to_string(), n.to_string(), p.to_string())).collect();
    let mut last_css = String::new();
    let mut last_probed = String::new();
    let mut pending: Option<(String, usize)> = None;
    let mut d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        Ok(match method {
            "Page.navigate" => {
                let p = v2_lib::autorun::nav::path_of(params["url"].as_str().unwrap_or(""));
                log.lock().unwrap().push(format!("navigate {p}"));
                *path.lock().unwrap() = p;
                pending = None;
                json!({ "frameId": "F", "loaderId": "L" })
            }
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" if params["expression"] == "location.href" => {
                if let Some((dest, left)) = pending.take() {
                    if left == 0 {
                        *path.lock().unwrap() = dest;
                    } else {
                        pending = Some((dest, left - 1));
                    }
                }
                json!({ "result": { "value": format!("https://hr.example.internal{}", path.lock().unwrap()) } })
            }
            "Runtime.evaluate" => json!({ "result": { "value": null } }),
            "Accessibility.queryAXTree" => {
                let role = params["role"].as_str().unwrap_or("");
                let nodes: Vec<Value> = entries
                    .iter()
                    .enumerate()
                    .filter(|(_, (r, _, _))| r == role)
                    .map(|(i, (r, n, _))| {
                        json!({ "nodeId": format!("n{i}"), "role": { "value": r }, "name": { "value": n }, "backendDOMNodeId": 100 + i })
                    })
                    .collect();
                json!({ "nodes": nodes })
            }
            "DOM.resolveNode" => json!({ "object": { "objectId": format!("ax-{}", params["backendNodeId"]) } }),
            "Runtime.callFunctionOn" if f == PROBE_JS => {
                last_probed = params["objectId"].as_str().unwrap_or("").to_string();
                json!({ "result": { "value": ready_probe() } })
            }
            "Runtime.callFunctionOn" if f == VISIBLE_JS || f == HIGHLIGHT_JS || f == HAS_FOCUS_JS => {
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if f == CHECK_TEXT_JS => {
                let want = params["arguments"][0]["value"].as_str().unwrap_or("").to_string();
                log.lock().unwrap().push(format!("check {want}"));
                json!({ "result": { "value": want == "yes" } })
            }
            "Runtime.callFunctionOn" => {
                if let Some(sel) = params["arguments"][0]["value"].as_str() {
                    last_css = sel.to_string();
                }
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                json!({ "result": [ { "name": "0", "value": { "objectId": format!("css:{last_css}") } } ] })
            }
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
                if let Some(css) = last_probed.strip_prefix("css:") {
                    log.lock().unwrap().push(format!("click {css}"));
                    if css == "#go" {
                        *path.lock().unwrap() = landing.to_string();
                    }
                } else if let Some(id) = last_probed.strip_prefix("ax-").and_then(|s| s.parse::<usize>().ok()) {
                    if let Some((_, name, dest)) = id.checked_sub(100).and_then(|i| entries.get(i)) {
                        log.lock().unwrap().push(format!("click {name}"));
                        if lag == 0 {
                            *path.lock().unwrap() = dest.clone();
                        } else {
                            pending = Some((dest.clone(), lag));
                        }
                    }
                }
                json!({})
            }
            "Page.captureScreenshot" => json!({ "data": "/9j/4AAQ" }),
            _ => json!({}),
        })
    });
    d.on_every_call_events.push((
        "Page.navigate".into(),
        Event { method: "Page.lifecycleEvent".into(), params: json!({ "frameId": "F", "loaderId": "L", "name": "load" }) },
    ));
    (d, app)
}
```

- [ ] **Step 2: Write the failing tests.**

`src-tauri/tests/autorun_nav.rs`, add to the `use` list `PathFailure, Where` and append:

```rust
#[test]
fn a_failed_trip_reads_as_the_designs_sentence_in_a_run_and_its_short_form_in_the_dialog() {
    let at_click = PathFailure {
        at: Where::Click { n: 2, locator: "link \"Apply Leave\"".into() },
        reason: "no visible match".into(),
        harness: false,
    };
    assert_eq!(at_click.for_run(" Leave "), "Could not reach module \"Leave\": click 2, link \"Apply Leave\" - no visible match.");
    assert_eq!(at_click.for_dialog(), "click 2, link \"Apply Leave\": no visible match");
    let at_home = PathFailure { at: Where::Home, reason: "the home page did not load".into(), harness: false };
    assert_eq!(at_home.for_run("Leave"), "Could not reach module \"Leave\": the home page did not open - the home page did not load.");
    let dotted = PathFailure { reason: "it moved.".into(), ..at_click };
    assert!(dotted.for_run("Leave").ends_with("it moved."), "one full stop, not two");
}
```

`src-tauri/tests/autorun_replay.rs`: extend the imports:

```rust
use std::path::Path;
use v2_lib::autorun::nav::{nav_path, no_path, save_nav, NavFile, NO_ACCOUNT, NO_MODULE};
use v2_lib::autorun::replay::{propose, run_cases, run_selection, Browsers, CaseToRun, MODULE_STEP, SIGN_IN_STEP};
```

(the second line replaces the existing `replay` import), then append:

```rust
// ---- Module paths --------------------------------------------------------

const MENU: &[(&str, &str, &str)] = &[("link", "Leave", "/hr/leave"), ("link", "Apply Leave", "/hr/leave/apply")];

fn leave_nav() -> NavFile {
    serde_json::from_value(serde_json::json!({
        "direct_urls": true,
        "modules": [{
            "module": "Leave",
            "clicks": [
                { "role": "link", "name": "Leave", "exact": true },
                { "role": "link", "name": "Apply Leave", "exact": true }
            ],
            "arrived": "/hr/leave/apply",
            "recorded": "2026-09-24T10:00:00Z"
        }]
    }))
    .unwrap()
}

/// A project with a recipe, the admin account and a path for Leave.
fn menu_project(root: &Path) {
    save_recipe(root, "Acme", "Web", &common::menu_recipe()).unwrap();
    save_accounts(root, &[common::account()]).unwrap();
    save_nav(root, "Acme", "Web", &leave_nav()).unwrap();
}

fn to_run(case_id: i32, module: Option<&str>) -> CaseToRun {
    CaseToRun { case_id, title: format!("case {case_id}"), module: module.map(str::to_string) }
}

fn one_check(account: Option<&str>) -> CaseScript {
    script(1, account, serde_json::json!([{ "step_number": 1, "actions": [{ "kind": "check_text", "value": "yes" }] }]))
}

fn browsers_of(drivers: Vec<common::ScriptedDriver>) -> FakeBrowsers {
    FakeBrowsers { queue: drivers.into_iter().map(Some).collect(), opened: 0, closed: 0, returned: vec![] }
}

#[tokio::test]
async fn a_case_signs_in_goes_home_clicks_to_its_module_checks_it_arrived_then_runs_its_steps() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &one_check(Some("admin"))).unwrap();
    let (d, app) = common::menu_app(MENU, "/hr/welcome", 0);
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    let mut phases: Vec<String> = vec![];
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some(" leave "))], None, &quick(), &cancel, &mut |p: ReplayProgress| {
        phases.push(p.phase);
    })
    .await
    .unwrap();

    assert_eq!(
        *app.log.lock().unwrap(),
        vec!["navigate /hr/home/index", "click #go", "navigate /hr/home/index", "click Leave", "click Apply Leave", "check yes"]
    );
    let rec = &run.cases[0];
    assert_eq!(rec.steps.iter().map(|s| s.step_number).collect::<Vec<_>>(), vec![SIGN_IN_STEP, MODULE_STEP, 1]);
    let module = &rec.steps[1].outcomes[0];
    assert!(module.ok, "{module:?}");
    assert_eq!(module.detail, "Go to Leave");
    assert_eq!(rec.proposed, "Passed", "{}", rec.reason);
    assert!(phases.contains(&"module".to_string()), "{phases:?}");
}

#[tokio::test]
async fn a_case_with_no_module_is_blocked_and_no_browser_opens() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &one_check(Some("admin"))).unwrap();
    let mut browsers = browsers_of(vec![]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None)], None, &quick(), &cancel, &mut |_| {}).await.unwrap();
    let rec = &run.cases[0];
    assert_eq!(browsers.opened, 0);
    assert_eq!(rec.proposed, "Blocked");
    assert_eq!(rec.reason, NO_MODULE);
    assert!(rec.steps.iter().flat_map(|s| &s.outcomes).all(|o| o.detail == format!("not run: {NO_MODULE}")), "{:?}", rec.steps);
}

#[tokio::test]
async fn a_module_with_no_recorded_path_is_blocked_and_named() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &one_check(Some("admin"))).unwrap();
    let mut browsers = browsers_of(vec![]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Payroll"))], None, &quick(), &cancel, &mut |_| {}).await.unwrap();
    assert_eq!(browsers.opened, 0);
    assert_eq!(run.cases[0].proposed, "Blocked");
    assert_eq!(run.cases[0].reason, no_path("Payroll"));
}

#[tokio::test]
async fn with_paths_a_case_no_account_applies_to_is_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &one_check(None)).unwrap();
    let mut browsers = browsers_of(vec![]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, &quick(), &cancel, &mut |_| {}).await.unwrap();
    assert_eq!(browsers.opened, 0);
    assert_eq!(run.cases[0].proposed, "Blocked");
    assert_eq!(run.cases[0].reason, NO_ACCOUNT);
}

#[tokio::test]
async fn a_path_click_that_finds_nothing_blocks_the_case_with_a_picture_and_runs_no_step() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &one_check(Some("admin"))).unwrap();
    let (d, app) = common::menu_app(&[("link", "Leave", "/hr/leave")], "/hr/home/index", 0);
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, &quick(), &cancel, &mut |_| {}).await.unwrap();
    let rec = &run.cases[0];
    let module = &rec.steps.iter().find(|s| s.step_number == MODULE_STEP).unwrap().outcomes[0];
    assert!(!module.ok);
    assert!(module.detail.starts_with("Could not reach module \"Leave\": click 2, link \"Apply Leave\" - "), "{}", module.detail);
    assert!(module.detail.ends_with('.'), "{}", module.detail);
    assert!(module.screenshot.is_some(), "a failed trip to the module keeps a picture");
    let step1 = rec.steps.iter().find(|s| s.step_number == 1).unwrap();
    assert!(step1.outcomes.iter().all(|o| o.detail == "not run: the module screen was not reached"), "{:?}", step1.outcomes);
    assert!(!app.log.lock().unwrap().iter().any(|l| l.starts_with("check")), "no step may run off the module screen");
    assert_eq!(rec.proposed, "Blocked");
    assert_eq!(rec.reason, module.detail);
}

#[tokio::test]
async fn a_path_that_ends_somewhere_else_fails_its_arrival_check() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &one_check(Some("admin"))).unwrap();
    let (d, _app) = common::menu_app(&[("link", "Leave", "/hr/leave"), ("link", "Apply Leave", "/hr/leave/other")], "/hr/home/index", 0);
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, &quick(), &cancel, &mut |_| {}).await.unwrap();
    let rec = &run.cases[0];
    assert_eq!(rec.proposed, "Blocked");
    assert_eq!(
        rec.reason,
        "Could not reach module \"Leave\": click 2, link \"Apply Leave\" - the page ended on /hr/leave/other, not /hr/leave/apply."
    );
}

/// Review focus 5.
#[tokio::test]
async fn an_address_that_changes_a_moment_after_the_last_click_still_counts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(root, &one_check(Some("admin"))).unwrap();
    let (d, _app) = common::menu_app(MENU, "/hr/home/index", 3);
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, &quick(), &cancel, &mut |_| {}).await.unwrap();
    assert_eq!(run.cases[0].proposed, "Passed", "{}", run.cases[0].reason);
}

#[tokio::test]
async fn a_sign_in_in_the_middle_of_a_script_goes_back_to_the_module_before_the_next_action() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    menu_project(root);
    store::save_script(
        root,
        &script(1, Some("admin"), serde_json::json!([
            { "step_number": 1, "actions": [{ "kind": "check_text", "value": "yes" }] },
            { "step_number": 2, "actions": [{ "kind": "sign_in", "account": "admin" }, { "kind": "check_text", "value": "yes" }] }
        ])),
    )
    .unwrap();
    let (d, app) = common::menu_app(MENU, "/hr/home/index", 0);
    let mut browsers = browsers_of(vec![d]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, &quick(), &cancel, &mut |_| {}).await.unwrap();

    // The second sign-in may come from the saved session (no form, so no
    // `#go`) - what matters is what happens around it.
    let log: Vec<String> = app.log.lock().unwrap().iter().filter(|l| *l != "click #go").cloned().collect();
    assert_eq!(
        log,
        vec![
            "navigate /hr/home/index", "click Leave", "click Apply Leave", "check yes",
            "navigate /hr/home/index", "click Leave", "click Apply Leave", "check yes",
        ]
    );
    let rec = &run.cases[0];
    let step2 = rec.steps.iter().find(|s| s.step_number == 2).unwrap();
    assert!(step2.outcomes[0].ok && step2.outcomes[0].detail.ends_with("; then Go to Leave"), "{:?}", step2.outcomes);
    assert_eq!(rec.proposed, "Passed", "{}", rec.reason);
}

#[tokio::test]
async fn a_project_whose_file_has_no_paths_runs_exactly_as_before() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_nav(root, "Acme", "Web", &NavFile::default()).unwrap();
    store::save_script(root, &passing_script(1)).unwrap();
    let mut browsers = browsers_of(vec![common::FakePage::default().driver()]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, &quick(), &cancel, &mut |_| {}).await.unwrap();
    let rec = &run.cases[0];
    assert_eq!(rec.steps.iter().map(|s| s.step_number).collect::<Vec<_>>(), vec![1]);
    assert_eq!(rec.proposed, "Passed");
    assert!(browsers.returned[0].calls_to("Page.navigate").is_empty(), "no trip home without paths");
}

/// Review focus 4.
#[tokio::test]
async fn an_unreadable_module_paths_file_stops_the_run_before_any_browser_opens() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save_script(root, &passing_script(1)).unwrap();
    std::fs::create_dir_all(root.join("projects")).unwrap();
    std::fs::write(nav_path(root, "Acme", "Web"), "{ not json").unwrap();
    let mut browsers = browsers_of(vec![common::FakePage::default().driver()]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    let err = run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, Some("Leave"))], None, &quick(), &cancel, &mut |_| {})
        .await
        .unwrap_err();
    assert!(err.contains("module paths file is not readable"), "{err}");
    assert_eq!(browsers.opened, 0);
    assert!(run.cases.is_empty());
}
```

`src-tauri/tests/autorun_failures.rs`, add `use v2_lib::autorun::nav::no_path;` and append:

```rust
#[test]
fn a_case_the_run_could_not_take_to_its_module_is_not_a_script_defect() {
    let unreached = "Could not reach module \"Leave\": click 2, link \"Apply Leave\" - waited 300ms: link \"Apply Leave\" not found.";
    let case = CaseRecord {
        proposed: "Blocked".to_string(),
        reason: unreached.to_string(),
        steps: vec![StepRecord { step_number: -1, outcomes: vec![ActionOutcome::failed(unreached)], screenshot: None }],
        ..empty_case()
    };
    let expected = Some(
        "the run could not take this case to its module screen - fix the module path or the case's Module in the app, not the script"
            .to_string(),
    );
    assert_eq!(stop_reason(&case), expected);
    let run = LocalRun {
        id: "run-1".into(),
        pbi_id: 1,
        started_at: "1".into(),
        cases: vec![case],
        mode: "unattended".into(),
        published: None,
    };
    let out = describe_failures(&run, &[]);
    assert!(out.contains(&format!("module: {unreached}")), "{out}");
    assert!(!out.contains("step -1"), "{out}");

    let no_path_case = CaseRecord { proposed: "Blocked".to_string(), reason: no_path("Payroll"), ..empty_case() };
    assert_eq!(stop_reason(&no_path_case), expected);
}
```

- [ ] **Step 3: Run to see them fail:** `CARGO_TARGET_DIR=target/gate cargo test --test autorun_nav`, then `--test autorun_replay`, then `--test autorun_failures` (one at a time). Expected: compile errors naming `PathFailure`, `run_cases`, `CaseToRun`, `MODULE_STEP`.

- [ ] **Step 4: Implement `nav.rs` additions.** Replace the `use` block at the top of `nav.rs` with:

```rust
use super::recipe::{origin_of, project_slug, SignInRecipe};
use crate::browser::actions::{execute_in, failed_by, Action, ActionOutcome, Policy};
use crate::browser::cdp::Driver;
use crate::browser::locator::Target;
use crate::browser::page;
use crate::browser::timing::Timing;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
```

and append:

```rust
/// Start of the sentence a case gets when its trip to the module fails.
pub const UNREACHED_PREFIX: &str = "Could not reach module \"";

/// Everything a run needs to take a signed-in browser to one module: where
/// home is, where `navigate` may go, and the recorded path.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub start_url: String,
    pub origins: Vec<String>,
    pub path: ModulePath,
}

impl Route {
    pub fn new(recipe: &SignInRecipe, path: ModulePath) -> Route {
        Route { start_url: recipe.start_url.clone(), origins: recipe.origins(), path }
    }
}

/// Where a trip to a module stopped.
#[derive(Debug, Clone, PartialEq)]
pub enum Where {
    Home,
    /// `n` counts from 1; `locator` is the click in words.
    Click { n: usize, locator: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathFailure {
    pub at: Where,
    pub reason: String,
    /// The browser failed, not the page: no picture is asked for.
    pub harness: bool,
}

impl PathFailure {
    /// The run's sentence (design §5).
    pub fn for_run(&self, module: &str) -> String {
        let module = module.trim();
        let reason = self.reason.trim_end_matches('.');
        match &self.at {
            Where::Home => format!("{UNREACHED_PREFIX}{module}\": the home page did not open - {reason}."),
            Where::Click { n, locator } => format!("{UNREACHED_PREFIX}{module}\": click {n}, {locator} - {reason}."),
        }
    }

    /// The Module paths dialog's shorter form (design §4).
    pub fn for_dialog(&self) -> String {
        match &self.at {
            Where::Home => format!("the home page did not open: {}", self.reason),
            Where::Click { n, locator } => format!("click {n}, {locator}: {}", self.reason),
        }
    }
}

/// Back to the recipe's home page unless the browser is already there.
/// This is the runner's own navigation: the "no direct addresses" rule is
/// about scripts and does not apply to it.
pub async fn go_home<D: Driver>(d: &mut D, start_url: &str, origins: &[String], timing: &Timing) -> ActionOutcome {
    let href = match page::eval_value(d, "location.href").await {
        Ok(v) => v.as_str().unwrap_or("").to_string(),
        Err(e) => return failed_by(e),
    };
    if same_page(&href, start_url) {
        return ActionOutcome::passed("already on the home page");
    }
    let out = execute_in(d, &Action::Navigate { url: start_url.to_string() }, timing, &Policy::only(origins.to_vec())).await;
    if out.ok {
        return ActionOutcome::passed("went to the home page");
    }
    // The navigate's own words name the address; this sentence reaches the
    // person, so it does not.
    let mut failed = ActionOutcome::failed(if out.harness {
        "the browser did not answer while the home page was opening"
    } else {
        "the home page did not load"
    });
    failed.harness = out.harness;
    failed
}

/// Home, then each recorded click with the runner's own click (so each
/// must find exactly one visible element), then wait up to `nav_ms` for
/// the address path to equal `arrived`. Ok carries the path it reached.
pub async fn go_to_module<D: Driver>(d: &mut D, route: &Route, timing: &Timing) -> Result<String, PathFailure> {
    let home = go_home(d, &route.start_url, &route.origins, timing).await;
    if !home.ok {
        return Err(PathFailure { at: Where::Home, reason: home.detail, harness: home.harness });
    }
    let policy = Policy::only(route.origins.clone());
    for (i, click) in route.path.clicks.iter().enumerate() {
        let out = execute_in(d, &Action::Click { selector: click.clone() }, timing, &policy).await;
        if !out.ok {
            return Err(PathFailure {
                at: Where::Click { n: i + 1, locator: click.describe() },
                reason: out.detail,
                harness: out.harness,
            });
        }
    }
    let at = match route.path.clicks.last() {
        Some(c) => Where::Click { n: route.path.clicks.len(), locator: c.describe() },
        None => Where::Home,
    };
    let deadline = Instant::now() + Duration::from_millis(timing.nav_ms);
    let mut last = String::new();
    loop {
        match page::eval_value(d, "location.href").await {
            Ok(v) => {
                last = path_of(v.as_str().unwrap_or(""));
                if last == route.path.arrived {
                    return Ok(last);
                }
            }
            // Between two documents the page refuses; that is an answer.
            Err(e) if e.is_transient() => {}
            Err(e) => {
                let o = failed_by(e);
                return Err(PathFailure { at, reason: o.detail, harness: o.harness });
            }
        }
        if Instant::now() >= deadline {
            let seen = if last.is_empty() { "an address it could not read" } else { last.as_str() };
            return Err(PathFailure {
                at,
                reason: format!("the page ended on {seen}, not {}", route.path.arrived),
                harness: false,
            });
        }
        tokio::time::sleep(Duration::from_millis(timing.poll_ms)).await;
    }
}

/// A trip to a module as the one outcome the run's "Go to X" line shows.
pub fn reached(module: &str, result: Result<String, PathFailure>) -> ActionOutcome {
    match result {
        Ok(_) => ActionOutcome::passed(format!("Go to {}", module.trim())),
        Err(f) => {
            let mut out = ActionOutcome::failed(f.for_run(module));
            out.harness = f.harness;
            out
        }
    }
}

/// An outcome that means the run could not put the case where its steps
/// begin: the case is Blocked, not Failed.
pub fn is_route_problem(detail: &str) -> bool {
    detail.contains(UNREACHED_PREFIX)
}

/// A case reason that is about the project's setup (paths, Module field,
/// account), not about the script.
pub fn is_setup_problem(reason: &str) -> bool {
    reason.contains(UNREACHED_PREFIX) || reason == NO_MODULE || reason == NO_ACCOUNT || reason.starts_with(NO_PATH_START)
}
```

- [ ] **Step 5: Replace `src-tauri/src/autorun/runner.rs`** with:

```rust
//! The step loop `auto_run_step` runs: one action at a time, `sign_in`
//! carried out here (it needs the tester's accounts and the project's
//! recipe, which the executor has neither of), and a failure screenshot
//! taken the same way regardless of which of the two produced it.
//!
//! Pulled out of the Tauri command so it can be exercised directly, the
//! same way `autorun::store` and `autorun::signin` already are.

use super::nav::{self, Route};
use super::recipe::{self, SignInRecipe};
use super::signin::{self, SignInOutcome};
use super::{store, StepScript};
use crate::browser::actions::{execute_in, Action, ActionOutcome, Policy};
use crate::browser::cdp::Driver;
use crate::browser::page;
use crate::browser::timing::{Timing, SHOT_TIMEOUT_MS};
use std::path::Path;
use std::time::Duration;

const AFTER_FAILED_SIGN_IN: &str = "not run: the sign-in before this action failed";
const AFTER_UNREACHED: &str = "not run: the module screen was not reached after the sign-in";

/// Where an authored `navigate` may go for this project: everywhere, for a
/// project with no recipe saved yet, or only the recipe's own origins once
/// there is one.
pub fn policy_for(recipe: Option<&SignInRecipe>) -> Policy {
    match recipe {
        Some(r) => Policy::only(r.origins()),
        None => Policy::open(),
    }
}

/// A sign-in reads as the one action outcome it stands in for. `harness`
/// carries over too, so a `sign_in` that failed because the browser itself
/// stopped answering does not then get asked for a screenshot below.
pub fn as_action_outcome(out: &SignInOutcome) -> ActionOutcome {
    let mut outcome = if out.ok {
        ActionOutcome::passed(out.detail.clone())
    } else {
        ActionOutcome::failed(out.detail.clone())
    };
    outcome.harness = out.harness;
    outcome
}

/// A picture of the page at the moment an action failed, or (from the
/// replay engine) at the moment an executed step ended. Best effort: a
/// browser that cannot take one (it has gone away, or is too busy to
/// answer within `SHOT_TIMEOUT_MS`) just means no picture - the failure is
/// already reported in words. Never called for a harness failure (asking a
/// browser that has already failed to answer for a picture is exactly the
/// stall this guards against) and never for an action that was never run.
pub(crate) async fn picture<D: Driver>(d: &mut D, root: &Path) -> Option<String> {
    let bytes = tokio::time::timeout(Duration::from_millis(SHOT_TIMEOUT_MS), page::screenshot(d))
        .await
        .ok()?
        .ok()?;
    store::save_shot(root, &bytes).ok()
}

/// Run one step's actions in order and report every outcome.
///
/// Actions after an ORDINARY failure still run: the watcher learns more
/// from "the click worked, the check did not" than from a run that stops
/// at the first red. A failed `sign_in` is the one exception - what
/// follows would run as the wrong person, or as nobody, and report
/// results that mean nothing - so once a `sign_in` action fails, every
/// remaining action of that step is not executed and is reported as such.
/// The outcomes list still has one entry per action, in order.
pub async fn run_step<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    step: &StepScript,
    timing: &Timing,
    account: &mut Option<String>,
) -> Result<Vec<ActionOutcome>, String> {
    run_step_routed(d, root, organization, project, step, timing, account, None).await
}

/// `run_step`, for an unattended run in a project with module paths. A
/// `sign_in` lands on the home page, so the runner takes the browser back
/// to the case's module screen before the next action; the `sign_in`'s one
/// outcome then says both halves. When the module is not reached, the rest
/// of the step is not run: it would act on the wrong screen.
#[allow(clippy::too_many_arguments)]
pub async fn run_step_routed<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    step: &StepScript,
    timing: &Timing,
    account: &mut Option<String>,
    route: Option<&Route>,
) -> Result<Vec<ActionOutcome>, String> {
    let recipe = recipe::load_recipe(root, organization, project)?;
    let policy = policy_for(recipe.as_ref());
    let mut out = Vec::with_capacity(step.actions.len());
    let mut blocked: Option<&'static str> = None;
    for action in &step.actions {
        if let Some(why) = blocked {
            out.push(ActionOutcome::failed(why));
            continue;
        }
        let mut outcome = match action {
            Action::SignIn { account: key } => match signin::prepare(root, organization, project, key) {
                Err(why) => {
                    *account = None;
                    blocked = Some(AFTER_FAILED_SIGN_IN);
                    ActionOutcome::failed(why)
                }
                Ok((r, who)) => {
                    let signed = signin::sign_in(d, root, &r, &who, timing).await;
                    *account = signed.ok.then(|| who.key.clone());
                    match route {
                        _ if !signed.ok => {
                            blocked = Some(AFTER_FAILED_SIGN_IN);
                            as_action_outcome(&signed)
                        }
                        None => as_action_outcome(&signed),
                        Some(rt) => {
                            let went = nav::reached(&rt.path.module, nav::go_to_module(d, rt, timing).await);
                            if !went.ok {
                                blocked = Some(AFTER_UNREACHED);
                            }
                            signed_then_went(&signed, went)
                        }
                    }
                }
            },
            other => execute_in(d, other, timing, &policy).await,
        };
        if !outcome.ok && !outcome.harness {
            outcome.screenshot = picture(d, root).await;
        }
        out.push(outcome);
    }
    Ok(out)
}

/// A sign-in and the trip back to the module that follows it, as the one
/// outcome the `sign_in` action stands for.
fn signed_then_went(signed: &SignInOutcome, went: ActionOutcome) -> ActionOutcome {
    let detail = format!("{}; then {}", signed.detail, went.detail);
    let mut out = if went.ok { ActionOutcome::passed(detail) } else { ActionOutcome::failed(detail) };
    out.harness = went.harness;
    out
}
```

- [ ] **Step 6: Replace `src-tauri/src/autorun/replay.rs`** with:

```rust
//! Running whole cases with nobody pressing a button per step.
//!
//! Pure: it is handed its browsers, its clock is `Instant`, and the only
//! thing it writes is the run file. It never calls Azure DevOps, and it
//! never fills in a verdict: what it thinks is `proposed`, and a person
//! confirms or changes it afterwards.
//!
//! In a project with recorded module paths a case goes the way a tester
//! goes: sign in, click through the menu to the case's module, check it
//! arrived, then run the steps. A project with none runs as it always has.

use super::nav::{self, Route};
use super::runner::{self, as_action_outcome};
use super::{recipe, signin};
use super::{store, CaseRecord, CaseScript, LocalRun, StepRecord, StepScript};
use crate::browser::actions::ActionOutcome;
use crate::browser::cdp::Driver;
use crate::browser::timing::Timing;
use crate::events::ReplayProgress;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

pub const SIGN_IN_STEP: i32 = 0;
/// The runner's own "Go to X" line: after the sign-in, before step 1.
pub const MODULE_STEP: i32 = -1;
const AFTER_FAILED_STEP: &str = "not run: an earlier step of this case failed";
const AFTER_FAILED_SIGN_IN: &str = "not run: the sign-in failed";
const AFTER_UNREACHED: &str = "not run: the module screen was not reached";
const AFTER_STOP: &str = "not run: the run was stopped";

/// Where a fresh browser per case comes from. The command gives real ones;
/// the tests give fakes.
pub trait Browsers {
    type D: Driver;
    fn open(&mut self) -> impl std::future::Future<Output = Result<Self::D, String>>;
    fn close(&mut self, d: Self::D) -> impl std::future::Future<Output = ()>;
}

pub struct Proposal {
    pub verdict: &'static str,
    pub reason: String,
}

/// One case of a selection, as the frontend knows it before its script is
/// read. `module` is the test case's Module field.
#[derive(Debug, Clone, PartialEq)]
pub struct CaseToRun {
    pub case_id: i32,
    pub title: String,
    pub module: Option<String>,
}

fn not_run(step: &StepScript, why: &str) -> StepRecord {
    StepRecord {
        step_number: step.step_number,
        outcomes: step.actions.iter().map(|_| ActionOutcome::failed(why)).collect(),
        screenshot: None,
    }
}

fn was_run(o: &ActionOutcome) -> bool {
    !o.detail.starts_with("not run:")
}

/// `signed_in`: None when no account applies to the case, Some(ok) otherwise.
pub fn propose(script: &CaseScript, steps: &[StepRecord], signed_in: Option<bool>, stopped: bool) -> Proposal {
    let ran = || {
        steps.iter().flat_map(|s| s.outcomes.iter().map(move |o| (s.step_number, o))).filter(|(_, o)| was_run(o))
    };
    if let Some((n, o)) = ran().find(|(_, o)| !o.ok && o.harness) {
        let at = match n {
            SIGN_IN_STEP => "while signing in".to_string(),
            MODULE_STEP => "while going to the module".to_string(),
            _ => format!("at step {n}"),
        };
        return Proposal { verdict: "Blocked", reason: format!("the browser stopped answering {at}: {}", o.detail) };
    }
    // The run could not put the case where its steps begin: that is not
    // the application failing the test.
    if let Some((_, o)) = ran().find(|(_, o)| !o.ok && nav::is_route_problem(&o.detail)) {
        return Proposal { verdict: "Blocked", reason: o.detail.clone() };
    }
    if signed_in == Some(false) {
        let why = steps
            .iter()
            .find(|s| s.step_number == SIGN_IN_STEP)
            .and_then(|s| s.outcomes.last())
            .map(|o| o.detail.clone())
            .unwrap_or_default();
        return Proposal { verdict: "Blocked", reason: why };
    }
    if let Some((n, o)) = ran().find(|(_, o)| !o.ok) {
        return Proposal { verdict: "Failed", reason: format!("step {n}: {}", o.detail) };
    }
    if stopped {
        return Proposal { verdict: "", reason: "stopped before it finished".into() };
    }
    if !script.steps.iter().flat_map(|s| &s.actions).any(|a| a.is_check()) {
        return Proposal { verdict: "", reason: "this script checks nothing, so there is nothing to propose".into() };
    }
    Proposal { verdict: "Passed", reason: format!("every action of {} steps passed", script.steps.len()) }
}

/// Run one whole case as the script's own account, with no module step.
/// Kept for callers that have no route; `run_case_as` is the full form.
pub async fn run_case<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    script: &CaseScript,
    timing: &Timing,
    cancel: &AtomicBool,
    on_step: &mut (dyn FnMut(i32) + Send),
) -> CaseRecord {
    run_case_as(d, root, organization, project, script, script.account.as_deref(), None, timing, cancel, on_step).await
}

/// Run one whole case: an optional sign-in as `account` (step
/// `SIGN_IN_STEP`), then, with a `route`, the trip to the module
/// (`MODULE_STEP`), then every scripted step in order, stopping the case
/// (but not the run) after the first step that fails or once `cancel` is
/// set.
#[allow(clippy::too_many_arguments)]
pub async fn run_case_as<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    script: &CaseScript,
    account: Option<&str>,
    route: Option<&Route>,
    timing: &Timing,
    cancel: &AtomicBool,
    on_step: &mut (dyn FnMut(i32) + Send),
) -> CaseRecord {
    let began = Instant::now();
    let mut steps: Vec<StepRecord> = Vec::with_capacity(script.steps.len() + 2);
    let mut signed_in = None;
    let mut current = None;
    let mut stopped = false;

    // Why the rest of the case is not being run, once something decided
    // that. Checked here too, before the sign-in - a stop asked for while
    // this case was still only "opening" must leave it completely
    // untouched, not just cut short after already having signed in.
    let mut skip: Option<&'static str> = if cancel.load(Ordering::SeqCst) {
        stopped = true;
        Some(AFTER_STOP)
    } else {
        None
    };

    if skip.is_none() {
        if let Some(key) = account {
            on_step(SIGN_IN_STEP);
            let out = match signin::prepare(root, organization, project, key) {
                Err(why) => vec![ActionOutcome::failed(why)],
                Ok((recipe, who)) => {
                    let signed = signin::sign_in(d, root, &recipe, &who, timing).await;
                    let mut all = signed.steps.clone();
                    all.push(as_action_outcome(&signed));
                    all
                }
            };
            let ok = out.last().is_some_and(|o| o.ok);
            signed_in = Some(ok);
            steps.push(StepRecord { step_number: SIGN_IN_STEP, outcomes: out, screenshot: None });
        }
        skip = (signed_in == Some(false)).then_some(AFTER_FAILED_SIGN_IN);
    }

    if let (None, Some(r)) = (skip, route) {
        if cancel.load(Ordering::SeqCst) {
            skip = Some(AFTER_STOP);
            stopped = true;
        } else {
            on_step(MODULE_STEP);
            let mut out = nav::reached(&r.path.module, nav::go_to_module(d, r, timing).await);
            if !out.ok && !out.harness {
                out.screenshot = runner::picture(d, root).await;
            }
            if !out.ok {
                skip = Some(AFTER_UNREACHED);
            }
            steps.push(StepRecord { step_number: MODULE_STEP, outcomes: vec![out], screenshot: None });
        }
    }

    for step in &script.steps {
        if skip.is_none() && cancel.load(Ordering::SeqCst) {
            skip = Some(AFTER_STOP);
            stopped = true;
        }
        if let Some(why) = skip {
            steps.push(not_run(step, why));
            continue;
        }
        on_step(step.step_number);
        let outcomes =
            match runner::run_step_routed(d, root, organization, project, step, timing, &mut current, route).await {
                Ok(o) => o,
                Err(why) => step.actions.iter().map(|_| ActionOutcome::failed(why.clone())).collect(),
            };
        let harness = outcomes.iter().any(|o| !o.ok && o.harness);
        let screenshot = if harness { None } else { runner::picture(d, root).await };
        if outcomes.iter().any(|o| !o.ok) {
            skip = Some(AFTER_FAILED_STEP);
        }
        steps.push(StepRecord { step_number: step.step_number, outcomes, screenshot });
    }

    let p = propose(script, &steps, signed_in, stopped);
    CaseRecord {
        case_id: script.case_id,
        title: script.title.clone(),
        verdict: String::new(),
        note: String::new(),
        steps,
        proposed: p.verdict.to_string(),
        reason: p.reason,
        duration_ms: i32::try_from(began.elapsed().as_millis()).ok(),
        account: account.map(str::to_string),
    }
}

fn unrun(case_id: i32, title: &str, proposed: &str, reason: String) -> CaseRecord {
    CaseRecord {
        case_id,
        title: title.to_string(),
        verdict: String::new(),
        note: String::new(),
        steps: vec![],
        proposed: proposed.to_string(),
        reason,
        duration_ms: None,
        account: None,
    }
}

/// A case that cannot start (design §5): every step shown as not run, the
/// verdict proposed is Blocked, and no browser was opened for it.
fn blocked_before_start(script: &CaseScript, account: Option<&str>, reason: String) -> CaseRecord {
    let why = format!("not run: {reason}");
    CaseRecord {
        case_id: script.case_id,
        title: script.title.clone(),
        verdict: String::new(),
        note: String::new(),
        steps: script.steps.iter().map(|s| not_run(s, &why)).collect(),
        proposed: "Blocked".to_string(),
        reason,
        duration_ms: None,
        account: account.map(str::to_string),
    }
}

/// One `ReplayProgress`, built from plain values rather than a closure so
/// it never has to borrow `run` or `progress` - both are busy elsewhere in
/// the loop this is called from.
#[allow(clippy::too_many_arguments)]
fn tell(
    run_id: &str,
    index: u32,
    total: u32,
    case_id: i32,
    title: &str,
    phase: &str,
    step_number: i32,
    steps: u32,
    proposed: &str,
) -> ReplayProgress {
    ReplayProgress {
        run_id: run_id.to_string(),
        index,
        total,
        case_id,
        title: title.to_string(),
        phase: phase.to_string(),
        step_number,
        steps,
        proposed: proposed.to_string(),
    }
}

/// Run a selection with no module or run account per case - the shape
/// every caller used before module paths.
#[allow(clippy::too_many_arguments)]
pub async fn run_selection<B: Browsers>(
    browsers: &mut B,
    root: &Path,
    organization: &str,
    project: &str,
    run: &mut LocalRun,
    cases: &[(i32, String)],
    timing: &Timing,
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(ReplayProgress) + Send),
) -> Result<(), String> {
    let cases: Vec<CaseToRun> =
        cases.iter().map(|(case_id, title)| CaseToRun { case_id: *case_id, title: title.clone(), module: None }).collect();
    run_cases(browsers, root, organization, project, run, &cases, None, timing, cancel, progress).await
}

/// Run a whole selection, one fresh browser each, saving the run after
/// every case so a crash or a stop loses nothing. `run_account` signs in
/// every script that names no account of its own. The module paths file
/// is read once, first: an unreadable one stops the run before any
/// browser opens.
#[allow(clippy::too_many_arguments)]
pub async fn run_cases<B: Browsers>(
    browsers: &mut B,
    root: &Path,
    organization: &str,
    project: &str,
    run: &mut LocalRun,
    cases: &[CaseToRun],
    run_account: Option<&str>,
    timing: &Timing,
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(ReplayProgress) + Send),
) -> Result<(), String> {
    let nav_file = nav::load_nav(root, organization, project)?;
    let sign_in_recipe = if nav_file.modules.is_empty() {
        None
    } else {
        recipe::load_recipe(root, organization, project).ok().flatten()
    };
    let total = cases.len() as u32;
    let run_id = run.id.clone();
    let mut save_error: Option<String> = None;

    for (i, case) in cases.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        let index = i as u32;
        let (case_id, title) = (case.case_id, case.title.as_str());
        // The script's own step count, not what the case actually ran -
        // it must read the same on every phase of a case, including
        // "done", whether the browser opened or the case has a script at
        // all. 0 only when there is genuinely no script to count.
        let mut count = 0u32;
        let record = match store::load_script(root, case_id) {
            Err(why) => unrun(case_id, title, "", format!("the script could not be read: {why}")),
            Ok(None) => unrun(case_id, title, "", "this case has no script on this machine".into()),
            Ok(Some(script)) => {
                count = script.steps.len() as u32;
                let account = script.account.as_deref().or(run_account);
                match nav::route_for(&nav_file, case.module.as_deref(), account) {
                    Err(why) => blocked_before_start(&script, account, why),
                    Ok(path) => {
                        // A path but no recipe: the sign-in fails first and
                        // says what to add, so no route is needed.
                        let route = path.zip(sign_in_recipe.as_ref()).map(|(p, r)| Route::new(r, p.clone()));
                        progress(tell(&run_id, index, total, case_id, title, "opening", 0, count, ""));
                        match browsers.open().await {
                            Err(why) => unrun(case_id, title, "Blocked", format!("the browser did not open: {why}")),
                            Ok(mut d) => {
                                let mut on_step = |n: i32| {
                                    let phase = match n {
                                        SIGN_IN_STEP => "signing_in",
                                        MODULE_STEP => "module",
                                        _ => "step",
                                    };
                                    progress(tell(&run_id, index, total, case_id, title, phase, n, count, ""));
                                };
                                let rec = run_case_as(
                                    &mut d,
                                    root,
                                    organization,
                                    project,
                                    &script,
                                    account,
                                    route.as_ref(),
                                    timing,
                                    cancel,
                                    &mut on_step,
                                )
                                .await;
                                browsers.close(d).await;
                                rec
                            }
                        }
                    }
                }
            }
        };
        let proposed = record.proposed.clone();
        run.cases.push(record);
        if let Err(e) = store::save_run(root, run) {
            save_error.get_or_insert(e);
        }
        progress(tell(&run_id, index, total, case_id, title, "done", 0, count, &proposed));
    }

    save_error.map_or(Ok(()), Err)
}
```

- [ ] **Step 7: `failures.rs`.** Change the imports to:

```rust
use super::edits::MAX_REPAIRS;
use super::nav;
use super::replay::{MODULE_STEP, SIGN_IN_STEP};
use super::{store, CaseRecord, CaseScript, LocalRun, StepRecord};
```

In `stop_reason`, between the browser check and the `Blocked` verdict check, insert:

```rust
    if nav::is_setup_problem(&case.reason) {
        return Some(
            "the run could not take this case to its module screen - fix the module path or the case's Module in the app, not the script"
                .to_string(),
        );
    }
```

In `describe_step`, directly after the `SIGN_IN_STEP` block's closing `}`, insert:

```rust
    if step.step_number == MODULE_STEP {
        if let Some(o) = step.outcomes.last() {
            if !o.ok {
                out.push(format!("module: {}", o.detail));
            }
        }
        return;
    }
```

- [ ] **Step 8: `events.rs`.** In `ReplayProgress`, change the two doc comments to:

```rust
    /// "opening", "signing_in", "module", "step" or "done"
    pub phase: String,
    /// Meaningful for "step", "signing_in" (0) and "module" (-1).
    pub step_number: i32,
```

- [ ] **Step 9: Run** (one at a time): `--test autorun_nav`, `--test autorun_replay`, `--test autorun_failures`, `--test autorun_runner`, `--test autorun_signin`, `--test browser_live` (compiles; its tests stay ignored). All pass. Then `CARGO_TARGET_DIR=target/gate cargo test --test bindings` (only the `ReplayProgress` doc changes; if the file differs only by line endings, revert it as the Global Constraints say).

- [ ] **Step 10: Commit**

```bash
git add src-tauri/src/autorun src-tauri/src/events.rs src-tauri/tests src/bindings.ts
git commit -q -F - <<'EOF'
feat(v2): an unattended case goes home, clicks through to its module and checks it arrived before step 1

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

(If `src/bindings.ts` was reverted, `git add` of it is a no-op.)

---

### Task 3: An account for the run (Rust and the Sign in as select)

**Files:**
- Modify: `src-tauri/src/autorun/accounts.rs` (`account_for_run`)
- Modify: `src-tauri/src/commands/autorun_replay.rs` (`ReplayCase.module`, `auto_run_replay` gains `account`, uses `run_cases`)
- Test: `src-tauri/tests/autorun_accounts.rs`, `src-tauri/tests/autorun_replay.rs`
- Replace: `src/screens/AutoRun/ReplayPane.tsx`; modify `ReplayPane.test.tsx`
- Modify: `src/screens/AutoRun/RunReview.tsx` (`stepLabel`), `RunReview.test.tsx`
- Modify: `src/screens/AutoRun/index.tsx` (module per replayed case), `src/screens/AutoRun.test.tsx` (allowlist)
- Generated: `src/bindings.ts`

**Interfaces:**
- Consumes: Task 2's `run_cases`, `CaseToRun`, `MODULE_STEP`; `accounts::{valid_key, find_account}`.
- Produces: `pub fn account_for_run(root: &Path, key: Option<&str>) -> Result<Option<String>, String>` in `autorun::accounts`.
- Produces: `ReplayCase { case_id: i32, title: String, module: Option<String> }` (`#[serde(default)]` on `module`); `auto_run_replay(organization, project, pbi_id, cases, account: Option<String>, browser_name, watch)`; TypeScript `commands.autoRunReplay(organization, project, pbiId, cases, account, browserName, watch)`.
- Produces (TS): `ReplayPane` prop `cases: { id: number; title: string; module?: string }[]`; exported `runAccountKey(org, project)`; `statusOf` returns "Going to the module" for phase `module`; `RunReview` exports `stepLabel(stepNumber: number): string`.

- [ ] **Step 1: Write the failing Rust tests.**

`src-tauri/tests/autorun_accounts.rs`, add `account_for_run` to the `use` list and append:

```rust
#[test]
fn the_account_for_a_run_is_a_real_key_on_this_machine_or_nothing() {
    let dir = tempfile::tempdir().unwrap();
    save_accounts(dir.path(), &[account("hr.admin", "kim", "pw")]).unwrap();
    assert_eq!(account_for_run(dir.path(), None).unwrap(), None);
    assert_eq!(account_for_run(dir.path(), Some("  ")).unwrap(), None);
    assert_eq!(account_for_run(dir.path(), Some("hr.admin")).unwrap(), Some("hr.admin".to_string()));
    assert_eq!(account_for_run(dir.path(), Some("Bad Key")).unwrap_err(), "\"Bad Key\" is not a usable account key");
    assert_eq!(
        account_for_run(dir.path(), Some("ghost")).unwrap_err(),
        "there is no account \"ghost\" on this machine - add it in Auto Run, Accounts"
    );
}
```

`src-tauri/tests/autorun_replay.rs`, append:

```rust
fn lee() -> v2_lib::autorun::accounts::Account {
    v2_lib::autorun::accounts::Account {
        key: "lee".into(),
        label: "Lee".into(),
        username: "lee".into(),
        password: common::PASSWORD.into(),
    }
}

#[tokio::test]
async fn the_scripts_own_account_wins_and_the_runs_account_fills_in_for_a_script_with_none() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_recipe(root, "Acme", "Web", &common::recipe()).unwrap();
    save_accounts(root, &[common::account(), lee()]).unwrap();
    store::save_script(root, &script(1, Some("admin"), serde_json::json!([]))).unwrap();
    store::save_script(root, &script(2, None, serde_json::json!([]))).unwrap();
    let (d1, _) = common::stateful_app(false, None);
    let (d2, _) = common::stateful_app(false, None);
    let mut browsers = browsers_of(vec![d1, d2]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_cases(&mut browsers, root, "Acme", "Web", &mut run, &[to_run(1, None), to_run(2, None)], Some("lee"), &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(run.cases[0].account.as_deref(), Some("admin"));
    assert_eq!(run.cases[1].account.as_deref(), Some("lee"));
    assert_eq!(run.cases[1].steps[0].step_number, SIGN_IN_STEP);
    assert!(run.cases[1].steps[0].outcomes.iter().all(|o| o.ok), "{:?}", run.cases[1].steps[0].outcomes);
}
```

- [ ] **Step 2: Run to see them fail:** `--test autorun_accounts`, then `--test autorun_replay`. Expected: `account_for_run` not found (the replay test compiles already and passes once Task 2 is in; it is here to pin the rule).

- [ ] **Step 3: Implement the Rust side.**

`src-tauri/src/autorun/accounts.rs`, after `find_account`:

```rust
/// The account picked for a whole unattended run, checked before anything
/// starts. Blank means none: every script runs as its own account, as
/// before. A key that is not usable, or not on this machine, refuses the
/// run rather than signing nobody in halfway through it.
pub fn account_for_run(root: &Path, key: Option<&str>) -> Result<Option<String>, String> {
    let Some(key) = key.map(str::trim).filter(|k| !k.is_empty()) else {
        return Ok(None);
    };
    if !valid_key(key) {
        return Err(format!("\"{key}\" is not a usable account key"));
    }
    match find_account(root, key)? {
        Some(_) => Ok(Some(key.to_string())),
        None => Err(format!("there is no account \"{key}\" on this machine - add it in Auto Run, Accounts")),
    }
}
```

(`accounts.rs` already imports `std::path::Path`; if not, add `use std::path::Path;`.)

`src-tauri/src/commands/autorun_replay.rs`: change the `use crate::autorun::replay::{self, Browsers};` line to `use crate::autorun::replay::{self, Browsers, CaseToRun};`, replace `ReplayCase` with:

```rust
/// A case from the frontend's selection: enough to run it (`case_id`),
/// enough to report on it before its script has even loaded (`title`), and
/// its Module field for the module paths.
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct ReplayCase {
    pub case_id: i32,
    pub title: String,
    /// Absent or blank when the test case has no Module.
    #[serde(default)]
    pub module: Option<String>,
}
```

and replace `auto_run_replay` with:

```rust
/// Run the selection unattended and return the finished run. Progress
/// arrives as `ReplayProgress` events while this is pending. `account`
/// signs in every script that names no account of its own; it must be a
/// key in the Accounts list, or the run does not start.
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn auto_run_replay(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    pbi_id: i32,
    cases: Vec<ReplayCase>,
    account: Option<String>,
    browser_name: String,
    watch: bool,
) -> Result<LocalRun, String> {
    let _claim = OneAtATime::claim().ok_or_else(|| {
        "an unattended run is already going - wait for it, or stop it first".to_string()
    })?;
    if super::autorun::supervised_session_is_open().await {
        return Err("close the supervised browser first".to_string());
    }
    CANCEL.store(false, Ordering::SeqCst);

    let root = super::autorun::root(&app)?;
    let run_account = crate::autorun::accounts::account_for_run(&root, account.as_deref())?;
    let mut run = LocalRun {
        id: store::new_run_id(),
        pbi_id,
        started_at: sessions::now_ms().to_string(),
        cases: vec![],
        mode: "unattended".to_string(),
        published: None,
    };

    let list: Vec<CaseToRun> = cases
        .iter()
        .map(|c| CaseToRun { case_id: c.case_id, title: c.title.clone(), module: c.module.clone() })
        .collect();
    let timing = replay_timing(watch);
    let mut browsers =
        RealBrowsers { which: Browser::from_name(&browser_name), watch, current: None };

    let outcome = replay::run_cases(
        &mut browsers,
        &root,
        &organization,
        &project,
        &mut run,
        &list,
        run_account.as_deref(),
        &timing,
        &CANCEL,
        &mut |p: ReplayProgress| {
            let _ = p.emit(&app);
        },
    )
    .await;

    // Counts only - never a case title or a failure detail. See house
    // rules: a password or page-specific detail never reaches the log at
    // this level, and neither does anything that would make this line grow
    // without bound for a big selection.
    let passed = run.cases.iter().filter(|c| c.proposed == "Passed").count();
    let failed = run.cases.iter().filter(|c| c.proposed == "Failed").count();
    let blocked = run.cases.iter().filter(|c| c.proposed == "Blocked").count();
    crate::applog::info(format!(
        "Auto-run unattended: {} cases, proposed {passed} passed / {failed} failed / {blocked} blocked",
        list.len(),
    ));

    match outcome {
        Ok(()) => Ok(run),
        Err(e) => Err(format!("the run did not finish: {e}")),
    }
}
```

(The last line's wording changes from "could not be saved" because `run_cases` can now also refuse at the start with an unreadable module paths file.)

- [ ] **Step 4: Run** `--test autorun_accounts`, `--test autorun_replay`, `--test autorun_commands`, then `--test bindings` (regenerates `autoRunReplay` with `account` and `ReplayCase.module`).

- [ ] **Step 5: Write the failing frontend tests.**

`src/screens/AutoRun/ReplayPane.test.tsx`: in the test "start sends the selection, the browser and the watch choice", change the expected object to:

```tsx
  expect(calls[0]).toEqual({
    organization: "acme",
    project: "Web",
    pbiId: 42,
    cases: [
      { case_id: 1, title: "A", module: null },
      { case_id: 2, title: "B", module: null },
    ],
    account: null,
    browserName: "edge",
    watch: false,
  });
```

In "statusOf reads a progress event's phase", add:

```tsx
  expect(statusOf({ phase: "module", step_number: -1, steps: 3, proposed: "" })).toBe(
    "Going to the module",
  );
```

and append:

```tsx
const ACCOUNTS = [{ key: "hr.admin", label: "HR Admin", username: "kim", password: "p" }];

function mountForAccount(calls: unknown[], asked: string[] = []) {
  mockIPC(
    (cmd, args) => {
      asked.push(String(cmd));
      if (cmd === "auto_run_list_accounts") return ACCOUNTS;
      if (cmd === "auto_run_replay") {
        calls.push(args);
        return new Promise(() => {});
      }
      return null;
    },
    { shouldMockEvents: true },
  );
  return render(
    <ReplayPane
      org="acme"
      project="Web"
      pbiId={42}
      cases={[{ id: 1, title: "A", module: " Leave " }]}
      onClose={vi.fn()}
      onFinished={vi.fn()}
    />,
  );
}

test("the account for the run is remembered per project and sent with each case's module", async () => {
  const calls: unknown[] = [];
  const { unmount } = mountForAccount(calls);
  const pick = await screen.findByRole("combobox", { name: "Sign in as" });
  expect(pick).toHaveTextContent("Each script's own account");
  fireEvent.click(pick);
  fireEvent.click(await screen.findByRole("option", { name: "HR Admin (hr.admin)" }));
  expect(localStorage.getItem("tcm-v2-autorun-run-account:acme/Web")).toBe("hr.admin");
  fireEvent.click(screen.getByRole("button", { name: "Start" }));
  await waitFor(() => expect(calls).toHaveLength(1));
  expect(calls[0]).toEqual(
    expect.objectContaining({ account: "hr.admin", cases: [{ case_id: 1, title: "A", module: "Leave" }] }),
  );
  unmount();

  mountForAccount([]);
  await waitFor(() =>
    expect(screen.getByRole("combobox", { name: "Sign in as" })).toHaveTextContent("HR Admin (hr.admin)"),
  );
});

/// Review focus 3.
test("an account removed since it was picked falls back to each script's own, silently", async () => {
  localStorage.setItem("tcm-v2-autorun-run-account:acme/Web", "gone.user");
  const calls: unknown[] = [];
  const asked: string[] = [];
  mountForAccount(calls, asked);
  const pick = await screen.findByRole("combobox", { name: "Sign in as" });
  // Let the accounts list arrive, so the fallback is judged against it.
  await waitFor(() => expect(asked).toContain("auto_run_list_accounts"));
  await act(async () => {
    await Promise.resolve();
  });
  expect(pick).toHaveTextContent("Each script's own account");
  fireEvent.click(screen.getByRole("button", { name: "Start" }));
  await waitFor(() => expect(calls).toHaveLength(1));
  expect(calls[0]).toEqual(expect.objectContaining({ account: null }));
  expect(screen.queryByText(/not on this machine/)).not.toBeInTheDocument();
});
```

`src/screens/AutoRun/RunReview.test.tsx`: change line 11 to `import RunReview, { stepLabel } from "./RunReview";` and append:

```tsx
test("the review names the sign-in, the trip to the module and each step", () => {
  expect(stepLabel(0)).toBe("Sign in");
  expect(stepLabel(-1)).toBe("Module");
  expect(stepLabel(3)).toBe("Step 3");
});
```

`src/screens/AutoRun.test.tsx`: in the `allowed` set, after `"auto_run_replay", ...`, add:

```tsx
    "auto_run_list_accounts", // read-only: the Sign in as choices in the unattended run dialog
```

- [ ] **Step 6: Run to see them fail:** `npx vitest run --exclude "**/.claude/**" src/screens/AutoRun/ReplayPane.test.tsx src/screens/AutoRun/RunReview.test.tsx`. Expected: missing combobox "Sign in as", missing `stepLabel`, wrong args.

- [ ] **Step 7: Implement the frontend.** Replace `src/screens/AutoRun/ReplayPane.tsx` with:

```tsx
// Driving a whole selection unattended: the app opens its own browser per
// case, works through the script alone, and proposes a verdict. Nobody is
// watching by default - the person can leave and come back once it says
// "case N of M" is done.
//
// Nothing here is a verdict. `proposed` is the machine's best guess at what
// a person would have picked; it becomes a real verdict only once someone
// reviews it, and only a reviewed run can ever reach Azure DevOps.

import { useEffect, useRef, useState } from "react";
import { commands, events } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import { IconCancel, IconStop, IconUnattended } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";

// Same two browsers, same values, as the supervised pane's picker - and the
// same storage key, so a person's choice there is their choice here too.
const BROWSERS = [
  { value: "edge", label: "Microsoft Edge" },
  { value: "chrome", label: "Google Chrome" },
];

/** Where the account picked for a run is remembered: per organisation and
 * project, on this machine only. */
export function runAccountKey(org: string, project: string): string {
  return `tcm-v2-autorun-run-account:${org}/${project}`;
}

/** The one line a row shows for the phase an unattended step is in - a pure
 * function so a test can drive it directly instead of through an event. */
export function statusOf(p: {
  phase: string;
  step_number: number;
  steps: number;
  proposed: string;
}): string {
  if (p.phase === "opening") return "Opening the browser";
  if (p.phase === "signing_in") return "Signing in";
  if (p.phase === "module") return "Going to the module";
  if (p.phase === "step") return `Step ${p.step_number} of ${p.steps}`;
  if (p.phase === "done") return p.proposed ? `Proposed: ${p.proposed}` : "Nothing proposed";
  return "Waiting";
}

export default function ReplayPane({
  org,
  project,
  pbiId,
  cases,
  onClose,
  onFinished,
}: {
  org: string;
  project: string;
  pbiId: number;
  /** The selection, run in this order - same contract as the supervised
   * pane's `cases` prop. `module` is the case's Module field. */
  cases: { id: number; title: string; module?: string }[];
  onClose: () => void;
  /** Called with the finished run's id once `auto_run_replay` resolves.
   * The review screen opens from it. */
  onFinished: (runId: string) => void;
}) {
  const [phase, setPhase] = useState<"setup" | "running" | "failed">("setup");
  const [error, setError] = useState("");

  /** Same key the supervised pane uses - a stored value the picker cannot
   * show falls back to Edge, same reasoning as there. */
  const [browserName, setBrowserName] = useState(() => {
    const saved = localStorage.getItem("tcm-v2-autorun-browser");
    return BROWSERS.some((b) => b.value === saved) ? (saved as string) : "edge";
  });
  /** Off by default: an unattended run's whole point is that nobody has to
   * sit in front of it. */
  const [watch, setWatch] = useState(() => localStorage.getItem("tcm-v2-autorun-watch") === "1");

  /** The tester's accounts, key and name only: the Sign in as choices. */
  const [accounts, setAccounts] = useState<{ key: string; label: string }[]>([]);
  useEffect(() => {
    let live = true;
    commands
      .autoRunListAccounts()
      .then((r) => {
        if (live && r.status === "ok") {
          setAccounts((r.data ?? []).map((a) => ({ key: a.key, label: a.label })));
        }
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);
  const [picked, setPicked] = useState(() => {
    try {
      return localStorage.getItem(runAccountKey(org, project)) ?? "";
    } catch {
      return "";
    }
  });
  /** An account removed since it was picked falls back to the default,
   * silently: the run just uses each script's own account. */
  const runAccount = accounts.some((a) => a.key === picked) ? picked : "";

  /** Status text per case id, filled in as `ReplayProgress` events arrive. */
  const [rows, setRows] = useState<Record<number, string>>({});
  /** The selection's own "case N of M" line - null until the first event. */
  const [position, setPosition] = useState<{ index: number; total: number } | null>(null);
  const [stopping, setStopping] = useState(false);

  /** The run this pane's own Start kicked off. Only set from the FIRST
   * progress event seen after Start - a stray event from a run this pane
   * did not start (a previous one that outlived its pane, say) must not
   * overwrite what is on screen for the run actually in progress. */
  const runId = useRef<string | null>(null);

  useEffect(() => {
    const un = events.replayProgress.listen((e) => {
      const p = e.payload;
      if (runId.current === null) runId.current = p.run_id;
      if (p.run_id !== runId.current) return;
      setRows((prev) => ({ ...prev, [p.case_id]: statusOf(p) }));
      setPosition({ index: p.index, total: p.total });
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  const start = async () => {
    setPhase("running");
    setError("");
    setRows({});
    setPosition(null);
    setStopping(false);
    runId.current = null;
    try {
      const r = await commands.autoRunReplay(
        org,
        project,
        pbiId,
        cases.map((c) => ({ case_id: c.id, title: c.title, module: c.module?.trim() || null })),
        runAccount || null,
        browserName,
        watch,
      );
      if (r.status === "error") {
        setError(r.error);
        setPhase("failed");
        return;
      }
      onFinished(r.data.id);
    } catch (e) {
      // Same rethrow hazard as the supervised pane's own IPC calls - the
      // generated wrapper rethrows an Error rather than resolving to
      // {status: "error"} when the call itself rejects.
      setError(e instanceof Error ? e.message : String(e));
      setPhase("failed");
    }
  };

  const stop = async () => {
    // Said and disabled the instant the person presses it - the run itself
    // only stops after its current step, and there is nothing else useful
    // to tell them until it does.
    setStopping(true);
    await commands.autoRunReplayCancel().catch(() => {});
  };

  /** The dialog closes on Escape/backdrop everywhere except while a run is
   * actually going - closing the window would not stop it, only Stop does,
   * so letting it look closeable there would be a lie. */
  const closeIfIdle = () => {
    if (phase === "running") return;
    onClose();
  };

  return (
    <Modal onClose={closeIfIdle} className="w-full max-w-2xl space-y-3 p-4">
      <h2 className="text-sm font-semibold text-text">Unattended run</h2>

      {phase !== "running" ? (
        <div className="space-y-3">
          {error && <p className="text-xs text-danger">{error}</p>}
          <label className="flex items-center gap-2 text-xs text-muted">
            Sign in as
            <Select
              aria-label="Sign in as"
              className="w-56"
              value={runAccount}
              onChange={(e) => {
                setPicked(e.target.value);
                try {
                  localStorage.setItem(runAccountKey(org, project), e.target.value);
                } catch {
                  // storage unavailable - the choice lasts this session
                }
              }}
            >
              <option value="">Each script's own account</option>
              {accounts.map((a) => (
                <option key={a.key} value={a.key}>
                  {a.label ? `${a.label} (${a.key})` : a.key}
                </option>
              ))}
            </Select>
          </label>
          <p className="text-xs text-faint">Used only for scripts that name no account.</p>
          <label className="flex items-center gap-2 text-xs text-muted">
            Browser
            <Select
              aria-label="Browser to run in"
              className="w-40"
              value={browserName}
              onChange={(e) => {
                setBrowserName(e.target.value);
                try {
                  localStorage.setItem("tcm-v2-autorun-browser", e.target.value);
                } catch {
                  // storage unavailable - the choice lasts this session
                }
              }}
            >
              {BROWSERS.map((b) => (
                <option key={b.value} value={b.value}>
                  {b.label}
                </option>
              ))}
            </Select>
          </label>
          <label className="flex cursor-pointer items-center gap-2 text-xs text-muted">
            <Checkbox
              checked={watch}
              ariaLabel="Watch the browser"
              onCheckedChange={(on) => {
                setWatch(on);
                try {
                  localStorage.setItem("tcm-v2-autorun-watch", on ? "1" : "0");
                } catch {
                  // storage unavailable - the choice lasts this session
                }
              }}
            />
            Watch the browser
          </label>
          <p className="text-xs text-faint">
            Off: the browser runs in the background and you can keep working. On: a window opens
            for every case.
          </p>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" size="sm" onClick={onClose}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button size="sm" onClick={start}>
              <IconUnattended aria-hidden />
              Start
            </Button>
          </div>
        </div>
      ) : (
        <div className="space-y-3">
          {position && (
            <p className="text-xs text-muted">
              case {position.index + 1} of {position.total}
            </p>
          )}
          <ul className="max-h-72 space-y-1 overflow-y-auto">
            {cases.map((c) => (
              <li
                key={c.id}
                className="flex items-center gap-2 rounded-md border border-border px-2 py-1.5 text-xs"
              >
                <span className="id-mono text-faint">#{c.id}</span>
                <span className="min-w-0 flex-1 truncate text-text">{c.title}</span>
                <span className={cn("text-muted", rows[c.id]?.startsWith("Proposed") && "text-text")}>
                  {rows[c.id] ?? "Waiting"}
                </span>
              </li>
            ))}
          </ul>
          <div className="flex justify-end">
            <Button size="sm" variant="outline" disabled={stopping} onClick={stop}>
              <IconStop aria-hidden />
              {stopping ? "Stopping after this step" : "Stop"}
            </Button>
          </div>
        </div>
      )}
    </Modal>
  );
}
```

`src/screens/AutoRun/RunReview.tsx`: above `export default function RunReview`, add:

```tsx
/** What a step's line is called in the review: the sign-in, the runner's
 * own trip to the case's module, or the case's own step. */
export function stepLabel(stepNumber: number): string {
  if (stepNumber === 0) return "Sign in";
  if (stepNumber === -1) return "Module";
  return `Step ${stepNumber}`;
}
```

and replace `{s.step_number === 0 ? "Sign in" : `Step ${s.step_number}`}` with `{stepLabel(s.step_number)}`.

`src/screens/AutoRun/index.tsx`: in the `replaying != null` block, change `.map((c) => ({ id: c.id, title: c.title }));` to `.map((c) => ({ id: c.id, title: c.title, module: c.module_value }));` (only in the `replaying` block; the supervised `running` block stays as it is).

- [ ] **Step 8: Run** (one at a time): `npx vitest run --exclude "**/.claude/**" src/screens/AutoRun/ReplayPane.test.tsx src/screens/AutoRun/RunReview.test.tsx src/screens/AutoRun.test.tsx src/ui-consistency.test.ts`, then `npx tsc --noEmit`. All green.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/autorun/accounts.rs src-tauri/src/commands/autorun_replay.rs src-tauri/tests src/bindings.ts src/screens/AutoRun src/screens/AutoRun.test.tsx
git commit -q -F - <<'EOF'
feat(v2): an unattended run can sign in as one chosen account, and carries each case's Module

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 4: Scripts may not open pages by address

**Files:**
- Modify: `src-tauri/src/autorun/nav.rs` (address rule, guide section, `is_route_problem`)
- Modify: `src-tauri/src/autorun/runner.rs` (`run_step_routed` refuses a saved `navigate`)
- Modify: `src-tauri/src/commands/autorun.rs` (`auto_run_save_script`, `save_script_from_editor`, `auto_run_import_scripts`, `import_scripts_from_path`)
- Modify: `src-tauri/src/ai_bridge.rs` (`save_autorun_scripts`, `autorun_guide_with_quirks`)
- Test: `src-tauri/tests/autorun_nav.rs`, `autorun_runner.rs`, `autorun_replay.rs`, `autorun_commands.rs`, `autorun_bridge.rs`
- Modify: `src/screens/AutoRun/ScriptEditor.tsx` + test, `src/screens/AutoRun/index.tsx`, `src/screens/AutoRun.test.tsx`
- Generated: `src/bindings.ts`

**Interfaces:**
- Consumes: Task 1's `NavFile`, `load_nav`; Task 2's `run_step_routed`, `is_route_problem`.
- Produces (in `nav`): `pub const NO_ADDRESS_PREFIX: &str`, `pub fn no_address(step: i32) -> String`, `pub fn check_no_addresses(nav: &NavFile, scripts: &[CaseScript]) -> Result<(), String>` (error `case <id>: <no_address(step)>`), `pub fn refuse_addresses(root, org, project, scripts: &[CaseScript]) -> Result<(), String>`, `pub fn guide_section(nav: &NavFile) -> String` ("" while the switch is on).
- Produces (commands): `auto_run_save_script(organization, project, script)` [`autoRunSaveScript(organization, project, script)`], `auto_run_import_scripts(organization, project, path)` [`autoRunImportScripts(organization, project, path)`], `pub fn save_script_from_editor(root: &Path, organization: &str, project: &str, script: CaseScript) -> Result<(), String>`, `pub fn import_scripts_from_path(root: &Path, organization: &str, project: &str, path: &str) -> Result<Vec<i32>, String>`.
- Produces (TS): `ScriptEditor` gains `project?: string`.

- [ ] **Step 1: Write the failing tests.**

`src-tauri/tests/autorun_nav.rs`, add `check_no_addresses, guide_section, no_address` to the `use` list and append:

```rust
fn case_with(actions: serde_json::Value) -> v2_lib::autorun::CaseScript {
    serde_json::from_value(json!({ "case_id": 7, "title": "t", "steps": [
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] },
        { "step_number": 2, "actions": actions }
    ] }))
    .unwrap()
}

#[test]
fn the_address_sentence_is_the_designs_own_words() {
    assert_eq!(
        no_address(2),
        "this project does not allow opening pages by address: a run starts on the case's module screen - use clicks instead of \"navigate\" (step 2)."
    );
}

#[test]
fn with_the_switch_off_any_navigate_absolute_or_relative_is_refused_and_named() {
    let off = NavFile { direct_urls: false, modules: vec![] };
    for url in ["https://hr.example.internal/hr/leave", "/hr/leave/apply"] {
        let sc = case_with(json!([{ "kind": "navigate", "url": url }]));
        assert_eq!(check_no_addresses(&off, &[sc.clone()]).unwrap_err(), format!("case 7: {}", no_address(2)));
        assert!(check_no_addresses(&NavFile::default(), &[sc]).is_ok(), "on by default");
    }
    let clicks_only = case_with(json!([{ "kind": "click", "selector": { "role": "link", "name": "Leave" } }]));
    assert!(check_no_addresses(&off, &[clicks_only]).is_ok());
}

#[test]
fn the_guide_section_is_there_only_while_the_switch_is_off() {
    assert_eq!(guide_section(&NavFile::default()), "");
    let text = guide_section(&NavFile { direct_urls: false, modules: vec![] });
    assert!(text.starts_with("## This project's runs start on the module screen"), "{text}");
    for must in ["before step 1", "starts there", "Never use `navigate`", "`sign_in`"] {
        assert!(text.contains(must), "missing {must:?}: {text}");
    }
    assert!(!text.contains('\u{2014}'), "no em dashes in text an assistant reads");
}
```

`src-tauri/tests/autorun_runner.rs`, append:

```rust
#[tokio::test]
async fn with_addresses_switched_off_a_saved_navigate_fails_with_the_projects_sentence_and_stops_the_step() {
    let dir = tempfile::tempdir().unwrap();
    v2_lib::autorun::nav::save_nav(
        dir.path(),
        "Acme",
        "Web",
        &v2_lib::autorun::nav::NavFile { direct_urls: false, modules: vec![] },
    )
    .unwrap();
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    let mut acc: Option<String> = None;
    let step = StepScript {
        step_number: 4,
        actions: vec![Action::Navigate { url: "/hr/leave/apply".into() }, Action::CheckText { value: "Leave".into() }],
        unchecked: None,
    };
    let out = run_step(&mut d, dir.path(), "Acme", "Web", &step, &quick(), &mut acc).await.unwrap();
    assert!(!out[0].ok);
    assert_eq!(out[0].detail, v2_lib::autorun::nav::no_address(4));
    assert_eq!(out[1].detail, "not run: this step opened a page by address, which this project does not allow");
    assert!(d.calls_to("Page.navigate").is_empty());
}
```

`src-tauri/tests/autorun_replay.rs`, add `no_address` to the `nav` import and append:

```rust
#[tokio::test]
async fn a_script_saved_before_addresses_were_switched_off_is_blocked_at_its_navigate() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_nav(root, "Acme", "Web", &NavFile { direct_urls: false, modules: vec![] }).unwrap();
    // `save_script` does not apply the rule: this is a file from before.
    store::save_script(
        root,
        &script(1, None, serde_json::json!([{ "step_number": 1, "actions": [
            { "kind": "navigate", "url": "https://app.example/leave" },
            { "kind": "check_text", "value": "yes" }
        ] }])),
    )
    .unwrap();
    let mut browsers = browsers_of(vec![common::FakePage::default().driver()]);
    let mut run = new_run("run-x");
    let cancel = AtomicBool::new(false);
    run_selection(&mut browsers, root, "Acme", "Web", &mut run, &[(1, "case 1".to_string())], &quick(), &cancel, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(run.cases[0].proposed, "Blocked");
    assert_eq!(run.cases[0].reason, no_address(1));
}
```

`src-tauri/tests/autorun_commands.rs`: change the `commands::autorun` import to

```rust
use v2_lib::commands::autorun::{
    describe_session_error, import_scripts_from_path, refuse_while_a_run_is_going, safe_run_id, save_script_from_editor,
};
```

add `use v2_lib::autorun::nav::{no_address, save_nav, NavFile};` and `use v2_lib::autorun::CaseScript;`, change the existing call in `importing_a_utf8_file_with_a_bom_keeps_non_ascii_text_intact` to `import_scripts_from_path(&root, "acme", "Web", file.to_str().unwrap())`, and append:

```rust
fn with_navigate(case_id: i32) -> serde_json::Value {
    serde_json::json!({ "case_id": case_id, "title": "t", "steps": [
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] },
        { "step_number": 2, "actions": [{ "kind": "navigate", "url": "/hr/leave/apply" }] }
    ] })
}

#[test]
fn with_addresses_switched_off_the_editor_and_an_import_refuse_a_navigate_and_write_nothing() {
    let dir = TempDir::new();
    let root = dir.path().join("data");
    save_nav(&root, "acme", "Web", &NavFile { direct_urls: false, modules: vec![] }).unwrap();
    let script: CaseScript = serde_json::from_value(with_navigate(7)).unwrap();

    let err = save_script_from_editor(&root, "acme", "Web", script.clone()).unwrap_err();
    assert_eq!(err, format!("case 7: {}", no_address(2)));
    assert!(load_script(&root, 7).unwrap().is_none());

    let file = dir.path().join("bundle.json");
    std::fs::write(&file, serde_json::Value::Array(vec![with_navigate(8)]).to_string()).unwrap();
    let err = import_scripts_from_path(&root, "acme", "Web", file.to_str().unwrap()).unwrap_err();
    assert_eq!(err, format!("case 8: {}", no_address(2)));
    assert!(load_script(&root, 8).unwrap().is_none());

    // Another project, whose switch was never touched, takes the same script.
    save_script_from_editor(&root, "acme", "Other", script).unwrap();
    assert!(load_script(&root, 7).unwrap().is_some());
}
```

`src-tauri/tests/autorun_bridge.rs`, append:

```rust
#[tokio::test]
async fn with_addresses_switched_off_a_bundle_with_a_navigate_is_refused_before_anything_else() {
    let dir = TempDir::new();
    let _root = ROOT_LOCK.lock().unwrap();
    set_root(dir.path().to_path_buf());
    v2_lib::autorun::nav::set_direct_urls(dir.path(), "acme", "Web", false).unwrap();
    let body = case_7("#toast", "Saved").to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, format!("case 7: {}", v2_lib::autorun::nav::no_address(1)));
    assert!(load_script(dir.path(), 7).unwrap().is_none());
}

#[tokio::test]
async fn the_guide_says_a_run_starts_on_the_module_screen_only_while_addresses_are_off() {
    let dir = TempDir::new();
    let _root = ROOT_LOCK.lock().unwrap();
    set_root(dir.path().to_path_buf());
    let (_, on) = route(&ctx(), None, "GET", "/autorun-guide", "", "1.0.0").await;
    assert!(!on.contains("## This project's runs start on the module screen"), "{on}");
    v2_lib::autorun::nav::set_direct_urls(dir.path(), "acme", "Web", false).unwrap();
    let (status, off) = route(&ctx(), None, "GET", "/autorun-guide", "", "1.0.0").await;
    assert_eq!(status, 200);
    assert!(off.contains("## This project's runs start on the module screen"), "{off}");
    assert!(!off.contains('\u{2014}'));
}
```

- [ ] **Step 2: Run to see them fail:** `--test autorun_nav`, `--test autorun_runner`, `--test autorun_replay`, `--test autorun_commands`, `--test autorun_bridge`, one at a time. Expected: missing `no_address`, `save_script_from_editor`, wrong arity for `import_scripts_from_path`.

- [ ] **Step 3: Implement `nav.rs` additions.** Add `use super::CaseScript;` to the `use` block, then append:

```rust
/// Start of the sentence a saved `navigate` gets while the switch is off.
pub const NO_ADDRESS_PREFIX: &str = "this project does not allow opening pages by address";

pub fn no_address(step: i32) -> String {
    format!(
        "{NO_ADDRESS_PREFIX}: a run starts on the case's module screen - use clicks instead of \"navigate\" (step {step})."
    )
}

/// While the switch is off, a script with any `navigate` (absolute or
/// relative) cannot be saved. Names the first case and step it finds.
pub fn check_no_addresses(nav: &NavFile, scripts: &[CaseScript]) -> Result<(), String> {
    if nav.direct_urls {
        return Ok(());
    }
    for sc in scripts {
        for step in &sc.steps {
            if step.actions.iter().any(|a| matches!(a, Action::Navigate { .. })) {
                return Err(format!("case {}: {}", sc.case_id, no_address(step.step_number)));
            }
        }
    }
    Ok(())
}

/// `check_no_addresses` against the project's own file: the one call every
/// save path makes (the Script editor, a JSON import, the assistant's
/// `save_autorun_script`).
pub fn refuse_addresses(root: &Path, org: &str, project: &str, scripts: &[CaseScript]) -> Result<(), String> {
    check_no_addresses(&load_nav(root, org, project)?, scripts)
}

/// What an assistant's guide gains while the switch is off. Empty while it
/// is on.
pub fn guide_section(nav: &NavFile) -> String {
    if nav.direct_urls {
        return String::new();
    }
    "## This project's runs start on the module screen\n\n\
     - The run signs in and goes to the case's module screen before step 1, by the menu path recorded in the app.\n\
     - The script starts there: its first action acts on the module screen.\n\
     - Never use `navigate`. This project refuses to save a script that opens a page by address; reach every other screen with clicks.\n\
     - A `sign_in` action lands on the home page, and the run brings the browser back to the module screen before the next action.\n"
        .to_string()
}
```

and replace `is_route_problem` with:

```rust
/// An outcome that means the run could not put the case where its steps
/// begin, or the script tried to open a page by address where that is not
/// allowed: the case is Blocked, not Failed.
pub fn is_route_problem(detail: &str) -> bool {
    detail.contains(UNREACHED_PREFIX) || detail.starts_with(NO_ADDRESS_PREFIX)
}
```

- [ ] **Step 4: `runner.rs`.** Add the constant under the other two:

```rust
const AFTER_REFUSED_ADDRESS: &str = "not run: this step opened a page by address, which this project does not allow";
```

In `run_step_routed`, after `let policy = policy_for(recipe.as_ref());` add:

```rust
    // Read per step, like the recipe: a person may flip the switch between
    // two steps of a supervised run.
    let nav_file = nav::load_nav(root, organization, project)?;
```

and in the `match action {`, insert this arm before `other => execute_in(...)`:

```rust
            // A script saved before the switch was turned off. The runner's
            // own trip home never comes through here.
            Action::Navigate { .. } if !nav_file.direct_urls => {
                blocked = Some(AFTER_REFUSED_ADDRESS);
                ActionOutcome::failed(nav::no_address(step.step_number))
            }
```

- [ ] **Step 5: `commands/autorun.rs`.** Replace `auto_run_save_script` with:

```rust
#[tauri::command]
#[specta::specta]
pub fn auto_run_save_script(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    script: CaseScript,
) -> Result<(), String> {
    save_script_from_editor(&root(&app)?, &organization, &project, script)
}

/// The pure half of [`auto_run_save_script`], so a test can reach it
/// without an `AppHandle`.
pub fn save_script_from_editor(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    mut script: CaseScript,
) -> Result<(), String> {
    // A person saving from the editor is a fresh start for the assistant's
    // repair count, whatever the editor happened to send - and the reason
    // for the last one is no longer relevant once a person has looked.
    script.repairs = 0;
    script.last_repair = None;
    // The project's address rule, the same one the import and the
    // assistant's save apply.
    crate::autorun::nav::refuse_addresses(root, organization, project, std::slice::from_ref(&script))?;
    // Through the same helper the bundle paths use, as a bundle of one:
    // the script editor is a THIRD way in, and a case id of 0 or an empty
    // step list refused from a file but accepted from the editor would be
    // a rule that depends on which door you came through.
    store::save_scripts_atomically(root, std::slice::from_ref(&script)).map_err(|e| e.to_string())
}
```

Change `auto_run_import_scripts` to:

```rust
#[tauri::command]
#[specta::specta]
pub fn auto_run_import_scripts(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    path: String,
) -> Result<Vec<i32>, String> {
    import_scripts_from_path(&root(&app)?, &organization, &project, &path)
}
```

(keep its doc comment), change `import_scripts_from_path`'s signature to `pub fn import_scripts_from_path(root: &std::path::Path, organization: &str, project: &str, path: &str) -> Result<Vec<i32>, String>`, and directly after its `if scripts.is_empty() { ... }` block add:

```rust
    crate::autorun::nav::refuse_addresses(root, organization, project, &scripts)?;
```

- [ ] **Step 6: `ai_bridge.rs`.** In `save_autorun_scripts`, directly after the `let root = match autorun_root() { ... };` statement, add:

```rust
    // Gate 0: a project whose runs start on the module screen refuses a
    // script that opens pages by address - before anything is read from
    // disk or Azure DevOps.
    if let Err(why) = crate::autorun::nav::refuse_addresses(&root, &ctx.org, &ctx.project, &scripts) {
        return (400, why);
    }
```

Replace `autorun_guide_with_quirks` with:

```rust
/// The guide's own text, plus this project's sections when it has any: the
/// module-screen rule while "Scripts may open pages by address" is off,
/// then the recorded quirks. The constant (`autorun::guide::autorun_guide`)
/// only says a quirks section exists; this reads what is actually on file,
/// so the guide can never go stale on a live project.
fn autorun_guide_with_quirks(ctx: &BridgeContext) -> String {
    let base = crate::autorun::guide::autorun_guide();
    if ctx.project.trim().is_empty() {
        return base;
    }
    let Some(root) = crate::autorun::store::configured_root() else {
        return base;
    };
    let nav = crate::autorun::nav::load_nav(&root, &ctx.org, &ctx.project).unwrap_or_default();
    let quirks = crate::autorun::quirks::load_quirks(&root, &ctx.org, &ctx.project).unwrap_or_default();
    let mut out = base;
    for section in [crate::autorun::nav::guide_section(&nav), crate::autorun::quirks::quirks_section(&quirks)] {
        if !section.is_empty() {
            out.push('\n');
            out.push_str(&section);
        }
    }
    out
}
```

- [ ] **Step 7: Run** the five Rust test files again (one at a time), then `--test autorun_guide`, `--test ai_bridge`, then `--test bindings` (regenerates `autoRunSaveScript(organization, project, script)` and `autoRunImportScripts(organization, project, path)`).

- [ ] **Step 8: The frontend callers and their tests.**

`src/screens/AutoRun/ScriptEditor.test.tsx`, append:

```tsx
test("saving names the organization and project, so the project's address rule applies", async () => {
  const sent: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_load_script") return { case_id: 7, title: "t", steps: ONE_STEP };
    if (cmd === "auto_run_list_accounts") return ACCOUNTS;
    if (cmd === "auto_run_save_script") {
      sent.push(args);
      return null;
    }
    return null;
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ScriptEditor caseId={7} title="t" steps={[]} org="acme" project="Web" onClose={vi.fn()} />
    </QueryClientProvider>,
  );
  await screen.findByRole("combobox", { name: "Runs as" });
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(sent).toHaveLength(1));
  expect(sent[0]).toEqual(expect.objectContaining({ organization: "acme", project: "Web" }));
});
```

`src/screens/AutoRun.test.tsx`, in "importing scripts sends the picked file's path, and the badge updates", change the expectation to:

```tsx
  expect(receivedArgs).toEqual({ organization: "acme", project: "Web", path: "C:\\scripts.json" });
```

`src/screens/AutoRun/ScriptEditor.tsx`: in the props destructuring add `project,` after `org,`, in the props type add `project?: string;` after `org?: string;`, and change the save call to:

```tsx
    const r = await commands.autoRunSaveScript(org ?? "", project ?? "", {
      case_id: caseId,
      title,
      steps: parsed,
      account: account === "" ? null : account,
    });
```

`src/screens/AutoRun/index.tsx`: change `const r = await commands.autoRunImportScripts(path);` to `const r = await commands.autoRunImportScripts(org, project, path);`, and in the `<ScriptEditor` render add `project={project}` after `org={org}`.

- [ ] **Step 9: Run** `npx vitest run --exclude "**/.claude/**" src/screens/AutoRun/ScriptEditor.test.tsx src/screens/AutoRun.test.tsx`, then `npx tsc --noEmit`. Green.

- [ ] **Step 10: Commit**

```bash
git add src-tauri/src src-tauri/tests src/bindings.ts src/screens/AutoRun src/screens/AutoRun.test.tsx
git commit -q -F - <<'EOF'
feat(v2): a project can refuse scripts that open pages by address, at every save and in a run

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 5: The recorder core

**Files:**
- Create: `src-tauri/src/autorun/recorder.rs`; add `pub mod recorder;` to `src-tauri/src/autorun/mod.rs` after `pub mod recipe;`
- Modify: `src-tauri/src/autorun/nav.rs` (`check_path`)
- Modify: `src-tauri/tests/common/mod.rs` (`ScriptedDriver::closed_when_drained`)
- Test: `src-tauri/tests/autorun_recorder.rs` (create)

**Interfaces:**
- Consumes: Task 1's `ModulePath`, `path_of`; Task 2's `Route`, `go_to_module`, `PathFailure::for_dialog`; `signin::sign_in`; `page::{document, call_elements, backend_id, release, eval_value}`.
- Produces (in `recorder`): `BINDING`, `LISTENER_JS`, `HELD_JS`, `PREFERRED_ROLES`; `UNREADABLE`, `BROWSER_CLOSED`, `CANCELLED`, `NO_CLICKS`; `pub struct ClickHints { tag, role, label, text: String }`; `pub struct ClickPayload { doc: String, i: u32, hints: ClickHints }`; `pub struct AxLink { role: String, name: String, ignored: bool }`; `pub enum Ended { Stopped { href: String }, Cancelled, Closed }`; `pub struct Captured { clicks: Vec<Target>, ended: Ended }`; `pub fn ax_chain(tree: &Value, backend: i64) -> Vec<AxLink>`; `pub fn locator_from_ax(chain: &[AxLink]) -> Option<Target>`; `pub fn locator_from_hints(h: &ClickHints) -> Option<Target>`; `pub async fn arm<D: Driver>(d) -> Result<(), CdpError>`; `pub async fn next_click<D: Driver>(d, wait: Duration) -> Result<Option<ClickPayload>, CdpError>`; `pub async fn locate<D: Driver>(d, click: &ClickPayload) -> Result<Target, String>`; `pub async fn capture<D: Driver>(d, stop: &AtomicBool, cancel: &AtomicBool, on_click: &mut (dyn FnMut(Result<&Target, &str>) + Send)) -> Captured`; `pub fn finish(module: &str, captured: Captured, recorded: &str) -> Result<ModulePath, String>`.
- Produces (in `nav`): `pub async fn check_path<D: Driver>(d, root: &Path, recipe: &SignInRecipe, account: &Account, path: &ModulePath, timing: &Timing) -> Result<String, String>` (Ok: the arrived path; Err: the dialog form, or `the sign-in did not work: ...`).
- Produces (tests/common): `ScriptedDriver.closed_when_drained: bool` (when true, `wait_event` with nothing buffered returns `CdpError::Closed`).

- [ ] **Step 1: Extend the fake.** In `src-tauri/tests/common/mod.rs`, add to `ScriptedDriver`:

```rust
    /// When no event is waiting, `wait_event` says the browser closed
    /// instead of timing out - a window the person shut.
    pub closed_when_drained: bool,
```

set `closed_when_drained: false,` in `ScriptedDriver::new`, and change `wait_event`'s `None` arm to:

```rust
            None if self.closed_when_drained => Err(CdpError::Closed),
            None => Err(CdpError::Timeout { what: method.to_string(), ms: 0 }),
```

- [ ] **Step 2: Write the failing tests** in `src-tauri/tests/autorun_recorder.rs`:

```rust
//! Recording a module's menu path: clicks reported by the page become the
//! locators a run clicks with, and a recording ends with a path or with
//! the reason nothing was saved.

mod common;

use common::ScriptedDriver;
use serde_json::json;
use std::sync::atomic::AtomicBool;
use v2_lib::autorun::nav::{check_path, ModulePath};
use v2_lib::autorun::recorder::{
    arm, ax_chain, capture, finish, locate, locator_from_ax, locator_from_hints, next_click, AxLink, Captured,
    ClickHints, ClickPayload, Ended, BINDING, BROWSER_CLOSED, CANCELLED, LISTENER_JS, NO_CLICKS, UNREADABLE,
};
use v2_lib::browser::cdp::Event;
use v2_lib::browser::locator::{LocatorStep, Target};

fn exact_role(role: &str, name: &str) -> Target {
    Target::One(LocatorStep { role: Some(role.into()), name: Some(name.into()), exact: true, ..LocatorStep::default() })
}

fn exact_text(text: &str) -> Target {
    Target::One(LocatorStep { text: Some(text.into()), exact: true, ..LocatorStep::default() })
}

fn node(role: &str, name: &str) -> AxLink {
    AxLink { role: role.into(), name: name.into(), ignored: false }
}

fn hints(role: &str, label: &str, text: &str) -> ClickHints {
    ClickHints { tag: "a".into(), role: role.into(), label: label.into(), text: text.into() }
}

/// A page's report of one click, the way the binding delivers it.
fn clicked(i: u32, text: &str) -> Event {
    Event {
        method: "Runtime.bindingCalled".into(),
        params: json!({
            "name": BINDING,
            "payload": json!({ "doc": "d1", "i": i, "tag": "a", "role": "", "label": "", "text": text }).to_string()
        }),
    }
}

/// An accessibility tree around the clicked element (backend 42): the
/// words inside a link "Leave", inside the menu.
fn leave_tree() -> serde_json::Value {
    json!({ "nodes": [
        { "nodeId": "1", "role": { "value": "RootWebArea" }, "name": { "value": "HR" }, "childIds": ["2"] },
        { "nodeId": "2", "role": { "value": "navigation" }, "name": { "value": "" }, "parentId": "1", "childIds": ["3"] },
        { "nodeId": "3", "role": { "value": "link" }, "name": { "value": " Leave " }, "parentId": "2", "childIds": ["4"], "backendDOMNodeId": 41 },
        { "nodeId": "4", "role": { "value": "StaticText" }, "name": { "value": "Leave" }, "parentId": "3", "backendDOMNodeId": 42 }
    ] })
}

/// A page whose held elements are still there (`held[i]`) or gone.
fn page_with(held: Vec<bool>, href: &'static str) -> ScriptedDriver {
    let mut last_i = 0usize;
    ScriptedDriver::new(move |method, params| {
        Ok(match method {
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" if params["expression"] == "location.href" => json!({ "result": { "value": href } }),
            "Runtime.callFunctionOn" => {
                last_i = params["arguments"][1]["value"].as_u64().unwrap_or(0) as usize;
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => {
                if held.get(last_i).copied().unwrap_or(false) {
                    json!({ "result": [{ "name": "0", "value": { "objectId": "el" } }] })
                } else {
                    json!({ "result": [] })
                }
            }
            "DOM.describeNode" => json!({ "node": { "backendNodeId": 42 } }),
            "Accessibility.getPartialAXTree" => leave_tree(),
            _ => json!({}),
        })
    })
}

#[test]
fn the_clicked_elements_own_role_and_name_make_the_locator() {
    assert_eq!(locator_from_ax(&[node("link", "Apply   Leave"), node("navigation", "")]), Some(exact_role("link", "Apply Leave")));
}

#[test]
fn the_nearest_ancestor_with_a_preferred_role_wins_over_the_words_inside_it() {
    let chain = [node("StaticText", "Apply Leave"), node("generic", ""), node("link", "Apply Leave"), node("listitem", ""), node("navigation", "")];
    assert_eq!(locator_from_ax(&chain), Some(exact_role("link", "Apply Leave")));
    let chain = [node("generic", ""), node("menuitem", "Leave"), node("menu", "")];
    assert_eq!(locator_from_ax(&chain), Some(exact_role("menuitem", "Leave")));
    let ignored = [AxLink { role: "button".into(), name: "Hidden".into(), ignored: true }, node("tab", "Leave")];
    assert_eq!(locator_from_ax(&ignored), Some(exact_role("tab", "Leave")));
}

#[test]
fn a_named_role_that_is_not_preferred_is_used_when_nothing_better_is_near() {
    assert_eq!(locator_from_ax(&[node("StaticText", "Leave"), node("heading", "Leave"), node("main", "")]), Some(exact_role("heading", "Leave")));
}

#[test]
fn the_climb_stops_at_a_container_and_finds_nothing_beyond_it() {
    assert_eq!(locator_from_ax(&[node("generic", ""), node("navigation", "Main"), node("link", "Home")]), None);
    assert_eq!(locator_from_ax(&[]), None);
}

#[test]
fn hints_give_a_role_from_aria_label_or_else_the_visible_words() {
    assert_eq!(locator_from_hints(&hints("menuitem", " Leave ", "L")), Some(exact_role("menuitem", "Leave")));
    assert_eq!(locator_from_hints(&hints("", "", "  Apply \n Leave ")), Some(exact_text("Apply Leave")));
    assert_eq!(locator_from_hints(&hints("", "", "")), None);
    assert_eq!(locator_from_hints(&hints("", "", &"x".repeat(81))), None, "a whole panel's text names nothing");
}

#[test]
fn ax_chain_walks_up_by_parent_id_or_else_by_child_ids() {
    let chain = ax_chain(&leave_tree(), 42);
    assert_eq!(chain.iter().map(|n| n.role.as_str()).collect::<Vec<_>>(), vec!["StaticText", "link", "navigation", "RootWebArea"]);
    let no_parent_ids = json!({ "nodes": [
        { "nodeId": "a", "role": { "value": "link" }, "name": { "value": "Leave" }, "childIds": ["b"] },
        { "nodeId": "b", "role": { "value": "StaticText" }, "name": { "value": "Leave" }, "backendDOMNodeId": 7 }
    ] });
    assert_eq!(ax_chain(&no_parent_ids, 7).iter().map(|n| n.role.as_str()).collect::<Vec<_>>(), vec!["StaticText", "link"]);
    assert!(ax_chain(&no_parent_ids, 99).is_empty());
}

#[tokio::test]
async fn arming_adds_the_binding_and_the_listener_to_every_document() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({ "result": {} })));
    arm(&mut d).await.unwrap();
    assert_eq!(d.methods(), vec!["Runtime.enable", "Runtime.addBinding", "Page.addScriptToEvaluateOnNewDocument", "Runtime.evaluate"]);
    assert_eq!(d.calls_to("Runtime.addBinding")[0]["name"], BINDING);
    assert_eq!(d.calls_to("Page.addScriptToEvaluateOnNewDocument")[0]["source"], LISTENER_JS);
    assert!(LISTENER_JS.contains("addEventListener('click'") && LISTENER_JS.contains(", true)"), "capture phase");
}

#[tokio::test]
async fn a_reported_click_is_read_back_and_another_binding_is_ignored() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    d.events.push_back(Event { method: "Runtime.bindingCalled".into(), params: json!({ "name": "other", "payload": "{}" }) });
    d.events.push_back(clicked(3, "Leave"));
    assert_eq!(next_click(&mut d, std::time::Duration::from_millis(10)).await.unwrap(), None);
    let got = next_click(&mut d, std::time::Duration::from_millis(10)).await.unwrap().unwrap();
    assert_eq!(got, ClickPayload { doc: "d1".into(), i: 3, hints: hints("", "", "Leave") });
    assert_eq!(next_click(&mut d, std::time::Duration::from_millis(10)).await.unwrap(), None, "nothing more: a timeout is not an error");
}

#[tokio::test]
async fn locate_asks_the_accessibility_tree_about_the_clicked_element() {
    let mut d = page_with(vec![true], "https://hr.example.internal/hr/leave");
    let click = ClickPayload { doc: "d1".into(), i: 0, hints: hints("", "", "something else") };
    assert_eq!(locate(&mut d, &click).await, Ok(exact_role("link", "Leave")));
    assert_eq!(d.calls_to("Accessibility.getPartialAXTree")[0]["backendNodeId"], 42);
}

#[tokio::test]
async fn locate_falls_back_to_the_hints_when_the_element_has_gone() {
    let mut d = page_with(vec![false], "https://hr.example.internal/hr/leave");
    let click = ClickPayload { doc: "d1".into(), i: 0, hints: hints("", "", "Apply Leave") };
    assert_eq!(locate(&mut d, &click).await, Ok(exact_text("Apply Leave")));
    assert!(d.calls_to("Accessibility.getPartialAXTree").is_empty());
    let nothing = ClickPayload { doc: "d1".into(), i: 0, hints: hints("", "", "") };
    assert_eq!(locate(&mut d, &nothing).await, Err(UNREADABLE.to_string()));
}

#[tokio::test]
async fn capture_keeps_every_reported_click_then_stops_where_the_page_is() {
    let mut d = page_with(vec![true, false], "https://hr.example.internal/hr/leave/apply?tab=2#top");
    d.events.push_back(clicked(0, "Leave"));
    d.events.push_back(clicked(1, "Apply Leave"));
    let (stop, cancel) = (AtomicBool::new(true), AtomicBool::new(false));
    let mut seen: Vec<String> = vec![];
    let captured = capture(&mut d, &stop, &cancel, &mut |c| {
        seen.push(match c {
            Ok(t) => t.describe(),
            Err(why) => why.to_string(),
        })
    })
    .await;
    assert_eq!(captured.clicks, vec![exact_role("link", "Leave"), exact_text("Apply Leave")]);
    assert_eq!(seen, vec!["link \"Leave\"".to_string(), "text \"Apply Leave\"".to_string()]);
    assert_eq!(captured.ended, Ended::Stopped { href: "https://hr.example.internal/hr/leave/apply?tab=2#top".into() });
    let path = finish(" Leave ", captured, "2026-09-24T10:00:00Z").unwrap();
    assert_eq!(path.module, "Leave");
    assert_eq!(path.arrived, "/hr/leave/apply");
    assert_eq!(path.clicks.len(), 2);
}

/// Review focus 1.
#[tokio::test]
async fn a_closed_recording_browser_ends_the_recording_and_saves_nothing() {
    let mut d = page_with(vec![false], "https://hr.example.internal/hr/leave");
    d.closed_when_drained = true;
    d.events.push_back(clicked(0, "Leave"));
    let (stop, cancel) = (AtomicBool::new(false), AtomicBool::new(false));
    let captured = capture(&mut d, &stop, &cancel, &mut |_| {}).await;
    assert_eq!(captured.clicks.len(), 1);
    assert_eq!(captured.ended, Ended::Closed);
    assert_eq!(finish("Leave", captured, "t").unwrap_err(), BROWSER_CLOSED);
}

#[tokio::test]
async fn cancel_ends_at_once_and_stop_with_no_clicks_saves_nothing() {
    let mut d = page_with(vec![true], "https://hr.example.internal/hr/home/index");
    d.events.push_back(clicked(0, "Leave"));
    let captured = capture(&mut d, &AtomicBool::new(false), &AtomicBool::new(true), &mut |_| {}).await;
    assert_eq!(captured, Captured { clicks: vec![], ended: Ended::Cancelled });
    assert_eq!(finish("Leave", captured, "t").unwrap_err(), CANCELLED);
    let none = Captured { clicks: vec![], ended: Ended::Stopped { href: "https://hr.example.internal/hr/home/index".into() } };
    assert_eq!(finish("Leave", none, "t").unwrap_err(), NO_CLICKS);
}

fn leave_path() -> ModulePath {
    serde_json::from_value(json!({
        "module": "Leave",
        "clicks": [ { "role": "link", "name": "Leave", "exact": true }, { "role": "link", "name": "Apply Leave", "exact": true } ],
        "arrived": "/hr/leave/apply",
        "recorded": "2026-09-24T10:00:00Z"
    }))
    .unwrap()
}

#[tokio::test]
async fn a_path_is_checked_by_signing_in_fresh_and_walking_it_to_where_it_ended() {
    let dir = tempfile::tempdir().unwrap();
    let menu: &[(&str, &str, &str)] = &[("link", "Leave", "/hr/leave"), ("link", "Apply Leave", "/hr/leave/apply")];
    let (mut d, _app) = common::menu_app(menu, "/hr/welcome", 0);
    let ok = check_path(&mut d, dir.path(), &common::menu_recipe(), &common::account(), &leave_path(), &common::quick()).await;
    assert_eq!(ok, Ok("/hr/leave/apply".to_string()));

    let (mut d, _app) = common::menu_app(&[("link", "Leave", "/hr/leave")], "/hr/welcome", 0);
    let err = check_path(&mut d, dir.path(), &common::menu_recipe(), &common::account(), &leave_path(), &common::quick())
        .await
        .unwrap_err();
    assert!(err.starts_with("click 2, link \"Apply Leave\": "), "{err}");
}
```

- [ ] **Step 3: Run to see it fail:** `CARGO_TARGET_DIR=target/gate cargo test --test autorun_recorder`. Expected: `could not find recorder in autorun`.

- [ ] **Step 4: Implement.** Create `src-tauri/src/autorun/recorder.rs`:

```rust
//! Turning a person's clicks in a real browser into the locators a run
//! clicks with.
//!
//! A click listener, added in the capture phase to every document through
//! a DevTools binding, reports each click: a handle on the element itself
//! (kept in the page), and what could be read on the spot - tag, role
//! attribute, aria-label, visible text - for when the click has already
//! taken the page somewhere else. The recorder asks Chrome's accessibility
//! tree for the role and name of the clicked element, or of its nearest
//! ancestor that has one, preferring the roles a menu is made of; when
//! that is not possible it builds the locator from the hints.
//!
//! Which locator to build is decided by plain functions, tested without a
//! browser. Nothing here saves anything: the command saves a path only
//! after it has replayed in a fresh browser.

use super::nav::{self, ModulePath};
use crate::browser::cdp::{CdpError, Driver};
use crate::browser::locator::{LocatorStep, Target};
use crate::browser::page;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The function the page calls to report a click: `window.<BINDING>(json)`.
pub const BINDING: &str = "__tcmRecordClick";

/// Added to every new document, and run once on the current one. The
/// clicked element is kept in the page (`__tcmRecHeld`) so the recorder can
/// ask the accessibility tree about it; `doc` tells an index into this
/// document's list apart from the same index on the next document. It only
/// listens: it never stops or changes a click.
pub const LISTENER_JS: &str = r#"(() => {
  if (window.__tcmRecArmed) return;
  window.__tcmRecArmed = true;
  const doc = Math.random().toString(36).slice(2) + Date.now().toString(36);
  const held = [];
  window.__tcmRecDoc = doc;
  window.__tcmRecHeld = held;
  const clean = (s) => String(s || '').replace(/\s+/g, ' ').trim().slice(0, 200);
  document.addEventListener('click', (e) => {
    if (!e.isTrusted) return;
    const t = e.target;
    const el = t instanceof Element ? t : (t && t.parentElement);
    if (!el) return;
    held.push(el);
    const near = el.closest('a,button,summary,[role]') || el;
    const payload = {
      doc: doc,
      i: held.length - 1,
      tag: near.tagName.toLowerCase(),
      role: near.getAttribute('role') || '',
      label: clean(near.getAttribute('aria-label')),
      text: clean(near.innerText || near.textContent),
    };
    try { window.__tcmRecordClick(JSON.stringify(payload)); } catch (_) {}
  }, true);
})()"#;

/// `this` is the document. Arguments: the document token, the index. The
/// element as a one-item list, or an empty list when it is gone.
pub const HELD_JS: &str = r#"function(doc, i) {
  const held = window.__tcmRecHeld;
  if (window.__tcmRecDoc !== doc || !held) return [];
  const el = held[i];
  return el && el.isConnected ? [el] : [];
}"#;

/// The roles a menu is made of, preferred over anything else near a click.
pub const PREFERRED_ROLES: [&str; 5] = ["link", "button", "menuitem", "tab", "treeitem"];

/// Roles that carry words but are not something a person clicks by name.
const NOT_A_TARGET: [&str; 8] =
    ["generic", "none", "presentation", "StaticText", "InlineTextBox", "LineBreak", "paragraph", "listitem"];

/// The climb from a click stops at these: past one, a name would describe
/// a whole region, not what was clicked.
const CONTAINERS: [&str; 16] = [
    "RootWebArea", "WebArea", "main", "navigation", "menubar", "menu", "tablist", "tree", "dialog", "banner",
    "contentinfo", "form", "region", "document", "application", "list",
];

const MAX_CLIMB: usize = 6;
const MAX_HINT_TEXT: usize = 80;
const POLL: Duration = Duration::from_millis(250);

pub const UNREADABLE: &str = "that click could not be named - click the words of the menu entry itself";
pub const BROWSER_CLOSED: &str = "the recording browser was closed - nothing was saved";
pub const CANCELLED: &str = "the recording was cancelled - nothing was saved";
pub const NO_CLICKS: &str = "no clicks were recorded - click through the menu in the recording browser, then press Stop";

/// What the listener read on the spot.
#[derive(Debug, Clone, PartialEq, Default, serde::Deserialize)]
#[serde(default)]
pub struct ClickHints {
    pub tag: String,
    pub role: String,
    pub label: String,
    pub text: String,
}

/// One click as the page reported it.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct ClickPayload {
    pub doc: String,
    pub i: u32,
    #[serde(flatten)]
    pub hints: ClickHints,
}

/// One accessibility node on the way up from a click.
#[derive(Debug, Clone, PartialEq)]
pub struct AxLink {
    pub role: String,
    pub name: String,
    pub ignored: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ended {
    /// Stop was pressed; where the page was at that moment.
    Stopped { href: String },
    Cancelled,
    /// The recording browser went away.
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Captured {
    pub clicks: Vec<Target>,
    pub ended: Ended,
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `exact`: a menu has "Leave" and "Apply Leave" side by side, and a
/// contains-match on the first would find both.
fn exact_role(role: &str, name: &str) -> Target {
    Target::One(LocatorStep {
        role: Some(role.to_string()),
        name: Some(collapse(name)),
        exact: true,
        ..LocatorStep::default()
    })
}

/// The clicked node first, then each ancestor, from
/// `Accessibility.getPartialAXTree`'s answer. Parents come from `parentId`
/// where Chrome gives one, else from the `childIds` that name the node.
pub fn ax_chain(tree: &Value, backend: i64) -> Vec<AxLink> {
    let nodes: Vec<&Value> = tree["nodes"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    let by_id: HashMap<String, &Value> =
        nodes.iter().filter_map(|n| n["nodeId"].as_str().map(|id| (id.to_string(), *n))).collect();
    let mut parent_of: HashMap<String, String> = HashMap::new();
    for n in &nodes {
        let Some(id) = n["nodeId"].as_str() else { continue };
        for child in n["childIds"].as_array().into_iter().flatten() {
            if let Some(c) = child.as_str() {
                parent_of.entry(c.to_string()).or_insert_with(|| id.to_string());
            }
        }
    }
    let mut out = vec![];
    let mut seen = HashSet::new();
    let mut current = nodes.iter().copied().find(|n| n["backendDOMNodeId"].as_i64() == Some(backend));
    while let Some(n) = current {
        let id = n["nodeId"].as_str().unwrap_or("").to_string();
        if !seen.insert(id.clone()) {
            break;
        }
        out.push(AxLink {
            role: n["role"]["value"].as_str().unwrap_or("").to_string(),
            name: n["name"]["value"].as_str().unwrap_or("").to_string(),
            ignored: n["ignored"].as_bool().unwrap_or(false),
        });
        let parent = n["parentId"].as_str().map(str::to_string).or_else(|| parent_of.get(&id).cloned());
        current = parent.and_then(|p| by_id.get(&p).copied());
    }
    out
}

/// The nearest node with a preferred role and a name; else the nearest
/// named node whose role a person clicks by; never past a container.
pub fn locator_from_ax(chain: &[AxLink]) -> Option<Target> {
    let near: Vec<&AxLink> =
        chain.iter().take(MAX_CLIMB).take_while(|n| !CONTAINERS.contains(&n.role.as_str())).collect();
    let usable = |n: &AxLink| !n.ignored && !collapse(&n.name).is_empty();
    if let Some(n) = near.iter().find(|n| usable(n) && PREFERRED_ROLES.contains(&n.role.as_str())) {
        return Some(exact_role(&n.role, &n.name));
    }
    near.iter()
        .find(|n| usable(n) && !NOT_A_TARGET.contains(&n.role.as_str()))
        .map(|n| exact_role(&n.role, &n.name))
}

/// When the element is gone or has no role nearby: a role attribute with an
/// aria-label, else the visible words (short ones only).
pub fn locator_from_hints(h: &ClickHints) -> Option<Target> {
    let role = h.role.trim();
    let label = collapse(&h.label);
    if !role.is_empty() && !label.is_empty() {
        return Some(exact_role(role, &label));
    }
    let text = collapse(&h.text);
    if !text.is_empty() && text.chars().count() <= MAX_HINT_TEXT {
        return Some(Target::One(LocatorStep { text: Some(text), exact: true, ..LocatorStep::default() }));
    }
    None
}

/// Start listening: the binding, the listener on every new document, and
/// the listener on the document already open.
pub async fn arm<D: Driver>(d: &mut D) -> Result<(), CdpError> {
    d.call("Runtime.enable", json!({})).await?;
    d.call("Runtime.addBinding", json!({ "name": BINDING })).await?;
    d.call("Page.addScriptToEvaluateOnNewDocument", json!({ "source": LISTENER_JS })).await?;
    page::eval_value(d, LISTENER_JS).await?;
    Ok(())
}

/// The next click the page reported, waiting up to `wait`. `Ok(None)` when
/// there was none (or it was another binding's call).
pub async fn next_click<D: Driver>(d: &mut D, wait: Duration) -> Result<Option<ClickPayload>, CdpError> {
    match d.wait_event("Runtime.bindingCalled", wait).await {
        Ok(ev) => {
            if ev.params["name"].as_str() != Some(BINDING) {
                return Ok(None);
            }
            Ok(serde_json::from_str::<ClickPayload>(ev.params["payload"].as_str().unwrap_or("")).ok())
        }
        Err(CdpError::Timeout { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// `Accessibility.getPartialAXTree` around one node, retried once after
/// `Accessibility.enable` - real Edge answers the first call that way.
async fn ax_around<D: Driver>(d: &mut D, backend: i64) -> Result<Value, CdpError> {
    let params = json!({ "backendNodeId": backend, "fetchRelatives": true });
    match d.call("Accessibility.getPartialAXTree", params.clone()).await {
        Ok(v) => Ok(v),
        Err(CdpError::Protocol { message, .. }) if message.to_lowercase().contains("enable") => {
            d.call("Accessibility.enable", json!({})).await?;
            d.call("Accessibility.getPartialAXTree", params).await
        }
        Err(e) => Err(e),
    }
}

async fn from_the_element<D: Driver>(d: &mut D, click: &ClickPayload) -> Result<Option<Target>, CdpError> {
    page::release(d).await;
    let doc = page::document(d).await?;
    let found = page::call_elements(d, &doc, HELD_JS, &[json!(click.doc), json!(click.i)]).await?;
    let Some(el) = found.first() else { return Ok(None) };
    let backend = page::backend_id(d, el).await?;
    let tree = ax_around(d, backend).await?;
    Ok(locator_from_ax(&ax_chain(&tree, backend)))
}

/// The locator for one reported click: from the accessibility tree when
/// the element is still there, else from the hints.
pub async fn locate<D: Driver>(d: &mut D, click: &ClickPayload) -> Result<Target, String> {
    if let Ok(Some(t)) = from_the_element(d, click).await {
        return Ok(t);
    }
    locator_from_hints(&click.hints).ok_or_else(|| UNREADABLE.to_string())
}

/// Where the page is, allowing it a moment if it is between documents.
async fn where_now<D: Driver>(d: &mut D) -> Result<String, CdpError> {
    let mut last = CdpError::Closed;
    for _ in 0..10 {
        match page::eval_value(d, "location.href").await {
            Ok(v) => return Ok(v.as_str().unwrap_or("").to_string()),
            Err(e) if e.is_transient() => {
                last = e;
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

/// Collect clicks until `cancel`, a closed browser, or `stop` - clicks
/// already reported when Stop is pressed are kept. `on_click` hears each
/// one as it is named (or why it could not be).
pub async fn capture<D: Driver>(
    d: &mut D,
    stop: &AtomicBool,
    cancel: &AtomicBool,
    on_click: &mut (dyn FnMut(Result<&Target, &str>) + Send),
) -> Captured {
    let mut clicks: Vec<Target> = vec![];
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Captured { clicks, ended: Ended::Cancelled };
        }
        match next_click(d, POLL).await {
            Ok(Some(click)) => match locate(d, &click).await {
                Ok(t) => {
                    on_click(Ok(&t));
                    clicks.push(t);
                }
                Err(why) => on_click(Err(&why)),
            },
            Ok(None) => {
                if stop.load(Ordering::SeqCst) {
                    let ended = match where_now(d).await {
                        Ok(href) => Ended::Stopped { href },
                        Err(CdpError::Closed) | Err(CdpError::Transport(_)) => Ended::Closed,
                        Err(_) => Ended::Stopped { href: String::new() },
                    };
                    return Captured { clicks, ended };
                }
            }
            Err(CdpError::Closed) | Err(CdpError::Transport(_)) => return Captured { clicks, ended: Ended::Closed },
            Err(_) => {}
        }
    }
}

/// A finished recording as a path to check, or why there is none.
pub fn finish(module: &str, captured: Captured, recorded: &str) -> Result<ModulePath, String> {
    match captured.ended {
        Ended::Cancelled => Err(CANCELLED.to_string()),
        Ended::Closed => Err(BROWSER_CLOSED.to_string()),
        Ended::Stopped { href } => {
            if captured.clicks.is_empty() {
                return Err(NO_CLICKS.to_string());
            }
            Ok(ModulePath {
                module: module.trim().to_string(),
                clicks: captured.clicks,
                arrived: nav::path_of(&href),
                recorded: recorded.to_string(),
            })
        }
    }
}
```

In `src-tauri/src/autorun/nav.rs`, add `use super::accounts::Account;` to the `use` block and append:

```rust
/// The check a path must pass before it is saved, and what Try runs: sign
/// in as `account` in a fresh browser, go home, click each click, and land
/// on `arrived`. Ok carries the path reached; Err is the dialog's sentence.
pub async fn check_path<D: Driver>(
    d: &mut D,
    root: &Path,
    recipe: &SignInRecipe,
    account: &Account,
    path: &ModulePath,
    timing: &Timing,
) -> Result<String, String> {
    let signed = super::signin::sign_in(d, root, recipe, account, timing).await;
    if !signed.ok {
        return Err(format!("the sign-in did not work: {}", signed.detail));
    }
    go_to_module(d, &Route::new(recipe, path.clone()), timing).await.map_err(|f| f.for_dialog())
}
```

Add `pub mod recorder;` to `src-tauri/src/autorun/mod.rs` after `pub mod recipe;`.

- [ ] **Step 5: Run** `--test autorun_recorder`, then `--test autorun_replay` and `--test browser_actions` (they share the fake), one at a time. All pass.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/autorun/recorder.rs src-tauri/src/autorun/mod.rs src-tauri/src/autorun/nav.rs src-tauri/tests/common/mod.rs src-tauri/tests/autorun_recorder.rs
git commit -q -F - <<'EOF'
feat(v2): the recorder names each click by its accessibility role and name, or by what the page showed

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 6: The recorder commands and a real-browser test

**Files:**
- Create: `src-tauri/src/commands/autorun_record.rs`; add `pub mod autorun_record;` to `src-tauri/src/commands/mod.rs` after `pub mod autorun_publish;`
- Modify: `src-tauri/src/events.rs` (`RecordingEvent`), `src-tauri/src/lib.rs` (event and four commands)
- Modify: `src-tauri/src/commands/autorun_replay.rs` (`open_real`, `RealBrowsers::open`, refusal while recording)
- Modify: `src-tauri/src/commands/autorun.rs` (`auto_run_open_browser` refuses while recording)
- Test: `src-tauri/tests/autorun_recorder.rs`, `src-tauri/tests/browser_live.rs`
- Generated: `src/bindings.ts`

**Interfaces:**
- Consumes: Task 5's `recorder::{arm, capture, finish, Ended, BROWSER_CLOSED}`, `nav::{check_path, go_home, put_path, load_nav, find_path, no_path}`; `signin::{prepare, sign_in}`; `autorun_replay::{replay_is_running, replay_timing}`; `autorun::{root, close_browser, supervised_session_is_open}`; `commands::queue::iso_utc`.
- Produces (in `commands::autorun_record`): `pub struct RecorderClaim`, `impl RecorderClaim { pub fn claim() -> Option<RecorderClaim> }`, `pub fn recording_is_going() -> bool`, `pub const ALREADY_RECORDING: &str`, `pub const RECORDING_BUSY: &str`, `pub fn refuse_while_recording() -> Result<(), String>`, `pub async fn refuse_to_record_now() -> Result<(), String>`; `ModuleRecordResult { saved: bool, module: String, failure: String }`, `ModuleTryResult { ok: bool, detail: String }` (specta).
- Produces (commands; TS): `auto_run_record_start(organization, project, module, account, browser_name) -> Result<(), String>` [`autoRunRecordStart(organization, project, module, account, browserName)`], `auto_run_record_stop() -> Result<ModuleRecordResult, String>` [`autoRunRecordStop()`], `auto_run_record_cancel() -> Result<(), String>` [`autoRunRecordCancel()`], `auto_run_try_module_path(organization, project, module, account, browser_name) -> Result<ModuleTryResult, String>` [`autoRunTryModulePath(...)`].
- Produces (event): `RecordingEvent { kind: String /* "click" | "unreadable" | "closed" */, index: u32, readable: String, detail: String }`, emitted as `recording-event` [`events.recordingEvent`].
- Produces (in `commands::autorun_replay`): `pub(crate) async fn open_real(which: Browser, visible: bool) -> Result<(Cdp, LaunchedBrowser), String>`.

- [ ] **Step 1: Write the failing tests.** Append to `src-tauri/tests/autorun_recorder.rs`:

```rust
use v2_lib::commands::autorun_record::{
    recording_is_going, refuse_to_record_now, refuse_while_recording, RecorderClaim, ALREADY_RECORDING, RECORDING_BUSY,
};
use v2_lib::commands::autorun_replay::OneAtATime;

/// One recording at a time; a recording and a run never together. Every
/// claim in this binary is taken in this one test, so parallel tests can
/// never see each other's.
#[tokio::test]
async fn a_recording_waits_for_a_run_and_a_run_waits_for_a_recording() {
    assert!(refuse_to_record_now().await.is_ok());
    let run = OneAtATime::claim().expect("nothing is running");
    assert_eq!(refuse_to_record_now().await.unwrap_err(), "an unattended run is going - wait for it, or stop it first");
    drop(run);

    let rec = RecorderClaim::claim().expect("nothing is recording");
    assert!(recording_is_going());
    assert!(RecorderClaim::claim().is_none(), "one recording at a time");
    assert_eq!(refuse_to_record_now().await.unwrap_err(), ALREADY_RECORDING);
    assert_eq!(refuse_while_recording().unwrap_err(), RECORDING_BUSY);
    drop(rec);
    assert!(!recording_is_going());
    assert!(refuse_while_recording().is_ok());
}
```

(Put the two `use` lines with the file's other imports at the top.)

`src-tauri/tests/browser_live.rs`: add to the imports `use v2_lib::autorun::nav::{check_path, load_nav, put_path};` and `use v2_lib::autorun::recorder;`. In `App::start`, change the signed-in home page body to carry a menu link:

```rust
                        Some(u) => respond(
                            &mut stream,
                            "",
                            &format!("<!doctype html><title>Home</title><nav><a href=\"/leave\">Leave</a></nav><h1 id=\"home\">Home</h1><p id=\"who\"></p><script>document.getElementById('who').textContent = '{u} / ' + localStorage.getItem('token');</script>"),
                        ),
```

and append:

```rust
#[tokio::test]
#[ignore = "starts real headless Edge processes"]
async fn a_recorded_menu_path_is_saved_only_after_it_replays_in_a_fresh_browser() {
    let app = App::start();
    let root = tempfile::tempdir().unwrap();
    save_recipe(root.path(), "acme", "Web", &recipe_for(&app)).unwrap();
    save_accounts(root.path(), &[kim()]).unwrap();
    let recipe = recipe_for(&app);

    // Record: sign in, listen, and click the menu entry with the real mouse.
    let mut live = open().await;
    assert!(sign_in(&mut live.cdp, root.path(), &recipe, &kim(), &timing()).await.ok);
    recorder::arm(&mut live.cdp).await.expect("the recorder could not listen");
    must(run(&mut live, json!({ "kind": "click", "selector": { "role": "link", "name": "Leave" } })).await);
    must(run(&mut live, json!({ "kind": "expect_visible", "selector": { "role": "heading", "name": "Leave" } })).await);
    let (stop, cancel) = (AtomicBool::new(true), AtomicBool::new(false));
    let captured = recorder::capture(&mut live.cdp, &stop, &cancel, &mut |_| {}).await;
    assert_eq!(captured.clicks.len(), 1, "{captured:?}");
    assert!(captured.clicks[0].describe().contains("Leave"), "{captured:?}");
    let path = recorder::finish("Leave", captured, "2026-09-24T10:00:00Z").unwrap();
    assert_eq!(path.arrived, "/leave");
    drop(live);

    // Check in a fresh browser, then save; a wrong ending is refused.
    let mut second = open().await;
    assert_eq!(check_path(&mut second.cdp, root.path(), &recipe, &kim(), &path, &timing()).await, Ok("/leave".to_string()));
    drop(second);
    let mut wrong = path.clone();
    wrong.arrived = "/nowhere".into();
    let mut third = open().await;
    let err = check_path(&mut third.cdp, root.path(), &recipe, &kim(), &wrong, &Timing { nav_ms: 2000, ..timing() }).await.unwrap_err();
    assert!(err.contains("the page ended on /leave, not /nowhere"), "{err}");
    put_path(root.path(), "acme", "Web", path).unwrap();
    assert_eq!(load_nav(root.path(), "acme", "Web").unwrap().modules.len(), 1);
}
```

- [ ] **Step 2: Run to see it fail:** `CARGO_TARGET_DIR=target/gate cargo test --test autorun_recorder`. Expected: `could not find autorun_record in commands`.

- [ ] **Step 3: Implement.** Create `src-tauri/src/commands/autorun_record.rs`:

```rust
//! Recording a module's menu path: a visible browser the person clicks
//! through, a check in a fresh browser, and the path saved only if that
//! replay lands where the recording did.
//!
//! One recording at a time, never alongside an unattended run or the
//! supervised browser. NOTHING here calls Azure DevOps.

use crate::autorun::accounts::Account;
use crate::autorun::nav::{self, ModulePath};
use crate::autorun::recipe::SignInRecipe;
use crate::autorun::recorder::{self, Ended};
use crate::autorun::signin;
use crate::browser::cdp::Cdp;
use crate::browser::launch::Browser;
use crate::browser::timing::Timing;
use crate::events::RecordingEvent;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri_specta::Event;

static RECORDING: AtomicBool = AtomicBool::new(false);

/// Held for as long as a recording (or a Try) is going. Dropping it is the
/// only way to free the slot, so an error or a panic frees it too.
pub struct RecorderClaim(());

impl RecorderClaim {
    pub fn claim() -> Option<RecorderClaim> {
        RECORDING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).ok().map(|_| RecorderClaim(()))
    }
}

impl Drop for RecorderClaim {
    fn drop(&mut self) {
        RECORDING.store(false, Ordering::SeqCst);
    }
}

pub fn recording_is_going() -> bool {
    RECORDING.load(Ordering::SeqCst)
}

pub const ALREADY_RECORDING: &str = "a module path is already being recorded - finish or cancel it first";
/// Said by an unattended run and the supervised browser while recording.
pub const RECORDING_BUSY: &str = "a module path is being recorded - finish or cancel it first";

pub fn refuse_while_recording() -> Result<(), String> {
    if recording_is_going() {
        Err(RECORDING_BUSY.to_string())
    } else {
        Ok(())
    }
}

/// Whether a recording (or a Try) may start now. The sentences match the
/// ones the run and the supervised browser already use.
pub async fn refuse_to_record_now() -> Result<(), String> {
    if crate::commands::autorun_replay::replay_is_running() {
        return Err("an unattended run is going - wait for it, or stop it first".to_string());
    }
    if crate::commands::autorun::supervised_session_is_open().await {
        return Err("close the supervised browser first".to_string());
    }
    if recording_is_going() {
        return Err(ALREADY_RECORDING.to_string());
    }
    Ok(())
}

struct Recording {
    _claim: RecorderClaim,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<recorder::Captured>,
    organization: String,
    project: String,
    module: String,
    account: String,
    which: Browser,
}

static CURRENT: tokio::sync::Mutex<Option<Recording>> = tokio::sync::Mutex::const_new(None);

/// A recording whose browser was closed has ended on its own; it must not
/// hold the slot.
async fn drop_a_finished_recording() {
    let mut slot = CURRENT.lock().await;
    if slot.as_ref().is_some_and(|r| r.task.is_finished()) {
        *slot = None;
    }
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ModuleRecordResult {
    pub saved: bool,
    pub module: String,
    /// Why nothing was saved; empty when `saved`.
    pub failure: String,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ModuleTryResult {
    pub ok: bool,
    pub detail: String,
}

fn now_iso() -> String {
    crate::commands::queue::iso_utc((crate::autorun::sessions::now_ms() / 1000) as i64)
}

async fn prepare_to_record(
    cdp: &mut Cdp,
    root: &Path,
    recipe: &SignInRecipe,
    who: &Account,
    timing: &Timing,
) -> Result<(), String> {
    let signed = signin::sign_in(cdp, root, recipe, who, timing).await;
    if !signed.ok {
        return Err(format!("the sign-in did not work: {}", signed.detail));
    }
    let home = nav::go_home(cdp, &recipe.start_url, &recipe.origins(), timing).await;
    if !home.ok {
        return Err(format!("the home page did not open: {}", home.detail));
    }
    recorder::arm(cdp).await.map_err(|e| format!("the browser would not report clicks: {e}"))
}

/// Sign in fresh in a background browser and walk the path.
async fn check_in_fresh_browser(
    root: &Path,
    organization: &str,
    project: &str,
    account: &str,
    which: Browser,
    path: &ModulePath,
) -> Result<String, String> {
    let (recipe, who) = signin::prepare(root, organization, project, account)?;
    let (mut cdp, browser) = super::autorun_replay::open_real(which, false)
        .await
        .map_err(|e| format!("the browser did not open: {e}"))?;
    let out = nav::check_path(&mut cdp, root, &recipe, &who, path, &super::autorun_replay::replay_timing(false)).await;
    drop(cdp);
    super::autorun::close_browser(browser);
    out
}

/// Open a visible browser, sign in as `account`, go home, and start
/// listening. Each captured click arrives as a `RecordingEvent`.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_record_start(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    module: String,
    account: String,
    browser_name: String,
) -> Result<(), String> {
    let module = module.trim().to_string();
    if module.is_empty() {
        return Err("name the module first".to_string());
    }
    drop_a_finished_recording().await;
    refuse_to_record_now().await?;
    let claim = RecorderClaim::claim().ok_or_else(|| ALREADY_RECORDING.to_string())?;
    let root = super::autorun::root(&app)?;
    let (recipe, who) = signin::prepare(&root, &organization, &project, &account)?;
    let which = Browser::from_name(&browser_name);
    let (mut cdp, browser) =
        super::autorun_replay::open_real(which, true).await.map_err(|e| format!("the browser did not open: {e}"))?;
    if let Err(why) = prepare_to_record(&mut cdp, &root, &recipe, &who, &Timing::default()).await {
        drop(cdp);
        super::autorun::close_browser(browser);
        return Err(why);
    }

    let stop = Arc::new(AtomicBool::new(false));
    let cancel = Arc::new(AtomicBool::new(false));
    let (stop_seen, cancel_seen, events_to) = (stop.clone(), cancel.clone(), app.clone());
    let task = tokio::spawn(async move {
        let mut index = 0u32;
        let captured = recorder::capture(&mut cdp, &stop_seen, &cancel_seen, &mut |seen| {
            let ev = match seen {
                Ok(t) => {
                    index += 1;
                    RecordingEvent { kind: "click".into(), index, readable: t.describe(), detail: String::new() }
                }
                Err(why) => RecordingEvent { kind: "unreadable".into(), index, readable: String::new(), detail: why.to_string() },
            };
            let _ = ev.emit(&events_to);
        })
        .await;
        if captured.ended == Ended::Closed {
            let _ = RecordingEvent {
                kind: "closed".into(),
                index: 0,
                readable: String::new(),
                detail: recorder::BROWSER_CLOSED.to_string(),
            }
            .emit(&events_to);
        }
        drop(cdp);
        super::autorun::close_browser(browser);
        captured
    });
    *CURRENT.lock().await = Some(Recording {
        _claim: claim,
        stop,
        cancel,
        task,
        organization,
        project,
        module,
        account,
        which,
    });
    crate::applog::info("Auto-run module recording started");
    Ok(())
}

/// Stop, close the recording browser, replay the path in a fresh signed-in
/// browser, and save it only if every click found its one element and the
/// page ended where the recording did.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_record_stop(app: tauri::AppHandle) -> Result<ModuleRecordResult, String> {
    let rec = CURRENT.lock().await.take().ok_or_else(|| "nothing is being recorded".to_string())?;
    rec.stop.store(true, Ordering::SeqCst);
    let Recording { _claim, task, organization, project, module, account, which, .. } = rec;
    let captured = task.await.map_err(|e| format!("the recorder stopped unexpectedly: {e}"))?;
    let path = match recorder::finish(&module, captured, &now_iso()) {
        Ok(p) => p,
        Err(failure) => return Ok(ModuleRecordResult { saved: false, module, failure }),
    };
    let root = super::autorun::root(&app)?;
    match check_in_fresh_browser(&root, &organization, &project, &account, which, &path).await {
        Ok(_) => {
            let clicks = path.clicks.len();
            nav::put_path(&root, &organization, &project, path)?;
            crate::applog::info(format!("Auto-run module path saved ({clicks} clicks)"));
            Ok(ModuleRecordResult { saved: true, module, failure: String::new() })
        }
        Err(failure) => Ok(ModuleRecordResult { saved: false, module, failure }),
    }
}

/// Close the recording browser and save nothing.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_record_cancel() -> Result<(), String> {
    let rec = CURRENT.lock().await.take();
    if let Some(rec) = rec {
        rec.cancel.store(true, Ordering::SeqCst);
        let _ = rec.task.await;
    }
    Ok(())
}

/// The same check a recording must pass, on a saved path.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_try_module_path(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    module: String,
    account: String,
    browser_name: String,
) -> Result<ModuleTryResult, String> {
    drop_a_finished_recording().await;
    refuse_to_record_now().await?;
    let _claim = RecorderClaim::claim().ok_or_else(|| ALREADY_RECORDING.to_string())?;
    let root = super::autorun::root(&app)?;
    let nav_file = nav::load_nav(&root, &organization, &project)?;
    let path = nav::find_path(&nav_file, &module).cloned().ok_or_else(|| nav::no_path(&module))?;
    Ok(
        match check_in_fresh_browser(&root, &organization, &project, &account, Browser::from_name(&browser_name), &path).await {
            Ok(arrived) => ModuleTryResult { ok: true, detail: format!("reached {arrived}") },
            Err(detail) => ModuleTryResult { ok: false, detail },
        },
    )
}
```

`src-tauri/src/commands/mod.rs`: add `pub mod autorun_record;` after `pub mod autorun_publish;`.

`src-tauri/src/events.rs`, after `ReplayProgress`:

```rust
/// Emitted while a module path is being recorded: one per captured click,
/// one per click that could not be named, and one if the recording
/// browser went away. Carries locator words only, never a login.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct RecordingEvent {
    /// "click", "unreadable" or "closed"
    pub kind: String,
    /// 1-based position of a captured click; 0 otherwise.
    pub index: u32,
    /// The click in words (`link "Leave"`), for "click".
    pub readable: String,
    /// Why, for "unreadable" and "closed".
    pub detail: String,
}
```

`src-tauri/src/lib.rs`: add `autorun_record,` to the `use commands::{...}` list inside `specta_builder`, change `events::ReplayProgress` in `collect_events![` to `events::ReplayProgress,` followed by a new line `events::RecordingEvent`, and after `autorun_replay::auto_run_replay_cancel,` add:

```rust
            autorun_record::auto_run_record_start,
            autorun_record::auto_run_record_stop,
            autorun_record::auto_run_record_cancel,
            autorun_record::auto_run_try_module_path,
```

`src-tauri/src/commands/autorun_replay.rs`: add, above `struct RealBrowsers`:

```rust
/// Start a browser and wait until its DevTools port answers. Shared by the
/// unattended run (one per case) and the module recorder.
pub(crate) async fn open_real(which: Browser, visible: bool) -> Result<(Cdp, LaunchedBrowser), String> {
    let extra = background_args();
    let browser = launch_with(which, if visible { &[] } else { &extra })?;
    // The debugging port answers when the browser is ready, which varies
    // with what else the machine is doing. Asking until it does beats a
    // fixed sleep that is either slow or flaky.
    let mut last = String::new();
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        match Cdp::connect(browser.port).await {
            Ok(cdp) => return Ok((cdp, browser)),
            Err(e) => last = e,
        }
    }
    super::autorun::close_browser(browser);
    Err(format!("it started but never answered: {last}"))
}
```

replace `RealBrowsers::open` with:

```rust
    async fn open(&mut self) -> Result<Cdp, String> {
        let (cdp, browser) = open_real(self.which, self.watch).await?;
        self.current = Some(browser);
        Ok(cdp)
    }
```

and in `auto_run_replay`, directly after the `supervised_session_is_open` check, add:

```rust
    super::autorun_record::refuse_while_recording()?;
```

`src-tauri/src/commands/autorun.rs`, in `auto_run_open_browser`, directly after the `replay_is_running` check, add:

```rust
    crate::commands::autorun_record::refuse_while_recording()?;
```

- [ ] **Step 4: Run** (one at a time): `--test autorun_recorder`, `--test autorun_commands`, `--test autorun_replay`, `--test browser_live` (compiles; ignored tests not run), `--test bindings` (regenerates the four record commands, `ModuleRecordResult`, `ModuleTryResult`, `RecordingEvent`). Then `npx tsc --noEmit`.

- [ ] **Step 5: The real browser, on purpose** (Edge must be installed; this starts headless Edge): `CARGO_TARGET_DIR=target/gate cargo test --test browser_live -- --ignored --test-threads=1 a_recorded_menu_path_is_saved_only_after_it_replays_in_a_fresh_browser`, then the whole ignored file once: `CARGO_TARGET_DIR=target/gate cargo test --test browser_live -- --ignored --test-threads=1` (the home page's new menu link must not disturb the older live tests). If Edge cannot start on this machine, say so in the report rather than skipping silently.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src src-tauri/tests src/bindings.ts
git commit -q -F - <<'EOF'
feat(v2): record a module's menu path in a real browser, saved only once it replays in a fresh one

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 7: The Module paths dialog

**Files:**
- Modify: `src/lib/actionIcons.ts` (`Route as IconModulePaths`)
- Create: `src/screens/AutoRun/ModulePathsDialog.tsx`, `src/screens/AutoRun/ModulePathsDialog.test.tsx`
- Modify: `src/screens/AutoRun/index.tsx` (button, state, `caseModules`), `src/screens/AutoRun/index.test.tsx`

**Interfaces:**
- Consumes: `commands.autoRunLoadNav`, `autoRunSetDirectUrls`, `autoRunRemoveModulePath` (Task 1), `autoRunRecordStart`, `autoRunRecordStop`, `autoRunRecordCancel`, `autoRunTryModulePath`, `events.recordingEvent` (Task 6), `autoRunListAccounts`.
- Produces: `export default function ModulePathsDialog({ org, project, caseModules, onClose }: { org: string; project: string; caseModules: string[]; onClose: () => void })`.

- [ ] **Step 1: Write the failing tests** in `src/screens/AutoRun/ModulePathsDialog.test.tsx`:

```tsx
// The Module paths dialog: the recorded paths in words, Remove that asks
// first, the address switch, and a recording driven by mocked events.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "../../lib/toast";
import ModulePathsDialog from "./ModulePathsDialog";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

const ACCOUNTS = [{ key: "hr.admin", label: "HR Admin", username: "kim", password: "p" }];
const LEAVE = {
  module: "Leave",
  clicks: ['link "Leave"', 'link "Apply Leave"'],
  arrived: "/hr/leave/apply",
  recorded: "2026-09-24T10:00:00Z",
};

function mount(handler: (cmd: string, args: Record<string, unknown>) => unknown) {
  mockIPC(
    (cmd, args) => {
      if (cmd === "auto_run_list_accounts") return ACCOUNTS;
      const out = handler(String(cmd), (args ?? {}) as Record<string, unknown>);
      return out === undefined ? null : out;
    },
    { shouldMockEvents: true },
  );
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ModulePathsDialog org="acme" project="Web" caseModules={["Leave", "Payroll"]} onClose={vi.fn()} />
    </QueryClientProvider>,
  );
}

async function startRecording() {
  const record = await screen.findByRole("button", { name: "Record a module…" });
  await waitFor(() => expect(record).toBeEnabled());
  fireEvent.click(record);
  fireEvent.click(screen.getByRole("combobox", { name: "Module" }));
  fireEvent.click(await screen.findByRole("option", { name: "Leave" }));
  fireEvent.click(screen.getByRole("button", { name: "Start recording" }));
  await screen.findByRole("button", { name: "Stop" });
}

async function send(payload: { kind: string; index: number; readable: string; detail: string }) {
  const { emit } = await import("@tauri-apps/api/event");
  await act(async () => {
    await emit("recording-event", payload);
  });
}

test("the list shows each module's clicks in words and where it ends", async () => {
  mount((cmd) => (cmd === "auto_run_load_nav" ? { direct_urls: true, modules: [LEAVE] } : undefined));
  expect(await screen.findByText('link "Leave" › link "Apply Leave"')).toBeInTheDocument();
  expect(screen.getByText("ends on /hr/leave/apply")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Re-record Leave" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Try Leave" })).toBeInTheDocument();
});

test("Remove asks first, and only the confirm removes", async () => {
  const removed: unknown[] = [];
  mount((cmd, args) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [LEAVE] };
    if (cmd === "auto_run_remove_module_path") {
      removed.push(args);
      return { direct_urls: true, modules: [] };
    }
  });
  fireEvent.click(await screen.findByRole("button", { name: "Remove Leave" }));
  expect(screen.getByText("Remove the path for Leave?")).toBeInTheDocument();
  expect(removed).toEqual([]);
  fireEvent.click(screen.getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(removed).toEqual([{ organization: "acme", project: "Web", module: "Leave" }]));
  expect(await screen.findByText(/No module paths yet/)).toBeInTheDocument();
});

test("the address switch saves the moment it is flipped", async () => {
  const sets: unknown[] = [];
  mount((cmd, args) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_set_direct_urls") {
      sets.push(args);
      return { direct_urls: false, modules: [] };
    }
  });
  const sw = await screen.findByRole("switch", { name: "Scripts may open pages by address" });
  await waitFor(() => expect(sw).toBeEnabled());
  expect(sw).toHaveAttribute("aria-checked", "true");
  fireEvent.click(sw);
  await waitFor(() => expect(sets).toEqual([{ organization: "acme", project: "Web", allowed: false }]));
  await waitFor(() => expect(sw).toHaveAttribute("aria-checked", "false"));
});

test("a recording lists each click as it arrives and saves on Stop", async () => {
  const started: unknown[] = [];
  mount((cmd, args) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_record_start") {
      started.push(args);
      return null;
    }
    if (cmd === "auto_run_record_stop") return { saved: true, module: "Leave", failure: "" };
  });
  await startRecording();
  expect(started).toEqual([
    { organization: "acme", project: "Web", module: "Leave", account: "hr.admin", browserName: "edge" },
  ]);
  expect(screen.getByRole("button", { name: "Stop" })).toBeDisabled();
  await send({ kind: "click", index: 1, readable: 'link "Leave"', detail: "" });
  expect(await screen.findByText('1. link "Leave"')).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  await waitFor(() => expect(toast.success).toHaveBeenCalledWith("Path saved for Leave."));
  expect(await screen.findByRole("button", { name: "Record a module…" })).toBeInTheDocument();
});

test("a path that does not replay says which click failed and offers to record again", async () => {
  let starts = 0;
  mount((cmd) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_record_start") {
      starts += 1;
      return null;
    }
    if (cmd === "auto_run_record_stop") {
      return { saved: false, module: "Leave", failure: 'click 2, link "Apply Leave": no visible match' };
    }
  });
  await startRecording();
  await send({ kind: "click", index: 1, readable: 'link "Leave"', detail: "" });
  fireEvent.click(await screen.findByRole("button", { name: "Stop" }));
  expect(await screen.findByText('click 2, link "Apply Leave": no visible match')).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Record again" }));
  await waitFor(() => expect(starts).toBe(2));
});

/// Review focus 1.
test("closing the recording browser ends the recording and frees it", async () => {
  let cancels = 0;
  mount((cmd) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_record_start") return null;
    if (cmd === "auto_run_record_cancel") {
      cancels += 1;
      return null;
    }
  });
  await startRecording();
  await send({ kind: "closed", index: 0, readable: "", detail: "the recording browser was closed - nothing was saved" });
  expect(await screen.findByText("The recording browser was closed. Nothing was saved.")).toBeInTheDocument();
  await waitFor(() => expect(cancels).toBe(1));
});
```

`src/screens/AutoRun/index.test.tsx`, append:

```tsx
test("the Module paths button opens its dialog", async () => {
  mockList([caseRow(1, "Login - valid credentials")], [1], [], (cmd) =>
    cmd === "auto_run_load_nav" ? { direct_urls: true, modules: [] } : null,
  );
  renderScreen();
  await screen.findByText("Login - valid credentials");
  fireEvent.click(screen.getByRole("button", { name: "Module paths" }));
  expect(await screen.findByRole("heading", { name: "Module paths" })).toBeInTheDocument();
});
```

- [ ] **Step 2: Run to see them fail:** `npx vitest run --exclude "**/.claude/**" src/screens/AutoRun/ModulePathsDialog.test.tsx src/screens/AutoRun/index.test.tsx`. Expected: cannot resolve `./ModulePathsDialog`; no "Module paths" button.

- [ ] **Step 3: Implement.** In `src/lib/actionIcons.ts`, under `KeyRound as IconRecipe,` add:

```ts
  // The menu paths an unattended run follows to each module's screen.
  Route as IconModulePaths,
```

Create `src/screens/AutoRun/ModulePathsDialog.tsx`:

```tsx
// Module paths: how an unattended run gets from the home page to each
// module's screen. A path is recorded by clicking through the menu in a
// real browser, checked by replaying it in a fresh one, and saved only if
// that replay lands where the recording did. The dialog also holds the
// project's "Scripts may open pages by address" switch.
//
// Nothing here reaches Azure DevOps: paths and the switch live on this
// machine, beside the project's sign-in recipe.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { commands, events } from "../../bindings";
import { Button } from "../../components/ui/button";
import Combobox from "../../components/ui/combobox";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import { Switch } from "../../components/ui/switch";
import { IconBack, IconCancel, IconRecord, IconRemove, IconRun, IconStop } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";

type Phase =
  | { kind: "list" }
  | { kind: "choose"; module: string }
  | { kind: "starting"; module: string }
  | { kind: "recording"; module: string; clicks: string[]; notes: string[] }
  | { kind: "checking"; module: string }
  | { kind: "failed"; module: string; why: string };

/** The browser the person last picked in Auto Run (the run panes' own
 * key), so a recording opens in the browser they already chose. */
function chosenBrowser(): string {
  try {
    return localStorage.getItem("tcm-v2-autorun-browser") === "chrome" ? "chrome" : "edge";
  } catch {
    return "edge";
  }
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export default function ModulePathsDialog({
  org,
  project,
  caseModules,
  onClose,
}: {
  org: string;
  project: string;
  /** The Module values of the cases loaded in Auto Run, for picking. */
  caseModules: string[];
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const navKey = ["autorun-nav", org, project];
  const nav = useQuery({
    queryKey: navKey,
    queryFn: () => unwrapStr(commands.autoRunLoadNav(org, project)),
    retry: false,
  });
  const accounts = useQuery({
    queryKey: ["autorun-accounts"],
    queryFn: () => unwrapStr(commands.autoRunListAccounts()),
    retry: false,
  });
  const [picked, setPicked] = useState("");
  const [phase, setPhase] = useState<Phase>({ kind: "list" });
  const [confirming, setConfirming] = useState<string | null>(null);
  const [trying, setTrying] = useState<string | null>(null);
  const [tried, setTried] = useState<Record<string, { ok: boolean; detail: string }>>({});
  const [problem, setProblem] = useState("");

  const keys = (accounts.data ?? []).map((a) => a.key);
  const who = keys.includes(picked) ? picked : (keys[0] ?? "");
  const busy =
    phase.kind === "starting" || phase.kind === "recording" || phase.kind === "checking" || trying !== null;

  useEffect(() => {
    const un = events.recordingEvent.listen((e) => {
      const p = e.payload;
      // The recording browser went away: free the recorder at once.
      if (p.kind === "closed") void commands.autoRunRecordCancel().catch(() => {});
      setPhase((cur) => {
        if (cur.kind !== "recording") return cur;
        if (p.kind === "click") return { ...cur, clicks: [...cur.clicks, p.readable] };
        if (p.kind === "unreadable") return { ...cur, notes: [...cur.notes, p.detail] };
        if (p.kind === "closed") {
          return { kind: "failed", module: cur.module, why: "The recording browser was closed. Nothing was saved." };
        }
        return cur;
      });
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  const record = async (module: string) => {
    setProblem("");
    setPhase({ kind: "starting", module });
    try {
      const r = await commands.autoRunRecordStart(org, project, module, who, chosenBrowser());
      if (r.status === "error") {
        setPhase({ kind: "failed", module, why: r.error });
        return;
      }
      setPhase({ kind: "recording", module, clicks: [], notes: [] });
    } catch (e) {
      setPhase({ kind: "failed", module, why: message(e) });
    }
  };

  const stop = async () => {
    if (phase.kind !== "recording") return;
    const { module } = phase;
    setPhase({ kind: "checking", module });
    try {
      const r = await commands.autoRunRecordStop();
      if (r.status === "error") {
        setPhase({ kind: "failed", module, why: r.error });
        return;
      }
      if (!r.data.saved) {
        setPhase({ kind: "failed", module, why: r.data.failure });
        return;
      }
      toast.success(`Path saved for ${r.data.module}.`);
      await qc.invalidateQueries({ queryKey: navKey });
      setPhase({ kind: "list" });
    } catch (e) {
      setPhase({ kind: "failed", module, why: message(e) });
    }
  };

  const cancel = async () => {
    await commands.autoRunRecordCancel().catch(() => {});
    setPhase({ kind: "list" });
  };

  const tryPath = async (module: string) => {
    setTrying(module);
    try {
      const r = await commands.autoRunTryModulePath(org, project, module, who, chosenBrowser());
      setTried((t) => ({ ...t, [module]: r.status === "ok" ? r.data : { ok: false, detail: r.error } }));
    } catch (e) {
      setTried((t) => ({ ...t, [module]: { ok: false, detail: message(e) } }));
    } finally {
      setTrying(null);
    }
  };

  const remove = useMutation({
    mutationFn: (module: string) => unwrapStr(commands.autoRunRemoveModulePath(org, project, module)),
    onSuccess: (view) => {
      qc.setQueryData(navKey, view);
      setConfirming(null);
    },
    onError: (e) => setProblem(message(e)),
  });

  const setDirect = useMutation({
    mutationFn: (allowed: boolean) => unwrapStr(commands.autoRunSetDirectUrls(org, project, allowed)),
    onSuccess: (view) => qc.setQueryData(navKey, view),
    onError: (e) => setProblem(message(e)),
  });

  /** Escape and the backdrop do nothing while a browser is working: only
   * Stop or Cancel ends a recording. */
  const closeIfIdle = () => {
    if (busy) return;
    onClose();
  };

  const modules = nav.data?.modules ?? [];

  return (
    <Modal onClose={closeIfIdle} className="flex max-h-[85vh] w-full max-w-2xl flex-col gap-3 p-5">
      <div>
        <h2 className="text-sm font-semibold text-text">Module paths</h2>
        <p className="mt-1 text-xs text-muted">
          How an unattended run reaches each module's screen after signing in. Record one by clicking
          through the menu; it is saved only if it works again in a fresh browser.
        </p>
      </div>
      {nav.isError && <p className="text-xs text-danger">{nav.error.message}</p>}
      {problem && <p className="text-xs text-danger">{problem}</p>}

      <label className="flex items-center gap-2 text-xs text-muted">
        <Switch
          checked={nav.data?.direct_urls ?? true}
          ariaLabel="Scripts may open pages by address"
          disabled={!nav.data || setDirect.isPending}
          onCheckedChange={(on) => setDirect.mutate(on)}
        />
        Scripts may open pages by address
      </label>
      <p className="text-xs text-faint">
        Off: a script with a navigate step cannot be saved, and every run starts on the case's module
        screen.
      </p>

      <label className="flex items-center gap-2 text-xs text-muted">
        Record and try as
        <Select
          aria-label="Record and try as"
          className="w-56"
          value={who}
          onChange={(e) => setPicked(e.target.value)}
        >
          {keys.length === 0 && <option value="">No accounts yet</option>}
          {(accounts.data ?? []).map((a) => (
            <option key={a.key} value={a.key}>
              {a.label ? `${a.label} (${a.key})` : a.key}
            </option>
          ))}
        </Select>
      </label>

      {phase.kind === "list" && (
        <>
          <ul className="min-h-0 flex-1 space-y-2 overflow-auto">
            {modules.length === 0 && (
              <li className="text-xs text-muted">
                No module paths yet. Runs start from the home page, as they always have.
              </li>
            )}
            {modules.map((m) => (
              <li key={m.module} className="rounded-md border border-border p-2 text-xs">
                <div className="flex items-center gap-2">
                  <span className="font-medium text-text">{m.module}</span>
                  <span className="min-w-0 flex-1 truncate text-muted">{m.clicks.join(" › ")}</span>
                </div>
                <p className="mt-1 text-faint">ends on {m.arrived}</p>
                {tried[m.module] && (
                  <p className={cn("mt-1", tried[m.module].ok ? "text-success" : "text-danger")}>
                    {tried[m.module].detail}
                  </p>
                )}
                {confirming === m.module ? (
                  <div className="mt-2 flex items-center justify-end gap-2">
                    <span className="text-muted">Remove the path for {m.module}?</span>
                    <Button size="sm" variant="ghost" onClick={() => setConfirming(null)}>
                      <IconCancel aria-hidden />
                      Keep it
                    </Button>
                    <Button
                      size="sm"
                      variant="danger"
                      disabled={remove.isPending}
                      onClick={() => remove.mutate(m.module)}
                    >
                      <IconRemove aria-hidden />
                      Remove
                    </Button>
                  </div>
                ) : (
                  <div className="mt-2 flex justify-end gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      aria-label={`Re-record ${m.module}`}
                      disabled={!who || busy}
                      onClick={() => record(m.module)}
                    >
                      <IconRecord aria-hidden />
                      Re-record
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      aria-label={`Try ${m.module}`}
                      disabled={!who || busy}
                      onClick={() => tryPath(m.module)}
                    >
                      <IconRun aria-hidden />
                      {trying === m.module ? "Trying" : "Try"}
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      aria-label={`Remove ${m.module}`}
                      disabled={busy}
                      onClick={() => setConfirming(m.module)}
                    >
                      <IconRemove aria-hidden />
                      Remove
                    </Button>
                  </div>
                )}
              </li>
            ))}
          </ul>
          <div className="flex justify-between gap-2">
            <Button
              size="sm"
              variant="outline"
              disabled={!who || busy}
              title={!who ? "Add an account first" : undefined}
              onClick={() => setPhase({ kind: "choose", module: "" })}
            >
              <IconRecord aria-hidden />
              Record a module…
            </Button>
            <Button size="sm" variant="ghost" disabled={busy} onClick={onClose}>
              <IconCancel aria-hidden />
              Close
            </Button>
          </div>
        </>
      )}

      {phase.kind === "choose" && (
        <div className="space-y-2">
          <label className="flex items-center gap-2 text-xs text-muted">
            Module
            <Combobox
              ariaLabel="Module"
              className="w-64"
              value={phase.module}
              options={caseModules}
              allowCustom
              placeholder="Pick or type a module"
              onChange={(v) => setPhase({ kind: "choose", module: v })}
            />
          </label>
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={() => setPhase({ kind: "list" })}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button size="sm" disabled={!phase.module.trim()} onClick={() => record(phase.module.trim())}>
              <IconRecord aria-hidden />
              Start recording
            </Button>
          </div>
        </div>
      )}

      {phase.kind === "starting" && (
        <p className="text-xs text-muted">Opening the browser and signing in as {who}…</p>
      )}

      {phase.kind === "recording" && (
        <div className="space-y-2">
          <p className="text-xs text-muted">
            Recording {phase.module}. In the browser that opened, click through the menu to the module's
            screen, then press Stop.
          </p>
          <ol aria-label="Recorded clicks" className="space-y-1 text-xs text-text">
            {phase.clicks.map((c, i) => (
              <li key={i}>{`${i + 1}. ${c}`}</li>
            ))}
          </ol>
          {phase.notes.map((n, i) => (
            <p key={i} className="text-xs text-warning">
              {n}
            </p>
          ))}
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={cancel}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button size="sm" disabled={phase.clicks.length === 0} onClick={stop}>
              <IconStop aria-hidden />
              Stop
            </Button>
          </div>
        </div>
      )}

      {phase.kind === "checking" && (
        <p className="text-xs text-muted">Checking the path for {phase.module} in a fresh browser…</p>
      )}

      {phase.kind === "failed" && (
        <div className="space-y-2">
          <p className="text-xs text-danger">{phase.why}</p>
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={() => setPhase({ kind: "list" })}>
              <IconBack aria-hidden />
              Back to the list
            </Button>
            <Button size="sm" disabled={!who} onClick={() => record(phase.module)}>
              <IconRecord aria-hidden />
              Record again
            </Button>
          </div>
        </div>
      )}
    </Modal>
  );
}
```

`src/screens/AutoRun/index.tsx`:
- add `IconModulePaths,` to the `actionIcons` import list (alphabetical, after `IconImport,`);
- add `import ModulePathsDialog from "./ModulePathsDialog";` after `import AccountsDialog from "./AccountsDialog";`;
- after `const [recipeOpen, setRecipeOpen] = useState(false);` add `const [navOpen, setNavOpen] = useState(false);`;
- after the `groups` `useMemo`, add:

```tsx
  /** The Module values of the loaded cases, for the Module paths dialog's
   * picker. A person can still type one that is not here. */
  const caseModules = useMemo(
    () =>
      Array.from(new Set(rows.map((c) => c.module_value.trim()).filter(Boolean))).sort((a, b) =>
        a.localeCompare(b),
      ),
    [rows],
  );
```

- after the "Sign-in recipe" `<Button>`, add:

```tsx
        <Button
          size="sm"
          variant="outline"
          disabled={!org || !project}
          title={!org || !project ? "Pick an organization and project first" : undefined}
          onClick={() => setNavOpen(true)}
        >
          <IconModulePaths aria-hidden />
          Module paths
        </Button>
```

- after the `{recipeOpen && (...)}` block, add:

```tsx
      {navOpen && (
        <ModulePathsDialog
          org={org}
          project={project}
          caseModules={caseModules}
          onClose={() => setNavOpen(false)}
        />
      )}
```

- [ ] **Step 4: Run** (one at a time): `npx vitest run --exclude "**/.claude/**" src/screens/AutoRun/ModulePathsDialog.test.tsx src/screens/AutoRun/index.test.tsx src/ui-consistency.test.ts`, then `npx tsc --noEmit`, then the whole frontend suite once: `npx vitest run --exclude "**/.claude/**"` (a single `src/App.test.tsx` failure that passes on a re-run is its documented flake; two different failures are not).

- [ ] **Step 5: Commit**

```bash
git add src/lib/actionIcons.ts src/screens/AutoRun
git commit -q -F - <<'EOF'
feat(v2): a Module paths dialog to record, try and remove menu paths, with the address switch

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

## After execution: the gates, then checks only a person can make

- [ ] Rust suite once, from `src-tauri/`: `CARGO_TARGET_DIR=target/gate cargo test --tests` (after the dev-app check). Then the frontend suite and `npx tsc --noEmit`, one at a time. No release.

By hand, owed by the owner (design §9; jsdom does no layout or hit testing, so a real walk-through is the only check of the dialog):
1. Record a real HR module path (for example Leave) from `https://hrmmainphdev01.phrsandbox.dev/hr/home/index`: the dialog lists each click as it is made, Stop checks it in a fresh browser, and the path appears with where it ends.
2. Try it; turn "Scripts may open pages by address" off and confirm a script with a `navigate` is refused in the Script editor with the project's sentence.
3. Run a module's cases unattended with "Sign in as" set: each case shows "Going to the module", the review shows a "Module" line reading `Go to Leave`, and a case whose Module has no path is Blocked with the design's sentence.
4. Close the recording browser mid-recording: the dialog says nothing was saved, and a new recording can start.

---

## Self-review

**1. Spec coverage.**

| Spec | Where |
| --- | --- |
| §3 file beside the recipe, fields, absent `direct_urls` is true, duplicates refused, trim/case matching, no file = today | Task 1 |
| §4 dialog list in words with where it ends; Re-record, Try, Remove (asks first); Record a module…; the switch | Task 7 (commands Tasks 1 and 6) |
| §4 recording: module from loaded cases or typed; account from Accounts; visible browser signs in with `signin::sign_in` and ends on `start_url`; clicks listed as captured; Stop records `arrived`; Cancel closes and saves nothing | Tasks 6 and 7 |
| §4 capture: capture-phase listener via CDP binding on every document; element plus hints; AX role/name of element or nearest ancestor with a role, preferring link/button/menuitem/tab/treeitem; hints when gone or roleless | Task 5 |
| §4 checking: replay in a fresh signed-in browser; exactly one visible element per click (the runner's click); arrival equal; failing click named; Record again; Try runs the same check | Tasks 5, 6, 7 |
| §4 limits: one recording, not with a run, not with the supervised browser | Task 6 |
| §5 order: account, sign-in, go home (runner only), find path by `ReplayCase.module`, runner click, arrival check, steps; review line `Go to X` with a picture on failure; the four blocked rows with their sentences; next case continues; mid-script `sign_in` goes back | Tasks 2 and 3 |
| §6 refusal at editor, import, bridge; guide section only while off; run-time refusal blocks the case; start_url, go-home and recorder not affected | Task 4 (go-home and recorder use `execute_in` directly, never `run_step`) |
| §7 Sign in as select, default, remembered per org/project in localStorage, silent fallback, `account: Option<String>` checked with `valid_key` and the list | Task 3 |
| §8 not in scope | Nothing added: no hover action, no per-account paths, no assistant tool, no hand editing |
| §9 Rust tests, frontend tests, live test | Tasks 1 to 7; live test in Task 6 |

**2. Placeholder scan.** No TBD, "similar to" or "add error handling"; every code step carries its code. Commit trailers name a concrete model, with the Global Constraints saying to write the implementer's own.

**3. Type consistency.** `ModulePath`, `NavFile`, `NavView`, `Route`, `PathFailure`, `Where`, `CaseToRun`, `MODULE_STEP`, `run_cases`, `run_case_as`, `run_step_routed`, `account_for_run`, `check_path`, `refuse_addresses`, `no_address`, `ModuleRecordResult`, `ModuleTryResult`, `RecordingEvent` are named the same wherever used. Command argument orders match the TypeScript calls: `autoRunReplay(org, project, pbiId, cases, account, browserName, watch)`, `autoRunSaveScript(org, project, script)`, `autoRunImportScripts(org, project, path)`, `autoRunRecordStart(org, project, module, account, browserName)`, `autoRunTryModulePath(org, project, module, account, browserName)`.

**4. Review Focus.** Five lines, each with its test in the owning task (Tasks 1, 2, 3, 5 and 7).
