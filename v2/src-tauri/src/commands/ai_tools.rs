//! IPC surface for the AI-tool detect/register feature: `detect_ai_tools`
//! reads real home/appdata dirs + PATH; `register_ai_tool` writes the real
//! config files (or shells out to the `claude` CLI for claude-code). All
//! parsing/merging logic lives in `crate::ai_tools` and is unit-tested there
//! against injected paths - this file is intentionally thin I/O plumbing.

use std::path::PathBuf;
use std::process::Command;

use crate::ai_tools::{
    atomic_write, detect, is_installed, merge_entry, remove_entry, skill_markdown, skill_path,
    tcm_server, DetectedTool, McpServer, DB_SERVER, SKILL_MARKER, TOOL_SPECS,
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
        register_claude_code(server)?;
        // Best-effort, and deliberately after the server is in: a
        // skill pointing at tools that are not registered would be
        // worse than no skill. A failure here does not undo a
        // registration that worked.
        if server.name == crate::ai_tools::TCM_SERVER {
            if let Err(e) = write_skill() {
                crate::applog::warn(format!("could not write the Claude Code skill: {e}"));
            }
        }
        return Ok(());
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
        if server_name == crate::ai_tools::TCM_SERVER {
            if let Err(e) = remove_skill() {
                crate::applog::warn(format!("could not remove the Claude Code skill: {e}"));
            }
        }
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

/// Drop the skill next to the registration so the tools get picked up from
/// an ordinary request instead of having to be named.
///
/// Refuses to overwrite a SKILL.md this app did not write. The path is
/// predictable and shared with whatever else the user keeps in
/// `~/.claude/skills`; replacing somebody's own skill because it happens to
/// sit under our name is not a trade to make on their behalf. Same rule the
/// intake plan file follows.
fn write_skill() -> Result<(), String> {
    let path = skill_path(&home_dir());
    if let Ok(existing) = std::fs::read_to_string(&path) {
        if !existing.contains(SKILL_MARKER) {
            return Err(format!(
                "{} already exists and was not written by this app - left alone",
                path.display()
            ));
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    atomic_write(&path, &skill_markdown())
}

/// Take it away again with the registration - a skill describing tools that
/// are no longer connected is worse than none.
fn remove_skill() -> Result<(), String> {
    let path = skill_path(&home_dir());
    match std::fs::read_to_string(&path) {
        // Ours: remove the file and the directory we made for it.
        Ok(existing) if existing.contains(SKILL_MARKER) => {
            std::fs::remove_file(&path)
                .map_err(|e| format!("failed to remove {}: {e}", path.display()))?;
            if let Some(parent) = path.parent() {
                // Only if empty - remove_dir refuses otherwise, which is
                // exactly the guard wanted.
                let _ = std::fs::remove_dir(parent);
            }
            Ok(())
        }
        // Somebody else's, or not there at all.
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("failed to read {}: {e}", path.display())),
    }
}

fn unregister_claude_code(server_name: &str) -> Result<(), String> {
    // Mirrors register: resolve the CLI, fall back to PATH, and failing
    // both take the entry out of ~/.claude.json ourselves. A machine that
    // could only be registered by the config route has to be
    // unregisterable by it too, or Remove reports success and leaves the
    // server in place.
    match claude_cli() {
        Some(cli) => run_claude_mcp_remove(&cli, server_name),
        None => match run_claude_mcp_remove(&PathBuf::from("claude"), server_name) {
            Ok(()) => Ok(()),
            Err(_) => unregister_claude_code_via_config(server_name),
        },
    }
}

fn unregister_claude_code_via_config(server_name: &str) -> Result<(), String> {
    let path = PathBuf::from(home_dir()).join(".claude.json");
    let existing = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        // Nothing to remove from is the state the user asked for.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("failed to read {}: {e}", path.display())),
    };
    match remove_entry(&existing, "mcpServers", server_name)? {
        Some(updated) => atomic_write(&path, &updated),
        None => Ok(()),
    }
}

fn run_claude_mcp_remove(cli: &std::path::Path, server_name: &str) -> Result<(), String> {
    let mut command = Command::new("cmd");
    command.arg("/C");
    command.arg(cli);
    command.args(["mcp", "remove", "--scope", "user", server_name]);
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
/// The Claude Code CLI's absolute path, or None when it is not where the
/// installers put it. See `ai_tools::claude_cli_candidates` for why this
/// does not simply trust PATH.
fn claude_cli() -> Option<PathBuf> {
    crate::ai_tools::claude_cli_candidates(&home_dir(), &appdata_dir())
        .into_iter()
        .find(|p| p.is_file())
}

/// Write the server straight into `~/.claude.json`, the file
/// `claude mcp add --scope user` would have written.
///
/// The last resort, and the one that cannot fail for want of a CLI. Same
/// `merge_entry` + `atomic_write` every other tool already registers
/// through, so it preserves the rest of that file rather than replacing
/// it - and `~/.claude.json` holds a great deal more than MCP servers.
fn register_claude_code_via_config(server: &McpServer) -> Result<(), String> {
    let path = PathBuf::from(home_dir()).join(".claude.json");
    let existing = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".to_string(),
        Err(e) => return Err(format!("failed to read {}: {e}", path.display())),
    };
    let updated = merge_entry(&existing, "mcpServers", server)?;
    atomic_write(&path, &updated)
}

fn register_claude_code(server: &McpServer) -> Result<(), String> {
    // Prefer the CLI - it owns the config's schema and will keep working
    // if that schema moves - but only when we can name it absolutely.
    if let Some(cli) = claude_cli() {
        return run_claude_mcp_add(&cli, server);
    }
    // No CLI where the installers put it. `claude` may still be on PATH
    // for an install we do not know about; if it is not, edit the file
    // ourselves rather than telling the user their working Claude Code
    // is not there.
    match run_claude_mcp_add(&PathBuf::from("claude"), server) {
        Ok(()) => Ok(()),
        Err(_) => register_claude_code_via_config(server),
    }
}

fn run_claude_mcp_add(cli: &std::path::Path, server: &McpServer) -> Result<(), String> {
    // Still via `cmd /C`: the npm install is a `.cmd` shim, which cannot be
    // executed directly.
    let mut command = Command::new("cmd");
    command.arg("/C");
    command.arg(cli);
    command.args(["mcp", "add", "--scope", "user"]);
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
