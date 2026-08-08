//! IPC surface for the AI-tool detect/register feature: `detect_ai_tools`
//! reads real home/appdata dirs + PATH; `register_ai_tool` writes the real
//! config files (or shells out to the `claude` CLI for claude-code). All
//! parsing/merging logic lives in `crate::ai_tools` and is unit-tested there
//! against injected paths - this file is intentionally thin I/O plumbing.

use std::path::PathBuf;
use std::process::Command;

use crate::ai_tools::{
    atomic_write, command_dir, command_files, command_files_for, detect, is_installed,
    legacy_command_path,
    merge_entry, remove_entry, tcm_server, DetectedTool, McpServer, COMMAND_MARKER, DB_SERVER,
    TOOL_SPECS,
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
#[derive(serde::Serialize, serde::Deserialize, specta::Type)]
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
            return Err("pick the database MCP server first".into());
        }
        // Only a real invocation may reach the config: the resolver turns
        // an exe/dll/folder pick into command+args, and refuses anything
        // an MCP client could not launch (a bare directory was registered
        // once - the server never started, silently).
        let (command, args) = crate::ai_tools::resolve_db_command(std::path::Path::new(exe))?;
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
            command,
            args,
            env,
        })
    }
}

/// The shipped defaults for the database server form - see db_defaults.rs
/// for why shipping them is acceptable here. The frontend applies these
/// only to a form nothing was ever saved into.
#[tauri::command]
#[specta::specta]
pub fn db_server_defaults() -> DbServerConfig {
    DbServerConfig {
        exe_path: crate::db_defaults::DEFAULT_EXE_PATH.to_string(),
        db_type: crate::db_defaults::DEFAULT_DB_TYPE.to_string(),
        connection_string: crate::db_defaults::default_connection_string().to_string(),
        schema_filter: crate::db_defaults::DEFAULT_SCHEMA_FILTER.to_string(),
    }
}

#[derive(serde::Serialize, specta::Type)]
pub struct DbPresetOut {
    pub label: String,
    pub connection_string: String,
}

/// The shipped environments for the AI Bridge's preset dropdown - picking
/// one fills the form; nothing registers until the explicit click.
#[tauri::command]
#[specta::specta]
pub fn db_server_presets() -> Vec<DbPresetOut> {
    crate::db_defaults::DB_PRESETS
        .iter()
        .map(|p| DbPresetOut {
            label: p.label.to_string(),
            connection_string: p.connection_string.to_string(),
        })
        .collect()
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
        // command pointing at tools that are not registered would be
        // worse than no command. A failure here does not undo a
        // registration that worked.
        if server.name == crate::ai_tools::TCM_SERVER {
            if let Err(e) = write_command() {
                crate::applog::warn(format!("could not write the Claude Code command: {e}"));
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
            if let Err(e) = remove_command() {
                crate::applog::warn(format!("could not remove the Claude Code command: {e}"));
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

/// Drop the commands next to the registration, so the whole tool set is in
/// the picker under `tcm:` instead of a name somebody has to remember.
///
/// Refuses to overwrite a file this app did not write. The paths are
/// predictable and shared with whatever else the user keeps in
/// `~/.claude/commands`; replacing somebody's own command because it
/// happens to sit under our name is not a trade to make on their behalf.
/// Same rule the intake plan file follows.
///
/// One failure does not abandon the rest - a single unwritable file should
/// cost that command, not all ten - but the first reason is reported.
fn write_command() -> Result<(), String> {
    write_commands_for(&[])
}

/// Bring `~/.claude/commands/tcm/` into line with which tools are on.
///
/// Public so `set_bridge_context` can call it when the AI Bridge tab's
/// toggles move. A no-op unless the directory already exists: somebody who
/// never registered should not acquire a command set because they changed
/// an unrelated setting.
pub fn sync_commands(disabled: &[String]) {
    if !command_dir(&home_dir()).is_dir() {
        return;
    }
    if let Err(e) = write_commands_for(disabled) {
        crate::applog::warn(format!("could not sync the Claude Code commands: {e}"));
    }
}

fn write_commands_for(disabled: &[String]) -> Result<(), String> {
    let home = home_dir();

    // An earlier version wrote one top-level file. Leaving it would put
    // `/tcm-testcases` in the picker beside the namespaced set, pointing at
    // the same thing. Only ours is removed.
    let legacy = legacy_command_path(&home);
    if matches!(std::fs::read_to_string(&legacy), Ok(t) if t.contains(COMMAND_MARKER)) {
        let _ = std::fs::remove_file(&legacy);
    }

    let dir = command_dir(&home);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create {}: {e}", dir.display()))?;

    // A tool switched off loses its command; switched back on, it returns.
    // Only ours is removed - see the marker check below.
    let wanted = command_files_for(&home, disabled);
    for (path, _) in command_files(&home) {
        let keep = wanted.iter().any(|(p, _)| *p == path);
        if !keep && matches!(std::fs::read_to_string(&path), Ok(t) if t.contains(COMMAND_MARKER)) {
            let _ = std::fs::remove_file(&path);
        }
    }

    let mut first_error: Option<String> = None;
    for (path, contents) in wanted {
        if matches!(std::fs::read_to_string(&path), Ok(t) if !t.contains(COMMAND_MARKER)) {
            let msg = format!(
                "{} already exists and was not written by this app - left alone",
                path.display()
            );
            first_error.get_or_insert(msg);
            continue;
        }
        if let Err(e) = atomic_write(&path, &contents) {
            first_error.get_or_insert(e);
        }
    }
    match first_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Take them away again with the registration - a command pointing at tools
/// that are no longer connected is worse than none.
fn remove_command() -> Result<(), String> {
    let home = home_dir();
    for path in command_files(&home)
        .into_iter()
        .map(|(p, _)| p)
        .chain(std::iter::once(legacy_command_path(&home)))
    {
        // Ours only: a file at one of these paths that we did not write
        // belongs to the user.
        if matches!(std::fs::read_to_string(&path), Ok(t) if t.contains(COMMAND_MARKER)) {
            let _ = std::fs::remove_file(&path);
        }
    }
    // Only if empty - `remove_dir` refuses otherwise, which is exactly the
    // guard wanted when the user has put something of their own in there.
    let _ = std::fs::remove_dir(command_dir(&home));
    Ok(())
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

/// The argument order for `claude mcp add`, exactly as the CLI's docs
/// show it: NAME first, then the `-e` pairs, then `--` and the command.
///
/// The order is load-bearing, not style. The CLI's `-e/--env` option is
/// VARIADIC - it keeps consuming arguments until something option-like or
/// `--` stops it - so env pairs placed before the name swallowed the name
/// too, and the CLI then bound the server binary to `name` and reported
/// `missing required argument 'commandOrUrl'`. Only the database server
/// sends env pairs, which is why registering it was the first to break.
fn mcp_add_args(server: &McpServer) -> Vec<String> {
    let mut args = vec![
        "mcp".into(),
        "add".into(),
        "--scope".into(),
        "user".into(),
        server.name.clone(),
    ];
    for (k, v) in &server.env {
        args.push("-e".into());
        args.push(format!("{k}={v}"));
    }
    args.push("--".into());
    args.push(server.command.clone());
    args.extend(server.args.iter().cloned());
    args
}

fn run_claude_mcp_add(cli: &std::path::Path, server: &McpServer) -> Result<(), String> {
    // Still via `cmd /C`: the npm install is a `.cmd` shim, which cannot be
    // executed directly.
    let mut command = Command::new("cmd");
    command.arg("/C");
    command.arg(cli);
    command.args(mcp_add_args(server));
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let args = mcp_add_args(&server);

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
            mcp_add_args(&server),
            vec!["mcp", "add", "--scope", "user", "tcm-testcases", "--", "v2.exe", "--mcp"]
        );
    }
}
