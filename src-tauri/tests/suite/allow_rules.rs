//! With Auto Run on, Claude Code is told to allow its Auto Run tools without
//! asking: the app writes `CLAUDE_AUTORUN_RULES` (and, with API Templates on,
//! `CLAUDE_API_TEMPLATE_RULES`) into the allow file it already keeps for the
//! database switch, and only ever those entries. Every test here works in
//! temp folders; the real `~/.claude` and any real repository are never read.

use std::collections::BTreeSet;

use serde_json::{json, Value};
use v2_lib::ai_tools::{
    entry_tool, plan_allow, AllowState, RuleWant, CLAUDE_API_TEMPLATE_RULES, CLAUDE_AUTORUN_RULES, CLAUDE_DB_RULES,
    CURSOR_API_TEMPLATE_ENTRIES, CURSOR_AUTORUN_ENTRIES, CURSOR_DB_ENTRIES, TCM_SERVER,
};
use v2_lib::commands::ai_tools::{apply_allow_in, ToolHome};
use v2_lib::modules::Module;

fn mods(ms: &[Module]) -> BTreeSet<Module> {
    ms.iter().copied().collect()
}

fn auto_run_on() -> BTreeSet<Module> {
    mods(&[Module::DbAccess, Module::AutoRun])
}

fn api_on() -> BTreeSet<Module> {
    mods(&[Module::DbAccess, Module::AutoRun, Module::ApiTemplates])
}

fn state(db: bool, modules: BTreeSet<Module>, disabled: &[&str]) -> AllowState {
    AllowState { db, modules, disabled: Some(disabled.iter().map(|s| s.to_string()).collect()) }
}

/// A machine with Claude Code installed and our server registered in one
/// repository (`.mcp.json`), whose `.claude/settings.local.json` holds
/// `settings` (nothing when `None`).
struct Machine {
    home: tempfile::TempDir,
    repo: tempfile::TempDir,
    /// The app's data folder, where the record of what it wrote is kept.
    data: tempfile::TempDir,
    tools: ToolHome,
}

fn machine(settings: Option<&str>) -> Machine {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(
        repo.path().join(".mcp.json"),
        r#"{"mcpServers": {"tcm": {"command": "x", "args": ["--mcp"]}}}"#,
    )
    .unwrap();
    if let Some(s) = settings {
        std::fs::create_dir_all(repo.path().join(".claude")).unwrap();
        std::fs::write(repo.path().join(".claude").join("settings.local.json"), s).unwrap();
    }
    let tools = ToolHome {
        home: home.path().to_string_lossy().to_string(),
        appdata: home.path().join("appdata").to_string_lossy().to_string(),
        on_path: |_| false,
    };
    Machine { home, repo, data: tempfile::tempdir().unwrap(), tools }
}

impl Machine {
    fn dir(&self) -> String {
        self.repo.path().to_string_lossy().to_string()
    }
    fn file(&self) -> std::path::PathBuf {
        self.repo.path().join(".claude").join("settings.local.json")
    }
    fn text(&self) -> String {
        std::fs::read_to_string(self.file()).unwrap_or_default()
    }
    fn allow(&self) -> Vec<String> {
        let v: Value = serde_json::from_str(&self.text()).unwrap_or(json!({}));
        v["permissions"]["allow"]
            .as_array()
            .map(|a| a.iter().filter_map(|r| r.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    }
    fn apply(&self, st: &AllowState) -> Vec<v2_lib::commands::ai_tools::AutoApproveOutcome> {
        apply_allow_in(&self.tools, st, Some(&self.dir()), self.data.path())
    }
    fn record_file(&self) -> std::path::PathBuf {
        v2_lib::allow_record::AllowRecord::path(self.data.path())
    }
    /// Put `rules` into the repository's file as the person's own.
    fn persons(&self, rules: &[&str]) {
        std::fs::create_dir_all(self.repo.path().join(".claude")).unwrap();
        let text = serde_json::to_string_pretty(&json!({ "permissions": { "allow": rules } })).unwrap();
        std::fs::write(self.file(), text).unwrap();
    }
}

fn has_all(held: &[String], rules: &[&str]) -> bool {
    rules.iter().all(|r| held.iter().any(|h| h == r))
}

fn has_none(held: &[String], rules: &[&str]) -> bool {
    !rules.iter().any(|r| held.iter().any(|h| h == r))
}

const THEIRS: &str = "Bash(git status)";

fn their_file() -> String {
    serde_json::to_string_pretty(&json!({
        "env": { "A": "1" },
        "permissions": { "allow": [THEIRS, "mcp__other__tool"], "deny": ["Read(./.env)"], "ask": ["Bash(rm *)"] },
        "model": "opus"
    }))
    .unwrap()
}

// ------------------------------------------------------------ the lists

/// Every rule names a tool the server offers today, of the right module,
/// and none is a retired name. The server is the constant, not a literal.
#[test]
fn every_rule_names_a_real_tool_of_its_module() {
    let real = v2_lib::mcp::all_tool_names();
    let retired: Vec<&str> = v2_lib::mcp::RETIRED_TOOLS.iter().map(|(old, _)| *old).collect();
    let prefix = format!("mcp__{TCM_SERVER}__");
    for (rules, module) in [(CLAUDE_AUTORUN_RULES, Module::AutoRun), (CLAUDE_API_TEMPLATE_RULES, Module::ApiTemplates)] {
        assert!(!rules.is_empty());
        for rule in rules {
            let tool = rule.strip_prefix(&prefix).unwrap_or_else(|| panic!("{rule} is not {prefix}<tool>"));
            assert!(real.iter().any(|t| t == tool), "{rule}: {tool} is not a tool the server offers");
            assert!(!retired.contains(&tool), "{rule}: {tool} is a retired tool name");
            assert_eq!(v2_lib::ai_tools::module_of_tool(tool), Some(module), "{rule}");
            assert_eq!(entry_tool(rule), tool);
        }
        let mut sorted: Vec<_> = rules.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), rules.len(), "a rule is listed twice");
    }
}

/// The owner's list, less the two names the server no longer has.
#[test]
fn the_lists_are_the_owners_less_the_retired_names() {
    let mut all: Vec<&str> = CLAUDE_AUTORUN_RULES.iter().chain(CLAUDE_API_TEMPLATE_RULES).copied().collect();
    all.sort_unstable();
    assert_eq!(
        all,
        [
            "mcp__tcm__discover_autorun_actions",
            "mcp__tcm__end_autorun_discovery",
            "mcp__tcm__get_autorun_page",
            "mcp__tcm__get_kept_actions",
            "mcp__tcm__list_setups",
            "mcp__tcm__prove_api_template",
            "mcp__tcm__reset_autorun_browser",
            "mcp__tcm__run_api_template",
            "mcp__tcm__run_setup",
            "mcp__tcm__save_api_flow",
            "mcp__tcm__save_autorun_component",
            "mcp__tcm__save_autorun_script",
            "mcp__tcm__save_setup",
            "mcp__tcm__start_autorun_discovery",
            "mcp__tcm__try_autorun_action",
        ]
    );
    assert!(!all.contains(&"mcp__tcm__discover_autorun_action"), "retired: discover_autorun_actions covers it");
    assert!(!all.contains(&"mcp__tcm__release_autorun_browser"), "retired: reset_autorun_browser with release");
}

/// Cursor's entries are the same tools, `server:tool`.
#[test]
fn cursor_gets_the_same_tools() {
    let tools = |entries: &[&str]| entries.iter().map(|e| entry_tool(e).to_string()).collect::<Vec<_>>();
    assert_eq!(tools(CURSOR_AUTORUN_ENTRIES), tools(CLAUDE_AUTORUN_RULES));
    assert_eq!(tools(CURSOR_API_TEMPLATE_ENTRIES), tools(CLAUDE_API_TEMPLATE_RULES));
    assert_eq!(tools(CURSOR_DB_ENTRIES), tools(CLAUDE_DB_RULES));
    for e in CURSOR_AUTORUN_ENTRIES.iter().chain(CURSOR_API_TEMPLATE_ENTRIES) {
        assert_eq!(*e, format!("{TCM_SERVER}:{}", entry_tool(e)));
    }
}

// ------------------------------------------------------------ the plan

#[test]
fn a_tool_is_wanted_while_its_module_is_on_and_the_person_has_not_switched_it_off() {
    let st = state(false, auto_run_on(), &["try_autorun_action"]);
    assert_eq!(st.want_tool("save_autorun_script"), RuleWant::Put);
    assert_eq!(st.want_tool("try_autorun_action"), RuleWant::Take, "switched off on the tab");
    assert_eq!(st.want_tool("run_api_template"), RuleWant::Take, "its module is off");
    let unknown = AllowState { db: false, modules: auto_run_on(), disabled: None };
    assert_eq!(unknown.want_tool("save_autorun_script"), RuleWant::Leave, "the person's list is not known yet");
    assert_eq!(unknown.want_tool("run_api_template"), RuleWant::Take, "a module that is off needs no list");
}

#[test]
fn the_database_switch_decides_its_own_entries_alone() {
    for db in [false, true] {
        let plan = plan_allow(
            &state(db, auto_run_on(), &[]),
            CLAUDE_DB_RULES,
            CLAUDE_AUTORUN_RULES,
            CLAUDE_API_TEMPLATE_RULES,
        );
        let (db_side, other) = if db { (&plan.put, &plan.take) } else { (&plan.take, &plan.put) };
        assert!(CLAUDE_DB_RULES.iter().all(|r| db_side.contains(r)), "db={db}");
        assert!(CLAUDE_DB_RULES.iter().all(|r| !other.contains(r)), "db={db}");
        assert!(CLAUDE_AUTORUN_RULES.iter().all(|r| plan.put.contains(r)), "Auto Run's go in whatever db is ({db})");
        assert!(CLAUDE_API_TEMPLATE_RULES.iter().all(|r| plan.take.contains(r)), "API Templates is off");
    }
}

// ------------------------------------------------------------ the file

#[test]
fn auto_run_on_writes_its_rules_into_an_empty_file_and_keeps_the_persons_own() {
    let m = machine(None);
    let out = m.apply(&state(false, auto_run_on(), &[]));
    assert!(out.iter().any(|o| o.tool == "Claude Code" && o.applied), "{out:?}");
    assert!(has_all(&m.allow(), CLAUDE_AUTORUN_RULES), "{}", m.text());
    assert!(has_none(&m.allow(), CLAUDE_API_TEMPLATE_RULES), "API Templates is off");
    assert!(has_none(&m.allow(), CLAUDE_DB_RULES), "the database switch is off");

    // The owner's own extra rule, and everything else in the file, stays.
    let m = machine(Some(&their_file()));
    m.apply(&state(false, auto_run_on(), &[]));
    let held = m.allow();
    assert!(has_all(&held, CLAUDE_AUTORUN_RULES));
    assert!(held.iter().any(|r| r == THEIRS) && held.iter().any(|r| r == "mcp__other__tool"), "{held:?}");
    let v: Value = serde_json::from_str(&m.text()).unwrap();
    assert_eq!(v["deny"], Value::Null);
    assert_eq!(v["permissions"]["deny"], json!(["Read(./.env)"]));
    assert_eq!(v["permissions"]["ask"], json!(["Bash(rm *)"]));
    assert_eq!(v["env"], json!({ "A": "1" }));
    assert_eq!(v["model"], json!("opus"));
}

#[test]
fn auto_run_off_takes_its_rules_out_and_leaves_the_persons() {
    let m = machine(Some(&their_file()));
    m.apply(&state(false, api_on(), &[]));
    assert!(has_all(&m.allow(), CLAUDE_AUTORUN_RULES) && has_all(&m.allow(), CLAUDE_API_TEMPLATE_RULES));

    // Auto Run off takes API Templates with it (the module tree).
    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    let held = m.allow();
    assert!(has_none(&held, CLAUDE_AUTORUN_RULES) && has_none(&held, CLAUDE_API_TEMPLATE_RULES), "{held:?}");
    assert_eq!(held, [THEIRS, "mcp__other__tool"]);
    assert_eq!(m.text(), their_file(), "the person's file is as it was");
}

#[test]
fn api_templates_on_and_off_moves_only_its_own_rules() {
    let m = machine(None);
    m.apply(&state(false, auto_run_on(), &[]));
    let before = m.allow();
    m.apply(&state(false, api_on(), &[]));
    let held = m.allow();
    assert!(has_all(&held, CLAUDE_API_TEMPLATE_RULES) && has_all(&held, CLAUDE_AUTORUN_RULES));
    assert_eq!(held.len(), before.len() + CLAUDE_API_TEMPLATE_RULES.len());
    m.apply(&state(false, auto_run_on(), &[]));
    assert_eq!(m.allow(), before, "off again: exactly the API template rules went");
}

#[test]
fn a_tool_switched_off_on_the_tab_has_no_rule() {
    let m = machine(None);
    m.apply(&state(false, api_on(), &["try_autorun_action", "run_api_template"]));
    let held = m.allow();
    assert!(!held.iter().any(|r| r == "mcp__tcm__try_autorun_action"), "{held:?}");
    assert!(!held.iter().any(|r| r == "mcp__tcm__run_api_template"), "{held:?}");
    assert!(held.iter().any(|r| r == "mcp__tcm__save_autorun_script"));
    assert!(held.iter().any(|r| r == "mcp__tcm__prove_api_template"));

    // Switched back on: the rule returns. Switched off again: it goes.
    m.apply(&state(false, api_on(), &[]));
    assert!(held.len() < m.allow().len());
    assert!(has_all(&m.allow(), CLAUDE_AUTORUN_RULES) && has_all(&m.allow(), CLAUDE_API_TEMPLATE_RULES));
    m.apply(&state(false, api_on(), &["try_autorun_action"]));
    assert!(!m.allow().iter().any(|r| r == "mcp__tcm__try_autorun_action"));
}

#[test]
fn until_the_tabs_list_is_known_a_module_on_writes_nothing_but_a_module_off_still_takes_out() {
    let m = machine(None);
    let unknown = |modules| AllowState { db: false, modules, disabled: None };
    m.apply(&unknown(auto_run_on()));
    assert!(!m.file().exists(), "no rule for a tool the person may have switched off");

    m.apply(&state(false, api_on(), &[]));
    assert!(has_all(&m.allow(), CLAUDE_AUTORUN_RULES));
    m.apply(&unknown(auto_run_on()));
    assert!(has_all(&m.allow(), CLAUDE_AUTORUN_RULES), "left as the file has them");
    assert!(has_none(&m.allow(), CLAUDE_API_TEMPLATE_RULES), "a module that is off needs no list");
}

#[test]
fn the_database_switch_and_the_module_rules_do_not_depend_on_each_other() {
    let m = machine(None);
    m.apply(&state(true, mods(&[Module::DbAccess]), &[]));
    assert!(has_all(&m.allow(), CLAUDE_DB_RULES) && has_none(&m.allow(), CLAUDE_AUTORUN_RULES), "{}", m.text());

    m.apply(&state(true, auto_run_on(), &[]));
    assert!(has_all(&m.allow(), CLAUDE_DB_RULES) && has_all(&m.allow(), CLAUDE_AUTORUN_RULES));

    m.apply(&state(false, auto_run_on(), &[]));
    assert!(has_none(&m.allow(), CLAUDE_DB_RULES), "the database switch took its own out");
    assert!(has_all(&m.allow(), CLAUDE_AUTORUN_RULES), "and nothing of Auto Run's");

    m.apply(&state(true, mods(&[Module::DbAccess]), &[]));
    assert!(has_all(&m.allow(), CLAUDE_DB_RULES) && has_none(&m.allow(), CLAUDE_AUTORUN_RULES));
}

#[test]
fn on_then_off_leaves_a_file_byte_identical() {
    let m = machine(Some(&their_file()));
    let before = m.text();
    m.apply(&state(false, api_on(), &[]));
    assert_ne!(m.text(), before);
    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    assert_eq!(m.text(), before);

    // Nothing to do: the file is not rewritten.
    let stamp = std::fs::metadata(m.file()).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    assert_eq!(std::fs::metadata(m.file()).unwrap().modified().unwrap(), stamp);

    // And a file that did not exist is not left behind, nor its folder's other files.
    let m = machine(None);
    m.apply(&state(false, auto_run_on(), &[]));
    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    let v: Value = serde_json::from_str(&m.text()).unwrap();
    assert_eq!(v, json!({}), "an empty object, no empty permissions or allow list");
}

#[test]
fn a_file_that_is_not_an_object_is_refused_unchanged_and_the_answer_names_no_path() {
    for body in ["[1, 2]", "{ not json", r#""text""#, r#"{"permissions": {"allow": "x"}}"#] {
        let m = machine(Some(body));
        let out = m.apply(&state(false, auto_run_on(), &[]));
        assert_eq!(m.text(), body, "{body}");
        let cc = out.iter().find(|o| o.tool == "Claude Code").expect("Claude Code is named");
        assert!(!cc.applied && !cc.note.is_empty(), "{body}: {cc:?}");
        for dir in [m.repo.path(), m.home.path()] {
            assert!(!cc.note.contains(&dir.to_string_lossy().to_string()), "the note carries a path: {}", cc.note);
        }
        assert!(cc.note.contains("settings.local.json"), "it names the file: {}", cc.note);
    }
    // Taking out from a file the app never put anything in has nothing to
    // do - and does not touch it.
    let m = machine(Some("[1, 2]"));
    let out = m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    assert_eq!(m.text(), "[1, 2]");
    assert!(out.iter().all(|o| o.tool != "Claude Code" || o.applied), "{out:?}");
}

#[test]
fn switching_off_leaves_a_missing_file_missing() {
    let m = machine(None);
    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    assert!(!m.file().exists());
    assert!(!m.repo.path().join(".claude").exists());
}

/// Entries go in only where the server is registered; a file where it is
/// not registered is never created, but is still cleaned.
#[test]
fn rules_go_only_where_the_server_is_registered() {
    let m = machine(None);
    std::fs::remove_file(m.repo.path().join(".mcp.json")).unwrap();
    m.apply(&state(false, auto_run_on(), &[]));
    assert!(!m.file().exists(), "not registered here, so nothing written");
    let user = m.home.path().join(".claude").join("settings.json");
    assert!(!user.exists());

    // Registered for the machine instead: the user settings get them.
    std::fs::write(m.home.path().join(".claude.json"), r#"{"mcpServers": {"tcm": {"command": "x"}}}"#).unwrap();
    m.apply(&state(false, auto_run_on(), &[]));
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&user).unwrap()).unwrap();
    let held: Vec<String> = serde_json::from_value(v["permissions"]["allow"].clone()).unwrap();
    assert!(has_all(&held, CLAUDE_AUTORUN_RULES));
    assert!(!m.file().exists(), "the repository's file is not the registered one");

    // Rules the app wrote into a file where the server is no longer
    // registered still come out; the person's own stay.
    m.persons(&["mine"]);
    std::fs::write(
        m.repo.path().join(".mcp.json"),
        r#"{"mcpServers": {"tcm": {"command": "x", "args": ["--mcp"]}}}"#,
    )
    .unwrap();
    m.apply(&state(false, auto_run_on(), &[]));
    assert!(has_all(&m.allow(), CLAUDE_AUTORUN_RULES));
    std::fs::remove_file(m.repo.path().join(".mcp.json")).unwrap();
    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    assert_eq!(m.allow(), ["mine"]);
}

/// Unregistered tool, database switch on: module rules the app wrote still
/// come out of its files when the module goes off.
#[test]
fn module_rules_come_out_of_an_unregistered_tools_files_while_the_database_switch_is_on() {
    let m = machine(None);
    m.apply(&state(true, auto_run_on(), &[]));
    assert!(has_all(&m.allow(), CLAUDE_AUTORUN_RULES) && has_all(&m.allow(), CLAUDE_DB_RULES));

    std::fs::remove_file(m.repo.path().join(".mcp.json")).unwrap();
    let out = m.apply(&state(true, mods(&[Module::DbAccess]), &[]));
    assert!(has_none(&m.allow(), CLAUDE_AUTORUN_RULES), "{:?}", m.allow());
    assert!(has_all(&m.allow(), CLAUDE_DB_RULES), "the database switch is still on and its rules stay");
    assert!(out.iter().all(|o| o.tool != "Claude Code"), "a tool the server is not registered with is not reported as set: {out:?}");
}

#[test]
fn cursor_gets_the_same_rules_in_its_allowlist() {
    let m = machine(None);
    std::fs::create_dir_all(m.home.path().join(".cursor")).unwrap();
    std::fs::write(
        m.home.path().join(".cursor").join("mcp.json"),
        r#"{"mcpServers": {"tcm": {"command": "x"}}}"#,
    )
    .unwrap();
    let file = m.home.path().join(".cursor").join("permissions.json");
    std::fs::write(&file, r#"{"mcpAllowlist": ["other:tool"], "terminalAllowlist": ["ls"]}"#).unwrap();
    let held = || -> Vec<String> {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        serde_json::from_value(v["mcpAllowlist"].clone()).unwrap()
    };

    m.apply(&state(false, api_on(), &["try_autorun_action"]));
    let on = held();
    assert!(on.iter().any(|e| e == "tcm:save_autorun_script") && on.iter().any(|e| e == "tcm:run_api_template"), "{on:?}");
    assert!(!on.iter().any(|e| e == "tcm:try_autorun_action"));
    assert!(on.iter().any(|e| e == "other:tool"));

    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    assert_eq!(held(), ["other:tool"]);
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["terminalAllowlist"], json!(["ls"]));
}

// ------------------------------------------------------------ whose rules they are

/// The owner's own hand-written rule, with the same name as one the app
/// writes, is theirs: Auto Run going off leaves it.
#[test]
fn a_hand_written_rule_stays_when_auto_run_goes_off() {
    let mine = "mcp__tcm__start_autorun_discovery";
    let m = machine(None);
    m.persons(&[mine, "Bash(git status)"]);
    m.apply(&state(false, auto_run_on(), &[]));
    let held = m.allow();
    assert!(has_all(&held, CLAUDE_AUTORUN_RULES), "{held:?}");
    assert_eq!(held.iter().filter(|r| *r == mine).count(), 1, "not written twice: {held:?}");

    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    assert_eq!(m.allow(), [mine, "Bash(git status)"], "the app's seven went, the owner's own stayed");
}

/// A rule the app wrote comes out; one the person had before stays, even when
/// both are in the same module.
#[test]
fn only_a_rule_the_app_wrote_is_taken_out() {
    let m = machine(None);
    m.persons(&["mcp__tcm__get_autorun_page", "mcp__tcm__run_api_template"]);
    m.apply(&state(false, api_on(), &["try_autorun_action"]));
    assert!(m.allow().iter().any(|r| r == "mcp__tcm__save_autorun_script"));
    assert!(!m.allow().iter().any(|r| r == "mcp__tcm__try_autorun_action"));

    // One of the app's own is switched off on the tab: it goes. The
    // person's two stay while their tools are switched off, too.
    m.apply(&state(false, api_on(), &["save_autorun_script", "get_autorun_page", "run_api_template"]));
    let held = m.allow();
    assert!(!held.iter().any(|r| r == "mcp__tcm__save_autorun_script"), "{held:?}");
    assert!(held.iter().any(|r| r == "mcp__tcm__get_autorun_page"), "the person's: {held:?}");
    assert!(held.iter().any(|r| r == "mcp__tcm__run_api_template"), "the person's: {held:?}");

    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    assert_eq!(m.allow(), ["mcp__tcm__get_autorun_page", "mcp__tcm__run_api_template"]);
}

/// On, then off, with every rule already in the file: nothing is written either way.
#[test]
fn on_then_off_with_pre_existing_identical_rules_leaves_the_file_byte_identical() {
    let m = machine(None);
    let all: Vec<&str> = CLAUDE_AUTORUN_RULES.iter().chain(CLAUDE_API_TEMPLATE_RULES).copied().collect();
    m.persons(&all);
    // Nothing to tell the file apart by but its bytes: a style the app would not write.
    let text = format!("{{\"permissions\":{{\"allow\":{}}}}}\n", serde_json::to_string(&all).unwrap());
    std::fs::write(m.file(), &text).unwrap();

    m.apply(&state(false, api_on(), &[]));
    assert_eq!(m.text(), text, "found there, so not written");
    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    assert_eq!(m.text(), text, "found there, so not the app's to take out");
}

/// The record holds what the app added and nothing else, and names a file by
/// its tool and scope, not by its path.
#[test]
fn the_record_holds_only_what_the_app_added_and_no_path() {
    let m = machine(None);
    m.persons(&["mcp__tcm__get_autorun_page"]);
    m.apply(&state(false, auto_run_on(), &[]));
    let record: Value = serde_json::from_str(&std::fs::read_to_string(m.record_file()).unwrap()).unwrap();
    let files = record["files"].as_object().unwrap();
    assert_eq!(files.len(), 1, "{record}");
    let (key, entries) = files.iter().next().unwrap();
    assert!(key.starts_with("claude-code:project:"), "{key}");
    let mut written: Vec<String> = serde_json::from_value(entries.clone()).unwrap();
    written.sort();
    let mut want: Vec<String> =
        CLAUDE_AUTORUN_RULES.iter().filter(|r| **r != "mcp__tcm__get_autorun_page").map(|r| r.to_string()).collect();
    want.sort();
    assert_eq!(written, want, "the person's own rule is not on the record");

    let raw = std::fs::read_to_string(m.record_file()).unwrap();
    for dir in [m.repo.path(), m.home.path(), m.data.path()] {
        let d = dir.to_string_lossy().to_string();
        assert!(!raw.contains(&d) && !raw.contains(&d.replace('\\', "/")), "the record carries a path: {raw}");
    }

    // Off: what was recorded is forgotten as it goes.
    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    let record: Value = serde_json::from_str(&std::fs::read_to_string(m.record_file()).unwrap()).unwrap();
    assert_eq!(record["files"], json!({}), "{record}");
}

/// One repository's record is not another's: a rule the person wrote in a
/// second repository is not taken out because the app wrote it in the first.
#[test]
fn a_repositorys_rules_are_recorded_apart_from_anothers() {
    let a = machine(None);
    a.apply(&state(false, auto_run_on(), &[]));

    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join(".mcp.json"), r#"{"mcpServers": {"tcm": {"command": "x"}}}"#).unwrap();
    std::fs::create_dir_all(other.path().join(".claude")).unwrap();
    let theirs = other.path().join(".claude").join("settings.local.json");
    std::fs::write(&theirs, r#"{"permissions":{"allow":["mcp__tcm__try_autorun_action"]}}"#).unwrap();
    apply_allow_in(
        &a.tools,
        &state(false, mods(&[Module::DbAccess]), &[]),
        Some(&other.path().to_string_lossy()),
        a.data.path(),
    );
    assert_eq!(
        std::fs::read_to_string(&theirs).unwrap(),
        r#"{"permissions":{"allow":["mcp__tcm__try_autorun_action"]}}"#,
        "the other repository's own rule was not the app's"
    );
}

// The database switch's rules predate the record.

fn their_db_rules_file(m: &Machine) {
    let mut allow: Vec<&str> = CLAUDE_DB_RULES.to_vec();
    allow.push("mine");
    m.persons(&allow);
}

/// An older version wrote the database rules (the switch was on when the app
/// last ran): the first removal after the upgrade takes them out, as before.
#[test]
fn the_database_rules_an_older_version_wrote_come_out_once_on_the_first_removal() {
    let m = machine(None);
    their_db_rules_file(&m);
    // First look at upgrade: the switch is still on, the rules are found there.
    m.apply(&state(true, mods(&[Module::DbAccess]), &[]));
    assert!(has_all(&m.allow(), CLAUDE_DB_RULES));
    // Switched off: they were the app's, written before the record existed.
    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    assert_eq!(m.allow(), ["mine"]);

    // That was the one removal. A rule the person writes now is theirs.
    their_db_rules_file(&m);
    m.apply(&state(true, mods(&[Module::DbAccess]), &[]));
    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    assert!(has_all(&m.allow(), CLAUDE_DB_RULES), "{:?}", m.allow());
}

/// The switch was off when the app first looked, so it wrote nothing: the
/// same rules in the file are the person's, and stay.
#[test]
fn database_rules_found_with_the_switch_off_at_the_start_are_the_persons() {
    let m = machine(None);
    their_db_rules_file(&m);
    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    assert!(has_all(&m.allow(), CLAUDE_DB_RULES), "left");
    m.apply(&state(true, mods(&[Module::DbAccess]), &[]));
    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    assert!(has_all(&m.allow(), CLAUDE_DB_RULES), "{:?}", m.allow());
}

/// Rules the app puts in with the switch on, after the record began, are
/// recorded and come out like any other.
#[test]
fn database_rules_the_app_writes_after_the_record_began_come_out_with_the_switch() {
    let m = machine(None);
    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    m.apply(&state(true, mods(&[Module::DbAccess]), &[]));
    assert!(has_all(&m.allow(), CLAUDE_DB_RULES));
    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    assert!(has_none(&m.allow(), CLAUDE_DB_RULES));
}

/// A record that cannot be read starts again, without losing the person's file.
#[test]
fn an_unreadable_record_starts_over_and_the_persons_file_is_safe() {
    let m = machine(None);
    m.persons(&["mcp__tcm__get_autorun_page"]);
    std::fs::write(m.record_file(), "{ not json").unwrap();
    m.apply(&state(false, auto_run_on(), &[]));
    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    assert_eq!(m.allow(), ["mcp__tcm__get_autorun_page"]);
}

#[test]
fn cursor_leaves_an_entry_the_person_had_and_removes_its_own() {
    let m = machine(None);
    std::fs::create_dir_all(m.home.path().join(".cursor")).unwrap();
    std::fs::write(
        m.home.path().join(".cursor").join("mcp.json"),
        r#"{"mcpServers": {"tcm": {"command": "x"}}}"#,
    )
    .unwrap();
    let file = m.home.path().join(".cursor").join("permissions.json");
    std::fs::write(&file, r#"{"mcpAllowlist": ["tcm:try_autorun_action", "other:tool"]}"#).unwrap();
    let held = || -> Vec<String> {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        serde_json::from_value(v["mcpAllowlist"].clone()).unwrap()
    };
    m.apply(&state(false, auto_run_on(), &[]));
    assert!(held().iter().any(|e| e == "tcm:save_autorun_script"));
    m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    assert_eq!(held(), ["tcm:try_autorun_action", "other:tool"]);
}

// ------------------------------------------------------------ off the main thread

/// These four now write the AI tools' files and look programs up on PATH
/// (the allow-rule sync), so they must be async commands run off the main
/// thread. This stops compiling if any goes back to a sync fn.
#[allow(dead_code)]
fn the_commands_that_sync_the_allow_files_are_async(app: tauri::AppHandle) {
    fn is_future<F: std::future::Future>(_: F) {}
    is_future(v2_lib::commands::ai_bridge::set_bridge_context(
        app.clone(),
        String::new(),
        String::new(),
        None,
        None,
        Vec::new(),
        None,
        None,
        Default::default(),
    ));
    is_future(v2_lib::commands::ai_tools::register_ai_tool(
        app.clone(),
        String::new(),
        None,
        Vec::new(),
        false,
    ));
    is_future(v2_lib::commands::modules::modules_set(app.clone(), Module::AutoRun, true));
    is_future(v2_lib::commands::misc::set_extras_unlocked(app, false));
}

/// The module switch's re-sync copies what it needs out of the bridge and
/// lets both locks go before it writes anything.
#[test]
fn the_allow_files_are_synced_with_no_bridge_lock_held() {
    use v2_lib::ai_bridge::{BridgeContext, BridgeState};
    use v2_lib::commands::ai_bridge::BridgeHandle;
    use v2_lib::commands::modules::resync_commands_with;

    let none = BridgeHandle::default();
    let mut called = false;
    resync_commands_with(&none, |_, _, _| called = true);
    assert!(!called, "no bridge running: nothing is synced");

    let ctx = BridgeContext {
        disabled_tools: vec!["try_autorun_action".to_string()],
        working_dir: Some("D:/repo".to_string()),
        ..Default::default()
    };
    let shared = BridgeState::new(ctx, "1.0.0".to_string());
    let handle = BridgeHandle::default();
    *handle.running.lock().unwrap() = Some((std::sync::Arc::clone(&shared), 1234));
    let mut seen: Option<(Vec<String>, Option<String>)> = None;
    resync_commands_with(&handle, |_, disabled, dir| {
        assert!(handle.running.try_lock().is_ok(), "the bridge handle is locked while files are written");
        assert!(shared.ctx.try_lock().is_ok(), "the bridge context is locked while files are written");
        seen = Some((disabled.to_vec(), dir.map(str::to_string)));
    });
    assert_eq!(seen, Some((vec!["try_autorun_action".to_string()], Some("D:/repo".to_string()))));
}

// ------------------------------------------------------------ one file does not stop the other

/// The repository's settings file is not readable as JSON: the machine's file
/// is still brought into line, and the failure is reported.
#[test]
fn an_unreadable_file_does_not_stop_the_other_file() {
    let m = machine(None);
    std::fs::write(m.home.path().join(".claude.json"), r#"{"mcpServers": {"tcm": {"command": "x"}}}"#).unwrap();
    let user = m.home.path().join(".claude").join("settings.json");
    let user_allow = || -> Vec<String> {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&user).unwrap_or_default()).unwrap_or(json!({}));
        serde_json::from_value(v["permissions"]["allow"].clone()).unwrap_or_default()
    };
    m.apply(&state(false, auto_run_on(), &[]));
    assert!(has_all(&user_allow(), CLAUDE_AUTORUN_RULES), "{:?}", user_allow());

    // The repository's file goes bad; it is the first the sync meets.
    std::fs::create_dir_all(m.repo.path().join(".claude")).unwrap();
    std::fs::write(m.file(), "{ not json").unwrap();
    let out = m.apply(&state(false, mods(&[Module::DbAccess]), &[]));
    assert!(has_none(&user_allow(), CLAUDE_AUTORUN_RULES), "the machine's file was cleaned: {:?}", user_allow());
    assert_eq!(m.text(), "{ not json", "the unreadable file is left as it was");
    let cc = out.iter().find(|o| o.tool == "Claude Code").expect("Claude Code is reported");
    assert!(!cc.applied && cc.note.contains("settings.local.json"), "{cc:?}");
}
