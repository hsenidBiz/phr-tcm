//! IPC surface for the AI-tool detect/register feature: `detect_ai_tools`
//! reads real home/appdata dirs + PATH; `register_ai_tool` writes the real
//! config files (or shells out to the `claude` CLI for claude-code). All
//! parsing/merging logic lives in `crate::ai_tools` and is unit-tested there
//! against injected paths - this file is intentionally thin I/O plumbing.

use std::path::PathBuf;
use std::process::Command;

use crate::ai_tools::{
    atomic_write, detect, is_installed, merge_entry, remove_entry, tcm_server, DetectedTool,
    McpServer, DB_SERVER, TOOL_SPECS,
};

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
    let exe = std::env::current_exe()
        .map_err(|e| format!("failed to resolve current exe: {e}"))?
        .to_string_lossy()
        .to_string();
    register_server(&id, &tcm_server(&exe))
}

/// The company's SQL Server MCP server, registered beside ours so an
/// assistant can read the schema and the test cases in one session. The
/// server itself is configured entirely through environment variables
/// (see its README); we only place them in the tool's config.
#[derive(serde::Deserialize, specta::Type)]
pub struct DbServerConfig {
    /// Path to the built PeoplesHR.DBMCPServer.exe.
    pub exe_path: String,
    /// "mssql" or "sqlserver".
    pub db_type: String,
    pub connection_string: String,
    /// Comma-separated; blank means the server's own default (dbo).
    pub schema_filter: String,
}

impl DbServerConfig {
    fn to_server(&self) -> Result<McpServer, String> {
        let exe = self.exe_path.trim();
        if exe.is_empty() {
            return Err("pick the PeoplesHR.DBMCPServer.exe first".into());
        }
        if !std::path::Path::new(exe).is_file() {
            return Err(format!("{exe} does not exist"));
        }
        if self.db_type.trim().is_empty() {
            return Err("DB_TYPE is required".into());
        }
        if self.connection_string.trim().is_empty() {
            return Err("CONNECTION_STRING is required".into());
        }
        let mut env = std::collections::BTreeMap::new();
        env.insert("DB_TYPE".to_string(), self.db_type.trim().to_string());
        env.insert(
            "CONNECTION_STRING".to_string(),
            self.connection_string.trim().to_string(),
        );
        // Omitted entirely when blank, so the server applies its default
        // rather than being handed an empty filter.
        if !self.schema_filter.trim().is_empty() {
            env.insert(
                "SCHEMA_FILTER".to_string(),
                self.schema_filter.trim().to_string(),
            );
        }
        Ok(McpServer {
            name: DB_SERVER.to_string(),
            command: exe.to_string(),
            args: vec![],
            env,
        })
    }
}

#[tauri::command]
#[specta::specta]
pub fn register_db_server(id: String, config: DbServerConfig) -> Result<(), String> {
    register_server(&id, &config.to_server()?)
}

#[tauri::command]
#[specta::specta]
pub fn unregister_db_server(id: String) -> Result<(), String> {
    unregister_server(&id, DB_SERVER)
}

/// Shared by both servers: refuse a tool that isn't installed, then either
/// shell out to the claude CLI or merge into the tool's JSON config.
fn register_server(id: &str, server: &McpServer) -> Result<(), String> {
    let spec = TOOL_SPECS
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("unknown AI tool id: {id}"))?;

    if !is_installed(spec, &home_dir(), &appdata_dir(), &is_on_path) {
        return Err(format!("{} is not installed", spec.name));
    }

    if spec.id == "claude-code" {
        return register_claude_code(server);
    }

    let config_path: PathBuf = (spec.config_path)(&home_dir(), &appdata_dir());
    let existing = match std::fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".to_string(),
        Err(e) => return Err(format!("failed to read {}: {e}", config_path.display())),
    };
    let merged = merge_entry(&existing, spec.entry_key, server)?;
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    atomic_write(&config_path, &merged)
}

/// Removes a server from the tool's config. No installed-guard: if a
/// config still carries an entry after the tool was uninstalled, removing
/// it is exactly what the user wants. Missing file/entry is a clean no-op.
#[tauri::command]
#[specta::specta]
pub fn unregister_ai_tool(id: String) -> Result<(), String> {
    unregister_server(&id, crate::ai_tools::TCM_SERVER)
}

fn unregister_server(id: &str, server_name: &str) -> Result<(), String> {
    let spec = TOOL_SPECS
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("unknown AI tool id: {id}"))?;

    if spec.id == "claude-code" {
        return unregister_claude_code(server_name);
    }

    let config_path: PathBuf = (spec.config_path)(&home_dir(), &appdata_dir());
    let existing = match std::fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("failed to read {}: {e}", config_path.display())),
    };
    match remove_entry(&existing, spec.entry_key, server_name)? {
        Some(updated) => atomic_write(&config_path, &updated),
        None => Ok(()),
    }
}

fn unregister_claude_code(server_name: &str) -> Result<(), String> {
    let mut command = Command::new("cmd");
    command.args(["/C", "claude", "mcp", "remove", "--scope", "user", server_name]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|e| format!("failed to run `claude mcp remove`: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let msg = stderr.trim();
        // Already gone = the state the user asked for.
        if msg.contains("not found") || msg.contains("No MCP server") {
            return Ok(());
        }
        return Err(format!("`claude mcp remove` failed: {msg}"));
    }
    Ok(())
}

/// Registers via `claude mcp add --scope user`, so it applies regardless
/// of the app's cwd. `claude` is a `.cmd` shim on Windows, hence `cmd /C`.
/// Environment pairs go through `-e`, which is how the CLI carries the
/// database server's connection settings.
fn register_claude_code(server: &McpServer) -> Result<(), String> {
    let mut command = Command::new("cmd");
    command.args(["/C", "claude", "mcp", "add", "--scope", "user"]);
    for (k, v) in &server.env {
        command.arg("-e");
        command.arg(format!("{k}={v}"));
    }
    command.arg(&server.name);
    command.arg("--");
    command.arg(&server.command);
    for a in &server.args {
        command.arg(a);
    }
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
