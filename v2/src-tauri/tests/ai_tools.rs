//! Pure-logic tests for AI-tool detection + registration: `merge_entry`
//! never clobbers unrelated JSON, and `detect` reads a fake temp-dir layout
//! the same way it would read the real home/appdata dirs.

use v2_lib::ai_tools::{
    claude_cli_candidates, detect, merge_entry, remove_entry, command_markdown, command_path,
    tcm_server, McpServer, DB_SERVER, COMMAND_MARKER, TCM_SERVER,
};

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
    let shown: Vec<String> = c.iter().map(|p| p.display().to_string().replace('\\', "/")).collect();

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

/// The command is what makes the tools reachable without naming them, so
/// its frontmatter has to carry a description worth showing in the picker.
#[test]
fn the_command_describes_itself_and_points_at_the_live_guide() {
    let md = command_markdown();
    assert!(md.starts_with("---\n"), "needs YAML frontmatter: {md}");
    assert!(md.contains(&format!("name: {TCM_SERVER}")), "{md}");

    let front = md.split("---").nth(1).expect("frontmatter");
    // The description is what shows in the picker, so it has to name the
    // job rather than the tool.
    assert!(front.contains("description:"), "{front}");
    assert!(front.to_lowercase().contains("test cases"), "{front}");
    // Arguments reach the prompt, so `/tcm-testcases 145664` works.
    assert!(md.contains("$ARGUMENTS"), "{md}");

    // A pointer, not a second copy of the rules. If this ever starts
    // restating the format, the module list or the tag list, it will be
    // stale the first time get_writing_guide changes.
    assert!(md.contains("get_writing_guide"), "{md}");
    for copied in ["Not Automated", "semicolon", "automation_status", "reviewer_notes"] {
        assert!(
            !md.contains(copied),
            "the command must point at the guide, not duplicate it - found {copied:?}"
        );
    }
    // And it identifies itself, so removal never touches a file the user
    // wrote at the same path.
    assert!(md.contains(COMMAND_MARKER), "{md}");
}

#[test]
fn the_command_lands_where_claude_code_looks_for_commands() {
    let p = command_path("C:/Users/Sam").display().to_string().replace('\\', "/");
    assert_eq!(p, format!("C:/Users/Sam/.claude/commands/{TCM_SERVER}.md"));
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
