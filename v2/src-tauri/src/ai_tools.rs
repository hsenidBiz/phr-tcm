//! AI-tool detection + registration, pure logic (testable with injected
//! paths). `commands/ai_tools.rs` supplies the real home/appdata dirs and
//! PATH probe, and does the actual file/process I/O.

use std::path::PathBuf;

/// Static description of one AI tool: where its MCP config lives, how to
/// tell it's installed, and which JSON key holds its server map.
pub struct ToolSpec {
    pub id: &'static str,
    pub name: &'static str,
    /// Command name to probe on PATH (via `where`), if any.
    pub path_cmd: Option<&'static str>,
    /// Directory under `home` whose presence marks the tool installed
    /// (in addition to / instead of a PATH probe).
    pub install_dir: Option<&'static str>,
    /// Directory under `appdata` whose presence marks the tool installed
    /// (for tools with no `~`-rooted marker, e.g. Claude Desktop).
    pub install_appdata_dir: Option<&'static str>,
    /// Builds the absolute config-file path from `home` / `appdata`.
    pub config_path: fn(home: &str, appdata: &str) -> PathBuf,
    /// JSON key the server entry lives under (e.g. "mcpServers", "servers").
    pub entry_key: &'static str,
}

pub const TOOL_SPECS: &[ToolSpec] = &[
    ToolSpec {
        id: "claude-code",
        name: "Claude Code",
        path_cmd: Some("claude"),
        install_dir: Some(".claude"),
        install_appdata_dir: None,
        // claude-code registers via its own CLI, but detection still reads
        // ~/.claude.json to see whether tcm-testcases is already there.
        config_path: |home, _appdata| PathBuf::from(home).join(".claude.json"),
        entry_key: "mcpServers",
    },
    ToolSpec {
        id: "claude-desktop",
        name: "Claude Desktop",
        path_cmd: None,
        install_dir: None,
        install_appdata_dir: Some("Claude"),
        config_path: |_home, appdata| {
            PathBuf::from(appdata).join("Claude").join("claude_desktop_config.json")
        },
        entry_key: "mcpServers",
    },
    ToolSpec {
        id: "vscode",
        name: "VS Code",
        path_cmd: Some("code"),
        install_dir: None,
        install_appdata_dir: None,
        config_path: |_home, appdata| {
            PathBuf::from(appdata).join("Code").join("User").join("mcp.json")
        },
        entry_key: "servers",
    },
    ToolSpec {
        id: "cursor",
        name: "Cursor",
        path_cmd: None,
        install_dir: Some(".cursor"),
        install_appdata_dir: None,
        config_path: |home, _appdata| PathBuf::from(home).join(".cursor").join("mcp.json"),
        entry_key: "mcpServers",
    },
    ToolSpec {
        id: "windsurf",
        name: "Windsurf",
        path_cmd: None,
        install_dir: Some(".codeium/windsurf"),
        install_appdata_dir: None,
        config_path: |home, _appdata| {
            PathBuf::from(home).join(".codeium").join("windsurf").join("mcp_config.json")
        },
        entry_key: "mcpServers",
    },
];

/// Our own MCP server's key in every tool's config.
pub const TCM_SERVER: &str = "tcm-testcases";
/// The company's SQL Server schema MCP server, registered alongside ours
/// so an assistant can read the database and the test cases in one place.
pub const DB_SERVER: &str = "phr-db-mcp";
/// Every server this app manages. Anything else in a config is somebody
/// else's and is never touched.
pub const MANAGED_SERVERS: &[&str] = &[TCM_SERVER, DB_SERVER];

/// One MCP server as it appears in a tool's config file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct McpServer {
    /// The config key, e.g. "tcm-testcases".
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    /// Environment the tool must set when launching it. BTreeMap so the
    /// written config is byte-stable rather than reordering on every save.
    pub env: std::collections::BTreeMap<String, String>,
}

/// What the frontend needs to render one row of the AI-tools list.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct DetectedTool {
    pub id: String,
    pub name: String,
    pub installed: bool,
    /// Which of `MANAGED_SERVERS` this tool's config currently carries.
    pub registered_servers: Vec<String>,
}

/// Shared installed-check used by both `detect` (for every tool) and
/// `register_ai_tool` (to refuse registering a tool that isn't there):
/// PATH probe first, then the home/appdata marker directories.
pub fn is_installed(spec: &ToolSpec, home: &str, appdata: &str, on_path: &dyn Fn(&str) -> bool) -> bool {
    spec.path_cmd.is_some_and(on_path)
        || spec.install_dir.is_some_and(|dir| PathBuf::from(home).join(dir).is_dir())
        || spec
            .install_appdata_dir
            .is_some_and(|dir| PathBuf::from(appdata).join(dir).is_dir())
}

/// Detects installed/registered state for every known tool, given injected
/// base dirs and a PATH probe (so tests never touch the real filesystem).
pub fn detect(home: &str, appdata: &str, on_path: &dyn Fn(&str) -> bool) -> Vec<DetectedTool> {
    TOOL_SPECS
        .iter()
        .map(|spec| {
            let installed = is_installed(spec, home, appdata, on_path);
            let config_path = (spec.config_path)(home, appdata);
            let entries = std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.get(spec.entry_key).cloned());
            let registered_servers = MANAGED_SERVERS
                .iter()
                .filter(|name| {
                    entries.as_ref().and_then(|e| e.get(**name)).is_some()
                })
                .map(|name| name.to_string())
                .collect();
            DetectedTool {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                installed,
                registered_servers,
            }
        })
        .collect()
}

/// Our own server, as it should appear in a config.
pub fn tcm_server(exe: &str) -> McpServer {
    McpServer {
        name: TCM_SERVER.to_string(),
        command: exe.to_string(),
        args: vec!["--mcp".to_string()],
        env: Default::default(),
    }
}

/// The Claude Code slash command written alongside the MCP registration.
///
/// # Why a command
///
/// Without one the tools are only reachable by name: somebody has to know
/// `begin_test_case_writing` exists and type it out. A command puts
/// `/tcm-testcases` in the picker next to everything else, so starting a
/// test-case session is one entry rather than a recalled tool name.
///
/// It deliberately does NOT restate the format, the workflow, the allowed
/// modules or the tag list. All of that already comes from
/// `get_writing_guide`, live and per-project, and a second copy on disk
/// would be stale the first time the guide changed - this codebase has
/// already paid for one hand-copied duplicate of an alias list that drifted
/// silently. So the command is an entry point and a pointer, and the guide
/// stays the single source.
pub const COMMAND_MARKER: &str = "<!-- generated by Test Case Manager -->";

/// `~/.claude/commands/tcm-testcases.md` - i.e. `/tcm-testcases`.
pub fn command_path(home: &str) -> PathBuf {
    PathBuf::from(home)
        .join(".claude")
        .join("commands")
        .join(format!("{TCM_SERVER}.md"))
}

pub fn command_markdown() -> String {
    // Built from explicit lines, NOT from `\n\` string continuations: Rust
    // drops the leading whitespace of the line after a continuation, which
    // silently un-indents everything. Harmless in the markdown body, fatal
    // in the YAML frontmatter - and the description is what shows in the
    // command picker. Single-line description for the same reason: nothing
    // to indent, nothing to get wrong.
    let lines: Vec<String> = vec![
        "---".into(),
        format!("name: {TCM_SERVER}"),
        "description: Write, review or bulk-edit Azure DevOps test cases with Test Case Manager"
            .into(),
        "argument-hint: \"[PBI id, or what the cases should cover]\"".into(),
        "---".into(),
        String::new(),
        COMMAND_MARKER.into(),
        String::new(),
        format!(
            "Write Azure DevOps test cases using the `{TCM_SERVER}` MCP server, which is connected \
to a running copy of Test Case Manager signed in to a real organization."
        ),
        String::new(),
        "What the developer asked for: $ARGUMENTS".into(),
        String::new(),
        "**Call `get_writing_guide` first and do what it says.** It is generated live from the org \
and project the developer currently has open - the allowed Module values, the tags the project \
already uses, the required format and the order to call things in. It is the instruction, not a \
summary of one, and it changes per project. Do not work from memory."
            .into(),
        String::new(),
        "Two things worth knowing before you read it:".into(),
        String::new(),
        "- **You never write to Azure DevOps.** Every tool here reads or transforms. You produce a \
JSON file; the developer reviews it in the app and decides what gets created."
            .into(),
        "- **Ask before assuming.** `begin_test_case_writing` returns the questions to put to the \
developer - where the file goes, which specs are authoritative, what is out of scope. Those are \
theirs to answer. Answer the file-location one and the app starts watching that path, so what you \
write imports itself."
            .into(),
        String::new(),
        "If a tool reports that the app is not signed in, say so and stop - the developer has to \
sign in to Test Case Manager first."
            .into(),
    ];
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Where the Claude Code CLI actually lives, in the order to try.
///
/// Deliberately NOT a PATH lookup. The native installer drops
/// `claude.exe` in `~/.local/bin` and puts that directory on the PATH of
/// the sessions IT starts - it is written to neither `HKCU\Environment`
/// nor the system Path. This app is launched from Explorer and inherits
/// Explorer's environment, so `cmd /C claude ...` came back
/// "'claude' is not recognized" on machines where Claude Code works
/// perfectly well in a terminal or in VS Code. Detection still said
/// "installed", because `~/.claude` exists - so the tool looked ready and
/// registering it failed.
///
/// Measured on a machine with the native install: the exe is at
/// `~/.local/bin/claude.exe`, and that directory appears in neither
/// persisted PATH.
pub fn claude_cli_candidates(home: &str, appdata: &str) -> Vec<PathBuf> {
    let home = PathBuf::from(home);
    let appdata = PathBuf::from(appdata);
    vec![
        // Native installer (what the VS Code crowd tends to have).
        home.join(".local").join("bin").join("claude.exe"),
        home.join(".local").join("bin").join("claude.cmd"),
        // npm -g: %APPDATA%\npm is normally on the persisted PATH, so
        // these were the installs that always worked.
        appdata.join("npm").join("claude.cmd"),
        appdata.join("npm").join("claude.exe"),
    ]
}

/// Inserts (or replaces) `server`'s entry under `key` in `existing_json`,
/// preserving every other entry and top-level field. Errors on unparseable
/// JSON rather than clobbering it with a fresh file.
///
/// `type: "stdio"` is written for VS Code (the `servers` key), which is
/// what its schema and the company server's own docs expect; the other
/// tools infer it. `env` is omitted entirely when empty rather than
/// written as `{}`.
pub fn merge_entry(existing_json: &str, key: &str, server: &McpServer) -> Result<String, String> {
    let mut root: serde_json::Value = serde_json::from_str(existing_json)
        .map_err(|e| format!("existing config is not valid JSON: {e}"))?;
    if !root.is_object() {
        return Err("existing config is not a JSON object".to_string());
    }
    let root_obj = root.as_object_mut().unwrap();
    let entries = root_obj
        .entry(key.to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !entries.is_object() {
        return Err(format!("\"{key}\" is not a JSON object"));
    }
    let mut entry = serde_json::Map::new();
    if key == "servers" {
        entry.insert("type".into(), serde_json::json!("stdio"));
    }
    entry.insert("command".into(), serde_json::json!(server.command));
    entry.insert("args".into(), serde_json::json!(server.args));
    if !server.env.is_empty() {
        entry.insert("env".into(), serde_json::json!(server.env));
    }
    entries
        .as_object_mut()
        .unwrap()
        .insert(server.name.clone(), serde_json::Value::Object(entry));
    serde_json::to_string_pretty(&root).map_err(|e| format!("failed to serialize config: {e}"))
}

/// Removes the named server from the tool's config, preserving everything
/// else. `Ok(None)` = the entry wasn't there (nothing to write);
/// `Ok(Some(json))` = write this back. Errors on unparseable input - never
/// fabricate a config we couldn't read.
pub fn remove_entry(
    existing_json: &str,
    key: &str,
    server_name: &str,
) -> Result<Option<String>, String> {
    let mut root: serde_json::Value = serde_json::from_str(existing_json)
        .map_err(|e| format!("existing config is not valid JSON: {e}"))?;
    let Some(entries) = root.get_mut(key).and_then(|v| v.as_object_mut()) else {
        return Ok(None);
    };
    if entries.remove(server_name).is_none() {
        return Ok(None);
    }
    serde_json::to_string_pretty(&root)
        .map(Some)
        .map_err(|e| format!("failed to serialize config: {e}"))
}

/// Writes `contents` to `path` atomically: write to a sibling temp file in
/// the same directory (so the final `rename` stays on one volume - atomic
/// on NTFS), then rename it over `path`. Cleans up the temp file if the
/// rename fails, so a crash mid-write never leaves a half-written config.
pub fn atomic_write(path: &std::path::Path, contents: &str) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("{} has no file name", path.display()))?
        .to_string_lossy();
    let tmp_path = parent.join(format!("{file_name}.tcm-tmp-{}", std::process::id()));

    std::fs::write(&tmp_path, contents)
        .map_err(|e| format!("failed to write temp file {}: {e}", tmp_path.display()))?;

    if let Err(e) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!("failed to move temp file into place at {}: {e}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod atomic_write_tests {
    use super::atomic_write;

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
            let dir = std::env::temp_dir().join(format!("tcm-ai-tools-atomic-write-{nanos}-{n}"));
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
}
