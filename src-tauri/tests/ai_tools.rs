//! Pure-logic tests for AI-tool detection + registration: `merge_entry`
//! never clobbers unrelated JSON, and `detect` reads a fake temp-dir layout
//! the same way it would read the real home/appdata dirs.

use v2_lib::ai_tools::{
    atomic_write, atomic_write_with, claude_cli_candidates, command_dir, command_files, command_files_for, command_files_in,
    command_markdown, config_for, detect, detect_in, merge_entry, project_command_dir,
    remove_entry, resolve_db_command, tcm_server, McpServer, COMMAND_MARKER, COMMANDS,
    DB_SERVER, TCM_SERVER, TOOL_SPECS,
};
use v2_lib::commands::ai_tools::{mcp_add_args, mcp_add_command, project_relative, project_root};

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
    // Auto Run is still in development: its tools are hidden and its
    // commands are gone with them, so nothing in the picker points at it.
    const TOOLS: [&str; 5] = [
        "begin-test-case-writing", "failures", "optimize", "get-wiki-info", "page",
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
    for gone in ["fanout", "guide", "examples", "suites", "suite-cases", "coverage", "validate", "transform", "tags", "pbis"] {
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
    let mcp = include_str!("../src/mcp.rs");
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
    assert_eq!(v["mcpServers"]["tcm-testcases"]["command"], "C:/app/v2.exe");
    assert_eq!(v["mcpServers"]["tcm-testcases"]["args"][0], "--mcp");
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
    assert_eq!(v["mcpServers"]["tcm-testcases"]["command"], "C:/app/v2.exe");
    assert_eq!(v["someOtherTopLevelField"], true);
}

#[test]
fn merge_entry_replaces_existing_tcm_testcases_entry() {
    let existing = r#"{
        "mcpServers": {
            "tcm-testcases": { "command": "stale/old.exe", "args": ["--old-flag"] }
        }
    }"#;
    let out = merge_entry(existing, "mcpServers", &tcm_server("C:/app/v2.exe")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["mcpServers"]["tcm-testcases"]["command"], "C:/app/v2.exe");
    assert_eq!(v["mcpServers"]["tcm-testcases"]["args"][0], "--mcp");
    // Only one entry under the key - no duplicate/stale leftover.
    assert_eq!(v["mcpServers"].as_object().unwrap().len(), 1);
}

#[test]
fn remove_entry_deletes_ours_and_preserves_everything_else() {
    let existing = r#"{
        "otherTopLevel": true,
        "mcpServers": {
            "someone-else": { "command": "x.exe" },
            "tcm-testcases": { "command": "C:/app/v2.exe", "args": ["--mcp"] }
        }
    }"#;
    let out = remove_entry(existing, "mcpServers", TCM_SERVER).unwrap().expect("entry was present");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["mcpServers"].get("tcm-testcases").is_none());
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
        r#"{"mcpServers": {"tcm-testcases": {"command": "x", "args": ["--mcp"]}}}"#,
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
    assert_eq!(cc.registered_servers, vec![TCM_SERVER], "claude-code: .claude.json already has tcm-testcases");

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


// ---------------------------------------------------------------- db server

fn db_server() -> McpServer {
    let mut env = std::collections::BTreeMap::new();
    env.insert("DB_TYPE".to_string(), "mssql".to_string());
    env.insert(
        "CONNECTION_STRING".to_string(),
        "Server=db,1433;Database=HR;User Id=sa;Password=p@ss;TrustServerCertificate=True;".to_string(),
    );
    env.insert("SCHEMA_FILTER".to_string(), "dbo,hr".to_string());
    McpServer {
        name: DB_SERVER.to_string(),
        command: "C:/tools/PeoplesHR.DBMCPServer.exe".to_string(),
        args: vec![],
        env,
    }
}

/// The company server is configured entirely through env vars, so those
/// have to survive into the config verbatim.
#[test]
fn the_db_server_writes_its_environment() {
    let out = merge_entry("{}", "mcpServers", &db_server()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let entry = &v["mcpServers"][DB_SERVER];
    assert_eq!(entry["command"], "C:/tools/PeoplesHR.DBMCPServer.exe");
    assert_eq!(entry["env"]["DB_TYPE"], "mssql");
    assert!(entry["env"]["CONNECTION_STRING"].as_str().unwrap().contains("Password=p@ss"));
    assert_eq!(entry["env"]["SCHEMA_FILTER"], "dbo,hr");
}

/// Both servers coexist: registering one must never disturb the other.
#[test]
fn both_servers_live_side_by_side() {
    let ours = merge_entry("{}", "mcpServers", &tcm_server("C:/app/v2.exe")).unwrap();
    let both = merge_entry(&ours, "mcpServers", &db_server()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&both).unwrap();
    assert_eq!(v["mcpServers"][TCM_SERVER]["args"][0], "--mcp");
    assert_eq!(v["mcpServers"][DB_SERVER]["env"]["DB_TYPE"], "mssql");

    // Removing the database server leaves ours untouched.
    let left = remove_entry(&both, "mcpServers", DB_SERVER).unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_str(&left).unwrap();
    assert!(v["mcpServers"][DB_SERVER].is_null());
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
    let vscode = merge_entry("{}", "servers", &db_server()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&vscode).unwrap();
    assert_eq!(v["servers"][DB_SERVER]["type"], "stdio");

    let other = merge_entry("{}", "mcpServers", &db_server()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&other).unwrap();
    assert!(v["mcpServers"][DB_SERVER].get("type").is_none());
}

#[test]
fn detect_reports_each_managed_server_separately() {
    let dir = TempDir::new();
    std::fs::create_dir_all(dir.path().join(".cursor")).unwrap();
    std::fs::write(
        dir.path().join(".cursor").join("mcp.json"),
        serde_json::json!({
            "mcpServers": {
                "tcm-testcases": { "command": "v2.exe" },
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

// ---- resolve_db_command: only a real invocation may reach a config ------

#[test]
fn a_picked_exe_registers_as_itself() {
    let dir = TempDir::new();
    let exe = dir.path().join("PeoplesHR.DBMCPServer.exe");
    std::fs::write(&exe, "x").unwrap();
    let (command, args) = resolve_db_command(&exe).unwrap();
    assert_eq!(command, exe.to_string_lossy());
    assert!(args.is_empty());
}

#[test]
fn a_picked_dll_runs_through_dotnet() {
    let dir = TempDir::new();
    let dll = dir.path().join("PeoplesHR.DBMCPServer.dll");
    std::fs::write(&dll, "x").unwrap();
    let (command, args) = resolve_db_command(&dll).unwrap();
    assert_eq!(command, "dotnet");
    assert_eq!(args, vec![dll.to_string_lossy().to_string()]);
}

/// The incident layout: the repo folder was picked, and a directory was
/// registered as `command` - unlaunchable, so the server never started.
/// The folder must resolve to the BUILT exe, past the obj\ intermediate
/// apphost and the test host.
#[test]
fn a_picked_folder_resolves_to_the_built_exe_only() {
    let dir = TempDir::new();
    let bin = dir
        .path()
        .join("src")
        .join("PeoplesHR.DBMCPServer")
        .join("bin")
        .join("Debug")
        .join("net10.0");
    std::fs::create_dir_all(&bin).unwrap();
    let real = bin.join("PeoplesHR.DBMCPServer.exe");
    std::fs::write(&real, "x").unwrap();
    let obj = dir.path().join("src/PeoplesHR.DBMCPServer/obj/Debug/net10.0");
    std::fs::create_dir_all(&obj).unwrap();
    std::fs::write(obj.join("apphost.exe"), "decoy").unwrap();
    let tests = dir.path().join("src/PeoplesHR.DBMCPServer.Tests/bin/Debug/net10.0");
    std::fs::create_dir_all(&tests).unwrap();
    std::fs::write(tests.join("testhost.exe"), "decoy").unwrap();

    let (command, args) = resolve_db_command(dir.path()).unwrap();
    assert_eq!(command, real.to_string_lossy());
    assert!(args.is_empty());
}

#[test]
fn an_unbuilt_folder_is_refused_with_the_build_instruction() {
    let dir = TempDir::new();
    std::fs::create_dir_all(dir.path().join("src/PeoplesHR.DBMCPServer")).unwrap();
    let err = resolve_db_command(dir.path()).unwrap_err();
    assert!(err.contains("dotnet build"), "error must say how to fix it: {err}");
}

#[test]
fn a_source_file_pick_is_refused_with_guidance() {
    let dir = TempDir::new();
    let proj = dir.path().join("PeoplesHR.DBMCPServer.csproj");
    std::fs::write(&proj, "<Project/>").unwrap();
    let err = resolve_db_command(&proj).unwrap_err();
    assert!(err.contains("pick the built server executable"), "{err}");
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
    let (home, appdata) = fake_layout(); // ~/.claude.json carries tcm-testcases globally
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
    assert_eq!(cc.registered_servers, vec![DB_SERVER], "the repo has the DB server, not ours");
    let cd = tools.iter().find(|t| t.id == "claude-desktop").unwrap();
    assert_eq!(cd.scope, "global", "no project config exists for Claude Desktop");
}

/// An entry left in the MACHINE-WIDE config while the row reads a
/// repository's is reported separately - it shadows the project one in most
/// clients, so the UI has to be able to offer to retire it. It must NOT
/// appear as `registered_servers`, which is what "Registered ✓" means.
#[test]
fn a_repo_row_reports_the_leftover_global_entry_separately() {
    let (home, appdata) = fake_layout(); // ~/.claude.json carries tcm-testcases globally
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

use v2_lib::ai_tools::{effective_disabled_for, CORE_TOOLS, DEV_ONLY_TOOLS};

use v2_lib::ai_tools::autorun_offered_for;

/// The Auto Run tools are offered in a development build, and in a release
/// build once this machine's optional extras are unlocked - and the
/// disabled set follows that answer: offered, they are ordinary switchable
/// tools; not offered, they are disabled first, always.
#[test]
fn auto_run_tools_are_offered_in_a_dev_build_or_once_unlocked() {
    assert!(autorun_offered_for(true, false));
    assert!(autorun_offered_for(true, true));
    assert!(autorun_offered_for(false, true), "an unlocked release build offers them");
    assert!(!autorun_offered_for(false, false), "a locked release build does not");

    assert_eq!(effective_disabled_for(&[], autorun_offered_for(false, true)), Vec::<String>::new());
    assert_eq!(
        effective_disabled_for(&["save_autorun_script".into()], autorun_offered_for(false, true)),
        vec!["save_autorun_script"],
        "on an unlocked machine the person's own switch still turns them off"
    );
    assert_eq!(effective_disabled_for(&[], autorun_offered_for(false, false)), DEV_ONLY_TOOLS.to_vec());
}

/// The policy: the seven Auto Run tools are development-build only, and
/// the core set can never be switched off - whatever the frontend's list
/// says, in either build kind.
///
/// Validate, optimise and merge joined the core set: finishing a draft is
/// part of writing one, and an assistant that can write cases but cannot
/// check, order or merge them hands over nothing anyone can ship.
///
/// `dev_build()` itself is `cfg!(debug_assertions)`, which is true for
/// this very test binary, so the release rule (`dev: false`) can only be
/// exercised through `effective_disabled_for`'s explicit seam - never
/// through `effective_disabled()`, which would always take this process's
/// own (development) build kind.
#[test]
fn the_effective_disabled_set_is_build_dependent_and_protects_the_core() {
    assert_eq!(
        DEV_ONLY_TOOLS,
        [
            "get_autorun_guide",
            "save_autorun_script",
            "get_autorun_page",
            "probe_autorun_locator",
            "try_autorun_action",
            "get_autorun_failures",
            "record_autorun_quirk"
        ]
    );
    assert_eq!(
        CORE_TOOLS,
        [
            "begin_test_case_writing",
            "get_writing_guide",
            "get_test_cases",
            "check_spec_coverage",
            "transform_cases",
            "validate_cases",
            "optimize_cases",
            "merge_case_files"
        ]
    );

    // Release (dev: false): the dev-only tools are disabled first,
    // always - the same rule the old always-hidden constant enforced.
    assert_eq!(effective_disabled_for(&[], false), DEV_ONLY_TOOLS.to_vec());
    // A core tool named in the frontend's list is dropped, not honoured.
    assert_eq!(
        effective_disabled_for(&["optimize_cases".into(), "merge_case_files".into()], false),
        DEV_ONLY_TOOLS.to_vec(),
        "the three that finish a draft cannot be switched off"
    );
    let got = effective_disabled_for(
        &["begin_test_case_writing".into(), "search_wiki".into(), "get_autorun_guide".into()],
        false,
    );
    let mut expected: Vec<&str> = DEV_ONLY_TOOLS.to_vec();
    expected.push("search_wiki");
    assert_eq!(got, expected, "dev-only first, core dropped, no duplicates");

    // Development (dev: true): the dev-only tools default to ON, like
    // every other switchable tool - not added unasked.
    assert_eq!(effective_disabled_for(&[], true), Vec::<String>::new());
    // Named in the person's own list, they switch off like anything else.
    assert_eq!(
        effective_disabled_for(&["save_autorun_script".into()], true),
        vec!["save_autorun_script"]
    );
    // Core tools still cannot be switched off, and duplicates still
    // collapse, in a development build too.
    assert_eq!(
        effective_disabled_for(
            &["optimize_cases".into(), "search_wiki".into(), "search_wiki".into()],
            true
        ),
        vec!["search_wiki"]
    );

    // The two database tools are neither: they can be switched off, and a
    // release build offers them exactly as a development build does. What
    // they may DO is decided by the chosen connection and the write
    // switch beside it, never by the build kind.
    for name in ["db_lookup", "db_query"] {
        assert!(!CORE_TOOLS.contains(&name), "{name} must be switchable");
        assert!(!DEV_ONLY_TOOLS.contains(&name), "{name} must ship in a release build");
        assert_eq!(
            effective_disabled_for(&[name.to_string()], true),
            vec![name],
            "{name} switches off like any other tool"
        );
        assert!(
            effective_disabled_for(&[name.to_string()], false).contains(&name.to_string()),
            "{name} switches off in a release build too"
        );
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

/// The regression that broke the database server: env pairs BEFORE the
/// name feed the CLI's variadic `-e`, which then eats the name. Pin
/// name-first, `--` before the binary, and every env pair in between.
#[test]
fn mcp_add_puts_the_name_before_the_env_pairs() {
    let mut env = std::collections::BTreeMap::new();
    env.insert("DB_TYPE".to_string(), "mssql".to_string());
    env.insert(
        "CONNECTION_STRING".to_string(),
        "Server=tcp:db,1433;Database=PHRX;User Id=ro".to_string(),
    );
    let server = McpServer {
        name: "phr-db-mcp".to_string(),
        command: r"C:	ools\PeoplesHR.DBMCPServer.exe".to_string(),
        args: vec![],
        env,
    };
    let args = mcp_add_args(&server, "project");

    let name_at = args.iter().position(|a| a == "phr-db-mcp").unwrap();
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
        name: "tcm-testcases".to_string(),
        command: "v2.exe".to_string(),
        args: vec!["--mcp".to_string()],
        env: Default::default(),
    };
    assert_eq!(
        mcp_add_args(&server, "user"),
        vec!["mcp", "add", "--scope", "user", "tcm-testcases", "--", "v2.exe", "--mcp"]
    );
}

/// A repository registration is `--scope project`, which the CLI keys
/// on its cwd - the caller runs it inside the repo (see
/// `run_claude_mcp_add`).
#[test]
fn a_repo_registration_asks_for_project_scope() {
    let server = McpServer {
        name: "tcm-testcases".to_string(),
        command: "v2.exe".to_string(),
        args: vec!["--mcp".to_string()],
        env: Default::default(),
    };
    let args = mcp_add_args(&server, "project");
    assert_eq!(&args[2..4], ["--scope", "project"]);
}

/// The path handed to `exclude_locally` for a non-Claude tool's
/// project config (e.g. Cursor's `.cursor/mcp.json`) - forward-slashed
/// regardless of platform, and None for anything not under the root.
#[test]
fn project_relative_forward_slashes_a_path_under_the_root() {
    assert_eq!(
        project_relative(r"D:\repo", std::path::Path::new(r"D:\repo\.cursor\mcp.json")),
        Some(".cursor/mcp.json".to_string())
    );
    assert_eq!(project_relative(r"D:\repo", std::path::Path::new(r"D:\elsewhere\mcp.json")), None);
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
    let server = McpServer { name: "phr-db-mcp".into(), command: "db.exe".into(), args: vec![], env };
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
        name: "tcm-testcases".into(),
        command: "v2.exe".into(),
        args: vec!["--mcp".into()],
        env: Default::default(),
    };
    let cmd = mcp_add_command(std::path::Path::new("C:/x/claude.cmd"), &server, "user", None);
    assert_eq!(cmd.get_program(), "C:/x/claude.cmd");
    let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(args, mcp_add_args(&server, "user"));
}
