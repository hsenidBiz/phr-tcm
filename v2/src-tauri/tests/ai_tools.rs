//! Pure-logic tests for AI-tool detection + registration: `merge_entry`
//! never clobbers unrelated JSON, and `detect` reads a fake temp-dir layout
//! the same way it would read the real home/appdata dirs.

use v2_lib::ai_tools::{detect, merge_entry};

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

#[test]
fn merge_entry_creates_key_and_entry_on_empty_object() {
    let out = merge_entry("{}", "mcpServers", "C:/app/v2.exe").unwrap();
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
    let out = merge_entry(existing, "mcpServers", "C:/app/v2.exe").unwrap();
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
    let out = merge_entry(existing, "mcpServers", "C:/app/v2.exe").unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["mcpServers"]["tcm-testcases"]["command"], "C:/app/v2.exe");
    assert_eq!(v["mcpServers"]["tcm-testcases"]["args"][0], "--mcp");
    // Only one entry under the key - no duplicate/stale leftover.
    assert_eq!(v["mcpServers"].as_object().unwrap().len(), 1);
}

#[test]
fn merge_entry_errors_on_invalid_json_never_clobbers() {
    let result = merge_entry("{ not valid json", "mcpServers", "C:/app/v2.exe");
    assert!(result.is_err());
}

#[test]
fn merge_entry_errors_when_key_is_not_an_object() {
    let existing = r#"{ "mcpServers": "oops-a-string" }"#;
    let result = merge_entry(existing, "mcpServers", "C:/app/v2.exe");
    assert!(result.is_err());
}

#[test]
fn merge_entry_errors_when_root_is_not_an_object() {
    let result = merge_entry("[]", "mcpServers", "C:/app/v2.exe");
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
    assert!(cc.registered, "claude-code: .claude.json already has tcm-testcases");

    let cd = by_id("claude-desktop");
    assert!(cd.installed, "claude-desktop: appdata/Claude dir present marks it installed");
    assert!(!cd.registered, "claude-desktop: config exists but has no tcm-testcases entry");

    let cursor = by_id("cursor");
    assert!(cursor.installed, "cursor: .cursor dir present");
    assert!(!cursor.registered, "cursor: no config file yet");

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
    assert!(!vscode.registered, "vscode: no mcp.json in fake appdata");
}
