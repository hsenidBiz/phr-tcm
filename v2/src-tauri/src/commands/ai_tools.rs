//! IPC surface for the AI-tool detect/register feature: `detect_ai_tools`
//! reads real home/appdata dirs + PATH; `register_ai_tool` writes the real
//! config files (or shells out to the `claude` CLI for claude-code). All
//! parsing/merging logic lives in `crate::ai_tools` and is unit-tested there
//! against injected paths - this file is intentionally thin I/O plumbing.

use std::path::PathBuf;
use std::process::Command;

use crate::ai_tools::{atomic_write, detect, is_installed, merge_entry, DetectedTool, TOOL_SPECS};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn home_dir() -> String {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default()
}

fn appdata_dir() -> String {
    std::env::var("APPDATA").unwrap_or_default()
}

/// `where <cmd>` on Windows - exits 0 with a match on stdout, non-zero
/// when nothing is found. Runs with no console window flash.
fn is_on_path(cmd: &str) -> bool {
    let mut command = Command::new("where");
    command.arg(cmd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.output().map(|o| o.status.success()).unwrap_or(false)
}

#[tauri::command]
#[specta::specta]
pub fn detect_ai_tools() -> Vec<DetectedTool> {
    detect(&home_dir(), &appdata_dir(), &is_on_path)
}

#[tauri::command]
#[specta::specta]
pub fn register_ai_tool(id: String) -> Result<(), String> {
    let spec = TOOL_SPECS
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("unknown AI tool id: {id}"))?;

    if !is_installed(spec, &home_dir(), &appdata_dir(), &is_on_path) {
        return Err(format!("{} is not installed", spec.name));
    }

    let exe = std::env::current_exe()
        .map_err(|e| format!("failed to resolve current exe: {e}"))?
        .to_string_lossy()
        .to_string();

    if spec.id == "claude-code" {
        return register_claude_code(&exe);
    }

    let config_path: PathBuf = (spec.config_path)(&home_dir(), &appdata_dir());
    let existing = match std::fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".to_string(),
        Err(e) => return Err(format!("failed to read {}: {e}", config_path.display())),
    };
    let merged = merge_entry(&existing, spec.entry_key, &exe)?;
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    atomic_write(&config_path, &merged)
}

/// Registers via `claude mcp add --scope user`, so it applies regardless
/// of the app's cwd. `claude` is a `.cmd` shim on Windows, hence `cmd /C`.
fn register_claude_code(exe: &str) -> Result<(), String> {
    let mut command = Command::new("cmd");
    command.args([
        "/C",
        "claude",
        "mcp",
        "add",
        "--scope",
        "user",
        "tcm-testcases",
        "--",
        exe,
        "--mcp",
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|e| format!("failed to run `claude mcp add`: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("`claude mcp add` failed: {}", stderr.trim()));
    }
    Ok(())
}
