//! IPC surface for the AI-tool detect/register feature: `detect_ai_tools`
//! reads real home/appdata dirs + PATH; `register_ai_tool` writes the real
//! config files (or shells out to the `claude` CLI for claude-code). All
//! parsing/merging logic lives in `crate::ai_tools` and is unit-tested there
//! against injected paths - this file is intentionally thin I/O plumbing.

use std::path::PathBuf;
use std::process::Command;

use tauri::State;

use crate::ai_tools::{
    atomic_write, command_dir, command_files_in, config_carries, config_for, detect_in, is_installed,
    legacy_command_path, merge_entry, migrate_claude_permissions, project_command_dir,
    remove_entry, remove_legacy_command, set_claude_allow, set_cursor_allow, superseded_by,
    tcm_server, DetectedTool, McpServer, ToolSpec, CLAUDE_DB_QUERY_RULE, CURSOR_DB_QUERY_ENTRY,
    COMMAND_MARKER, LEGACY_DB_SERVER, MANAGED_SERVERS, TCM_SERVER, TOOL_SPECS,
};
use crate::db::credentials::{self, DbCredentialsForm, DbDatabase, DbSecrets};

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

/// `claude` as PATH resolves it. `where` honours PATHEXT, so this finds the
/// npm `.cmd` shim as well as a native `.exe`; `Command::new("claude")`
/// alone would only look for `claude.exe`. Falls back to the bare name so
/// the spawn fails with "not found" rather than something stranger.
fn claude_on_path() -> PathBuf {
    let mut command = Command::new("where");
    command.arg("claude");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::trim)
                .find(|l| {
                    let l = l.to_ascii_lowercase();
                    l.ends_with(".exe") || l.ends_with(".cmd") || l.ends_with(".bat")
                })
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from("claude"))
}

/// A trimmed, non-empty working directory, or None.
fn root_of(working_dir: Option<&str>) -> Option<&str> {
    working_dir.map(str::trim).filter(|s| !s.is_empty())
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
    global: bool,
) -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("failed to resolve current exe: {e}"))?
        .to_string_lossy()
        .to_string();
    register_server(&id, &tcm_server(&exe), working_dir.as_deref(), &disabled_tools, global)?;
    // A tool registered while "Run database changes without asking" is on
    // gets its own "always allow" too, the same as the ones registered
    // before the switch was turned on. Best-effort: the registration worked.
    if crate::app_settings::current().db_auto_approve {
        for o in apply_db_auto_approve_now(true, working_dir.as_deref()) {
            if !o.applied && !o.note.is_empty() {
                crate::applog::info(format!("auto-approve for {}: {}", o.tool, o.note));
            }
        }
    }
    Ok(())
}

/// What switching "Run database changes without asking" did for one
/// registered tool.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct AutoApproveOutcome {
    /// The tool's name as the AI Bridge tab shows it.
    pub tool: String,
    /// The tool's own "always allow" for `db_query` was written (or taken
    /// out, switching off).
    pub applied: bool,
    /// What the person has to do in the tool itself: when the app could
    /// not set it, why a write failed, or - set or not - a setting of the
    /// tool's own it also depends on. Empty when there is nothing to do.
    pub note: String,
}

/// Switch "Run database changes without asking" on or off: keep the choice,
/// then write (or take out) each registered tool's own "always allow" for
/// `db_query`. Answers per tool, so the AI Bridge tab can say which tools
/// were set and which the person still has to set themselves.
#[tauri::command]
#[specta::specta]
pub fn set_db_auto_approve(on: bool, working_dir: Option<String>) -> Result<Vec<AutoApproveOutcome>, String> {
    crate::app_settings::update(|s| s.db_auto_approve = on)?;
    Ok(apply_db_auto_approve_now(on, working_dir.as_deref()))
}

/// Where a tool keeps the "always allow" the app can write: a file per
/// repository and one for the machine, and how an entry goes in or out.
struct AllowFile {
    project: fn(&str) -> PathBuf,
    user: fn() -> PathBuf,
    merge: fn(&str, &str, bool) -> Result<Option<String>, String>,
    entry: &'static str,
}

/// Claude Code: `settings.local.json` in the repository, which Claude Code
/// keeps out of source control - letting a tool run unasked is this
/// person's choice, not the repository's - and the user settings for a
/// machine-wide registration. One `permissions.allow` rule.
const CLAUDE_ALLOW: AllowFile = AllowFile {
    project: |root| PathBuf::from(root).join(".claude").join("settings.local.json"),
    user: || PathBuf::from(home_dir()).join(".claude").join("settings.json"),
    merge: set_claude_allow,
    entry: CLAUDE_DB_QUERY_RULE,
};

/// Cursor: `permissions.json` beside its `mcp.json`, in the repository or
/// the home folder. One `mcpAllowlist` entry.
const CURSOR_ALLOW: AllowFile = AllowFile {
    project: |root| PathBuf::from(root).join(".cursor").join("permissions.json"),
    user: || PathBuf::from(home_dir()).join(".cursor").join("permissions.json"),
    merge: set_cursor_allow,
    entry: CURSOR_DB_QUERY_ENTRY,
};

/// The tools whose "always allow" for one tool is a file the app can
/// write. The others keep it only in their own windows - or, for VS Code,
/// only as a switch that approves EVERY tool, which this is not - so for
/// them the app says what to do instead.
fn allow_file(id: &str) -> Option<&'static AllowFile> {
    match id {
        "claude-code" => Some(&CLAUDE_ALLOW),
        "cursor" => Some(&CURSOR_ALLOW),
        _ => None,
    }
}

/// What to tell the person about a tool the app cannot set, or one it set
/// that needs a setting of its own as well.
fn allow_note(id: &str, name: &str) -> String {
    match id {
        "cursor" => "Cursor only uses this outside its ask-every-time run mode - check Cursor Settings > Agents".to_string(),
        "claude-desktop" => "Claude Desktop keeps this in its own window: when it asks about db_query, choose Always allow".to_string(),
        "vscode" => "VS Code keeps this in its own window: when it asks about db_query, choose to always allow it (or use Chat: Manage Tool Approval)".to_string(),
        _ => format!("{name} keeps this in its own settings: allow the {TCM_SERVER} db_query tool there"),
    }
}

/// Put the entry into (or take it out of) one file. A missing file is
/// created only to switch ON; switching off leaves a missing file missing.
fn write_allow(file: &AllowFile, path: &std::path::Path, on: bool) -> Result<(), String> {
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if !on {
                return Ok(());
            }
            String::new()
        }
        Err(e) => return Err(format!("could not read {}: {e}", path.display())),
    };
    let Some(updated) = (file.merge)(&existing, file.entry, on).map_err(|e| format!("{}: {e}", path.display()))?
    else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    atomic_write(path, &updated)
}

/// Carry the server's old-name permission rules over to its current name
/// in the Claude Code settings files the database switch writes - the
/// repository's (when registering into one) and the machine's - and in no
/// other file. Best-effort: the registration has already worked, and a
/// rule left under the old name only means one more "Allow this tool?".
fn carry_over_claude_rules(root: Option<&str>) {
    let mut paths = Vec::new();
    if let Some(r) = root {
        paths.push((CLAUDE_ALLOW.project)(r));
    }
    paths.push((CLAUDE_ALLOW.user)());
    for path in paths {
        if let Err(e) = carry_over_claude_rules_in(&path) {
            crate::applog::warn(format!("could not carry the old permission rules over: {e}"));
        }
    }
}

/// `migrate_claude_permissions` for one file. A missing file stays
/// missing, and a file with nothing to carry over is not rewritten. Public
/// for `tests/suite/ai_tools.rs`.
pub fn carry_over_claude_rules_in(path: &std::path::Path) -> Result<(), String> {
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("could not read {}: {e}", path.display())),
    };
    match migrate_claude_permissions(&existing).map_err(|e| format!("{}: {e}", path.display()))? {
        Some(updated) => atomic_write(path, &updated),
        None => Ok(()),
    }
}

/// Each installed tool that carries our server, and what was done for it.
///
/// A tool with a writable allow file gets the entry where the server is
/// registered: the working repository's file, the machine's, or both.
/// Switching off takes it out of both wherever it is, registered or not -
/// nothing the app wrote should outlive the switch. Every other tool that
/// carries the server is named with what the person has to do.
pub fn apply_db_auto_approve_now(on: bool, working_dir: Option<&str>) -> Vec<AutoApproveOutcome> {
    let root = root_of(working_dir);
    let mut out = Vec::new();
    for t in detect_in(&home_dir(), &appdata_dir(), &is_on_path, root) {
        let here = t.registered_servers.iter().any(|s| s == TCM_SERVER);
        let global_too = t.global_registered_servers.iter().any(|s| s == TCM_SERVER);
        let Some(file) = allow_file(&t.id) else {
            if on && t.installed && (here || global_too) {
                out.push(AutoApproveOutcome {
                    tool: t.name.clone(),
                    applied: false,
                    note: allow_note(&t.id, &t.name),
                });
            }
            continue;
        };
        let mut paths = Vec::new();
        if on {
            if here && t.scope == "project" {
                if let Some(r) = root {
                    paths.push((file.project)(r));
                }
            }
            if (here && t.scope == "global") || global_too {
                paths.push((file.user)());
            }
        } else {
            if let Some(r) = root {
                paths.push((file.project)(r));
            }
            paths.push((file.user)());
        }
        if !t.installed || (on && paths.is_empty()) {
            continue;
        }
        let result = paths.iter().try_for_each(|p| write_allow(file, p, on));
        out.push(match result {
            Ok(()) => AutoApproveOutcome {
                tool: t.name.clone(),
                applied: true,
                // Cursor reads it only in some run modes: say so, even when set.
                note: if on && t.id == "cursor" { allow_note(&t.id, &t.name) } else { String::new() },
            },
            Err(e) => AutoApproveOutcome { tool: t.name.clone(), applied: false, note: e },
        });
    }
    out
}

/// Every database the Company database card offers, as the public view:
/// who signs in and whether a password is saved, never the password or
/// the connection string.
#[tauri::command]
#[specta::specta]
pub fn db_databases(secrets: State<'_, DbSecrets>) -> Vec<DbDatabase> {
    credentials::databases(&*secrets.0)
}

#[tauri::command]
#[specta::specta]
pub fn save_db_credentials(
    secrets: State<'_, DbSecrets>,
    id: String,
    form: DbCredentialsForm,
) -> Result<DbDatabase, String> {
    credentials::save(&*secrets.0, &id, &form)
}

/// Signs in with what the form holds now (a blank password meaning the
/// saved one), or with the saved login when there is no form, and runs
/// `SELECT 1`. Nothing is saved either way.
#[tauri::command]
#[specta::specta]
pub async fn test_db_connection(
    secrets: State<'_, DbSecrets>,
    id: String,
    form: Option<DbCredentialsForm>,
) -> Result<String, String> {
    let store = &*secrets.0;
    let conn = match form {
        Some(f) => credentials::apply_form(store, &id, &f)?,
        None => credentials::resolve(store, &id)?
            .ok_or_else(|| "No login saved for this database yet.".to_string())?,
    };
    // The same lookup, and the same sentence when it fails, as the
    // assistant's database tools: a test that finds sqlcmd means they will.
    let exe = crate::db::sqlcmd_path().ok_or_else(|| crate::db::NOT_INSTALLED.to_string())?;
    credentials::test_connection_with(&crate::db::RealRunner, &exe, &conn).await
}

/// A shipped database back to its shipped login.
#[tauri::command]
#[specta::specta]
pub fn reset_db_credentials(secrets: State<'_, DbSecrets>, id: String) -> Result<DbDatabase, String> {
    credentials::reset(&*secrets.0, &id)
}

/// Every saved login off this machine - part of "Forget them".
#[tauri::command]
#[specta::specta]
pub fn forget_db_credentials(secrets: State<'_, DbSecrets>) -> Result<(), String> {
    credentials::forget_all(&*secrets.0)
}

/// The one-time move of a connection string the webview kept before
/// databases had ids. Answers the id the card should now select.
#[tauri::command]
#[specta::specta]
pub fn import_legacy_db_connection(
    secrets: State<'_, DbSecrets>,
    connection_string: String,
) -> Result<String, String> {
    credentials::import_legacy(&*secrets.0, &connection_string)
}

/// Removes the separate database server an earlier version of this app
/// registered (`LEGACY_DB_SERVER`) from the tool's config: the repository's
/// when one is set and the tool has one, else the global one - the same
/// removal `unregister_ai_tool` does for our own server, and nothing else in
/// the config is touched. Nothing registers that server any more; the AI
/// Bridge tab calls this, quietly, for a tool whose scan still lists it.
/// Off the main thread: for Claude Code it runs the `claude` CLI, and it
/// fires on opening the tab rather than on a click, so a slow CLI must not
/// stall the window.
#[tauri::command]
#[specta::specta]
pub async fn remove_legacy_db_server(
    id: String,
    working_dir: Option<String>,
    global: bool,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || remove_legacy_db_server_now(&id, working_dir.as_deref(), global))
        .await
        .map_err(|e| format!("could not remove the old database server: {e}"))?
}

/// The removal itself, on the calling thread. Public for
/// `tests/suite/ai_tools.rs`.
pub fn remove_legacy_db_server_now(id: &str, working_dir: Option<&str>, global: bool) -> Result<(), String> {
    unregister_server(id, LEGACY_DB_SERVER, working_dir, global)
}

/// The repository a registration for `spec` targets: a tool with a project
/// config needs one and refuses without (the UI never offers that - the
/// AI Bridge tab is gated on the repository); a tool without one ignores it.
/// Public for `tests/suite/ai_tools.rs` - its only caller writes real config files.
pub fn project_root<'a>(
    spec: &ToolSpec,
    working_dir: Option<&'a str>,
    global: bool,
) -> Result<Option<&'a str>, String> {
    // Machine-wide by explicit choice (Settings allows it, the AI Bridge
    // card picked it): every tool goes to its global config, the way
    // registering worked before per-repo scoping. Explicit, so a missing
    // repository can never turn into a global registration by accident.
    if global {
        return Ok(None);
    }
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

/// Refuse a tool that isn't installed, pick the repository or global
/// target, then either shell out to the claude CLI
/// or merge into the tool's JSON config. A repository registration also
/// retires the app's own global copies - a user-scope server of the same
/// name would shadow the project one, and `/tcm:*` twice in the picker is
/// exactly the confusion per-repo scoping removes.
fn register_server(
    id: &str,
    server: &McpServer,
    working_dir: Option<&str>,
    disabled: &[String],
    global: bool,
) -> Result<(), String> {
    let spec = TOOL_SPECS
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("unknown AI tool id: {id}"))?;

    if !is_installed(spec, &home_dir(), &appdata_dir(), &is_on_path) {
        return Err(format!("{} is not installed", spec.name));
    }
    let root = project_root(spec, working_dir, global)?;

    if spec.id == "claude-code" {
        // `project_root` gives None here only for the machine-wide choice:
        // user scope and the global command set, exactly what registering
        // did before per-repo scoping.
        let Some(r) = root else {
            register_claude_code_global(server)?;
            retire_superseded_claude_now(&server.name, "user", None);
            if server.name == TCM_SERVER {
                carry_over_claude_rules(None);
                if let Err(e) = write_commands_in(&command_dir(&home_dir()), disabled) {
                    crate::applog::warn(format!("could not write the Claude Code commands: {e}"));
                }
            }
            return Ok(());
        };
        register_claude_code_in(r, server)?;
        retire_superseded_claude_now(&server.name, "project", Some(r));
        if server.name == TCM_SERVER {
            carry_over_claude_rules(Some(r));
            // Best-effort, and deliberately after the server is in: a
            // command pointing at tools that are not registered would be
            // worse than no command. A failure here does not undo a
            // registration that worked.
            if let Err(e) = write_commands_in(&project_command_dir(r), disabled) {
                crate::applog::warn(format!("could not write the Claude Code commands: {e}"));
            }
        }
        retire_global_with_superseded(spec, &server.name, r);
        return Ok(());
    }

    // `merge_entry` also takes any name this one supersedes out of the
    // same config.
    let (config_path, key, _scope) = config_for(spec, &home_dir(), &appdata_dir(), root);
    merge_into_file(&config_path, key, server)?;
    if let Some(r) = root {
        retire_global_with_superseded(spec, &server.name, r);
    }
    Ok(())
}

/// `retire_global` for the server just registered into `root`, and for
/// each name it supersedes that the tool's global config still carries -
/// an old-name copy there would shadow the repository's as surely as a
/// current one. Checked first so no CLI runs for a name that is not there.
fn retire_global_with_superseded(spec: &ToolSpec, server_name: &str, root: &str) {
    retire_global(spec, server_name, Some(root));
    let (global_path, key, _) = config_for(spec, &home_dir(), &appdata_dir(), None);
    for old in superseded_by(server_name) {
        if config_carries(&global_path, key, old) {
            retire_global(spec, old, Some(root));
        }
    }
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
pub fn unregister_ai_tool(
    id: String,
    working_dir: Option<String>,
    global: bool,
) -> Result<(), String> {
    unregister_server(&id, TCM_SERVER, working_dir.as_deref(), global)?;
    // The server's old name is ours too, so it goes with it - only where
    // that same config still carries it, so no CLI runs for nothing.
    // Best-effort: what was asked for, the current entry, is gone.
    if let Some(spec) = TOOL_SPECS.iter().find(|s| s.id == id) {
        let root = unregister_root(spec, working_dir.as_deref(), global);
        let (path, key, _) = config_for(spec, &home_dir(), &appdata_dir(), root);
        for old in superseded_by(TCM_SERVER) {
            if config_carries(&path, key, old) {
                if let Err(e) = unregister_server(&id, old, working_dir.as_deref(), global) {
                    crate::applog::warn(format!("could not remove the old {old} registration: {e}"));
                }
            }
        }
    }
    Ok(())
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

/// The repository an unregister for `spec` acts on, or None for the global
/// config. The machine-wide choice removes the global entry even when a
/// repository happens to be set - the row the user clicked showed the
/// global state, so that is the one to act on.
fn unregister_root<'a>(spec: &ToolSpec, working_dir: Option<&'a str>, global: bool) -> Option<&'a str> {
    match (global, spec.project_config, root_of(working_dir)) {
        (false, Some(_), Some(r)) => Some(r),
        _ => None,
    }
}

/// Removes a server from the repository's config when one is set and the
/// tool has such a config, else from the global one. No installed-guard:
/// if a config still carries an entry after the tool was uninstalled,
/// removing it is exactly what the user wants.
fn unregister_server(
    id: &str,
    server_name: &str,
    working_dir: Option<&str>,
    global: bool,
) -> Result<(), String> {
    let spec = TOOL_SPECS
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("unknown AI tool id: {id}"))?;
    let root = unregister_root(spec, working_dir, global);

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
pub fn write_commands_in(dir: &std::path::Path, disabled: &[String]) -> Result<(), String> {
    // An earlier version wrote one top-level GLOBAL file. Leaving it would
    // put `/tcm-testcases` in the picker beside the namespaced set. Only
    // ours, and only when writing the global set.
    if dir == command_dir(&home_dir()) {
        remove_legacy_command(&home_dir());
    }

    std::fs::create_dir_all(dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;

    // A tool switched off loses its command; switched back on, it returns.
    let wanted = command_files_in(dir, &crate::ai_tools::effective_disabled(disabled));

    // Sweep the directory itself, not just the paths `COMMANDS` names
    // today: a command that was renamed or dropped from that const (this
    // branch trimmed 17 down to 7) still has its old `.md` file sitting in
    // every existing install, and a loop bounded by the current `COMMANDS`
    // can never see - let alone remove - a file no longer named by it.
    // Ours only: a file without the marker is somebody else's and is never
    // touched.
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let keep = wanted.iter().any(|(p, _)| *p == path);
            if !keep && matches!(std::fs::read_to_string(&path), Ok(t) if t.contains(COMMAND_MARKER)) {
                let _ = std::fs::remove_file(&path);
            }
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
        None => match run_claude_mcp_remove(&claude_on_path(), server_name, "user", None) {
            Ok(()) => Ok(()),
            Err(_) => remove_from_file(
                &PathBuf::from(home_dir()).join(".claude.json"),
                "mcpServers",
                server_name,
            ),
        },
    }
}

/// User scope, by the machine-wide choice: `claude mcp add --scope user`,
/// which applies regardless of cwd. CLI first, PATH second, and
/// `~/.claude.json` - the file that command would have written - last,
/// merged the same way every other tool registers.
fn register_claude_code_global(server: &McpServer) -> Result<(), String> {
    let via_file = || merge_into_file(&PathBuf::from(home_dir()).join(".claude.json"), "mcpServers", server);
    if let Some(cli) = claude_cli() {
        return run_claude_mcp_add(&cli, server, "user", None).or_else(|_| via_file());
    }
    match run_claude_mcp_add(&claude_on_path(), server, "user", None) {
        Ok(()) => Ok(()),
        Err(_) => via_file(),
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
        None => match run_claude_mcp_remove(&claude_on_path(), server_name, "project", Some(cwd)) {
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
    let output = mcp_remove_command(cli, server_name, scope, cwd)
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

/// The Claude Code CLI's absolute path, or None when it is not where the
/// installers put it. See `ai_tools::claude_cli_candidates` for why this
/// does not simply trust PATH.
fn claude_cli() -> Option<PathBuf> {
    crate::ai_tools::claude_cli_candidates(&home_dir(), &appdata_dir())
        .into_iter()
        .find(|p| p.is_file())
}

/// After `server_name` is registered with Claude Code in `scope`, take out
/// every name it supersedes from that same scope, so nobody is left with
/// one server under two names. Only a name `config` - the file that scope
/// lives in: `<repo>/.mcp.json` or `~/.claude.json` - actually carries, so
/// nothing runs for a name that is not there. Through the CLI at `cli`
/// (run in `cwd`, which project scope is keyed on) when there is one; from
/// `config` itself when there is not, or when the CLI fails. Public for
/// `tests/suite/ai_tools.rs`, which drives it with a probe for the CLI.
pub fn retire_superseded_claude(
    cli: Option<&std::path::Path>,
    server_name: &str,
    scope: &str,
    cwd: Option<&std::path::Path>,
    config: &std::path::Path,
) -> Result<(), String> {
    let mut first_error = None;
    for old in superseded_by(server_name) {
        if !config_carries(config, "mcpServers", old) {
            continue;
        }
        let via_cli = match cli {
            Some(c) => run_claude_mcp_remove(c, old, scope, cwd),
            None => Err("no Claude Code CLI".to_string()),
        };
        if let Err(e) = via_cli.or_else(|_| remove_from_file(config, "mcpServers", old)) {
            first_error.get_or_insert(e);
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// `retire_superseded_claude` for the real CLI and config, best-effort:
/// the registration itself has worked.
fn retire_superseded_claude_now(server_name: &str, scope: &str, root: Option<&str>) {
    let (cwd, config) = match root {
        Some(r) => (Some(std::path::Path::new(r)), std::path::Path::new(r).join(".mcp.json")),
        None => (None, PathBuf::from(home_dir()).join(".claude.json")),
    };
    let cli = claude_cli().unwrap_or_else(claude_on_path);
    if let Err(e) = retire_superseded_claude(Some(&cli), server_name, scope, cwd, &config) {
        crate::applog::warn(format!("could not remove the old name of the {server_name} registration: {e}"));
    }
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
    match run_claude_mcp_add(&claude_on_path(), server, "project", Some(cwd)) {
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
/// `missing required argument 'commandOrUrl'`. Our own server sends no env
/// pairs today; the order is kept right for any server that does.
///
/// Public for `tests/suite/ai_tools.rs` - its only caller runs the real CLI.
pub fn mcp_add_args(server: &McpServer, scope: &str) -> Vec<String> {
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

/// The `claude mcp add` process, run DIRECTLY - not through `cmd /C`.
///
/// cmd.exe parsed the command line itself, so an env value containing
/// `& | ^ < >` ran as syntax and `%VAR%` was expanded. std escapes arguments
/// for a `.cmd`/`.bat` target itself (the npm shim) and for an `.exe` the
/// normal way, and refuses an argument it cannot pass safely.
pub fn mcp_add_command(
    cli: &std::path::Path,
    server: &McpServer,
    scope: &str,
    cwd: Option<&std::path::Path>,
) -> Command {
    let mut command = Command::new(cli);
    command.args(mcp_add_args(server, scope));
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn mcp_remove_command(
    cli: &std::path::Path,
    server_name: &str,
    scope: &str,
    cwd: Option<&std::path::Path>,
) -> Command {
    let mut command = Command::new(cli);
    command.args(["mcp", "remove", "--scope", scope, server_name]);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn run_claude_mcp_add(
    cli: &std::path::Path,
    server: &McpServer,
    scope: &str,
    cwd: Option<&std::path::Path>,
) -> Result<(), String> {
    let output = mcp_add_command(cli, server, scope, cwd)
        .output()
        .map_err(|e| format!("failed to run `claude mcp add`: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("`claude mcp add` failed: {}", stderr.trim()));
    }
    Ok(())
}
