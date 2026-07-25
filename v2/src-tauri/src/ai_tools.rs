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

/// What the frontend needs to render one row of the AI-tools list.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct DetectedTool {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub registered: bool,
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
            let registered = std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.get(spec.entry_key).cloned())
                .and_then(|entries| entries.get("tcm-testcases").cloned())
                .is_some();
            DetectedTool {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                installed,
                registered,
            }
        })
        .collect()
}

/// Inserts (or replaces) the `tcm-testcases` server entry under `key` in
/// `existing_json`, preserving every other entry and top-level field.
/// Errors on unparseable JSON rather than clobbering it with a fresh file.
pub fn merge_entry(existing_json: &str, key: &str, exe: &str) -> Result<String, String> {
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
    entries.as_object_mut().unwrap().insert(
        "tcm-testcases".to_string(),
        serde_json::json!({ "command": exe, "args": ["--mcp"] }),
    );
    serde_json::to_string_pretty(&root).map_err(|e| format!("failed to serialize config: {e}"))
}

/// Removes our `tcm-testcases` entry from the tool's config, preserving
/// everything else. `Ok(None)` = the entry wasn't there (nothing to write);
/// `Ok(Some(json))` = write this back. Errors on unparseable input - never
/// fabricate a config we couldn't read.
pub fn remove_entry(existing_json: &str, key: &str) -> Result<Option<String>, String> {
    let mut root: serde_json::Value = serde_json::from_str(existing_json)
        .map_err(|e| format!("existing config is not valid JSON: {e}"))?;
    let Some(entries) = root.get_mut(key).and_then(|v| v.as_object_mut()) else {
        return Ok(None);
    };
    if entries.remove("tcm-testcases").is_none() {
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
