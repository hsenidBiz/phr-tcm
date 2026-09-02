//! IPC surface for the AI-tool detect/register feature: `detect_ai_tools`
//! reads real home/appdata dirs + PATH; `register_ai_tool` writes the real
//! config files (or shells out to the `claude` CLI for claude-code). All
//! parsing/merging logic lives in `crate::ai_tools` and is unit-tested there
//! against injected paths - this file is intentionally thin I/O plumbing.

use std::path::PathBuf;
use std::process::Command;

use crate::ai_tools::{
    atomic_write, command_dir, command_files_in, config_for, detect_in, is_installed,
    legacy_command_path, merge_entry, project_command_dir, remove_entry, tcm_server,
    DetectedTool, McpServer, ToolSpec, COMMAND_MARKER, DB_SERVER, MANAGED_SERVERS, TCM_SERVER,
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

/// A trimmed, non-empty working directory, or None.
fn root_of(working_dir: Option<&str>) -> Option<&str> {
    working_dir.map(str::trim).filter(|s| !s.is_empty())
}

/// `path` relative to `root`, forward-slashed - the shape
/// `crate::workspace::exclude_locally` wants. None when `path` is not
/// actually under `root`, which the caller treats as "cannot exclude".
fn project_relative(root: &str, path: &std::path::Path) -> Option<String> {
    path.strip_prefix(std::path::Path::new(root))
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}

#[tauri::command]
#[specta::specta]
pub fn detect_ai_tools(working_dir: Option<String>) -> Vec<DetectedTool> {
    detect_in(&home_dir(), &appdata_dir(), &is_on_path, root_of(working_dir.as_deref()))
}

/// `disabled_tools` is the AI Bridge tab's current on/off set: registering
/// writes the repository's command files, and writing them from an empty
/// set would hand back the commands for tools the user has switched off.
#[tauri::command]
#[specta::specta]
pub fn register_ai_tool(
    id: String,
    working_dir: Option<String>,
    disabled_tools: Vec<String>,
) -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("failed to resolve current exe: {e}"))?
        .to_string_lossy()
        .to_string();
    // The warning `register_server` can return is about the database
    // server's connection string; ours carries no secret, so there is
    // nothing to say here.
    register_server(&id, &tcm_server(&exe), working_dir.as_deref(), &disabled_tools).map(|_| ())
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

/// `Ok(Some(warning))` when the registration worked but the connection
/// string is somewhere git can carry it away - the UI shows that instead of
/// the plain success toast. `Ok(None)` = registered and excluded.
#[tauri::command]
#[specta::specta]
pub fn register_db_server(
    id: String,
    config: DbServerConfig,
    working_dir: Option<String>,
) -> Result<Option<String>, String> {
    // No command files are written for the database server, so the disabled
    // set is irrelevant here.
    register_server(&id, &config.to_server()?, working_dir.as_deref(), &[])
}

#[tauri::command]
#[specta::specta]
pub fn unregister_db_server(id: String, working_dir: Option<String>) -> Result<(), String> {
    unregister_server(&id, DB_SERVER, working_dir.as_deref())
}

/// The repository a registration for `spec` targets: a tool with a project
/// config needs one and refuses without (the UI never offers that - the
/// AI Bridge tab is gated on the repository); a tool without one ignores it.
fn project_root<'a>(spec: &ToolSpec, working_dir: Option<&'a str>) -> Result<Option<&'a str>, String> {
    match (spec.project_config, root_of(working_dir)) {
        (Some(_), Some(r)) => Ok(Some(r)),
        (Some(_), None) => Err(format!(
            "pick a working repository first - {} registers per repository",
            spec.name
        )),
        (None, _) => Ok(None),
    }
}

/// Merge `server` into the JSON config at `path` (created if absent).
fn merge_into_file(path: &std::path::Path, key: &str, server: &McpServer) -> Result<(), String> {
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".to_string(),
        Err(e) => return Err(format!("failed to read {}: {e}", path.display())),
    };
    let merged = merge_entry(&existing, key, server)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    atomic_write(path, &merged)
}

/// Remove `name` from the JSON config at `path`. Missing file or entry is
/// a clean no-op - the state the caller asked for.
fn remove_from_file(path: &std::path::Path, key: &str, name: &str) -> Result<(), String> {
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("failed to read {}: {e}", path.display())),
    };
    match remove_entry(&existing, key, name)? {
        Some(updated) => atomic_write(path, &updated),
        None => Ok(()),
    }
}

/// Shared by both servers: refuse a tool that isn't installed, pick the
/// repository or global target, then either shell out to the claude CLI
/// or merge into the tool's JSON config. A repository registration also
/// retires the app's own global copies - a user-scope server of the same
/// name would shadow the project one, and `/tcm:*` twice in the picker is
/// exactly the confusion per-repo scoping removes.
fn register_server(
    id: &str,
    server: &McpServer,
    working_dir: Option<&str>,
    disabled: &[String],
) -> Result<Option<String>, String> {
    let spec = TOOL_SPECS
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("unknown AI tool id: {id}"))?;

    if !is_installed(spec, &home_dir(), &appdata_dir(), &is_on_path) {
        return Err(format!("{} is not installed", spec.name));
    }
    let root = project_root(spec, working_dir)?;

    if spec.id == "claude-code" {
        // `project_root` guarantees Some for a tool with a project config.
        let r = root.ok_or_else(|| "pick a working repository first".to_string())?;
        register_claude_code_in(r, server)?;
        if server.name == TCM_SERVER {
            // Best-effort, and deliberately after the server is in: a
            // command pointing at tools that are not registered would be
            // worse than no command. A failure here does not undo a
            // registration that worked.
            if let Err(e) = write_commands_in(&project_command_dir(r), disabled) {
                crate::applog::warn(format!("could not write the Claude Code commands: {e}"));
            }
        }
        let warning = if server.name == DB_SERVER {
            // The connection string is in the config now; keep every
            // project-scoped tool's config out of `git status` for this
            // checkout (owner's decision - the repo's .gitignore is not
            // ours to edit), not just Claude Code's - see `exclude_db_config`.
            exclude_db_config(r, ".mcp.json")
        } else {
            None
        };
        retire_global(spec, &server.name, Some(r));
        return Ok(warning);
    }

    let (config_path, key, _scope) = config_for(spec, &home_dir(), &appdata_dir(), root);
    merge_into_file(&config_path, key, server)?;
    let mut warning = None;
    if let Some(r) = root {
        if server.name == DB_SERVER {
            match project_relative(r, &config_path) {
                Some(rel) => warning = exclude_db_config(r, &rel),
                None => crate::applog::warn(format!(
                    "could not exclude {} locally - not inside {r}",
                    config_path.display()
                )),
            }
        }
        retire_global(spec, &server.name, Some(r));
    }
    Ok(warning)
}

/// Keep the DB server's connection string out of git for this project-scoped
/// config file, and say so when that could not be done: `None` means the
/// file is genuinely out of git's way, `Some(text)` is a sentence for the
/// person who just clicked Register.
///
/// Best-effort like every other retirement/exclude step here - a failure to
/// exclude does not undo a registration that already worked, it warns.
fn exclude_db_config(root: &str, rel: &str) -> Option<String> {
    let warning = match crate::workspace::exclude_locally(std::path::Path::new(root), rel) {
        Ok(crate::workspace::Exclusion::Excluded) => return None,
        Ok(crate::workspace::Exclusion::Tracked) => format!(
            "The connection string is in {rel}, which git is tracking in this repository — it \
             will be committed unless you remove the file from the index (git rm --cached {rel}) \
             or move the secret out."
        ),
        Ok(crate::workspace::Exclusion::NotGit) => format!(
            "{root} is not a git checkout, so nothing was excluded — the connection string sits \
             in {rel} in plain text."
        ),
        Err(e) => e,
    };
    crate::applog::warn(warning.clone());
    Some(warning)
}

/// Take the app's OWN global copy away once the repository carries it.
/// Only managed names ever reach here, and command files are removed only
/// when they carry our marker. Best-effort: the repository registration
/// has already succeeded, and a global leftover is a nuisance, not a fault.
///
/// `root` is the repository just registered, when there is one - needed
/// only to avoid deleting the command set that registration itself wrote.
fn retire_global(spec: &ToolSpec, server_name: &str, root: Option<&str>) {
    let result = if spec.id == "claude-code" {
        // A repository that IS the home directory makes the project and
        // global command dirs the same folder - "retiring the global copy"
        // would then delete the set just written.
        let same_dir = root.is_some_and(|r| project_command_dir(r) == command_dir(&home_dir()));
        if server_name == TCM_SERVER && !same_dir {
            let _ = remove_commands_in(&command_dir(&home_dir()));
        }
        unregister_claude_code(server_name)
    } else {
        let (path, key, _) = config_for(spec, &home_dir(), &appdata_dir(), None);
        remove_from_file(&path, key, server_name)
    };
    if let Err(e) = result {
        crate::applog::warn(format!("could not retire the global {server_name} registration: {e}"));
    }
}

/// Removes a server from the tool's config. No installed-guard: if a
/// config still carries an entry after the tool was uninstalled, removing
/// it is exactly what the user wants. Missing file/entry is a clean no-op.
#[tauri::command]
#[specta::specta]
pub fn unregister_ai_tool(id: String, working_dir: Option<String>) -> Result<(), String> {
    unregister_server(&id, TCM_SERVER, working_dir.as_deref())
}

/// Take away every global registration this app made for `id` - the copies
/// `detect_ai_tools` reports in `global_registered_servers`.
///
/// Registering into a repository already retires them, so this exists for
/// the leftovers of a machine that registered globally before per-repo
/// scoping, or of a tool registered from another repository. Same
/// best-effort contract as that automatic retirement: only our own managed
/// names, only marker-stamped command files, and a failure is logged rather
/// than surfaced - the point is to leave nothing shadowing the repository.
#[tauri::command]
#[specta::specta]
pub fn retire_global_registrations(id: String) -> Result<(), String> {
    let spec = TOOL_SPECS
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("unknown AI tool id: {id}"))?;
    for name in MANAGED_SERVERS {
        retire_global(spec, name, None);
    }
    Ok(())
}

/// Removes a server from the repository's config when one is set and the
/// tool has such a config, else from the global one. No installed-guard:
/// if a config still carries an entry after the tool was uninstalled,
/// removing it is exactly what the user wants.
fn unregister_server(id: &str, server_name: &str, working_dir: Option<&str>) -> Result<(), String> {
    let spec = TOOL_SPECS
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("unknown AI tool id: {id}"))?;
    let root = match (spec.project_config, root_of(working_dir)) {
        (Some(_), Some(r)) => Some(r),
        _ => None,
    };

    if spec.id == "claude-code" {
        return match root {
            Some(r) => {
                if server_name == TCM_SERVER {
                    let _ = remove_commands_in(&project_command_dir(r));
                }
                unregister_claude_code_in(r, server_name)
            }
            None => {
                if server_name == TCM_SERVER {
                    let _ = remove_commands_in(&command_dir(&home_dir()));
                }
                unregister_claude_code(server_name)
            }
        };
    }

    let (config_path, key, _) = config_for(spec, &home_dir(), &appdata_dir(), root);
    remove_from_file(&config_path, key, server_name)
}

/// Bring a command directory into line with which tools are on: the
/// repository's when one is set, else the global one. A no-op unless the
/// directory already exists - somebody who never registered should not
/// acquire a command set because they changed an unrelated setting.
///
/// Public so `set_bridge_context` can call it when the AI Bridge tab's
/// toggles move.
pub fn sync_commands(disabled: &[String], working_dir: Option<&str>) {
    let dir = match root_of(working_dir) {
        Some(r) => project_command_dir(r),
        None => command_dir(&home_dir()),
    };
    if !dir.is_dir() {
        return;
    }
    if let Err(e) = write_commands_in(&dir, disabled) {
        crate::applog::warn(format!("could not sync the Claude Code commands: {e}"));
    }
}

/// Write the command set into `dir`, dropping ours for any tool that is
/// switched off. Refuses to overwrite a file this app did not write - the
/// paths are predictable and shared with whatever else the user keeps
/// there. One failure does not abandon the rest, but the first reason is
/// reported.
fn write_commands_in(dir: &std::path::Path, disabled: &[String]) -> Result<(), String> {
    // An earlier version wrote one top-level GLOBAL file. Leaving it would
    // put `/tcm-testcases` in the picker beside the namespaced set. Only
    // ours, and only when writing the global set.
    if dir == command_dir(&home_dir()) {
        let legacy = legacy_command_path(&home_dir());
        if matches!(std::fs::read_to_string(&legacy), Ok(t) if t.contains(COMMAND_MARKER)) {
            let _ = std::fs::remove_file(&legacy);
        }
    }

    std::fs::create_dir_all(dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;

    // A tool switched off loses its command; switched back on, it returns.
    let wanted = command_files_in(dir, disabled);
    for (path, _) in command_files_in(dir, &[]) {
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

/// Take a command set away with its registration - a command pointing at
/// tools that are no longer connected is worse than none. Ours only.
fn remove_commands_in(dir: &std::path::Path) -> Result<(), String> {
    let mut paths: Vec<PathBuf> = command_files_in(dir, &[]).into_iter().map(|(p, _)| p).collect();
    if dir == command_dir(&home_dir()) {
        paths.push(legacy_command_path(&home_dir()));
    }
    for path in paths {
        if matches!(std::fs::read_to_string(&path), Ok(t) if t.contains(COMMAND_MARKER)) {
            let _ = std::fs::remove_file(&path);
        }
    }
    // Only if empty - `remove_dir` refuses otherwise, which is exactly the
    // guard wanted when the user has put something of their own in there.
    let _ = std::fs::remove_dir(dir);
    Ok(())
}

// ------------------------------------------------------------ claude code

/// User scope, as before per-repo scoping: still used to retire the app's
/// old global entry. CLI first, PATH second, the config file last.
fn unregister_claude_code(server_name: &str) -> Result<(), String> {
    match claude_cli() {
        Some(cli) => run_claude_mcp_remove(&cli, server_name, "user", None),
        None => match run_claude_mcp_remove(&PathBuf::from("claude"), server_name, "user", None) {
            Ok(()) => Ok(()),
            Err(_) => remove_from_file(
                &PathBuf::from(home_dir()).join(".claude.json"),
                "mcpServers",
                server_name,
            ),
        },
    }
}

/// Project scope: `claude mcp remove --scope project` run INSIDE the repo
/// (the CLI keys project scope on its cwd), falling back to editing
/// `<repo>/.mcp.json` - the file that command would have edited.
fn unregister_claude_code_in(root: &str, server_name: &str) -> Result<(), String> {
    let cwd = std::path::Path::new(root);
    let via_file = || remove_from_file(&cwd.join(".mcp.json"), "mcpServers", server_name);
    match claude_cli() {
        Some(cli) => run_claude_mcp_remove(&cli, server_name, "project", Some(cwd)).or_else(|_| via_file()),
        None => match run_claude_mcp_remove(&PathBuf::from("claude"), server_name, "project", Some(cwd)) {
            Ok(()) => Ok(()),
            Err(_) => via_file(),
        },
    }
}

fn run_claude_mcp_remove(
    cli: &std::path::Path,
    server_name: &str,
    scope: &str,
    cwd: Option<&std::path::Path>,
) -> Result<(), String> {
    let mut command = Command::new("cmd");
    command.arg("/C");
    command.arg(cli);
    command.args(["mcp", "remove", "--scope", scope, server_name]);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
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

/// Project scope: `claude mcp add --scope project` run INSIDE the repo -
/// the CLI writes `<cwd>/.mcp.json`. Prefer the CLI (it owns the schema),
/// try PATH when it is not where the installers put it, and edit
/// `<repo>/.mcp.json` ourselves as the last resort, with the same
/// `merge_entry` every other tool registers through.
fn register_claude_code_in(root: &str, server: &McpServer) -> Result<(), String> {
    let cwd = std::path::Path::new(root);
    let via_file = || merge_into_file(&cwd.join(".mcp.json"), "mcpServers", server);
    if let Some(cli) = claude_cli() {
        return run_claude_mcp_add(&cli, server, "project", Some(cwd)).or_else(|_| via_file());
    }
    match run_claude_mcp_add(&PathBuf::from("claude"), server, "project", Some(cwd)) {
        Ok(()) => Ok(()),
        Err(_) => via_file(),
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
fn mcp_add_args(server: &McpServer, scope: &str) -> Vec<String> {
    let mut args = vec![
        "mcp".into(),
        "add".into(),
        "--scope".into(),
        scope.into(),
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

fn run_claude_mcp_add(
    cli: &std::path::Path,
    server: &McpServer,
    scope: &str,
    cwd: Option<&std::path::Path>,
) -> Result<(), String> {
    // Still via `cmd /C`: the npm install is a `.cmd` shim, which cannot be
    // executed directly.
    let mut command = Command::new("cmd");
    command.arg("/C");
    command.arg(cli);
    command.args(mcp_add_args(server, scope));
    if let Some(dir) = cwd {
        command.current_dir(dir);
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
}
