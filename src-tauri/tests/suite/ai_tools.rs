//! Pure-logic tests for AI-tool detection + registration: `merge_entry`
//! never clobbers unrelated JSON, and `detect` reads a fake temp-dir layout
//! the same way it would read the real home/appdata dirs.

use v2_lib::ai_tools::{
    atomic_write, atomic_write_with, claude_cli_candidates, command_dir, command_files, command_files_for, command_files_in,
    command_markdown, config_for, detect, detect_in, merge_entry, project_command_dir,
    remove_entry, renamed_command_markdown, renamed_line, tcm_server, McpServer, COMMAND_MARKER, COMMANDS,
    LEGACY_DB_SERVER, RENAMED_COMMANDS, TCM_SERVER, TOOL_SPECS,
};
use v2_lib::commands::ai_tools::{mcp_add_args, mcp_add_command, project_root, remove_legacy_db_server_now};

/// Minimal self-cleaning temp directory (no `tempfile` crate - none is a
/// dependency of this project). Unique per-call via time + an atomic
/// counter so parallel `#[test]` runs never collide.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tcm-ai-tools-test-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}


/// The VS Code report. `claude` resolves to `~/.local/bin/claude.exe` on a
/// native install, and that directory is on the PATH of the sessions Claude
/// Code starts - not in HKCU\\Environment and not in the system Path. A GUI
/// app inherits Explorer's environment, so shelling out to a bare `claude`
/// failed on exactly the machines where Claude Code demonstrably worked.
/// Detection still said installed, because ~/.claude is there.
#[test]
fn the_claude_cli_is_looked_for_where_the_installers_put_it() {
    let c = claude_cli_candidates("C:/Users/Sam", "C:/Users/Sam/AppData/Roaming");
    let shown: Vec<String> = c.iter().map(|p| p.display().to_string().replace(std::path::MAIN_SEPARATOR, "/")).collect();

    // The native install - the one that was failing.
    assert!(
        shown.iter().any(|p| p.ends_with("Sam/.local/bin/claude.exe")),
        "{shown:?}"
    );
    // And the npm shim, which worked all along because %APPDATA%/npm is
    // normally on the persisted PATH.
    assert!(
        shown.iter().any(|p| p.ends_with("AppData/Roaming/npm/claude.cmd")),
        "{shown:?}"
    );
    // Native first: it is the install that needs the absolute path.
    assert!(shown[0].ends_with("Sam/.local/bin/claude.exe"), "{shown:?}");
}

/// One command per thing a person reaches for by name; every other tool
/// is called by the assistant when the guide says so. This test checks
/// that each command that exists describes itself, not that every tool
/// gets one.
#[test]
fn every_tool_gets_a_command_and_each_describes_itself() {
    // Only what a person reaches for by name. Everything else the
    // assistant calls on its own when the guide says so; a command per
    // tool made the picker a list of things nobody should have to know.
    // Auto Test is still in development: its tools are hidden and its
    // commands (`setup`, `heal`) are left out with them wherever they are
    // off, so nothing in the picker points at it.
    const TOOLS: [&str; 10] = [
        "begin-test-case-writing", "optimize", "get-wiki-info", "page", "setup", "discover", "map-menus", "heal",
        "gen-tests", "explore",
    ];
    let stems: Vec<&str> = COMMANDS.iter().map(|c| c.stem).collect();
    assert_eq!(stems, TOOLS, "one command per tool, in call order");

    for c in COMMANDS {
        let md = command_markdown(c);
        assert!(md.starts_with("---
"), "{} needs frontmatter: {md}", c.stem);
        assert!(md.contains(&format!("name: {}", c.stem)), "{md}");
        // Quoted, always. Two descriptions read naturally with a colon in
        // them, and an unquoted colon makes the frontmatter unparseable -
        // YAML reads it as a second mapping key. Claude Code happens to be
        // lenient enough to show it anyway, which is what made it worth
        // catching: nothing complains until something stricter reads it.
        assert!(
            md.contains(&format!("description: \"{}\"", c.desc)),
            "description must be a quoted scalar: {md}"
        );
        assert!(!c.desc.is_empty(), "{} has nothing to show in the picker", c.stem);
        // Identifies itself, so removal never touches a file the user
        // wrote at the same path.
        assert!(md.contains(COMMAND_MARKER), "{md}");
        // Anything taking input has to pass it on, or the argument is
        // silently dropped.
        if !c.hint.is_empty() {
            assert!(md.contains("$ARGUMENTS"), "{} takes a hint but ignores it: {md}", c.stem);
        }
        // A pointer, not a second copy of the rules: anything restating
        // the guide is stale the first time the guide changes.
        for copied in ["Not Automated", "semicolon", "automation_status"] {
            assert!(
                !md.contains(copied),
                "{} duplicates the guide - found {copied:?}",
                c.stem
            );
        }
    }
}

/// The rename: the one command everyone uses says what it does. Spaces
/// are not usable in a slash-command name, so the words are hyphenated.
#[test]
fn the_writing_command_is_named_for_what_it_does() {
    let c = COMMANDS.iter().find(|c| c.stem == "begin-test-case-writing").unwrap();
    assert_eq!(c.tool, "begin_test_case_writing");
    assert!(COMMANDS.iter().all(|c| c.stem != "write"));
    let w = COMMANDS.iter().find(|c| c.stem == "get-wiki-info").unwrap();
    assert_eq!(w.tool, "search_wiki");
    assert!(COMMANDS.iter().all(|c| c.stem != "wiki"));
    // "failures" went with get_run_failures: its successor, get_run_results,
    // is one an assistant reaches for on its own.
    for gone in ["fanout", "guide", "examples", "suites", "suite-cases", "coverage", "validate", "transform", "tags", "pbis", "failures"] {
        assert!(COMMANDS.iter().all(|c| c.stem != gone), "{gone} is no longer a command");
    }
}

/// A tool switched off in the app must lose its command. The tool side
/// already refuses a disabled tool twice - filtered from `tools/list`,
/// refused again on call for clients working from a cached list - but the
/// picker was still offering it, and the picker is where a person looks.
#[test]
fn a_disabled_tool_loses_its_command() {
    let all = command_files("C:/Users/Sam");
    let some = command_files_for("C:/Users/Sam", &["optimize_cases".to_string()]);
    assert_eq!(some.len(), all.len() - 1);
    assert!(
        !some.iter().any(|(p, _)| p.ends_with("optimize.md")),
        "the command for a switched-off tool must not be written"
    );
    // And everything else stays - disabling one must not clear the set.
    assert!(some.iter().any(|(p, _)| p.ends_with("begin-test-case-writing.md")));

    // Nothing disabled is the whole set, so the plain call is unchanged.
    assert_eq!(command_files_for("C:/Users/Sam", &[]).len(), all.len());
}

/// Every command names the tool it reaches, and that name has to be the
/// one the MCP server actually exposes - a typo here silently makes the
/// command un-disableable, because nothing would ever match it.
#[test]
fn each_command_names_a_real_tool() {
    // The tool list as `mcp.rs` declares it. Checked by scanning that
    // file rather than by copying the names, so the two cannot drift.
    let mcp = include_str!("../../src/mcp.rs");
    for c in COMMANDS {
        assert!(
            mcp.contains(&format!("\"name\": \"{}\"", c.tool)),
            "/tcm:{} points at {:?}, which mcp.rs does not expose",
            c.stem,
            c.tool
        );
    }
}

/// Namespaced, so the entries group as `tcm:*` rather than scattering
/// through a picker that already has other things in it.
#[test]
fn the_commands_land_in_their_own_namespace() {
    let dir = command_dir("C:/Users/Sam").display().to_string().replace(std::path::MAIN_SEPARATOR, "/");
    assert_eq!(dir, "C:/Users/Sam/.claude/commands/tcm");

    let files = command_files("C:/Users/Sam");
    assert_eq!(files.len(), COMMANDS.len());
    let first = files[0].0.display().to_string().replace(std::path::MAIN_SEPARATOR, "/");
    assert_eq!(first, "C:/Users/Sam/.claude/commands/tcm/begin-test-case-writing.md");
}
#[test]
fn merge_entry_creates_key_and_entry_on_empty_object() {
    let out = merge_entry("{}", "mcpServers", &tcm_server("C:/app/v2.exe")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["mcpServers"]["tcm"]["command"], "C:/app/v2.exe");
    assert_eq!(v["mcpServers"]["tcm"]["args"][0], "--mcp");
}

#[test]
fn merge_entry_preserves_unrelated_servers() {
    let existing = r#"{
        "mcpServers": {
            "other-server": { "command": "other.exe", "args": [] }
        },
        "someOtherTopLevelField": true
    }"#;
    let out = merge_entry(existing, "mcpServers", &tcm_server("C:/app/v2.exe")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["mcpServers"]["other-server"]["command"], "other.exe");
    assert_eq!(v["mcpServers"]["tcm"]["command"], "C:/app/v2.exe");
    assert_eq!(v["someOtherTopLevelField"], true);
}

#[test]
fn merge_entry_replaces_our_existing_entry() {
    let existing = r#"{
        "mcpServers": {
            "tcm": { "command": "stale/old.exe", "args": ["--old-flag"] }
        }
    }"#;
    let out = merge_entry(existing, "mcpServers", &tcm_server("C:/app/v2.exe")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["mcpServers"]["tcm"]["command"], "C:/app/v2.exe");
    assert_eq!(v["mcpServers"]["tcm"]["args"][0], "--mcp");
    // Only one entry under the key - no duplicate/stale leftover.
    assert_eq!(v["mcpServers"].as_object().unwrap().len(), 1);
}

#[test]
fn remove_entry_deletes_ours_and_preserves_everything_else() {
    let existing = r#"{
        "otherTopLevel": true,
        "mcpServers": {
            "someone-else": { "command": "x.exe" },
            "tcm": { "command": "C:/app/v2.exe", "args": ["--mcp"] }
        }
    }"#;
    let out = remove_entry(existing, "mcpServers", TCM_SERVER).unwrap().expect("entry was present");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["mcpServers"].get("tcm").is_none());
    assert_eq!(v["mcpServers"]["someone-else"]["command"], "x.exe");
    assert_eq!(v["otherTopLevel"], true);
}

#[test]
fn remove_entry_is_a_no_op_when_absent() {
    // No key at all, and key present but without our entry: both None.
    assert_eq!(remove_entry("{}", "mcpServers", TCM_SERVER).unwrap(), None);
    let existing = r#"{ "mcpServers": { "someone-else": { "command": "x.exe" } } }"#;
    assert_eq!(remove_entry(existing, "mcpServers", TCM_SERVER).unwrap(), None);
}

#[test]
fn remove_entry_errors_on_invalid_json_never_clobbers() {
    assert!(remove_entry("{ not valid json", "mcpServers", TCM_SERVER).is_err());
}

#[test]
fn merge_entry_errors_on_invalid_json_never_clobbers() {
    let result = merge_entry("{ not valid json", "mcpServers", &tcm_server("C:/app/v2.exe"));
    assert!(result.is_err());
}

#[test]
fn merge_entry_errors_when_key_is_not_an_object() {
    let existing = r#"{ "mcpServers": "oops-a-string" }"#;
    let result = merge_entry(existing, "mcpServers", &tcm_server("C:/app/v2.exe"));
    assert!(result.is_err());
}

#[test]
fn merge_entry_errors_when_root_is_not_an_object() {
    let result = merge_entry("[]", "mcpServers", &tcm_server("C:/app/v2.exe"));
    assert!(result.is_err());
}

/// Builds a temp-dir "home" and "appdata" with a fake install layout:
/// - claude-code: `.claude` dir present, `.claude.json` already registered
/// - claude-desktop: config file present but NOT registered
/// - cursor: `.cursor` dir present, no config file at all yet
/// - vscode / windsurf: nothing present (not installed)
fn fake_layout() -> (TempDir, TempDir) {
    let home = TempDir::new();
    let appdata = TempDir::new();

    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::write(
        home.path().join(".claude.json"),
        r#"{"mcpServers": {"tcm": {"command": "x", "args": ["--mcp"]}}}"#,
    )
    .unwrap();

    std::fs::create_dir_all(appdata.path().join("Claude")).unwrap();
    std::fs::write(
        appdata.path().join("Claude").join("claude_desktop_config.json"),
        r#"{"mcpServers": {"other-server": {"command": "y"}}}"#,
    )
    .unwrap();

    std::fs::create_dir_all(home.path().join(".cursor")).unwrap();

    (home, appdata)
}

#[test]
fn detect_finds_installed_and_registered_states() {
    let (home, appdata) = fake_layout();
    let home_str = home.path().to_string_lossy().to_string();
    let appdata_str = appdata.path().to_string_lossy().to_string();
    let on_path = |_cmd: &str| false;

    let tools = detect(&home_str, &appdata_str, &on_path);
    assert_eq!(tools.len(), 5);

    let by_id = |id: &str| tools.iter().find(|t| t.id == id).unwrap();

    let cc = by_id("claude-code");
    assert!(cc.installed, "claude-code: .claude dir should mark it installed");
    assert_eq!(cc.registered_servers, vec![TCM_SERVER], "claude-code: .claude.json already has ours");

    let cd = by_id("claude-desktop");
    assert!(cd.installed, "claude-desktop: appdata/Claude dir present marks it installed");
    assert!(cd.registered_servers.is_empty(), "claude-desktop: config exists but has no managed entry");

    let cursor = by_id("cursor");
    assert!(cursor.installed, "cursor: .cursor dir present");
    assert!(cursor.registered_servers.is_empty(), "cursor: no config file yet");

    let vscode = by_id("vscode");
    assert!(!vscode.installed, "vscode: not on PATH, no marker dir");

    let windsurf = by_id("windsurf");
    assert!(!windsurf.installed, "windsurf: no .codeium/windsurf dir");
}

#[test]
fn detect_uses_on_path_probe_for_path_based_tools() {
    let (home, appdata) = fake_layout();
    let home_str = home.path().to_string_lossy().to_string();
    let appdata_str = appdata.path().to_string_lossy().to_string();
    let on_path = |cmd: &str| cmd == "code";

    let tools = detect(&home_str, &appdata_str, &on_path);
    let vscode = tools.iter().find(|t| t.id == "vscode").unwrap();
    assert!(vscode.installed, "vscode: 'code' reported on PATH");
    assert!(vscode.registered_servers.is_empty(), "vscode: no mcp.json in fake appdata");
}


// ------------------------------------------------ servers with environment

/// Somebody else's server, the shape a config entry with environment takes.
fn env_server() -> McpServer {
    let mut env = std::collections::BTreeMap::new();
    env.insert("API_URL".to_string(), "https://example.test/api".to_string());
    env.insert("MODE".to_string(), "read-only".to_string());
    McpServer {
        name: "another-server".to_string(),
        command: "C:/tools/other.exe".to_string(),
        args: vec![],
        env,
    }
}

/// A server configured through env vars has them survive into the config
/// verbatim.
#[test]
fn a_server_with_environment_writes_it_verbatim() {
    let out = merge_entry("{}", "mcpServers", &env_server()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let entry = &v["mcpServers"]["another-server"];
    assert_eq!(entry["command"], "C:/tools/other.exe");
    assert_eq!(entry["env"]["API_URL"], "https://example.test/api");
    assert_eq!(entry["env"]["MODE"], "read-only");
}

/// Removing the old database server's entry leaves ours untouched.
#[test]
fn removing_the_legacy_entry_leaves_ours_alone() {
    let legacy = McpServer {
        name: LEGACY_DB_SERVER.to_string(),
        command: "db.exe".to_string(),
        args: vec![],
        env: Default::default(),
    };
    let ours = merge_entry("{}", "mcpServers", &tcm_server("C:/app/v2.exe")).unwrap();
    let both = merge_entry(&ours, "mcpServers", &legacy).unwrap();

    let left = remove_entry(&both, "mcpServers", LEGACY_DB_SERVER).unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_str(&left).unwrap();
    assert!(v["mcpServers"][LEGACY_DB_SERVER].is_null());
    assert_eq!(v["mcpServers"][TCM_SERVER]["command"], "C:/app/v2.exe");
}

/// Our own server has no environment, and an empty `env: {}` in a config
/// is noise some tools complain about.
#[test]
fn a_server_without_environment_omits_the_key_entirely() {
    let out = merge_entry("{}", "mcpServers", &tcm_server("C:/app/v2.exe")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["mcpServers"][TCM_SERVER].get("env").is_none());
}

/// VS Code's schema wants an explicit transport; the other tools infer it.
#[test]
fn vs_code_entries_declare_the_stdio_transport() {
    let vscode = merge_entry("{}", "servers", &env_server()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&vscode).unwrap();
    assert_eq!(v["servers"]["another-server"]["type"], "stdio");

    let other = merge_entry("{}", "mcpServers", &env_server()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&other).unwrap();
    assert!(v["mcpServers"]["another-server"].get("type").is_none());
}

#[test]
fn detect_reports_each_managed_server_separately() {
    let dir = TempDir::new();
    std::fs::create_dir_all(dir.path().join(".cursor")).unwrap();
    std::fs::write(
        dir.path().join(".cursor").join("mcp.json"),
        serde_json::json!({
            "mcpServers": {
                "tcm": { "command": "v2.exe" },
                "somebody-elses": { "command": "other.exe" }
            }
        })
        .to_string(),
    )
    .unwrap();

    let tools = detect(&dir.path().to_string_lossy(), "", &|_| false);
    let cursor = tools.iter().find(|t| t.id == "cursor").unwrap();
    assert_eq!(cursor.registered_servers, vec![TCM_SERVER]);
    assert!(
        !cursor.registered_servers.iter().any(|s| s == "somebody-elses"),
        "only servers this app manages are reported"
    );
}

// ---------------------------------------------------------- per-repo scope

/// Which tools can be told about a server per repository, and where.
#[test]
fn project_configs_sit_in_the_repo_for_the_tools_that_have_them() {
    let at = |id: &str| TOOL_SPECS.iter().find(|s| s.id == id).unwrap();
    let path = |id: &str| {
        let (p, _, _) = config_for(at(id), "C:/Users/Sam", "C:/Users/Sam/AppData/Roaming", Some("D:/repo"));
        p.display().to_string().replace(std::path::MAIN_SEPARATOR, "/")
    };
    assert_eq!(path("claude-code"), "D:/repo/.mcp.json");
    assert_eq!(path("cursor"), "D:/repo/.cursor/mcp.json");
    assert_eq!(path("vscode"), "D:/repo/.vscode/mcp.json");
    // No project scope exists for these two - a repo changes nothing.
    assert!(path("claude-desktop").contains("claude_desktop_config.json"));
    assert!(path("windsurf").contains("mcp_config.json"));
    assert_eq!(config_for(at("vscode"), "h", "a", Some("D:/repo")).1, "servers");
    assert_eq!(config_for(at("claude-code"), "h", "a", Some("D:/repo")).2, "project");
    assert_eq!(config_for(at("claude-code"), "h", "a", None).2, "global");
    assert_eq!(config_for(at("windsurf"), "h", "a", Some("D:/repo")).2, "global");
}

/// With a repo given, registration state is read from the repo's own
/// config - a user-scope entry must not make the repo look registered.
#[test]
fn detect_reads_the_repo_config_when_a_working_dir_is_given() {
    let (home, appdata) = fake_layout(); // ~/.claude.json carries ours globally
    let repo = TempDir::new();
    std::fs::write(
        repo.path().join(".mcp.json"),
        r#"{"mcpServers": {"phr-db-mcp": {"command": "db.exe"}}}"#,
    )
    .unwrap();
    let home_str = home.path().to_string_lossy().to_string();
    let appdata_str = appdata.path().to_string_lossy().to_string();
    let repo_str = repo.path().to_string_lossy().to_string();
    let on_path = |_cmd: &str| false;

    let tools = detect_in(&home_str, &appdata_str, &on_path, Some(repo_str.as_str()));
    let cc = tools.iter().find(|t| t.id == "claude-code").unwrap();
    assert_eq!(cc.scope, "project");
    assert_eq!(
        cc.registered_servers,
        vec![LEGACY_DB_SERVER],
        "the repo has the old database server's entry, not ours - reported so it can be removed"
    );
    let cd = tools.iter().find(|t| t.id == "claude-desktop").unwrap();
    assert_eq!(cd.scope, "global", "no project config exists for Claude Desktop");
}

/// An entry left in the MACHINE-WIDE config while the row reads a
/// repository's is reported separately - it shadows the project one in most
/// clients, so the UI has to be able to offer to retire it. It must NOT
/// appear as `registered_servers`, which is what "Registered ✓" means.
#[test]
fn a_repo_row_reports_the_leftover_global_entry_separately() {
    let (home, appdata) = fake_layout(); // ~/.claude.json carries ours globally
    let repo = TempDir::new();
    std::fs::write(repo.path().join(".mcp.json"), r#"{"mcpServers": {}}"#).unwrap();
    let home_str = home.path().to_string_lossy().to_string();
    let appdata_str = appdata.path().to_string_lossy().to_string();
    let repo_str = repo.path().to_string_lossy().to_string();
    let on_path = |_cmd: &str| false;

    let tools = detect_in(&home_str, &appdata_str, &on_path, Some(repo_str.as_str()));
    let cc = tools.iter().find(|t| t.id == "claude-code").unwrap();
    assert!(cc.registered_servers.is_empty(), "the repo carries nothing: {cc:?}");
    assert_eq!(cc.global_registered_servers, vec![TCM_SERVER]);
    // A tool with no project config reads its global config as its own row -
    // repeating it here would show every such tool as doubly registered.
    let cd = tools.iter().find(|t| t.id == "claude-desktop").unwrap();
    assert_eq!(cd.scope, "global");
    assert!(cd.global_registered_servers.is_empty(), "{cd:?}");
}

#[test]
fn without_a_working_dir_detection_is_global_and_says_so() {
    let (home, appdata) = fake_layout();
    let home_str = home.path().to_string_lossy().to_string();
    let appdata_str = appdata.path().to_string_lossy().to_string();
    let on_path = |_cmd: &str| false;
    let tools = detect_in(&home_str, &appdata_str, &on_path, None);
    assert!(tools.iter().all(|t| t.scope == "global"));
    assert_eq!(tools, detect(&home_str, &appdata_str, &on_path));
}

/// The skills go where Claude Code looks for a repository's own commands.
#[test]
fn the_repo_commands_land_under_the_repos_dot_claude() {
    let dir = project_command_dir("D:/repo").display().to_string().replace(std::path::MAIN_SEPARATOR, "/");
    assert_eq!(dir, "D:/repo/.claude/commands/tcm");
    let files = command_files_in(&project_command_dir("D:/repo"), &[]);
    assert_eq!(files.len(), COMMANDS.len());
    let first = files[0].0.display().to_string().replace(std::path::MAIN_SEPARATOR, "/");
    assert_eq!(first, "D:/repo/.claude/commands/tcm/begin-test-case-writing.md");
    assert!(files[0].1.contains(COMMAND_MARKER), "still ours to remove later");
}

use std::collections::BTreeSet;
use v2_lib::ai_tools::{
    effective_disabled_for, module_of_tool, API_TEMPLATE_TOOLS, AUTORUN_TOOLS, CORE_TOOLS, CRYSTAL_TOOLS, DB_TOOLS,
};
use v2_lib::modules::{apply, Module};

fn mods(ms: &[Module]) -> BTreeSet<Module> {
    ms.iter().copied().collect()
}

fn all_mods() -> BTreeSet<Module> {
    Module::ALL.into_iter().collect()
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

/// Every module tool belongs to exactly one module, and the rest are Core.
#[test]
fn each_tool_belongs_to_its_module() {
    assert_eq!(
        AUTORUN_TOOLS,
        [
            "save_autorun_script",
            "get_autorun_script",
            "delete_autorun_script",
            "get_autorun_page",
            "probe_autorun_locator",
            "try_autorun_action",
            "replay_autorun_to_step",
            "start_autorun_discovery",
            "discover_autorun_actions",
            "save_autorun_area",
            "end_autorun_discovery",
            "reset_autorun_browser",
            "save_autorun_component",
            "get_autorun_failures",
            "record_quirk",
            "record_validation_rule",
            "mark_autorun_suspected_defect",
            "report_case_divergence",
            "rule_case_divergence",
            "clear_autorun_suspected_defect",
            "set_autorun_order",
            "propose_accounts",
            "set_account_lookup",
            "get_accounts",
            "check_autorun_sign_in",
            "inspect_sign_in_page",
            "propose_sign_in_recipe",
            "save_test_file",
            "get_autorun_network_wait",
            "search_autorun_requests",
            "get_autorun_request",
            "autorun_capture",
            "autorun_har",
            "set_autorun_headers",
            "autorun_cookies",
            "record_explore_change",
            "get_explore_changes",
            "get_kept_actions",
            "save_setup",
            "list_setups",
            "run_setup"
        ]
    );
    assert_eq!(
        API_TEMPLATE_TOOLS,
        [
            "list_api_templates",
            "prove_api_template",
            "run_api_template",
            "save_api_flow",
            "get_api_flow_progress",
            "save_api_fixture",
            "run_api_fixture",
            "send_api_request",
            "undo_explore_creates"
        ]
    );
    assert_eq!(DB_TOOLS, ["db_lookup", "db_query"]);
    assert_eq!(
        CRYSTAL_TOOLS,
        [
            "open_report",
            "read_report",
            "edit_report",
            "compare_reports",
            "check_report",
            "preview_report",
            "preview_report_live",
            "get_report_parameters",
            "find_in_report",
            "replace_in_report"
        ]
    );
    for t in AUTORUN_TOOLS {
        assert_eq!(module_of_tool(t), Some(Module::AutoRun), "{t}");
    }
    for t in API_TEMPLATE_TOOLS {
        assert_eq!(module_of_tool(t), Some(Module::ApiTemplates), "{t}");
    }
    for t in DB_TOOLS {
        assert_eq!(module_of_tool(t), Some(Module::DbAccess), "{t}");
    }
    for t in CRYSTAL_TOOLS {
        assert_eq!(module_of_tool(t), Some(Module::CrystalReports), "{t}");
    }
    for t in CORE_TOOLS.iter().chain(&["search_wiki", "search_pbis", "get_tags"]) {
        assert_eq!(module_of_tool(t), None, "{t}");
    }
}

/// Each module off removes exactly its own tools - and nothing else - from
/// what the assistant is offered; on, its tools are ordinary switchable
/// tools that the person's own switch still turns off.
#[test]
fn each_module_off_disables_exactly_its_tools() {
    let all = all_mods();
    assert_eq!(effective_disabled_for(&[], &all), Vec::<String>::new());
    for (m, tools) in [
        (Module::AutoRun, AUTORUN_TOOLS),
        (Module::ApiTemplates, API_TEMPLATE_TOOLS),
        (Module::DbAccess, DB_TOOLS),
        (Module::CrystalReports, CRYSTAL_TOOLS),
    ] {
        // Only `m` off - a state the tree forbids for DbAccess and AutoRun,
        // used here to show each module removes its own tools and no other.
        let mut on = all.clone();
        on.remove(&m);
        assert_eq!(effective_disabled_for(&[], &on), strings(tools), "{m:?}");
    }
    // Writing Style and Playwright have no assistant tools.
    for m in [Module::WritingStyle, Module::Playwright] {
        let mut on = all.clone();
        on.remove(&m);
        assert_eq!(effective_disabled_for(&[], &on), Vec::<String>::new(), "{m:?}");
    }
    assert_eq!(
        effective_disabled_for(&["save_autorun_script".into()], &all),
        vec!["save_autorun_script"],
        "with Auto Test on the person's own switch still turns its tools off"
    );
}

/// Switching Database Access off while Auto Test is on switches the whole
/// family off, so every Auto Test, API template and database tool is
/// disabled at once.
#[test]
fn database_access_off_takes_the_auto_run_family_tools_with_it() {
    let on = apply(&all_mods(), Module::DbAccess, false).unwrap();
    assert_eq!(on, mods(&[Module::WritingStyle, Module::CrystalReports]));
    let off = effective_disabled_for(&[], &on);
    let mut expected = strings(AUTORUN_TOOLS);
    expected.extend(strings(API_TEMPLATE_TOOLS));
    expected.extend(strings(DB_TOOLS));
    assert_eq!(off, expected, "Crystal Reports hangs off Core, so its tools stay");
}

/// The core set can never be switched off - whatever the frontend's list
/// says, with any modules on - and the off modules' tools come first,
/// without duplicates.
///
/// Validate, optimise and merge joined the core set: finishing a draft is
/// part of writing one, and an assistant that can write cases but cannot
/// check, order or merge them hands over nothing anyone can ship.
#[test]
fn the_effective_disabled_set_follows_the_modules_and_protects_the_core() {
    assert_eq!(
        CORE_TOOLS,
        [
            "begin_test_case_writing",
            "get_guide",
            "get_current_pbi",
            "get_test_cases",
            "get_bugs",
            "check_spec_coverage",
            "transform_cases",
            "validate_cases",
            "optimize_cases",
            "merge_case_files"
        ]
    );
    let none = mods(&[]);
    let mut module_tools = strings(AUTORUN_TOOLS);
    module_tools.extend(strings(API_TEMPLATE_TOOLS));
    module_tools.extend(strings(DB_TOOLS));
    module_tools.extend(strings(CRYSTAL_TOOLS));
    assert_eq!(effective_disabled_for(&[], &none), module_tools);
    assert_eq!(
        effective_disabled_for(&["optimize_cases".into(), "merge_case_files".into()], &none),
        module_tools,
        "the three that finish a draft cannot be switched off"
    );
    let got = effective_disabled_for(
        &["begin_test_case_writing".into(), "search_wiki".into(), "save_autorun_script".into()],
        &none,
    );
    let mut expected = module_tools.clone();
    expected.push("search_wiki".into());
    assert_eq!(got, expected, "module tools first, core dropped, no duplicates");

    let all = all_mods();
    assert_eq!(
        effective_disabled_for(&["optimize_cases".into(), "search_wiki".into(), "search_wiki".into()], &all),
        vec!["search_wiki"]
    );
    for name in DB_TOOLS {
        assert!(!CORE_TOOLS.contains(name), "{name} must be switchable");
        assert_eq!(effective_disabled_for(&[name.to_string()], &all), vec![*name], "{name} switches off like any other tool");
    }
}

/// `/tcm:heal` walks an assistant through failing Auto Test cases. It is an
/// Auto Test command, so it is written only where the Auto Test tools are
/// offered, and it names only tools the bridge really lists.
#[test]
fn the_heal_command_is_an_auto_run_routine_offered_only_with_auto_run() {
    let c = COMMANDS.iter().find(|c| c.stem == "heal").expect("a heal command");
    assert_eq!(c.tool, "get_autorun_failures");
    assert_eq!(c.desc, "Diagnose and repair failing Auto Test cases, one at a time");
    assert_eq!(c.hint, "[case ids, or blank for every failed case in the newest run]");

    let dir = std::path::Path::new("D:/repo/.claude/commands/tcm");
    let hidden = command_files_in(dir, &effective_disabled_for(&[], &mods(&[Module::DbAccess])));
    assert!(
        hidden.iter().all(|(p, _)| p.file_name().unwrap() != "heal.md"),
        "heal must not be written while Auto Test is off"
    );
    let offered = command_files_in(dir, &effective_disabled_for(&[], &all_mods()));
    assert!(offered.iter().any(|(p, _)| p.file_name().unwrap() == "heal.md"));

    let body = c.body.join("\n");
    let mcp = include_str!("../../src/mcp.rs");
    for tool in [
        "get_guide",
        "get_autorun_failures",
        "get_autorun_page",
        "probe_autorun_locator",
        "try_autorun_action",
        "replay_autorun_to_step",
        "save_autorun_script",
        "mark_autorun_suspected_defect",
    ] {
        assert!(body.contains(tool), "the heal routine never names `{tool}`");
        assert!(
            mcp.contains(&format!("\"name\": \"{tool}\"")),
            "`{tool}` is not in the bridge's tool list"
        );
    }
    assert!(!body.contains('\u{2014}'), "no em dashes in text an assistant reads");
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains("matched on its stable part with a non-exact name"), "{flat}");
    assert!(
        flat.contains("one `precondition` edit in the case's `edits` entry, with a `reason` saying what now creates that state"),
        "the heal routine never says a wrong precondition is fixed with a precondition edit: {flat}"
    );
    assert!(
        flat.contains("with the case's entry in `edits` - a TOP-LEVEL list beside `scripts`, one entry per case, never inside a script"),
        "{flat}"
    );
    assert!(flat.contains("refused unless that step failed in the case's newest run"), "{flat}");
    assert!(flat.contains("refused when the failure is one of the `STOP:` lines"), "{flat}");
    assert!(flat.contains("is one of the `STOP:` lines is reported as it is"), "{flat}");
    // A run that cannot reach an area: the assistant repairs the area's
    // path itself (owner decision 2026-10-20), never the script.
    assert!(flat.contains("repair the AREA's path yourself instead of asking the person"), "{flat}");
    assert!(flat.contains("`save_autorun_area` with the area's name"), "{flat}");
    // The skill quotes the unreached STOP line's own words, apart from the
    // setup line, which goes to the person.
    assert!(flat.contains("except the line \"the run could not reach this case's area\""), "{flat}");
    assert!(flat.contains("(\"the run could not take this case to its module screen\": no Module, no account, no path, an area never recorded) goes to the person"), "{flat}");
    assert!(!flat.contains("Could not reach module"), "the skill quotes the run's wording, not the STOP line: {flat}");
    assert!(mcp.contains("a case that could not reach its area: repair the area with save_autorun_area"), "the failures tool leaves out the area repair");
    assert!(flat.contains("put the old path back"), "{flat}");
    assert!(!flat.to_lowercase().contains("three"), "the routine must not count the STOP lines: {flat}");
    assert!(mcp.contains("could not reach its module screen"), "the failures tool leaves out the module-path stop");
    assert!(body.contains("Choosing a model for the work"), "the routine points at the guide's model section");
    let lower = flat.to_lowercase();
    for bound in [
        "at most 3 fix-run cycles",
        "stop at cycle 2 when the failure is identical to cycle 1's",
        "stop at once on a missing precondition or a product defect",
        "a real bug must surface as a failure",
        "2 green of 3 is a flake, not a fix",
        "report_case_divergence",
        "a healer pointed at a missing precondition will keep rewriting assertions and locators until an empty or disabled page satisfies them, which produces a script that passes and proves nothing",
        "needs a ruling",
        "gives_up",
        "every assumed and invented value a save listed",
        "every check given up",
        "a check whose value differs from what the page shows is never fixed by changing the value",
    ] {
        assert!(lower.contains(bound), "the heal routine never says {bound:?}: {flat}");
    }
}

/// `/tcm:setup` walks an assistant through getting the app ready to run a
/// module's cases: environment, accounts, areas, test files and the data
/// each case needs first. An Auto Test command, so it is written only where
/// the Auto Test tools are offered, and every tool it names is one the
/// bridge really lists.
#[test]
fn the_setup_command_is_an_auto_run_routine_offered_only_with_auto_run() {
    let c = COMMANDS.iter().find(|c| c.stem == "setup").expect("a setup command");
    // An Auto Test tool: the guide is Core's now, and the skill must still
    // be written only where Auto Test is.
    assert_eq!(c.tool, "start_autorun_discovery");
    assert_eq!(
        c.desc,
        "Set the app up for testing: environment, accounts, module, areas, test files and preconditions"
    );
    assert_eq!(c.hint, "[PBI id or suite, optional]");

    let dir = std::path::Path::new("D:/repo/.claude/commands/tcm");
    let hidden = command_files_in(dir, &effective_disabled_for(&[], &mods(&[Module::DbAccess])));
    assert!(
        hidden.iter().all(|(p, _)| p.file_name().unwrap() != "setup.md"),
        "setup must not be written while Auto Test is off"
    );
    let offered = command_files_in(dir, &effective_disabled_for(&[], &all_mods()));
    assert!(offered.iter().any(|(p, _)| p.file_name().unwrap() == "setup.md"));

    let body = c.body.join("\n");
    let mcp = include_str!("../../src/mcp.rs");
    let is_tool = |name: &str| mcp.contains(&format!("\"name\": \"{name}\""));
    for tool in [
        "get_guide",
        "db_lookup",
        "db_query",
        "propose_accounts",
        "set_account_lookup",
        "get_accounts",
        "list_api_templates",
    ] {
        assert!(body.contains(&format!("`{tool}`")), "the setup routine never names `{tool}`");
        assert!(is_tool(tool), "`{tool}` is not in the bridge's tool list");
    }
    // Everything it names in backticks is a real tool: a misspelt one is a
    // step the assistant cannot take.
    let named: Vec<&str> = body.split('`').skip(1).step_by(2).collect();
    assert!(named.len() >= 7, "{named:?}");
    for name in named {
        assert!(is_tool(name), "/tcm:setup names `{name}`, which mcp.rs does not expose");
    }
    assert!(!body.contains('\u{2014}'), "no em dashes in text an assistant reads");
    assert!(body.contains("$ARGUMENTS"), "{body}");
    assert!(body.contains("Choosing a model for the work"), "the routine points at the guide's model section");

    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    // The real controls, by the names the screens give them.
    for control in [
        "Auto Test, Setup, Site address",
        "Edit environments",
        "Add selected",
        "Record an area",
        "Test files, Manage, Add files",
        "API templates (create, edit and delete)",
    ] {
        assert!(flat.contains(control), "the routine does not name {control:?}: {flat}");
    }
    assert!(!flat.contains("HR_SM_USER_ACCOUNT"), "the table is HS_SM_USER_ACCOUNT: {flat}");
    for line in [
        "without reading or sending passwords: on a test environment the app looks each password up itself",
        "The user accounts are generally in the HS_SM_USER_ACCOUNT table: look there first (with `db_query`, on usernames and roles only, never a password column).",
        "If it says the lookup did not run, find the right table or columns with `db_query` on non-secret columns and call `set_account_lookup`",
    ] {
        assert!(flat.contains(line), "{line}: {flat}");
    }
    assert!(flat.contains("You cannot record an area yourself"), "{flat}");
    assert!(flat.contains("Scripts never run templates"), "{flat}");
    assert!(flat.contains("supervised run of a single case"), "{flat}");
}

/// Everywhere the assistant is told to propose accounts it is also told that
/// the app looks the passwords up itself, where the logins are, and how to
/// fix the lookup.
/// (The writer role is at its size cap and only points at `propose_accounts`;
/// the guide it reads holds the lines.)
#[test]
fn every_place_that_proposes_accounts_names_the_password_lookup_and_the_user_table() {
    let flatten = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let places = [
        ("ai_tools.rs", flatten(include_str!("../../src/ai_tools.rs").replace("\",\r\n", " ").replace("\"", "").as_str())),
        ("autorun/guide.rs", flatten(include_str!("../../src/autorun/guide.rs"))),
        ("api_templates/guide.rs", flatten(include_str!("../../src/api_templates/guide.rs"))),
        ("roles/repo-tools.md", flatten(include_str!("../../src/autorun/roles/repo-tools.md"))),
        ("roles/gen-test-tools.md", flatten(include_str!("../../src/autorun/roles/gen-test-tools.md"))),
    ];
    for (name, text) in &places {
        assert!(text.contains("HS_SM_USER_ACCOUNT"), "{name} does not name the user table");
        assert!(!text.contains("HR_SM_USER_ACCOUNT"), "{name} misspells the user table");
        assert!(text.contains("up itself"), "{name} does not say the app looks passwords up itself");
        assert!(text.contains("set_account_lookup"), "{name} does not name set_account_lookup");
        assert!(text.contains("look there first (with `db_query`"), "{name}");
        assert!(!text.contains("their passwords"), "{name} still sends the assistant to read passwords");
    }
}

/// A command renamed or dropped from `COMMANDS` (this branch trimmed 17
/// down to 7) has to be swept out of every existing install, not just left
/// unwritten going forward - and the sweep must never touch a file this
/// app did not write.
#[test]
fn writing_the_commands_sweeps_stale_ones_and_leaves_foreign_files_alone() {
    use v2_lib::commands::ai_tools::write_commands_in;

    let dir = TempDir::new();
    std::fs::write(
        dir.path().join("write.md"),
        format!("{COMMAND_MARKER}\nan old command this app wrote before the rename"),
    )
    .unwrap();
    std::fs::write(dir.path().join("mine.md"), "not generated by this app").unwrap();

    write_commands_in(dir.path(), &[]).unwrap();

    assert!(!dir.path().join("write.md").exists(), "a stale, renamed command must be swept");
    assert!(dir.path().join("mine.md").exists(), "a file without the marker is never touched");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("mine.md")).unwrap(),
        "not generated by this app"
    );
    assert!(dir.path().join("begin-test-case-writing.md").exists(), "the current command set is written");
}

#[test]
fn atomic_write_creates_new_file() {
    let dir = TempDir::new();
    let target = dir.path().join("config.json");
    atomic_write(&target, "hello").unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello");
}

#[test]
fn atomic_write_replaces_existing_file() {
    let dir = TempDir::new();
    let target = dir.path().join("config.json");
    std::fs::write(&target, "old-contents").unwrap();
    atomic_write(&target, "new-contents").unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "new-contents");
}

#[test]
fn atomic_write_leaves_no_stray_temp_file() {
    let dir = TempDir::new();
    let target = dir.path().join("config.json");
    atomic_write(&target, "contents").unwrap();

    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| name.contains("tcm-tmp"))
        .collect();
    assert!(leftovers.is_empty(), "expected no leftover temp files, found: {leftovers:?}");
}

// ---- final review: on Windows the rename over a draft fails while another
// ---- process (antivirus, an editor, the indexer) briefly holds the file.
// ---- A sharing violation / access denied is retried with backoff; the
// ---- temp file never outlives a failure. -----------------------------------

fn temp_files_in(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| name.contains("tcm-tmp"))
        .collect()
}

fn write_real(p: &std::path::Path, s: &str) -> std::io::Result<()> {
    std::fs::write(p, s)
}

#[test]
fn a_rename_held_by_another_process_is_retried_until_it_goes_through() {
    let dir = TempDir::new();
    let target = dir.path().join("draft.json");
    std::fs::write(&target, "old").unwrap();
    let mut attempts = 0;
    let mut waits = vec![];
    atomic_write_with(
        &target,
        "new",
        write_real,
        |from, to| {
            attempts += 1;
            if attempts <= 2 {
                // ERROR_SHARING_VIOLATION
                Err(std::io::Error::from_raw_os_error(32))
            } else {
                std::fs::rename(from, to)
            }
        },
        |d| waits.push(d.as_millis()),
    )
    .unwrap();
    assert_eq!(attempts, 3);
    assert_eq!(waits, vec![50, 100]);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
    assert!(temp_files_in(dir.path()).is_empty());
}

#[test]
fn a_rename_that_stays_denied_gives_up_after_five_retries_and_removes_the_temp_file() {
    let dir = TempDir::new();
    let target = dir.path().join("draft.json");
    std::fs::write(&target, "old").unwrap();
    let mut attempts = 0;
    let mut waits = vec![];
    let err = atomic_write_with(
        &target,
        "new",
        write_real,
        |_, _| {
            attempts += 1;
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        },
        |d| waits.push(d.as_millis()),
    )
    .unwrap_err();
    assert_eq!(attempts, 6, "the first try and five retries");
    assert_eq!(waits, vec![50, 100, 200, 400, 800]);
    assert!(err.contains("draft.json"), "{err}");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "old", "the draft is untouched");
    assert!(temp_files_in(dir.path()).is_empty(), "{:?}", temp_files_in(dir.path()));
}

#[test]
fn any_other_rename_failure_is_not_retried() {
    let dir = TempDir::new();
    let target = dir.path().join("draft.json");
    let mut attempts = 0;
    let mut waits = vec![];
    atomic_write_with(
        &target,
        "new",
        write_real,
        |_, _| {
            attempts += 1;
            Err(std::io::Error::from(std::io::ErrorKind::NotFound))
        },
        |d| waits.push(d.as_millis()),
    )
    .unwrap_err();
    assert_eq!(attempts, 1);
    assert!(waits.is_empty());
    assert!(temp_files_in(dir.path()).is_empty());
}

/// A temp write that fails part-way (a full disk) used to leave the
/// half-written temp file behind.
#[test]
fn a_failed_temp_write_leaves_no_temp_file() {
    let dir = TempDir::new();
    let target = dir.path().join("draft.json");
    let err = atomic_write_with(
        &target,
        "new",
        |p, _| {
            std::fs::write(p, "ne")?;
            Err(std::io::Error::other("disk full"))
        },
        |_, _| panic!("nothing to rename"),
        |_| {},
    )
    .unwrap_err();
    assert!(err.contains("disk full"), "{err}");
    assert!(temp_files_in(dir.path()).is_empty(), "{:?}", temp_files_in(dir.path()));
    assert!(!target.exists());
}

/// The regression that once broke a registration with environment: env
/// pairs BEFORE the name feed the CLI's variadic `-e`, which then eats the
/// name. Pin
/// name-first, `--` before the binary, and every env pair in between.
#[test]
fn mcp_add_puts_the_name_before_the_env_pairs() {
    let mut env = std::collections::BTreeMap::new();
    env.insert("MODE".to_string(), "read-only".to_string());
    env.insert(
        "CONNECTION_STRING".to_string(),
        "Server=tcp:db,1433;Database=HR;User Id=ro".to_string(),
    );
    let server = McpServer {
        name: "another-server".to_string(),
        command: r"C:\tools\other.exe".to_string(),
        args: vec![],
        env,
    };
    let args = mcp_add_args(&server, "project");

    let name_at = args.iter().position(|a| a == "another-server").unwrap();
    let first_env = args.iter().position(|a| a == "-e").unwrap();
    let dashes = args.iter().position(|a| a == "--").unwrap();
    let cmd_at = args.iter().position(|a| a.ends_with(".exe")).unwrap();
    assert!(name_at < first_env, "name must come before -e: {args:?}");
    assert!(first_env < dashes, "-e pairs sit before --: {args:?}");
    assert!(dashes < cmd_at, "the binary follows --: {args:?}");
}

/// No env pairs (the tcm server): name, then straight to `--`.
#[test]
fn mcp_add_without_env_is_name_then_command() {
    let server = McpServer {
        name: "tcm".to_string(),
        command: "v2.exe".to_string(),
        args: vec!["--mcp".to_string()],
        env: Default::default(),
    };
    assert_eq!(
        mcp_add_args(&server, "user"),
        vec!["mcp", "add", "--scope", "user", "tcm", "--", "v2.exe", "--mcp"]
    );
}

/// A repository registration is `--scope local`, which the CLI keys on its
/// cwd - the caller runs it inside the repo (see `run_claude_mcp_add`). It
/// was `project` until Claude Code's VS Code extension was found never to
/// approve a `.mcp.json` server: local scope needs no approval.
#[test]
fn a_repo_registration_asks_for_local_scope() {
    let server = McpServer {
        name: "tcm".to_string(),
        command: "v2.exe".to_string(),
        args: vec!["--mcp".to_string()],
        env: Default::default(),
    };
    let args = mcp_add_args(&server, "local");
    assert_eq!(&args[2..4], ["--scope", "local"]);
    assert_eq!(args, vec!["mcp", "add", "--scope", "local", "tcm", "--", "v2.exe", "--mcp"]);
}

/// The machine-wide choice is explicit: it sends every tool to its
/// global config even with a repository set, and without it a tool
/// that registers per repository still refuses to go anywhere else.
#[test]
fn the_machine_wide_choice_targets_the_global_config_for_every_tool() {
    let cc = TOOL_SPECS.iter().find(|s| s.id == "claude-code").unwrap();
    assert_eq!(project_root(cc, None, true).unwrap(), None);
    assert_eq!(project_root(cc, Some("D:/repo"), true).unwrap(), None, "explicit global wins");
    assert!(project_root(cc, None, false).is_err(), "no choice, no repo: refused");
    assert_eq!(project_root(cc, Some("D:/repo"), false).unwrap(), Some("D:/repo"));
    let desktop = TOOL_SPECS.iter().find(|s| s.id == "claude-desktop").unwrap();
    assert_eq!(project_root(desktop, Some("D:/repo"), false).unwrap(), None, "no project config");
}

/// `cmd /C` used to carry the env values, and cmd.exe acted on `& | ^`
/// and expanded `%VAR%` in them. The probe is a `.cmd` - the shape of the
/// npm install - that writes the arguments it received.
#[cfg(windows)]
#[test]
fn cmd_metacharacters_in_an_env_value_reach_the_cli_literally() {
    let dir = TempDir::new();
    let probe = dir.path().join("probe.cmd");
    let out = dir.path().join("args.txt");
    std::fs::write(&probe, format!("@echo off\r\n>\"{}\" echo(%*\r\n", out.display())).unwrap();
    let mut env = std::collections::BTreeMap::new();
    env.insert(
        "CONNECTION_STRING".to_string(),
        "Server=db;Password=a&b|c^d<e>f(g)%PATH%&echo pwned>pwned.txt".to_string(),
    );
    let server = McpServer { name: "another-server".into(), command: "db.exe".into(), args: vec![], env };
    let status = mcp_add_command(&probe, &server, "user", Some(dir.path())).status().unwrap();
    assert!(status.success());
    let seen = std::fs::read_to_string(&out).unwrap();
    assert!(seen.contains("a&b|c^d<e>f(g)"), "{seen}");
    assert!(seen.contains("%PATH%"), "cmd expanded a variable: {seen}");
    assert!(!dir.path().join("pwned.txt").exists(), "an & in the value ran a second command");
}

#[test]
fn the_cli_is_run_directly_not_through_cmd() {
    let server = McpServer {
        name: "tcm".into(),
        command: "v2.exe".into(),
        args: vec!["--mcp".into()],
        env: Default::default(),
    };
    let cmd = mcp_add_command(std::path::Path::new("C:/x/claude.cmd"), &server, "user", None);
    assert_eq!(cmd.get_program(), "C:/x/claude.cmd");
    let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(args, mcp_add_args(&server, "user"));
}

// ------------------------------------------ the old database server's leftovers

/// The quiet cleanup the AI Bridge tab runs: the old database server's
/// entry leaves the repository's config, and nothing else in it moves - not
/// our own server, not somebody else's, not the file's other keys.
#[test]
fn removing_the_legacy_db_server_takes_only_its_entry() {
    let repo = TempDir::new();
    let cursor = repo.path().join(".cursor").join("mcp.json");
    std::fs::create_dir_all(cursor.parent().unwrap()).unwrap();
    std::fs::write(
        &cursor,
        serde_json::json!({
            "mcpServers": {
                "tcm": { "command": "v2.exe", "args": ["--mcp"] },
                "phr-db-mcp": { "command": "db.exe", "env": { "CONNECTION_STRING": "Server=db;Password=p" } },
                "somebody-elses": { "command": "other.exe" }
            },
            "theirSetting": true
        })
        .to_string(),
    )
    .unwrap();
    let vscode = repo.path().join(".vscode").join("mcp.json");
    std::fs::create_dir_all(vscode.parent().unwrap()).unwrap();
    std::fs::write(
        &vscode,
        serde_json::json!({
            "servers": {
                "phr-db-mcp": { "type": "stdio", "command": "db.exe" },
                "tcm": { "type": "stdio", "command": "v2.exe" }
            }
        })
        .to_string(),
    )
    .unwrap();
    let root = repo.path().to_string_lossy().to_string();

    remove_legacy_db_server_now("cursor", Some(root.as_str()), false).unwrap();
    remove_legacy_db_server_now("vscode", Some(root.as_str()), false).unwrap();

    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&cursor).unwrap()).unwrap();
    assert!(v["mcpServers"][LEGACY_DB_SERVER].is_null(), "{v}");
    assert_eq!(v["mcpServers"][TCM_SERVER]["command"], "v2.exe");
    assert_eq!(v["mcpServers"]["somebody-elses"]["command"], "other.exe");
    assert_eq!(v["theirSetting"], true);

    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&vscode).unwrap()).unwrap();
    assert!(v["servers"][LEGACY_DB_SERVER].is_null(), "{v}");
    assert_eq!(v["servers"][TCM_SERVER]["command"], "v2.exe");

    // Detection then has nothing left to clean, so the tab never asks again.
    let tools = detect_in("", "", &|_| false, Some(root.as_str()));
    for t in tools.iter().filter(|t| t.id == "cursor" || t.id == "vscode") {
        assert_eq!(t.registered_servers, vec![TCM_SERVER], "{t:?}");
    }
}

/// Already gone - or never there - is the state asked for, not a failure.
#[test]
fn removing_the_legacy_db_server_where_there_is_none_is_a_no_op() {
    let repo = TempDir::new();
    let root = repo.path().to_string_lossy().to_string();
    remove_legacy_db_server_now("cursor", Some(root.as_str()), false).unwrap();
    assert!(!repo.path().join(".cursor").exists(), "nothing is created to remove nothing");

    let cursor = repo.path().join(".cursor").join("mcp.json");
    std::fs::create_dir_all(cursor.parent().unwrap()).unwrap();
    let only_ours = r#"{"mcpServers":{"tcm":{"command":"v2.exe"}}}"#;
    std::fs::write(&cursor, only_ours).unwrap();
    remove_legacy_db_server_now("cursor", Some(root.as_str()), false).unwrap();
    assert_eq!(std::fs::read_to_string(&cursor).unwrap(), only_ours, "a file without it is left as it was");
}

#[test]
fn removing_the_legacy_db_server_refuses_an_unknown_tool() {
    assert!(remove_legacy_db_server_now("not-a-tool", None, false).is_err());
}

// ------------------------------------------------ run database changes unasked

#[test]
fn the_claude_code_rule_goes_in_and_out_and_touches_nothing_else() {
    use v2_lib::ai_tools::{set_claude_allow, CLAUDE_DB_QUERY_RULE as RULE};
    let theirs = r#"{"model":"opus","permissions":{"allow":["Bash(git status)"],"deny":["Read(.env)"]}}"#;
    let on = set_claude_allow(theirs, RULE, true).unwrap().expect("a change");
    let v: serde_json::Value = serde_json::from_str(&on).unwrap();
    assert_eq!(v["model"], "opus");
    assert_eq!(v["permissions"]["allow"], serde_json::json!(["Bash(git status)", RULE]));
    assert_eq!(v["permissions"]["deny"], serde_json::json!(["Read(.env)"]));
    // Already there: nothing to write.
    assert_eq!(set_claude_allow(&on, RULE, true).unwrap(), None);

    let off = set_claude_allow(&on, RULE, false).unwrap().expect("a change");
    let v: serde_json::Value = serde_json::from_str(&off).unwrap();
    assert_eq!(v["permissions"]["allow"], serde_json::json!(["Bash(git status)"]));
    assert_eq!(set_claude_allow(&off, RULE, false).unwrap(), None);
}

#[test]
fn switching_on_and_off_leaves_a_fresh_claude_file_empty_and_refuses_one_it_cannot_read() {
    use v2_lib::ai_tools::{set_claude_allow, CLAUDE_DB_QUERY_RULE as RULE};
    let on = set_claude_allow("", RULE, true).unwrap().unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&on).unwrap(), serde_json::json!({"permissions":{"allow":[RULE]}}));
    let off = set_claude_allow(&on, RULE, false).unwrap().unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&off).unwrap(), serde_json::json!({}));
    // The person's own file, unreadable: refused, never replaced.
    assert!(set_claude_allow("{ not json", RULE, true).is_err());
    assert!(set_claude_allow("[1,2]", RULE, true).is_err());
    assert!(set_claude_allow(r#"{"permissions":{"allow":"x"}}"#, RULE, true).is_err());
}

#[test]
fn the_cursor_entry_goes_in_and_out_of_its_allowlist() {
    use v2_lib::ai_tools::{set_cursor_allow, CURSOR_DB_QUERY_ENTRY as ENTRY};
    assert_eq!(ENTRY, "tcm:db_query");
    let theirs = r#"{"mcpAllowlist":["github:list_issues"],"terminalAllowlist":["git status"]}"#;
    let on = set_cursor_allow(theirs, ENTRY, true).unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_str(&on).unwrap();
    assert_eq!(v["mcpAllowlist"], serde_json::json!(["github:list_issues", ENTRY]));
    assert_eq!(v["terminalAllowlist"], serde_json::json!(["git status"]));
    // Cursor matches case-insensitively, so a differently-cased copy counts.
    assert_eq!(set_cursor_allow(r#"{"mcpAllowlist":["TCM:DB_Query"]}"#, ENTRY, true).unwrap(), None);
    let off = set_cursor_allow(&on, ENTRY, false).unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_str(&off).unwrap();
    assert_eq!(v["mcpAllowlist"], serde_json::json!(["github:list_issues"]));
    let emptied = set_cursor_allow(r#"{"mcpAllowlist":["tcm:db_query"]}"#, ENTRY, false).unwrap().unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&emptied).unwrap(), serde_json::json!({}));
}

/// A live report preview runs the report's stored procedure, so "Run
/// database changes without asking" lets it run unasked with db_query -
/// both entries go in together and come out together, in each tool's file.
#[test]
fn auto_approve_covers_the_live_preview_too() {
    use v2_lib::ai_tools::{merge_all, set_claude_allow, set_cursor_allow, CLAUDE_DB_RULES, CURSOR_DB_ENTRIES};
    assert_eq!(CLAUDE_DB_RULES, ["mcp__tcm__db_query", "mcp__tcm__preview_report_live"]);
    assert_eq!(CURSOR_DB_ENTRIES, ["tcm:db_query", "tcm:preview_report_live"]);

    let allow = |text: &str| serde_json::from_str::<serde_json::Value>(text).unwrap()["permissions"]["allow"].clone();
    let theirs = r#"{"permissions":{"allow":["Bash(git status)"]}}"#;
    let on = merge_all(theirs, CLAUDE_DB_RULES, true, set_claude_allow).unwrap().expect("a change");
    assert_eq!(allow(&on), serde_json::json!(["Bash(git status)", "mcp__tcm__db_query", "mcp__tcm__preview_report_live"]));
    assert_eq!(merge_all(&on, CLAUDE_DB_RULES, true, set_claude_allow).unwrap(), None, "already on");
    // A file switched on by an earlier release, with db_query alone, gains the other.
    let older = r#"{"permissions":{"allow":["mcp__tcm__db_query"]}}"#;
    let caught_up = merge_all(older, CLAUDE_DB_RULES, true, set_claude_allow).unwrap().unwrap();
    assert_eq!(allow(&caught_up), serde_json::json!(["mcp__tcm__db_query", "mcp__tcm__preview_report_live"]));
    // Off takes both out, and only them.
    let off = merge_all(&on, CLAUDE_DB_RULES, false, set_claude_allow).unwrap().unwrap();
    assert_eq!(allow(&off), serde_json::json!(["Bash(git status)"]));
    let one_left = r#"{"permissions":{"allow":["mcp__tcm__preview_report_live","Read(*)"]}}"#;
    let off = merge_all(one_left, CLAUDE_DB_RULES, false, set_claude_allow).unwrap().unwrap();
    assert_eq!(allow(&off), serde_json::json!(["Read(*)"]));
    assert_eq!(merge_all(r#"{"permissions":{"allow":[]}}"#, CLAUDE_DB_RULES, false, set_claude_allow).unwrap(), None);

    let list = |text: &str| serde_json::from_str::<serde_json::Value>(text).unwrap()["mcpAllowlist"].clone();
    let on = merge_all(r#"{"mcpAllowlist":["github:list_issues"]}"#, CURSOR_DB_ENTRIES, true, set_cursor_allow)
        .unwrap()
        .unwrap();
    assert_eq!(list(&on), serde_json::json!(["github:list_issues", "tcm:db_query", "tcm:preview_report_live"]));
    let off = merge_all(&on, CURSOR_DB_ENTRIES, false, set_cursor_allow).unwrap().unwrap();
    assert_eq!(list(&off), serde_json::json!(["github:list_issues"]));
}

// ------------------------------------------- the server's old name, `tcm-testcases`

use v2_lib::ai_tools::{
    config_carries, legacy_command_path, migrate_claude_permissions, migrate_cursor_permissions,
    remove_legacy_command, superseded_by, LEGACY_TCM_SERVER, MANAGED_SERVERS,
};
use v2_lib::commands::ai_tools::{carry_over_rules_in, retire_superseded_claude};

#[test]
fn the_server_is_tcm_and_its_old_name_is_still_managed() {
    assert_eq!(TCM_SERVER, "tcm");
    assert_eq!(LEGACY_TCM_SERVER, "tcm-testcases");
    assert!(MANAGED_SERVERS.contains(&TCM_SERVER));
    assert!(MANAGED_SERVERS.contains(&LEGACY_TCM_SERVER), "detection must still report the old entry");
    assert!(MANAGED_SERVERS.contains(&LEGACY_DB_SERVER));
    assert_eq!(superseded_by(TCM_SERVER), &[LEGACY_TCM_SERVER]);
    assert!(superseded_by("another-server").is_empty());
}

/// Registering writes `tcm` and takes `tcm-testcases` out of the same
/// config, so nobody ends up with both - and nothing else in it moves.
#[test]
fn registering_replaces_the_old_name_in_the_same_config() {
    for key in ["mcpServers", "servers"] {
        let existing = serde_json::json!({
            "theirSetting": 1,
            key: {
                "somebody-elses": { "command": "other.exe" },
                "tcm-testcases": { "command": "old/v2.exe", "args": ["--mcp"] },
                "phr-db-mcp": { "command": "db.exe" }
            }
        })
        .to_string();
        let out = merge_entry(&existing, key, &tcm_server("C:/app/v2.exe")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v[key][TCM_SERVER]["command"], "C:/app/v2.exe", "{key}: {v}");
        assert!(v[key].get(LEGACY_TCM_SERVER).is_none(), "{key}: {v}");
        assert_eq!(v[key]["somebody-elses"]["command"], "other.exe");
        assert_eq!(v[key][LEGACY_DB_SERVER]["command"], "db.exe", "not this function's to remove");
        assert_eq!(v["theirSetting"], 1);
        assert_eq!(v[key].as_object().unwrap().len(), 3, "{key}: {v}");
    }
}

/// Only our own server supersedes anything: registering somebody else's
/// leaves an old `tcm-testcases` entry where it is.
#[test]
fn another_servers_registration_leaves_the_old_name_alone() {
    let existing = r#"{"mcpServers":{"tcm-testcases":{"command":"old/v2.exe"}}}"#;
    let out = merge_entry(existing, "mcpServers", &env_server()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["mcpServers"][LEGACY_TCM_SERVER]["command"], "old/v2.exe");
}

/// A config holding only the old name is reported with exactly that name,
/// not as ours - the AI Bridge tab reads it as needing an update, and its
/// Register button is what updates it.
#[test]
fn a_config_with_only_the_old_name_reads_as_needing_an_update() {
    let repo = TempDir::new();
    std::fs::write(
        repo.path().join(".mcp.json"),
        r#"{"mcpServers": {"tcm-testcases": {"command": "old/v2.exe"}, "somebody-elses": {"command": "x"}}}"#,
    )
    .unwrap();
    let root = repo.path().to_string_lossy().to_string();
    let tools = detect_in("", "", &|_| false, Some(root.as_str()));
    let cc = tools.iter().find(|t| t.id == "claude-code").unwrap();
    assert_eq!(cc.registered_servers, vec![LEGACY_TCM_SERVER], "{cc:?}");
    assert!(!cc.registered_servers.iter().any(|s| s == TCM_SERVER), "{cc:?}");
    assert!(config_carries(&repo.path().join(".mcp.json"), "mcpServers", LEGACY_TCM_SERVER));
    assert!(!config_carries(&repo.path().join(".mcp.json"), "mcpServers", TCM_SERVER));
    assert!(!config_carries(&repo.path().join("missing.json"), "mcpServers", LEGACY_TCM_SERVER));
}

/// The single top-level command an earlier version wrote was named after
/// the server's OLD name - the cleanup must keep looking for that file.
#[test]
fn the_old_top_level_command_is_still_cleaned_up() {
    let path = legacy_command_path("C:/Users/Sam").display().to_string().replace(std::path::MAIN_SEPARATOR, "/");
    assert_eq!(path, "C:/Users/Sam/.claude/commands/tcm-testcases.md");

    let home = TempDir::new();
    let home_str = home.path().to_string_lossy().to_string();
    let file = legacy_command_path(&home_str);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, format!("---\nname: x\n---\n\n{COMMAND_MARKER}\n")).unwrap();
    remove_legacy_command(&home_str);
    assert!(!file.exists(), "ours is removed");

    // Somebody's own file of that name is not ours.
    std::fs::write(&file, "my own notes").unwrap();
    remove_legacy_command(&home_str);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "my own notes");
    // Nothing there: nothing to do, and no error.
    std::fs::remove_file(&file).unwrap();
    remove_legacy_command(&home_str);
}

// --- Claude Code: the CLI path takes the old name out of the same scope

/// A `.cmd` that appends the arguments and directory it ran in to `log`,
/// then exits with `code`.
#[cfg(windows)]
fn cli_probe(dir: &std::path::Path, log: &std::path::Path, code: u8) -> std::path::PathBuf {
    let probe = dir.join(format!("claude-probe-{code}.cmd"));
    std::fs::write(
        &probe,
        format!("@echo off\r\n>>\"{}\" echo(%*^|%CD%\r\nexit /b {code}\r\n", log.display()),
    )
    .unwrap();
    probe
}

#[cfg(windows)]
#[test]
fn the_cli_removes_the_old_name_in_the_scope_just_registered() {
    let bin = TempDir::new();
    let repo = TempDir::new();
    let log = bin.path().join("calls.txt");
    let probe = cli_probe(bin.path(), &log, 0);
    let config = repo.path().join(".mcp.json");
    std::fs::write(&config, r#"{"mcpServers":{"tcm":{"command":"v2.exe"},"tcm-testcases":{"command":"old.exe"}}}"#)
        .unwrap();

    retire_superseded_claude(Some(&probe), TCM_SERVER, "project", Some(repo.path()), &config).unwrap();

    let calls = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = calls.lines().collect();
    assert_eq!(lines.len(), 1, "{calls}");
    let (args, cwd) = lines[0].split_once('|').unwrap();
    assert_eq!(args.trim(), "mcp remove --scope project tcm-testcases");
    assert_eq!(
        std::path::Path::new(cwd.trim()).canonicalize().unwrap(),
        repo.path().canonicalize().unwrap(),
        "project scope is keyed on the directory the CLI runs in"
    );
}

/// No CLI, or a CLI that fails: the old name comes out of the scope's own
/// config file, and nothing else in it moves.
#[cfg(windows)]
#[test]
fn without_a_working_cli_the_old_name_leaves_the_config_file() {
    let bin = TempDir::new();
    let log = bin.path().join("calls.txt");
    let failing = cli_probe(bin.path(), &log, 1);
    for cli in [None, Some(failing.as_path())] {
        let repo = TempDir::new();
        let config = repo.path().join(".mcp.json");
        std::fs::write(
            &config,
            r#"{"other":true,"mcpServers":{"tcm":{"command":"v2.exe"},"tcm-testcases":{"command":"old.exe"},"theirs":{"command":"t.exe"}}}"#,
        )
        .unwrap();
        retire_superseded_claude(cli, TCM_SERVER, "project", Some(repo.path()), &config).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert!(v["mcpServers"].get(LEGACY_TCM_SERVER).is_none(), "{cli:?}: {v}");
        assert_eq!(v["mcpServers"][TCM_SERVER]["command"], "v2.exe");
        assert_eq!(v["mcpServers"]["theirs"]["command"], "t.exe");
        assert_eq!(v["other"], true);
    }
}

/// A scope that never carried the old name runs nothing at all.
#[cfg(windows)]
#[test]
fn a_scope_without_the_old_name_runs_nothing() {
    let bin = TempDir::new();
    let repo = TempDir::new();
    let log = bin.path().join("calls.txt");
    let probe = cli_probe(bin.path(), &log, 0);
    let config = repo.path().join(".mcp.json");
    let only_ours = r#"{"mcpServers":{"tcm":{"command":"v2.exe"}}}"#;
    std::fs::write(&config, only_ours).unwrap();
    retire_superseded_claude(Some(&probe), TCM_SERVER, "project", Some(repo.path()), &config).unwrap();
    retire_superseded_claude(Some(&probe), TCM_SERVER, "user", None, &repo.path().join("absent.json")).unwrap();
    assert!(!log.exists(), "the CLI was run for nothing");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), only_ours);
}

// --- permission rules

/// The database switch's rule under the old name is the same setting: it
/// reads as on, and is rewritten to the new name where it stood.
#[test]
fn the_old_database_rule_counts_as_on_and_is_rewritten() {
    use v2_lib::ai_tools::{set_claude_allow, CLAUDE_DB_QUERY_RULE as RULE};
    assert_eq!(RULE, "mcp__tcm__db_query");
    let old = r#"{"model":"opus","permissions":{"allow":["Bash(git status)","mcp__tcm-testcases__db_query","Read(*)"],"deny":["mcp__tcm-testcases__get_tags"]}}"#;

    let on = set_claude_allow(old, RULE, true).unwrap().expect("rewritten");
    let v: serde_json::Value = serde_json::from_str(&on).unwrap();
    assert_eq!(v["permissions"]["allow"], serde_json::json!(["Bash(git status)", RULE, "Read(*)"]));
    assert_eq!(v["permissions"]["deny"], serde_json::json!(["mcp__tcm-testcases__get_tags"]), "only the switch's own rule");
    assert_eq!(v["model"], "opus");
    assert_eq!(set_claude_allow(&on, RULE, true).unwrap(), None, "already on");

    // Both names present: one rule, in the new name, where the new one stood.
    let both = r#"{"permissions":{"allow":["mcp__tcm__db_query","mcp__tcm-testcases__db_query"]}}"#;
    let v: serde_json::Value = serde_json::from_str(&set_claude_allow(both, RULE, true).unwrap().unwrap()).unwrap();
    assert_eq!(v["permissions"]["allow"], serde_json::json!([RULE]));

    // Off takes the old one out too.
    let off = set_claude_allow(r#"{"permissions":{"allow":["mcp__tcm-testcases__db_query"]}}"#, RULE, false)
        .unwrap()
        .unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&off).unwrap(), serde_json::json!({}));
}

#[test]
fn the_old_cursor_entry_counts_as_on_and_is_rewritten() {
    use v2_lib::ai_tools::{set_cursor_allow, CURSOR_DB_QUERY_ENTRY as ENTRY};
    let old = r#"{"mcpAllowlist":["github:list_issues","TCM-TestCases:DB_Query"],"x":1}"#;
    let on = set_cursor_allow(old, ENTRY, true).unwrap().expect("rewritten");
    let v: serde_json::Value = serde_json::from_str(&on).unwrap();
    assert_eq!(v["mcpAllowlist"], serde_json::json!(["github:list_issues", ENTRY]));
    assert_eq!(v["x"], 1);
    assert_eq!(set_cursor_allow(&on, ENTRY, true).unwrap(), None);
    let off = set_cursor_allow(r#"{"mcpAllowlist":["tcm-testcases:db_query"]}"#, ENTRY, false).unwrap().unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&off).unwrap(), serde_json::json!({}));
}

/// Every "always allow" (and deny / ask) for the old name carries over to
/// the new one: same place in the list, never twice, and nothing else in
/// the file moves.
#[test]
fn the_old_names_permission_rules_carry_over() {
    let before = r#"{
  "model": "opus",
  "permissions": {
    "allow": [
      "Bash(git status)",
      "mcp__tcm-testcases__get_test_cases",
      "mcp__tcm__validate_cases",
      "mcp__tcm-testcases__validate_cases",
      "mcp__tcm-testcases-other__x",
      "mcp__tcm-testcases",
      "mcp__github__list_issues"
    ],
    "deny": ["mcp__tcm-testcases__db_query", "Read(.env)"],
    "ask": ["mcp__tcm-testcases__run_api_template"],
    "defaultMode": "acceptEdits"
  },
  "env": { "A": "1" }
}"#;
    let after = migrate_claude_permissions(before).unwrap().expect("a change");
    let v: serde_json::Value = serde_json::from_str(&after).unwrap();
    assert_eq!(
        v["permissions"]["allow"],
        serde_json::json!([
            "Bash(git status)",
            "mcp__tcm__get_test_cases",
            "mcp__tcm__validate_cases",
            "mcp__tcm-testcases-other__x",
            "mcp__tcm",
            "mcp__github__list_issues"
        ])
    );
    assert_eq!(v["permissions"]["deny"], serde_json::json!(["mcp__tcm__db_query", "Read(.env)"]));
    assert_eq!(v["permissions"]["ask"], serde_json::json!(["mcp__tcm__run_api_template"]));
    assert_eq!(v["permissions"]["defaultMode"], "acceptEdits");
    assert_eq!(v["model"], "opus");
    assert_eq!(v["env"], serde_json::json!({ "A": "1" }));
    // Key order is the file's own.
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["model", "permissions", "env"]);

    // Idempotent: a second run has nothing to do.
    assert_eq!(migrate_claude_permissions(&after).unwrap(), None);
    // Nothing of ours, or nothing at all: nothing to write.
    assert_eq!(migrate_claude_permissions(r#"{"permissions":{"allow":["Bash(ls)"]}}"#).unwrap(), None);
    assert_eq!(migrate_claude_permissions("").unwrap(), None);
    assert_eq!(migrate_claude_permissions(r#"{"model":"opus"}"#).unwrap(), None);
    // The person's own file, unreadable: refused, never replaced.
    assert!(migrate_claude_permissions("{ not json").is_err());
    assert!(migrate_claude_permissions("[1]").is_err());
}

/// The file step: a missing settings file stays missing, a file with
/// nothing of ours is not rewritten, and one with old rules is.
#[test]
fn carrying_over_rewrites_only_a_file_that_needs_it() {
    let dir = TempDir::new();
    let missing = dir.path().join(".claude").join("settings.local.json");
    carry_over_rules_in(&missing, migrate_claude_permissions).unwrap();
    assert!(!missing.exists());
    assert!(!missing.parent().unwrap().exists(), "nothing is created to rename nothing");

    let untouched = dir.path().join("theirs.json");
    let text = "{\"permissions\":{\"allow\":[\"Bash(ls)\"]}}";
    std::fs::write(&untouched, text).unwrap();
    carry_over_rules_in(&untouched, migrate_claude_permissions).unwrap();
    assert_eq!(std::fs::read_to_string(&untouched).unwrap(), text, "byte for byte");

    let old = dir.path().join("settings.json");
    std::fs::write(&old, r#"{"permissions":{"allow":["mcp__tcm-testcases__get_tags"]}}"#).unwrap();
    carry_over_rules_in(&old, migrate_claude_permissions).unwrap();
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&old).unwrap()).unwrap();
    assert_eq!(v["permissions"]["allow"], serde_json::json!(["mcp__tcm__get_tags"]));

    // Cursor's file, the same way.
    let cursor = dir.path().join("permissions.json");
    std::fs::write(&cursor, r#"{"mcpAllowlist":["tcm-testcases:get_tags"]}"#).unwrap();
    carry_over_rules_in(&cursor, migrate_cursor_permissions).unwrap();
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&cursor).unwrap()).unwrap();
    assert_eq!(v["mcpAllowlist"], serde_json::json!(["tcm:get_tags"]));
}

/// Every Cursor allowlist entry for the old name carries over: same place,
/// never twice (Cursor compares case-insensitively, so neither does this),
/// nothing else touched, and a second run has nothing to do.
#[test]
fn the_old_names_cursor_allowlist_carries_over() {
    let before = r#"{
  "terminalAllowlist": ["git status"],
  "mcpAllowlist": [
    "github:list_issues",
    "tcm-testcases:get_test_cases",
    "TCM:Validate_Cases",
    "TCM-TestCases:validate_cases",
    "tcm-testcases-other:x",
    "tcm-testcases:*"
  ],
  "theirs": { "a": 1 }
}"#;
    let after = migrate_cursor_permissions(before).unwrap().expect("a change");
    let v: serde_json::Value = serde_json::from_str(&after).unwrap();
    assert_eq!(
        v["mcpAllowlist"],
        serde_json::json!([
            "github:list_issues",
            "tcm:get_test_cases",
            "TCM:Validate_Cases",
            "tcm-testcases-other:x",
            "tcm:*"
        ])
    );
    assert_eq!(v["terminalAllowlist"], serde_json::json!(["git status"]));
    assert_eq!(v["theirs"], serde_json::json!({ "a": 1 }));
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["terminalAllowlist", "mcpAllowlist", "theirs"]);

    assert_eq!(migrate_cursor_permissions(&after).unwrap(), None, "idempotent");
    assert_eq!(migrate_cursor_permissions(r#"{"mcpAllowlist":["github:x"]}"#).unwrap(), None);
    assert_eq!(migrate_cursor_permissions("").unwrap(), None);
    assert_eq!(migrate_cursor_permissions(r#"{"mcpAllowlist":"odd"}"#).unwrap(), None);
    assert!(migrate_cursor_permissions("{ not json").is_err());
    assert!(migrate_cursor_permissions("[1]").is_err());
}

/// A config holding BOTH names (say a teammate on an older version added
/// the old one back) reports both, so the tab can flag the duplicate.
#[test]
fn a_config_with_both_names_reports_both() {
    let repo = TempDir::new();
    std::fs::write(
        repo.path().join(".mcp.json"),
        r#"{"mcpServers": {"tcm": {"command": "v2.exe"}, "tcm-testcases": {"command": "old.exe"}}}"#,
    )
    .unwrap();
    let root = repo.path().to_string_lossy().to_string();
    let tools = detect_in("", "", &|_| false, Some(root.as_str()));
    let cc = tools.iter().find(|t| t.id == "claude-code").unwrap();
    assert_eq!(cc.registered_servers, vec![TCM_SERVER, LEGACY_TCM_SERVER], "{cc:?}");
}

/// `/tcm:discover` walks an assistant through writing a script from what it
/// saw on the live app. An Auto Test command, so it is written only where
/// the Auto Test tools are offered; it names its steps in order, and every
/// tool it names in backticks is one the bridge really lists.
#[test]
fn the_discover_command_writes_a_script_from_the_live_app() {
    let c = COMMANDS.iter().find(|c| c.stem == "discover").expect("a discover command");
    assert_eq!(c.tool, "start_autorun_discovery");
    assert!(!c.desc.is_empty());
    assert!(!c.hint.is_empty());

    let dir = std::path::Path::new("D:/repo/.claude/commands/tcm");
    let hidden = command_files_in(dir, &effective_disabled_for(&[], &mods(&[Module::DbAccess])));
    assert!(
        hidden.iter().all(|(p, _)| p.file_name().unwrap() != "discover.md"),
        "discover must not be written while Auto Test is off"
    );
    let offered = command_files_in(dir, &effective_disabled_for(&[], &all_mods()));
    assert!(offered.iter().any(|(p, _)| p.file_name().unwrap() == "discover.md"));
    let off = command_files_in(dir, &["start_autorun_discovery".to_string()]);
    assert!(off.iter().all(|(p, _)| p.file_name().unwrap() != "discover.md"), "switched off, no command");

    let body = c.body.join("\n");
    assert!(body.contains("$ARGUMENTS"), "{body}");
    let mcp = include_str!("../../src/mcp.rs");
    let is_tool = |name: &str| mcp.contains(&format!("\"name\": \"{name}\""));
    // The order of the work: each named tool appears after the one before.
    let order = [
        "get_test_cases",
        "start_autorun_discovery",
        "save_autorun_area",
        "discover_autorun_actions",
        "save_autorun_script",
        // Optional, after the save: never part of writing the script.
        "replay_autorun_to_step",
        "end_autorun_discovery",
    ];
    let mut from = 0;
    for tool in order {
        let at = body[from..]
            .find(&format!("`{tool}`"))
            .unwrap_or_else(|| panic!("/tcm:discover never names `{tool}` after the step before it: {body}"));
        from += at;
        assert!(is_tool(tool), "`{tool}` is not in the bridge's tool list");
    }
    // Field names (`area`, `edits`) are one word; a tool name has an underscore.
    let named: Vec<&str> = body.split('`').skip(1).step_by(2).filter(|n| n.contains('_')).collect();
    assert!(named.len() >= 9, "{named:?}");
    for name in named {
        assert!(is_tool(name), "/tcm:discover names `{name}`, which mcp.rs does not expose");
    }
    let save_at = body.find("`save_autorun_script`").unwrap();
    assert!(
        !body[..save_at].contains("replay_autorun_to_step"),
        "a script is carried out live while it is written, not replayed before it is saved: {body}"
    );
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    for said in [
        "carrying each step's actions out live with `discover_autorun_actions` as you write them",
        "Only once every step has been carried out, save it",
        "Any fix after the save is a repair, declared in `edits`.",
        "ask the person to record the area in Auto Test",
        "test name prefix",
        "delete only records carrying that prefix",
        "never read the application's code to write scripts",
        "Choosing a model for the work",
        "send them as `actions`: up to 20 in one call",
    ] {
        assert!(flat.contains(said), "/tcm:discover never says {said:?}: {flat}");
    }
    assert!(flat.contains("`area`"), "the script's area must be named: {flat}");
    assert!(!body.contains('\u{2014}') && !body.contains('\u{2013}'), "no em or en dashes");
    for said in [
        "never fit the check to the page",
        "call `report_case_divergence` and keep the case's value",
        "assumed (the case defines it indirectly",
        "invented (you chose it where the case leaves it free",
        "Repeat each assumed and invented value the save lists in your report before you say the work is done",
    ] {
        assert!(flat.contains(said), "/tcm:discover never says {said:?}: {flat}");
    }
}

/// `/tcm:map-menus` walks the named modules' menus and saves each screen as
/// an area. It rides the same gate as `/tcm:discover`, names its steps in
/// order, and promises never to fill in or submit anything.
#[test]
fn the_map_menus_command_walks_the_menus_and_never_submits() {
    let c = COMMANDS.iter().find(|c| c.stem == "map-menus").expect("a map-menus command");
    assert_eq!(c.tool, "start_autorun_discovery");
    assert!(!c.desc.is_empty() && !c.hint.is_empty());
    let dir = std::path::Path::new("D:/repo/.claude/commands/tcm");
    let hidden = command_files_in(dir, &effective_disabled_for(&[], &mods(&[Module::DbAccess])));
    assert!(hidden.iter().all(|(p, _)| p.file_name().unwrap() != "map-menus.md"));
    let offered = command_files_in(dir, &effective_disabled_for(&[], &all_mods()));
    assert!(offered.iter().any(|(p, _)| p.file_name().unwrap() == "map-menus.md"));
    let off = command_files_in(dir, &["start_autorun_discovery".to_string()]);
    assert!(off.iter().all(|(p, _)| p.file_name().unwrap() != "map-menus.md"));

    let body = c.body.join("\n");
    assert!(body.contains("$ARGUMENTS"), "{body}");
    let mcp = include_str!("../../src/mcp.rs");
    let is_tool = |name: &str| mcp.contains(&format!("\"name\": \"{name}\""));
    let order = ["start_autorun_discovery", "discover_autorun_actions", "save_autorun_area", "end_autorun_discovery"];
    let mut from = 0;
    for tool in order {
        let at = body[from..]
            .find(&format!("`{tool}`"))
            .unwrap_or_else(|| panic!("/tcm:map-menus never names `{tool}` in order: {body}"));
        from += at;
        assert!(is_tool(tool), "`{tool}` is not in the bridge's tool list");
    }
    for name in body.split('`').skip(1).step_by(2).filter(|n| n.contains('_')) {
        assert!(is_tool(name), "/tcm:map-menus names `{name}`, which mcp.rs does not expose");
    }
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    for said in [
        "`mapping` set to true",
        "`modules`",
        "the save answers `unchanged`",
        "When the clicks did not arrive, skip that screen and carry on",
        "pick a more specific name and save again",
        "Only these stop the run",
        "the areas list as it stood when you read the guide",
        "the admin side first and then self-service",
        "\"Module / Menu path\"",
        "Never fill in or submit anything",
        "the session expires",
        "150 saved screens",
        "Last menu mapping",
    ] {
        assert!(flat.contains(said), "/tcm:map-menus never says {said:?}: {flat}");
    }
    assert!(!body.contains('\u{2014}') && !body.contains('\u{2013}'), "no em or en dashes");
}

/// `/tcm:gen-tests` scripts a batch of a PBI's cases within the bounds, reading the writer's
/// rules first, and never fits a check to the page.
#[test]
fn the_gen_tests_command_scripts_a_batch_within_the_bounds() {
    let c = COMMANDS.iter().find(|c| c.stem == "gen-tests").expect("a gen-tests command");
    assert_eq!(c.tool, "start_autorun_discovery");
    assert_eq!(c.desc, "Script a batch of a PBI's test cases from the live app, within the bounds");
    assert_eq!(c.hint, "[PBI id] [how many] [area or title prefix] [case ids]");
    assert!(command_markdown(c).contains(COMMAND_MARKER));
    assert!(command_markdown(c).contains("argument-hint: \"[PBI id] [how many] [area or title prefix] [case ids]\""));

    let dir = std::path::Path::new("D:/repo/.claude/commands/tcm");
    let hidden = command_files_in(dir, &effective_disabled_for(&[], &mods(&[Module::DbAccess])));
    assert!(hidden.iter().all(|(p, _)| p.file_name().unwrap() != "gen-tests.md"));
    let offered = command_files_in(dir, &effective_disabled_for(&[], &all_mods()));
    assert!(offered.iter().any(|(p, _)| p.file_name().unwrap() == "gen-tests.md"));
    let off = command_files_in(dir, &["start_autorun_discovery".to_string()]);
    assert!(off.iter().all(|(p, _)| p.file_name().unwrap() != "gen-tests.md"));

    let body = c.body.join("\n");
    let mcp = include_str!("../../src/mcp.rs");
    let is_tool = |name: &str| mcp.contains(&format!("\"name\": \"{name}\""));
    for name in body.split('`').skip(1).step_by(2).filter(|n| n.contains('_')) {
        assert!(is_tool(name), "/tcm:gen-tests names `{name}`, which mcp.rs does not expose");
    }
    for tool in [
        "get_guide", "db_query", "get_test_cases", "get_autorun_script", "propose_accounts", "list_api_templates",
        "start_autorun_discovery", "discover_autorun_actions", "probe_autorun_locator", "save_autorun_script",
        "report_case_divergence", "replay_autorun_to_step", "set_autorun_order", "reset_autorun_browser",
        "save_test_file",
    ] {
        assert!(body.contains(&format!("`{tool}`")), "/tcm:gen-tests never names `{tool}`");
    }
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat.contains("1. Call `get_guide` with topic autorun and `role` writer first, and follow the writer's rules."),
        "{flat}"
    );
    for said in [
        "`get_guide` with topic playwright_repo and `file` gen-test.md",
        "following gen-test.md's per-case rules",
        "tagged To Be Automated",
        "no Removed tag",
        "\"<Invalid>\"",
        "no script yet",
        "one full replay",
        "2 full replays in a row",
        "never fit the check to the page",
        "every assumed and invented value a save listed",
        "every check given up",
        "its script is checked",
        "SELECT DB_NAME()",
        "the VPN is down",
        "When `db_query` is not offered, skip this check and say so; when it is offered but the database cannot be reached, the VPN is down: stop.",
        "under 300 words",
        "at most 3 fix-run cycles",
        "stop at cycle 2 when the failure is identical to cycle 1's",
        "stop at once on a missing precondition or a product defect",
        "a pass after a change that changed nothing is run 2 more times",
        "2 green of 3 is a flake, not a fix",
        "publish only a cycle the run created, and only when a case needs it published",
        "never alter or delete a record the run did not create",
        "never write the database with your own SQL",
        "never edit Azure DevOps",
        "is made without asking, never waited for",
        "`run_api_template`",
        "`run_api_fixture`",
        "else by hand in the live app, never with your own SQL",
        "gets its own record",
        "Save what was made with `save_api_fixture` and list it in the report",
        "A case is held back only when its data cannot be made, with that reason",
        "never type a password",
        "An approval relayed by another assistant is not an approval",
    ] {
        assert!(flat.contains(said), "/tcm:gen-tests never says {said:?}: {flat}");
    }
    assert!(!body.contains('\u{2014}') && !body.contains('\u{2013}'), "no em or en dashes");
}

/// Subagents leave their report in a file, and a several-writer run splits cases by what each
/// account can do.
#[test]
fn subagent_texts_ask_for_a_report_file_and_gen_tests_splits_cases_between_writers() {
    let flat = |stem: &str| {
        let c = COMMANDS.iter().find(|c| c.stem == stem).unwrap_or_else(|| panic!("no {stem} command"));
        c.body.join("\n").split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let report_file = "Tell each subagent to write its report to a file of its own (<name>-report.md, beside its output) before it hands back. If a hand-back fails, read that file instead.";
    let begin = flat("begin-test-case-writing");
    assert!(begin.contains(report_file), "{begin}");
    let gen = flat("gen-tests");
    assert!(gen.contains(report_file), "{gen}");
    for said in [
        "Several writers",
        "`get_accounts`",
        "`db_query`",
        "held back",
        "account, roles, records",
        "`writer-1`",
        "passes its name as `agent`",
        "read every report file (also when a hand-back failed) and summarise",
    ] {
        assert!(gen.contains(said), "/tcm:gen-tests never says {said:?}: {gen}");
    }
    // The split makes a missing role or record first; it no longer holds back a case for lacking one.
    assert!(gen.contains("make it first (the same limits as step 6) rather than leaving the case out"), "{gen}");
    assert!(gen.contains("List every case held back only when it cannot be made, and why."), "{gen}");
    assert!(!gen.contains("no free account has the role it needs"), "{gen}");
    // Step 6 no longer holds a case back for missing setup data.
    assert!(!gen.contains("A case whose setup is missing is held back"), "{gen}");
    assert!(!gen.contains('\u{2014}') && !gen.contains('\u{2013}'), "no em or en dashes");
    // The writer role is at its size cap: the coordinator tells each writer
    // its report file instead (step d above).
    assert!(gen.contains("told its account, its cases and its report file"), "{gen}");
}

/// `RENAMED_COMMANDS` is empty since 2.3.0: the `script-pbi` stand-in for `gen-tests` is gone, so no
/// install is offered the old name any more.
#[test]
fn no_renamed_command_stand_in_is_written_any_more() {
    assert!(RENAMED_COMMANDS.is_empty(), "{RENAMED_COMMANDS:?}");
    let files = command_files_in(std::path::Path::new("D:/repo/.claude/commands/tcm"), &[]);
    assert!(files.iter().all(|(p, _)| p.file_name().unwrap() != "script-pbi.md"));
    assert!(files.iter().any(|(p, _)| p.file_name().unwrap() == "gen-tests.md"), "/tcm:gen-tests is the command");
}

/// The generator for a future rename still builds the stand-in file: the old name, the marker, the
/// renamed line, then the new command's body.
#[test]
fn a_renamed_command_stand_in_is_built_from_the_new_command() {
    let new = COMMANDS.iter().find(|c| c.stem == "gen-tests").unwrap();
    let old = renamed_command_markdown("old-name", new);
    assert!(old.contains(COMMAND_MARKER), "{old}");
    assert!(old.contains("name: old-name"), "{old}");
    assert!(old.contains("description: \"Renamed: now /tcm:gen-tests\""), "{old}");
    assert!(old.contains(&format!("argument-hint: \"{}\"", new.hint)), "{old}");
    assert!(old.contains(&renamed_line("old-name", "gen-tests")), "{old}");
    assert!(old.contains(&new.body.join("\n")), "the stand-in carries the new one's body: {old}");
    assert_eq!(
        renamed_line("old-name", "gen-tests"),
        "/tcm:old-name is now /tcm:gen-tests: tell the person it was renamed, once, then do what /tcm:gen-tests does."
    );
    assert!(!old.contains('\u{2014}') && !old.contains('\u{2013}'), "no em or en dashes");
}

/// An install that still has the 2.2.x stand-in loses it on the next sync, but only when it is ours: a
/// `script-pbi.md` without the marker belongs to somebody else and stays.
#[test]
fn the_old_script_pbi_stand_in_is_swept_on_the_next_sync_unless_it_is_not_ours() {
    use v2_lib::commands::ai_tools::write_commands_in;

    let new = COMMANDS.iter().find(|c| c.stem == "gen-tests").unwrap();
    let ours = TempDir::new();
    std::fs::write(ours.path().join("script-pbi.md"), renamed_command_markdown("script-pbi", new)).unwrap();
    write_commands_in(ours.path(), &[]).unwrap();
    assert!(!ours.path().join("script-pbi.md").exists(), "our stand-in goes on the next sync");
    assert!(ours.path().join("gen-tests.md").exists(), "/tcm:gen-tests is written");

    let theirs = TempDir::new();
    std::fs::write(theirs.path().join("script-pbi.md"), "my own command").unwrap();
    write_commands_in(theirs.path(), &[]).unwrap();
    assert_eq!(
        std::fs::read_to_string(theirs.path().join("script-pbi.md")).unwrap(),
        "my own command",
        "a script-pbi.md without the marker is left alone"
    );
}

/// `/tcm:explore` does a task with no test case, checks it, writes the summary, and undoes only on request.
#[test]
fn the_explore_command_does_a_task_and_ends_with_a_summary() {
    let c = COMMANDS.iter().find(|c| c.stem == "explore").expect("an explore command");
    assert_eq!(c.tool, "start_autorun_discovery");
    assert_eq!(c.desc, "Do a task in the live app with no test case, check it, and report what changed");
    assert_eq!(c.hint, "[the task, or undo <session> / remove <session>]");
    let body = c.body.join("\n");
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    for said in [
        "explore true",
        "`record_explore_change`",
        "wait for their yes in this chat",
        "`end_autorun_discovery`",
        "tcm-explore/<date>-<short-task>.md",
        "`undo_explore_creates`",
        "only because the person asked",
        "`send_api_request`",
        "`get_explore_changes`",
        "`list_setups`",
        "`run_setup`",
    ] {
        assert!(flat.contains(said), "/tcm:explore never says {said:?}: {flat}");
    }
    assert!(!body.contains('\u{2014}') && !body.contains('\u{2013}'), "no em or en dashes");
    let tools = v2_lib::mcp::all_tool_names();
    for name in body.split('`').skip(1).step_by(2).filter(|n| n.contains('_')) {
        assert!(tools.iter().any(|t| t == name), "/tcm:explore names `{name}`, which is no tool");
    }
}

/// `/tcm:heal` explores a stale area again before repairing in it, and
/// `/tcm:setup` names `/tcm:discover` as the way to find an area.
#[test]
fn heal_explores_a_stale_area_first_and_setup_names_discover() {
    let heal = COMMANDS.iter().find(|c| c.stem == "heal").unwrap().body.join(" ");
    let heal = heal.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(heal.contains("If the area's map is stale, run discovery on it before repairing."), "{heal}");
    // Discovery is refused while the person's browser is open, and a replay
    // is refused while discovery holds the browser: close, explore, end,
    // then replay - in that order.
    assert!(
        heal.contains("if the person's Auto Test browser is open and no other assistant is using it, ask them to close it first"),
        "{heal}"
    );
    let at = |what: &str| heal.find(what).unwrap_or_else(|| panic!("/tcm:heal never says {what:?}: {heal}"));
    let discovery = at("run discovery on it");
    let start = at("`start_autorun_discovery`");
    let end = at("`end_autorun_discovery`");
    let replay = at("`replay_autorun_to_step`");
    assert!(discovery < start && start < end && end < replay, "{heal}");
    let setup = COMMANDS.iter().find(|c| c.stem == "setup").unwrap().body.join(" ");
    assert!(setup.contains("/tcm:discover"), "{setup}");
}

/// Heal ends a discovery and then replays: every end it names keeps the
/// browser (an unkept end now closes it and frees the account, and a replay
/// after that has nothing to run in).
#[test]
fn every_end_in_heal_keeps_the_browser_for_the_replay() {
    let heal = COMMANDS.iter().find(|c| c.stem == "heal").unwrap().body.join(" ");
    let heal = heal.split_whitespace().collect::<Vec<_>>().join(" ");
    let ends = heal.matches("call `end_autorun_discovery`").count();
    let keeping = heal.matches("`end_autorun_discovery` with `keep_browser: true`").count();
    assert!(ends > 0 && ends == keeping, "{ends} ends, {keeping} keeping: {heal}");
}

/// Final review, finding 1: a discovery that names no area files
/// everything in the bucket for no area. `/tcm:discover` and `/tcm:heal`
/// both say to pass `area` to `start_autorun_discovery`, how to reach a
/// listed area (click its menu path with `discover_autorun_actions`), and to
/// pass `area` when moving to another one; discover also says to save an
/// unlisted screen with `save_autorun_area` as soon as it is found.
#[test]
fn discover_and_heal_say_to_pass_the_area_to_discovery() {
    for stem in ["discover", "heal"] {
        let c = COMMANDS.iter().find(|c| c.stem == stem).unwrap();
        let flat = c.body.join("\n").split_whitespace().collect::<Vec<_>>().join(" ");
        for said in ["`start_autorun_discovery`", "`area`", "menu path", "passing `area`"] {
            let said = if stem == "discover" && said == "passing `area`" { "pass `area`" } else { said };
            assert!(flat.contains(said), "/tcm:{stem} never says {said:?}: {flat}");
        }
        assert!(
            flat.contains("`area` on `discover_autorun_actions`"),
            "/tcm:{stem} never says to pass `area` when moving: {flat}"
        );
    }
    let discover = COMMANDS.iter().find(|c| c.stem == "discover").unwrap().body.join(" ");
    let flat = discover.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains("pass `area` with its name"), "{flat}");
    assert!(flat.contains("call `save_autorun_area` as soon as you are on it"), "{flat}");
}

/// Components in the two commands: `/tcm:discover` checks the guide's list
/// before working a widget out, and saves a new one only after trying it
/// live as a draft; `/tcm:heal` fixes a failure inside a component in the
/// component, not in the scripts that use it.
#[test]
fn discover_and_heal_say_how_to_use_components() {
    let flat = |stem: &str| {
        let c = COMMANDS.iter().find(|c| c.stem == stem).unwrap();
        c.body.join("\n").split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let discover = flat("discover");
    let at = |what: &str| discover.find(what).unwrap_or_else(|| panic!("/tcm:discover never says {what:?}: {discover}"));
    // The list is checked before a widget is worked out, a new component
    // is tried live as a draft, then saved, all before the script's save.
    let list = at("This project's components");
    let draft = at("`draft`");
    let save = at("`save_autorun_component`");
    let script = at("`save_autorun_script`");
    assert!(list < draft && draft < save && save < script, "{discover}");
    // An action kind, not a tool: unquoted, since every quoted name with an
    // underscore must be a tool.
    for said in ["a use_component action", "more than once", "unchanged"] {
        at(said);
    }

    let heal = flat("heal");
    for said in [
        "names a component",
        "fix it in the component",
        "`save_autorun_component`",
        "`why`",
    ] {
        assert!(heal.contains(said), "/tcm:heal never says {said:?}: {heal}");
    }
    // Scripts are repaired, and components changed, as often as needed,
    // and a repair may drop a check: no command names a cap on either.
    for body in [&discover, &heal] {
        for gone in ["repair cap", "3 changes", "weaken a check"] {
            assert!(!body.contains(gone), "a command still says {gone:?}: {body}");
        }
    }
    for body in [&discover, &heal] {
        assert!(!body.contains('\u{2014}') && !body.contains('\u{2013}'), "no em or en dashes");
    }
}

/// `/tcm:discover`, `/tcm:map-menus` and `/tcm:heal` read the quick guide
/// first, and the full guide when something it does not cover comes up.
#[test]
fn the_commands_point_at_the_quick_guide() {
    for stem in ["discover", "map-menus", "heal", "explore"] {
        let c = COMMANDS.iter().find(|c| c.stem == stem).unwrap();
        let flat = c.body.join("\n").split_whitespace().collect::<Vec<_>>().join(" ");
        for said in [
            "Call `get_guide` with topic autorun and `quick` true first and follow it.",
            "Call it again with `section` (or `full` true for the full guide) when something the quick rules do not cover comes up",
        ] {
            assert!(flat.contains(said), "/tcm:{stem} never says {said:?}: {flat}");
        }
        let quick = flat.find("`get_guide` with topic autorun and `quick` true").unwrap();
        let first_tool = ["get_test_cases", "start_autorun_discovery", "get_autorun_failures"]
            .iter()
            .filter_map(|t| flat.find(&format!("`{t}`")))
            .min()
            .unwrap();
        assert!(quick < first_tool, "/tcm:{stem} reads the quick guide before it starts: {flat}");
        assert!(!flat.contains("it holds every rule"), "/tcm:{stem} still sends the full guide first: {flat}");
        assert!(!flat.contains('\u{2014}') && !flat.contains('\u{2013}'), "no em or en dashes");
    }
}

/// Setup and discovery both prompt for the marks and the run order.
#[test]
fn setup_and_discover_prompt_for_marks_and_the_run_order() {
    for stem in ["setup", "discover"] {
        let body = COMMANDS.iter().find(|c| c.stem == stem).unwrap().body.join("\n");
        for word in ["changes", "needs_unchanged", "`set_autorun_order`", "reset points"] {
            assert!(body.contains(word), "/tcm:{stem} never mentions {word}");
        }
        assert!(!body.contains('\u{2014}') && !body.contains('\u{2013}'), "/tcm:{stem} has a dash");
    }
}

// ------------------------------------------------------------ role agent files

use v2_lib::ai_tools::{agent_dir, agent_files_in, project_agent_dir, AGENT_PREFIX};
use v2_lib::autorun::roles::Role;
use v2_lib::commands::ai_tools::{remove_agents_in, write_agents_in};

#[test]
fn the_agent_folders_sit_beside_the_command_folders() {
    assert_eq!(agent_dir("H"), std::path::PathBuf::from("H").join(".claude").join("agents"));
    assert_eq!(project_agent_dir("R"), std::path::PathBuf::from("R").join(".claude").join("agents"));
    assert_eq!(AGENT_PREFIX, "tcm-");
}

#[test]
fn each_role_file_is_generated_with_the_marker() {
    let dir = std::path::Path::new("D:/repo/.claude/agents");
    let files = agent_files_in(dir, &[], "");
    let names: Vec<String> = files
        .iter()
        .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["tcm-writer.md", "tcm-healer.md", "tcm-case-rewriter.md", "tcm-auditor.md", "tcm-explorer.md"]);
    for (role, (_, content)) in Role::ALL.iter().zip(files.iter()) {
        let key = role.key();
        assert!(content.starts_with(&format!("---\nname: tcm-{key}\ndescription: ")), "{key}");
        assert!(content.contains(COMMAND_MARKER), "{key} has no marker");
        assert!(content.contains(role.text().trim_end()), "{key} lacks its text");
        assert!(
            content.ends_with(&format!("These rules also come from `get_guide` with topic autorun and role {key}.\n")),
            "{key} ends wrong"
        );
    }
}

#[test]
fn no_role_file_without_auto_run() {
    let dir = std::path::Path::new("D:/repo/.claude/agents");
    assert!(agent_files_in(dir, &["start_autorun_discovery".to_string()], "").is_empty());
    assert!(agent_files_in(dir, &effective_disabled_for(&[], &mods(&[Module::DbAccess])), "").is_empty());
    assert_eq!(agent_files_in(dir, &effective_disabled_for(&[], &all_mods()), "").len(), 5);
}

#[test]
fn writing_the_agents_leaves_the_persons_files_alone() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("my-agent.md"), "my own agent").unwrap();
    std::fs::write(dir.path().join("tcm-healer.md"), "the person's own healer").unwrap();

    let err = write_agents_in(dir.path(), &[], "").unwrap_err();
    assert!(err.contains("tcm-healer.md"), "{err}");
    assert!(err.contains("was not written by this app"), "{err}");

    assert_eq!(std::fs::read_to_string(dir.path().join("my-agent.md")).unwrap(), "my own agent");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("tcm-healer.md")).unwrap(),
        "the person's own healer"
    );
    for n in ["tcm-writer.md", "tcm-case-rewriter.md", "tcm-auditor.md", "tcm-explorer.md"] {
        let t = std::fs::read_to_string(dir.path().join(n)).unwrap();
        assert!(t.contains(COMMAND_MARKER), "{n}");
    }
}

#[test]
fn an_old_role_file_is_swept_and_others_stay() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("tcm-planner.md"), format!("{COMMAND_MARKER}\nan old role")).unwrap();
    std::fs::write(dir.path().join("notes.md"), format!("{COMMAND_MARKER}\nnot ours by name")).unwrap();

    write_agents_in(dir.path(), &[], "").unwrap();

    assert!(!dir.path().join("tcm-planner.md").exists());
    assert!(dir.path().join("notes.md").exists());
    assert!(dir.path().join("tcm-writer.md").exists());
}

#[test]
fn removing_the_agents_takes_only_ours() {
    let dir = TempDir::new();
    write_agents_in(dir.path(), &[], "").unwrap();
    std::fs::write(dir.path().join("my-agent.md"), "mine").unwrap();
    std::fs::write(dir.path().join("tcm-mine.md"), "no marker").unwrap();
    std::fs::write(dir.path().join("notes.md"), format!("{COMMAND_MARKER}\nno prefix")).unwrap();

    remove_agents_in(dir.path()).unwrap();

    for r in Role::ALL {
        assert!(!dir.path().join(format!("tcm-{}.md", r.key())).exists());
    }
    for n in ["my-agent.md", "tcm-mine.md", "notes.md"] {
        assert!(dir.path().join(n).exists(), "{n} must stay");
    }

    // With only ours in it, the folder goes too.
    let lone = TempDir::new();
    let sub = lone.path().join("agents");
    write_agents_in(&sub, &[], "").unwrap();
    remove_agents_in(&sub).unwrap();
    assert!(!sub.exists());
}

/// A fake Playwright clone holding the healer's agent file with `body` after its frontmatter.
fn clone_with_healer(body: &str) -> TempDir {
    let clone = TempDir::new();
    let agents = clone.path().join(".claude").join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(agents.join("playwright-test-healer.md"), format!("---
tools: Edit
---
{body}
")).unwrap();
    clone
}

fn path_str(d: &TempDir) -> String {
    d.path().to_string_lossy().into_owned()
}

#[test]
fn the_agent_files_carry_the_live_rules() {
    let clone = clone_with_healer("LIVE HEALER");
    let files = agent_files_in(std::path::Path::new("D:/repo/.claude/agents"), &[], &path_str(&clone));
    let (_, healer) = files.iter().find(|(p, _)| p.ends_with("tcm-healer.md")).unwrap();
    assert!(healer.contains("LIVE HEALER"), "{healer}");
    // A file on disk says the day it was read, never "just now".
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    assert!(healer.contains(&format!("playwright-test-healer.md, read from the repo on {today}")), "{healer}");
    assert!(!healer.contains("just now"), "{healer}");
    assert!(!healer.contains("tools: Edit"), "{healer}");
    assert!(
        healer.ends_with("These rules also come from `get_guide` with topic autorun and role healer.
"),
        "{healer}"
    );
}

#[test]
fn an_agent_file_is_rewritten_only_when_its_rules_changed() {
    let clone = clone_with_healer("LIVE HEALER");
    let c = path_str(&clone);
    let d = TempDir::new();
    assert_eq!(write_agents_in(d.path(), &[], &c).unwrap().len(), 5);
    assert!(write_agents_in(d.path(), &[], &c).unwrap().is_empty());

    clone_with_healer_rewrite(&clone, "A CHANGED HEALER");
    assert_eq!(write_agents_in(d.path(), &[], &c).unwrap(), [d.path().join("tcm-healer.md")]);
    let t = std::fs::read_to_string(d.path().join("tcm-healer.md")).unwrap();
    assert!(t.contains("A CHANGED HEALER"), "{t}");
}

#[test]
fn the_date_in_an_agent_file_moves_only_when_the_rules_do() {
    use v2_lib::commands::ai_tools::write_agents_on;
    let clone = clone_with_healer("LIVE HEALER");
    let c = path_str(&clone);
    let d = TempDir::new();
    assert_eq!(write_agents_on(d.path(), &[], &c, "2026-10-01").unwrap().len(), 5);
    let first = std::fs::read_to_string(d.path().join("tcm-healer.md")).unwrap();
    assert!(first.contains("read from the repo on 2026-10-01"), "{first}");
    // A later day with the same rules writes nothing: the file keeps the day it was read.
    assert!(write_agents_on(d.path(), &[], &c, "2026-10-09").unwrap().is_empty());
    assert_eq!(std::fs::read_to_string(d.path().join("tcm-healer.md")).unwrap(), first);
    // The rules change: that file is rewritten with the new day, the others stay.
    clone_with_healer_rewrite(&clone, "A CHANGED HEALER");
    assert_eq!(write_agents_on(d.path(), &[], &c, "2026-10-12").unwrap(), [d.path().join("tcm-healer.md")]);
    let t = std::fs::read_to_string(d.path().join("tcm-healer.md")).unwrap();
    assert!(t.contains("A CHANGED HEALER") && t.contains("read from the repo on 2026-10-12"), "{t}");
    // The shipped copy has no live mark to date.
    let shipped = std::fs::read_to_string(d.path().join("tcm-writer.md")).unwrap();
    assert!(!shipped.contains("read from the repo on"), "{shipped}");
}

#[test]
fn only_a_date_shaped_day_is_ignored_when_comparing() {
    use v2_lib::autorun::pw_rules::without_read_dates;
    assert_eq!(
        without_read_dates("a, read from the repo on 2026-10-01\nb, read from the repo on 1999-01-31."),
        "a, read from the repo on DATE\nb, read from the repo on DATE."
    );
    // Anything else after the words is kept as it is.
    assert_eq!(without_read_dates("read from the repo on tuesday"), "read from the repo on tuesday");
    assert_eq!(without_read_dates("read from the repo on 2026-1"), "read from the repo on 2026-1");
    assert_eq!(without_read_dates("read from the repo just now"), "read from the repo just now");
}

fn clone_with_healer_rewrite(clone: &TempDir, body: &str) {
    let file = clone.path().join(".claude").join("agents").join("playwright-test-healer.md");
    std::fs::write(file, format!("---
tools: Edit
---
{body}
")).unwrap();
}

#[test]
fn a_persons_own_agent_file_stays_when_the_rules_change() {
    let clone = clone_with_healer("LIVE HEALER");
    let c = path_str(&clone);
    let d = TempDir::new();
    std::fs::write(d.path().join("tcm-healer.md"), "mine").unwrap();
    clone_with_healer_rewrite(&clone, "A CHANGED HEALER");

    let err = write_agents_in(d.path(), &[], &c).unwrap_err();
    assert!(err.contains("tcm-healer.md"), "{err}");
    assert_eq!(std::fs::read_to_string(d.path().join("tcm-healer.md")).unwrap(), "mine");
    for n in ["tcm-writer.md", "tcm-case-rewriter.md", "tcm-auditor.md", "tcm-explorer.md"] {
        assert!(std::fs::read_to_string(d.path().join(n)).unwrap().contains(COMMAND_MARKER), "{n}");
    }
}

#[test]
fn the_status_says_whether_a_clone_is_set() {
    use v2_lib::commands::pw_rules::pw_rules_status_for;
    assert!(!pw_rules_status_for("").clone_set);
    assert!(!pw_rules_status_for("   ").clone_set);
    assert!(!pw_rules_status_for("C:/no/such/folder").clone_set);
    let d = TempDir::new();
    assert!(pw_rules_status_for(&path_str(&d)).clone_set);
    assert!(pw_rules_status_for(&format!("  {}  ", path_str(&d))).clone_set);
}

#[test]
fn the_status_tells_a_repo_that_is_gone_from_one_never_set() {
    use v2_lib::commands::pw_rules::pw_rules_status_for;
    for unset in ["", "   "] {
        let s = pw_rules_status_for(unset);
        assert!(!s.clone_set && !s.clone_gone, "{unset:?}");
    }
    let gone = pw_rules_status_for("C:/no/such/folder");
    assert!(!gone.clone_set && gone.clone_gone);
    let d = TempDir::new();
    assert!(!pw_rules_status_for(&path_str(&d)).clone_gone);
    // A file where the folder should be is gone as a folder too.
    let file = d.path().join("a-file");
    std::fs::write(&file, "x").unwrap();
    let s = pw_rules_status_for(&file.to_string_lossy());
    assert!(!s.clone_set && s.clone_gone);
}

/// These two reach the disk (the repo's files, the agent files), so they must be async
/// commands, run off the main thread. This stops compiling if either goes back to a sync fn.
#[allow(dead_code)]
fn the_commands_that_touch_the_disk_are_async(app: tauri::AppHandle) {
    fn is_future<F: std::future::Future>(_: F) {}
    is_future(v2_lib::commands::pw_rules::pw_rules_status(app.clone()));
    is_future(v2_lib::commands::app_settings::set_playwright_clone(app, String::new()));
}

#[test]
fn the_agent_files_are_synced_with_no_bridge_lock_held() {
    use v2_lib::ai_bridge::{BridgeContext, BridgeState};
    use v2_lib::commands::ai_bridge::BridgeHandle;
    use v2_lib::commands::modules::resync_agents_with;

    // No bridge running: nothing is synced.
    let none = BridgeHandle::default();
    let mut called = false;
    resync_agents_with(&none, |_, _| called = true);
    assert!(!called);

    // A running bridge: its toggles and directory reach the sync, and neither lock is held then.
    let ctx = BridgeContext {
        disabled_tools: vec!["db_query".to_string()],
        working_dir: Some("D:/repo".to_string()),
        ..Default::default()
    };
    let shared = BridgeState::new(ctx, "1.0.0".to_string());
    let handle = BridgeHandle::default();
    *handle.running.lock().unwrap() = Some((std::sync::Arc::clone(&shared), 1234));
    let mut seen: Option<(Vec<String>, Option<String>)> = None;
    resync_agents_with(&handle, |disabled, dir| {
        assert!(handle.running.try_lock().is_ok(), "the bridge handle is locked while files are written");
        assert!(shared.ctx.try_lock().is_ok(), "the bridge context is locked while files are written");
        seen = Some((disabled.to_vec(), dir.map(str::to_string)));
    });
    assert_eq!(seen, Some((vec!["db_query".to_string()], Some("D:/repo".to_string()))));
}

#[test]
fn role_files_name_only_real_tools() {
    let tools = v2_lib::mcp::all_tool_names();
    // The generated files now hold the Playwright repo's own text too, which is not ours to check:
    // what is checked is each role's own rules and its tool note.
    for role in v2_lib::autorun::roles::Role::ALL {
        let content = format!("{}{}", role.tcm_rules(), role.tool_note().unwrap_or_default());
        assert!(!content.contains('\u{2014}') && !content.contains('\u{2013}'), "{} has a dash", role.name());
        for (i, span) in content.split('`').enumerate() {
            if i % 2 == 1 && span.contains('_') {
                assert!(tools.iter().any(|t| t == span), "{} names `{span}`, which is no tool", role.name());
            }
        }
    }
}

/// `/tcm:discover` and `/tcm:heal` say where their role's rules come from, and do not copy them.
#[test]
fn the_discover_and_heal_commands_say_where_their_rules_come_from() {
    let flat_of = |stem: &str| {
        COMMANDS.iter().find(|c| c.stem == stem).unwrap().body.join("
").split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let discover = flat_of("discover");
    assert!(
        discover.contains("The writer's rules come ahead of your first `start_autorun_discovery` answer and hold for the whole session, whatever tools you use: follow them."),
        "{discover}"
    );
    let heal = flat_of("heal");
    assert!(
        heal.contains("The healer's rules come ahead of your first `get_autorun_failures` answer, and the case rewriter's ahead of your first repair save; both hold for the whole session, whatever tools you use: follow them."),
        "{heal}"
    );
    assert!(!heal.contains("three"), "{heal}");
}

/// The explore tools ride in their modules: the change list with Auto Test,
/// the one-off request and the undo with API Templates.
#[test]
fn the_explore_tools_belong_to_their_modules() {
    assert_eq!(module_of_tool("send_api_request"), Some(Module::ApiTemplates));
    assert_eq!(module_of_tool("undo_explore_creates"), Some(Module::ApiTemplates));
    assert_eq!(module_of_tool("record_explore_change"), Some(Module::AutoRun));
    assert_eq!(module_of_tool("get_explore_changes"), Some(Module::AutoRun));
    for tool in ["get_kept_actions", "save_setup", "list_setups", "run_setup"] {
        assert_eq!(module_of_tool(tool), Some(Module::AutoRun), "{tool}");
    }
}

/// The two ruling tools are Auto Test's.
#[test]
fn the_ruling_tools_belong_to_auto_run() {
    assert_eq!(module_of_tool("rule_case_divergence"), Some(Module::AutoRun));
    assert_eq!(module_of_tool("clear_autorun_suspected_defect"), Some(Module::AutoRun));
}

/// Writers of one path from several threads at once (an assistant's parallel
/// subagents) must never share a temp file: every read in between, and the
/// last one, is exactly one of the documents written, and no temp file stays.
#[test]
fn eight_threads_writing_one_path_never_tear_it() {
    let dir = TempDir::new();
    let target = dir.path().join("runs.json");
    let doc = |n: usize| format!("{{\"writer\":{n},\"pad\":\"{}\"}}", "x".repeat(2000 + n * 700));
    let all: Vec<String> = (0..8).map(doc).collect();
    atomic_write(&target, &all[0]).unwrap();
    let barrier = std::sync::Barrier::new(9);
    std::thread::scope(|s| {
        for n in 0..8 {
            let (barrier, target, mine) = (&barrier, &target, &all[n]);
            s.spawn(move || {
                barrier.wait();
                for _ in 0..40 {
                    atomic_write(target, mine).unwrap();
                }
            });
        }
        barrier.wait();
        for _ in 0..200 {
            if let Ok(text) = std::fs::read_to_string(&target) {
                assert!(all.contains(&text), "a read saw a torn file: {} bytes", text.len());
            }
        }
    });
    let last = std::fs::read_to_string(&target).unwrap();
    assert!(all.contains(&last), "the file is one of the written documents");
    assert!(temp_files_in(dir.path()).is_empty(), "no temp file left: {:?}", temp_files_in(dir.path()));
}

/// With several assistants, "the browser" is not one: `/tcm:heal` asks the
/// person to close theirs only when no assistant is using it, and
/// `/tcm:gen-tests` releases the assistant's own.
#[test]
fn the_commands_do_not_treat_the_browser_as_the_only_one() {
    let flat_of = |stem: &str| {
        COMMANDS.iter().find(|c| c.stem == stem).unwrap().body.join("\n").split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let heal = flat_of("heal");
    assert!(
        heal.contains("Discovery needs a browser of its own: if the person's Auto Test browser is open and no other assistant is using it, ask them to close it first."),
        "{heal}"
    );
    assert!(!heal.contains("Discovery needs the Auto Test browser to itself"), "{heal}");
    let gen = flat_of("gen-tests");
    assert!(gen.contains("At the end, release your browser with `reset_autorun_browser` and release true."), "{gen}");
    assert!(!gen.contains("release the browser"), "{gen}");
    for stem in ["discover", "map-menus", "heal", "gen-tests"] {
        let all = flat_of(stem);
        assert!(!all.contains('\u{2014}') && !all.contains('\u{2013}'), "{stem}");
    }
}

/// Every live-app command points at the guide's one sign-in rule instead of
/// stopping on the first failed sign-in, and the two new tools are on the
/// allow-lists (the read-only inspect one is pre-approved; propose is not).
#[test]
fn live_app_commands_point_to_the_sign_in_rule() {
    let body_of = |stem: &str| COMMANDS.iter().find(|c| c.stem == stem).unwrap_or_else(|| panic!("no {stem} command")).body.join("\n");
    for stem in ["discover", "map-menus", "explore", "gen-tests", "heal"] {
        let flat = body_of(stem).split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(flat.contains("When the sign-in page does not fit"), "{stem}: {flat}");
        assert!(!flat.contains('\u{2014}') && !flat.contains('\u{2013}'), "{stem}");
    }
    let setup = body_of("setup");
    assert!(setup.contains("propose_sign_in_recipe") && setup.contains("Record sign-in"), "{setup}");
    let map = body_of("map-menus").split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(map.contains("wrong password, a locked account, a captcha or a one-time code"), "{map}");
    for tool in ["inspect_sign_in_page", "propose_sign_in_recipe"] {
        assert_eq!(v2_lib::ai_tools::AUTORUN_TOOLS.iter().filter(|t| **t == tool).count(), 1, "{tool}");
    }
    assert!(v2_lib::ai_tools::CLAUDE_AUTORUN_RULES.contains(&"mcp__tcm__inspect_sign_in_page"));
    assert!(v2_lib::ai_tools::CURSOR_AUTORUN_ENTRIES.contains(&"tcm:inspect_sign_in_page"));
    assert!(!v2_lib::ai_tools::CLAUDE_AUTORUN_RULES.iter().any(|r| r.contains("propose_sign_in_recipe")));
    assert!(!v2_lib::ai_tools::CURSOR_AUTORUN_ENTRIES.iter().any(|r| r.contains("propose_sign_in_recipe")));
}

/// The role files the assistants read carry the same pointer.
#[test]
fn agent_rules_name_the_sign_in_rule() {
    for (name, text) in [
        ("writer", include_str!("../../src/autorun/roles/writer.md")),
        ("explorer", include_str!("../../src/autorun/roles/explorer.md")),
        ("healer", include_str!("../../src/autorun/roles/healer.md")),
    ] {
        assert!(text.contains("When the sign-in page does not fit"), "{name}");
        assert!(!text.contains('\u{2014}') && !text.contains('\u{2013}'), "{name}");
    }
    let healer = include_str!("../../src/autorun/roles/healer.md");
    let stop = healer.lines().find(|l| l.contains("under a STOP line")).expect("healer STOP line");
    assert!(stop.contains("When the sign-in page does not fit"), "{stop}");
    let heal = COMMANDS.iter().find(|c| c.stem == "heal").unwrap().body.join(" ");
    let heal = heal.split_whitespace().collect::<Vec<_>>().join(" ");
    let at = heal.find("For the sign-in lines").expect("heal step 2 sign-in sentence");
    assert!(heal[at..at + 200].contains("When the sign-in page does not fit"), "{heal}");
    assert!(heal[..at].contains("put the old path back in Auto Test, Areas."), "the sign-in sentence follows the area exception: {heal}");
    assert!(!healer.contains("Stop at once on a login page"), "healer still stops on a login page outright");
}

// --- Claude Code local scope (a repository's registration lives in ~/.claude.json)

use v2_lib::ai_tools::{
    claude_project_key, clear_mcpjson_approvals, local_carries, managed_local_servers_in, merge_local_entry,
    remove_local_entry,
};
use v2_lib::commands::ai_tools::{merge_local_into_file, migrate_project_scope, remove_local_from_file};

fn tcm_for_tests() -> McpServer {
    tcm_server(r"C:\Program Files\TCM\v2.exe")
}

fn read_json(path: &std::path::Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The VS Code extension's bundled binary is a CLI candidate, newest version
/// first, and only where the file really exists (real layout:
/// `.vscode/extensions/anthropic.claude-code-<v>-<platform>/resources/native-binary/claude.exe`).
#[test]
fn the_vscode_extensions_bundled_cli_is_a_candidate() {
    let home = TempDir::new();
    for v in ["2.1.9", "2.1.311", "2.1.40"] {
        let bin = home
            .path()
            .join(".vscode/extensions")
            .join(format!("anthropic.claude-code-{v}-win32-x64"))
            .join("resources/native-binary");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("claude.exe"), b"").unwrap();
    }
    // A newer folder with no binary in it is skipped.
    std::fs::create_dir_all(home.path().join(".vscode/extensions/anthropic.claude-code-9.0.0-win32-x64")).unwrap();
    let c = claude_cli_candidates(home.path().to_str().unwrap(), "C:/nowhere");
    let last = c.last().unwrap().display().to_string().replace('\\', "/");
    assert!(last.contains("anthropic.claude-code-2.1.311-win32-x64"), "{last}");
    assert!(last.ends_with("resources/native-binary/claude.exe"), "{last}");
    // The installers' own locations still come first.
    assert!(c[0].ends_with("claude.exe") && c[0].to_string_lossy().contains(".local"));
}

/// A write always goes to the forward-slash key: an existing one for the
/// same folder is reused (any case, keeping its drive-letter case), a legacy
/// backslash key never is. A drive root keeps its slash.
#[test]
fn the_project_key_is_always_the_forward_slash_form() {
    let cfg = serde_json::json!({"projects": {
        r"C:\Users\Sam\repo": {}, "C:/Users/Sam/repo": {}, "D:/other": {}
    }});
    assert_eq!(claude_project_key(&cfg, r"c:\users\sam\repo\"), "C:/Users/Sam/repo");
    assert_eq!(claude_project_key(&cfg, "C:/Users/Sam/repo"), "C:/Users/Sam/repo");
    assert_eq!(claude_project_key(&cfg, r"d:\OTHER"), "D:/other");
    assert_eq!(claude_project_key(&cfg, r"E:\new\place"), "E:/new/place");
    assert_eq!(claude_project_key(&serde_json::json!({}), r"E:\new"), "E:/new");
    assert_eq!(claude_project_key(&serde_json::json!({}), r"C:\"), "C:/");
    // Only a legacy key exists: the write still goes to the forward form.
    let legacy_only = serde_json::json!({"projects": {r"C:\Users\Sam\repo": {}}});
    assert_eq!(claude_project_key(&legacy_only, r"C:\Users\Sam\repo"), "C:/Users/Sam/repo");
}

/// Real files hold both forms for one folder, the stale backslash one first.
/// The write lands in the forward-slash key, detection reads it, and ours is
/// taken out of the stale one (other servers there stay).
#[test]
fn with_both_key_forms_present_the_write_lands_in_the_forward_slash_key() {
    let dir = TempDir::new();
    let file = dir.path().join(".claude.json");
    std::fs::write(
        &file,
        r#"{"projects":{
            "C:\\work\\repo":{"mcpServers":{"tcm":{"command":"stale.exe"},"theirs":{"command":"t.exe"}}},
            "C:/work/repo":{"mcpServers":{"mine":{"command":"m.exe"}}}}}"#,
    )
    .unwrap();
    merge_local_into_file(&file, r"C:\work\repo", &tcm_for_tests()).unwrap();
    let v = read_json(&file);
    assert_eq!(v["projects"]["C:/work/repo"]["mcpServers"]["tcm"]["args"][0], "--mcp");
    assert_eq!(v["projects"]["C:/work/repo"]["mcpServers"]["mine"]["command"], "m.exe");
    let legacy = &v["projects"][r"C:\work\repo"]["mcpServers"];
    assert!(legacy.get("tcm").is_none(), "ours is moved out of the stale key");
    assert_eq!(legacy["theirs"]["command"], "t.exe", "nothing else there is touched");
    assert_eq!(v["projects"].as_object().unwrap().len(), 2, "no third key");
    assert!(local_carries(&file, r"C:\work\repo", "tcm"));
    assert_eq!(managed_local_servers_in(&file, "C:/work/repo"), vec!["tcm"]);
}

/// A server only in the stale backslash key reads as registered but "needs
/// updating", and Register (the write) moves it.
#[test]
fn a_legacy_only_entry_needs_updating_and_is_moved() {
    let home = TempDir::new();
    let repo = TempDir::new();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    let home_str = home.path().to_str().unwrap().to_string();
    let root = repo.path().to_str().unwrap().replace('\\', "/");
    let backslashed = root.replace('/', "\\");
    let file = home.path().join(".claude.json");
    std::fs::write(
        &file,
        serde_json::json!({"projects": {backslashed.clone(): {"mcpServers": {"tcm": {"command": "stale.exe"}}}}}).to_string(),
    )
    .unwrap();
    let row = |_: ()| {
        detect_in(&home_str, "", &|_| false, Some(&root)).into_iter().find(|t| t.id == "claude-code").unwrap()
    };
    let before = row(());
    assert_eq!(before.registered_servers, vec!["tcm"]);
    assert_eq!(before.project_scope_servers, vec!["tcm"], "flagged for moving");

    merge_local_into_file(&file, &root, &tcm_for_tests()).unwrap();
    let after = row(());
    assert_eq!(after.registered_servers, vec!["tcm"]);
    assert!(after.project_scope_servers.is_empty(), "moved: {after:?}");
    let v = read_json(&file);
    assert!(v["projects"][&backslashed]["mcpServers"].get("tcm").is_none());
    assert_eq!(v["projects"][&root]["mcpServers"]["tcm"]["args"][0], "--mcp");
}

/// An empty or unparseable `~/.claude.json` is Claude Code mid-save: refused
/// with a sentence that names no path, and left exactly as it was.
#[test]
fn an_empty_or_invalid_claude_json_is_refused_without_a_path() {
    let dir = TempDir::new();
    for (name, body) in [("empty.json", ""), ("bad.json", "{ not json"), ("list.json", "[]")] {
        let file = dir.path().join(name);
        std::fs::write(&file, body).unwrap();
        let e = merge_local_into_file(&file, "C:/a/b", &tcm_for_tests()).unwrap_err();
        assert_eq!(e, "Claude Code was saving its settings. Try again in a moment.");
        assert!(!e.contains(name) && !e.contains("tcm-ai-tools"), "{e}");
        assert!(remove_local_from_file(&file, "C:/a/b", "tcm").is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), body);
    }
}

/// The file fallback writes `projects[key].mcpServers.tcm`, keeps every
/// other key, and a second run changes nothing.
#[test]
fn the_file_fallback_writes_the_local_entry_and_is_idempotent() {
    let dir = TempDir::new();
    let file = dir.path().join(".claude.json");
    std::fs::write(
        &file,
        serde_json::json!({
            "numStartups": 7,
            "mcpServers": {"user-one": {"command": "u.exe"}},
            "projects": {
                "C:/work/repo": {"allowedTools": ["x"], "mcpServers": {"theirs": {"command": "t.exe"}, "tcm-testcases": {"command": "old.exe"}}},
                "C:/work/other": {"mcpServers": {"o": {"command": "o.exe"}}}
            }
        })
        .to_string(),
    )
    .unwrap();
    merge_local_into_file(&file, r"C:\work\repo", &tcm_for_tests()).unwrap();
    let once = std::fs::read_to_string(&file).unwrap();
    merge_local_into_file(&file, r"C:\work\repo", &tcm_for_tests()).unwrap();
    assert_eq!(once, std::fs::read_to_string(&file).unwrap(), "not idempotent");

    let v = read_json(&file);
    assert_eq!(v["numStartups"], 7);
    assert_eq!(v["mcpServers"]["user-one"]["command"], "u.exe");
    assert_eq!(v["projects"]["C:/work/other"]["mcpServers"]["o"]["command"], "o.exe");
    let p = &v["projects"]["C:/work/repo"];
    assert_eq!(p["allowedTools"][0], "x");
    assert_eq!(p["mcpServers"]["theirs"]["command"], "t.exe");
    assert_eq!(p["mcpServers"]["tcm"]["command"], r"C:\Program Files\TCM\v2.exe");
    assert_eq!(p["mcpServers"]["tcm"]["args"][0], "--mcp");
    assert!(p["mcpServers"].get("tcm-testcases").is_none(), "the old name is superseded");
    assert!(v.get("tcm").is_none());
}

/// A home with no `.claude.json` gets one, and a file that is not JSON is
/// refused rather than overwritten.
#[test]
fn the_file_fallback_creates_the_file_and_never_clobbers_garbage() {
    let dir = TempDir::new();
    let file = dir.path().join(".claude.json");
    merge_local_into_file(&file, "C:/a/b", &tcm_for_tests()).unwrap();
    assert_eq!(read_json(&file)["projects"]["C:/a/b"]["mcpServers"]["tcm"]["args"][0], "--mcp");

    // (An empty or invalid existing file is covered by
    // `an_empty_or_invalid_claude_json_is_refused_without_a_path`.)
}

/// Unregister by file removes just that entry; a missing file or entry is a
/// clean no-op.
#[test]
fn unregister_by_file_removes_only_that_entry() {
    let dir = TempDir::new();
    let file = dir.path().join(".claude.json");
    std::fs::write(
        &file,
        r#"{"k":1,"projects":{"C:/r":{"mcpServers":{"tcm":{"command":"v2.exe"},"theirs":{"command":"t.exe"}}}}}"#,
    )
    .unwrap();
    remove_local_from_file(&file, r"C:\r", "tcm").unwrap();
    let v = read_json(&file);
    assert!(v["projects"]["C:/r"]["mcpServers"].get("tcm").is_none());
    assert_eq!(v["projects"]["C:/r"]["mcpServers"]["theirs"]["command"], "t.exe");
    assert_eq!(v["k"], 1);
    remove_local_from_file(&file, r"C:\r", "tcm").unwrap();
    remove_local_from_file(&dir.path().join("absent.json"), r"C:\r", "tcm").unwrap();
    assert_eq!(remove_local_entry(r#"{"projects":{}}"#, "C:/r", "tcm").unwrap(), None);
}

/// Migration takes TCM's entries out of `<repo>/.mcp.json`, keeps the other
/// servers and the file, and leaves `mcpServers: {}` when ours were all.
#[test]
fn migration_removes_our_entries_from_the_shared_mcp_json() {
    let repo = TempDir::new();
    let home = TempDir::new();
    let claude = home.path().join(".claude.json");
    let root = repo.path().to_str().unwrap().to_string();
    let names = vec!["tcm".to_string(), "tcm-testcases".to_string()];

    let mcp = repo.path().join(".mcp.json");
    std::fs::write(
        &mcp,
        r#"{"mcpServers":{"tcm":{"command":"v2.exe"},"tcm-testcases":{"command":"old.exe"},"theirs":{"command":"t.exe"}}}"#,
    )
    .unwrap();
    migrate_project_scope(&root, &claude, &names);
    let v = read_json(&mcp);
    assert!(v["mcpServers"].get("tcm").is_none() && v["mcpServers"].get("tcm-testcases").is_none());
    assert_eq!(v["mcpServers"]["theirs"]["command"], "t.exe");

    std::fs::write(&mcp, r#"{"mcpServers":{"tcm":{"command":"v2.exe"}}}"#).unwrap();
    migrate_project_scope(&root, &claude, &names);
    assert_eq!(read_json(&mcp), serde_json::json!({"mcpServers": {}}));

    // No `.mcp.json` at all: nothing is created.
    std::fs::remove_file(&mcp).unwrap();
    migrate_project_scope(&root, &claude, &names);
    assert!(!mcp.exists());
}

/// A stale approval for our names leaves the project's lists; other names
/// and other projects stay.
#[test]
fn migration_tidies_the_approval_lists() {
    let text = serde_json::json!({"projects": {
        "C:/r": {"enabledMcpjsonServers": ["tcm", "theirs"], "disabledMcpjsonServers": ["tcm-testcases"]},
        "C:/s": {"enabledMcpjsonServers": ["tcm"]}
    }})
    .to_string();
    let out = clear_mcpjson_approvals(&text, r"C:\r", &["tcm", "tcm-testcases"]).unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["projects"]["C:/r"]["enabledMcpjsonServers"], serde_json::json!(["theirs"]));
    assert_eq!(v["projects"]["C:/r"]["disabledMcpjsonServers"], serde_json::json!([]));
    assert_eq!(v["projects"]["C:/s"]["enabledMcpjsonServers"], serde_json::json!(["tcm"]));
    assert_eq!(clear_mcpjson_approvals(&out, r"C:\r", &["tcm"]).unwrap(), None);

    // The same migration, end to end through the real home-relative file.
    let repo = TempDir::new();
    let home = TempDir::new();
    let claude = home.path().join(".claude.json");
    let root = repo.path().to_str().unwrap().replace('\\', "/");
    std::fs::write(
        &claude,
        serde_json::json!({"projects": {root.clone(): {"enabledMcpjsonServers": ["tcm"]}}}).to_string(),
    )
    .unwrap();
    migrate_project_scope(&root, &claude, &["tcm".to_string()]);
    assert_eq!(read_json(&claude)["projects"][&root]["enabledMcpjsonServers"], serde_json::json!([]));
}

/// A local entry counts as registered, an old `.mcp.json` entry still counts
/// but is flagged for moving, and another tool's row never carries the flag.
#[test]
fn detection_sees_local_as_registered_and_project_as_needing_a_move() {
    let home = TempDir::new();
    let repo = TempDir::new();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    let home_str = home.path().to_str().unwrap().to_string();
    let root = repo.path().to_str().unwrap().replace('\\', "/");
    let on_path = |_: &str| false;
    let claude_row = |root: &str| {
        detect_in(&home_str, "", &on_path, Some(root)).into_iter().find(|t| t.id == "claude-code").unwrap()
    };

    // Nothing anywhere.
    let none = claude_row(&root);
    assert!(none.registered_servers.is_empty() && none.project_scope_servers.is_empty());

    // Old project scope: registered, and it needs moving.
    std::fs::write(repo.path().join(".mcp.json"), r#"{"mcpServers":{"tcm":{"command":"v2.exe"}}}"#).unwrap();
    let old = claude_row(&root);
    assert_eq!(old.scope, "project");
    assert_eq!(old.registered_servers, vec!["tcm"]);
    assert_eq!(old.project_scope_servers, vec!["tcm"]);

    // Local scope as well, under an old backslash key: registered, and the
    // move is still pending until that copy is gone.
    let backslashed = root.replace('/', "\\");
    std::fs::write(
        home.path().join(".claude.json"),
        serde_json::json!({"projects": {backslashed: {"mcpServers": {"tcm": {"command": "v2.exe"}}}}}).to_string(),
    )
    .unwrap();
    let both = claude_row(&root);
    assert_eq!(both.registered_servers, vec!["tcm"], "one name, not listed twice");
    assert_eq!(both.project_scope_servers, vec!["tcm"]);

    // Local only, under the current forward-slash key: registered, nothing
    // to move.
    std::fs::remove_file(repo.path().join(".mcp.json")).unwrap();
    std::fs::write(
        home.path().join(".claude.json"),
        serde_json::json!({"projects": {root.clone(): {"mcpServers": {"tcm": {"command": "v2.exe"}}}}}).to_string(),
    )
    .unwrap();
    let local = claude_row(&root);
    assert_eq!(local.registered_servers, vec!["tcm"]);
    assert!(local.project_scope_servers.is_empty());
    assert!(local_carries(&home.path().join(".claude.json"), &root, "tcm"));
    assert_eq!(managed_local_servers_in(&home.path().join(".claude.json"), &root), vec!["tcm"]);

    // Another repository sees none of it.
    let other = TempDir::new();
    let elsewhere = claude_row(&other.path().to_str().unwrap().replace('\\', "/"));
    assert!(elsewhere.registered_servers.is_empty());
    for t in detect_in(&home_str, "", &on_path, Some(&root)).into_iter().filter(|t| t.id != "claude-code") {
        assert!(t.project_scope_servers.is_empty(), "{}", t.id);
    }
}

/// `retire_superseded_claude` in local scope looks inside the repository's
/// key of `~/.claude.json`, not at its top level.
#[cfg(windows)]
#[test]
fn the_old_name_is_retired_from_the_local_scope_too() {
    let bin = TempDir::new();
    let repo = TempDir::new();
    let home = TempDir::new();
    let log = bin.path().join("calls.txt");
    let probe = cli_probe(bin.path(), &log, 0);
    let config = home.path().join(".claude.json");
    let root = repo.path().to_str().unwrap().replace('\\', "/");
    std::fs::write(
        &config,
        serde_json::json!({"mcpServers": {"tcm-testcases": {"command": "user.exe"}},
            "projects": {root.clone(): {"mcpServers": {"tcm": {"command": "v2.exe"}, "tcm-testcases": {"command": "old.exe"}}}}})
        .to_string(),
    )
    .unwrap();

    retire_superseded_claude(Some(&probe), TCM_SERVER, "local", Some(repo.path()), &config).unwrap();
    let calls = std::fs::read_to_string(&log).unwrap();
    assert_eq!(calls.lines().count(), 1, "{calls}");
    assert!(calls.contains("mcp remove --scope local tcm-testcases"), "{calls}");

    // No CLI: removed from the repository's entry, user scope untouched.
    retire_superseded_claude(None, TCM_SERVER, "local", Some(repo.path()), &config).unwrap();
    let v = read_json(&config);
    assert!(v["projects"][&root]["mcpServers"].get("tcm-testcases").is_none());
    assert_eq!(v["projects"][&root]["mcpServers"]["tcm"]["command"], "v2.exe");
    assert_eq!(v["mcpServers"]["tcm-testcases"]["command"], "user.exe");
}

#[test]
fn merge_local_entry_refuses_a_misshapen_config() {
    assert!(merge_local_entry("[]", "C:/r", &tcm_for_tests()).is_err());
    assert!(merge_local_entry(r#"{"projects": 3}"#, "C:/r", &tcm_for_tests()).is_err());
}
