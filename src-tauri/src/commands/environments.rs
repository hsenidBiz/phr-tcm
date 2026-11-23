//! The environments, for the AI Bridge tab's Environment card and its
//! dialog. Each command is a thin wrapper over a `*_with` function that
//! takes the root and the secret store, so the tests drive the same body
//! against `MemoryStore`.
//!
//! The default password goes IN through `env_set_default_password` and never
//! comes back out: a view carries `has_default_password` and nothing more.

use crate::ai_bridge::BridgeContext;
use crate::db::credentials::{self, DbSecrets, SecretStore};
use crate::environments::{self, EnvFile, Environment};
use std::path::Path;
use std::sync::Mutex;
use tauri::State;

/// One environment as the webview sees it. There is deliberately no
/// password field, not even an empty one.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct EnvView {
    pub id: String,
    pub name: String,
    pub start_url: String,
    pub allowed_origins: Vec<String>,
    pub db_id: String,
    pub test_environment: bool,
    pub test_prefix: String,
    pub has_default_password: bool,
    /// "Run database changes without asking", this environment's own.
    pub db_auto_approve: bool,
    /// The SELECT that reads when logins' passwords fall due; empty when not
    /// checked.
    pub password_expiry_query: String,
    /// The SELECT that reads whether a login is signed in elsewhere; empty
    /// when not checked.
    pub active_session_query: String,
    /// "Reset expired passwords", this environment's own. Matters only
    /// while it is marked a test environment.
    pub reset_expired_passwords: bool,
    /// Where this environment's logins and passwords are looked up.
    pub account_lookup: crate::autorun::account_lookup::AccountLookup,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct EnvListView {
    pub active: String,
    /// The Playwright export environment's id (the active one while none is
    /// named). Changed only by `pw_set_export_env`.
    pub playwright_export: String,
    pub environments: Vec<EnvView>,
}

/// An environment to save. An empty `id` adds a new one.
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct EnvInput {
    pub id: String,
    pub name: String,
    pub start_url: String,
    pub allowed_origins: Vec<String>,
    pub db_id: String,
    pub test_environment: bool,
    /// Left out, an edit keeps the environment's prefix and a new
    /// environment gets the default.
    #[serde(default)]
    pub test_prefix: Option<String>,
    /// Left out, an edit keeps the environment's password expiry query.
    #[serde(default)]
    pub password_expiry_query: Option<String>,
    /// Left out, an edit keeps the environment's active session query.
    #[serde(default)]
    pub active_session_query: Option<String>,
    /// Left out, an edit keeps the environment's account lookup.
    #[serde(default)]
    pub account_lookup: Option<crate::autorun::account_lookup::AccountLookup>,
}

/// Said while an Auto Test browser holds a signed-in account: the person
/// closes it before the environment changes.
pub const SWITCH_BROWSER_SIGNED_IN: &str =
    "the environment cannot be switched while an Auto Test browser is signed in to an account - close it first";

/// Said when a switch would land in the middle of a run, recording, check
/// or Try - which would mix two environments' accounts and sessions.
pub const SWITCH_BUSY: &str =
    "the environment cannot be switched while something is being recorded or run in Auto Test - finish or cancel it first";
/// Said while an API template run holds its slot.
pub const SWITCH_TEMPLATE_RUNNING: &str =
    "the environment cannot be switched while an API template is running - wait for it to finish first";
/// Said while the supervised Auto Test browser is open: it is signed in to
/// the environment it was opened in.
pub const SWITCH_SUPERVISED_OPEN: &str =
    "the environment cannot be switched while the supervised browser is open in Auto Test - close it first";

fn has_password(store: &dyn SecretStore, id: &str) -> bool {
    match store.get(&environments::password_target(id)) {
        Ok(v) => v.is_some(),
        Err(e) => {
            crate::applog::warn(format!("Environments: could not read whether a default password is set: {e}"));
            false
        }
    }
}

fn view(store: &dyn SecretStore, file: EnvFile) -> EnvListView {
    EnvListView {
        playwright_export: crate::environments::export_id(&file).to_string(),
        active: file.active,
        environments: file
            .environments
            .into_iter()
            .map(|e| EnvView {
                has_default_password: has_password(store, &e.id),
                id: e.id,
                name: e.name,
                start_url: e.start_url,
                allowed_origins: e.allowed_origins,
                db_id: e.db_id,
                test_environment: e.test_environment,
                test_prefix: e.test_prefix,
                db_auto_approve: e.db_auto_approve,
                password_expiry_query: e.password_expiry_query,
                active_session_query: e.active_session_query,
                reset_expired_passwords: e.reset_expired_passwords,
                account_lookup: e.account_lookup,
            })
            .collect(),
    }
}

fn known_db_ids(store: &dyn SecretStore) -> Vec<String> {
    credentials::databases(store).into_iter().map(|d| d.id).collect()
}

fn must_exist(root: &Path, id: &str) -> Result<(), String> {
    if environments::load_or_init(root, None)?.environments.iter().any(|e| e.id == id) {
        Ok(())
    } else {
        Err("that environment is not there any more".to_string())
    }
}

/// `current_db` is the database the Company database card has chosen, and
/// it only seeds a NEW Default: a choice that names no database this build
/// knows is dropped (Default then takes the first shipped one), so an id
/// that names nothing is never written into an environment.
pub fn list_view(root: &Path, store: &dyn SecretStore, current_db: Option<&str>) -> Result<EnvListView, String> {
    let known = known_db_ids(store);
    let current_db = current_db.map(str::trim).filter(|d| known.iter().any(|k| k == d));
    Ok(view(store, environments::load_or_init(root, current_db)?))
}

/// `list_view`, after giving older versions' machine-wide "Run database
/// changes without asking" (`legacy_on`) to the environment active now -
/// once (`environments::adopt_legacy_auto_approve`). The first list of all
/// makes Default first, so the setting has somewhere to go.
pub fn list_adopting(
    root: &Path,
    store: &dyn SecretStore,
    current_db: Option<&str>,
    legacy_on: bool,
) -> Result<EnvListView, String> {
    let first = list_view(root, store, current_db)?;
    if environments::adopt_legacy_auto_approve(root, legacy_on)? {
        return list_view(root, store, None);
    }
    Ok(first)
}

/// What every switch refusal starts with; an address change says
/// `ADDRESS_CHANGE_REFUSED` in its place.
const SWITCH_REFUSED: &str = "the environment cannot be switched";
/// Said, with the switch's reason, when the ACTIVE environment's address
/// or allowed sites would change under a run.
pub const ADDRESS_CHANGE_REFUSED: &str = "the active environment's website address cannot be changed";

fn sites(list: &[String]) -> Vec<String> {
    list.iter().map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect()
}

/// Add or edit an environment. Changing where an environment signs in -
/// its address or allowed sites - is handled like a switch when it is the
/// ACTIVE one: refused while something records, runs, or holds the
/// supervised browser, because the rest of a run would sign in somewhere
/// else. A changed address also drops that environment's saved sessions:
/// they were made at the old address, and cookies are not port-scoped.
pub async fn save_with(root: &Path, store: &dyn SecretStore, env: EnvInput) -> Result<EnvListView, String> {
    let before = if env.id.is_empty() {
        None
    } else {
        environments::load_or_init(root, None)?.environments.into_iter().find(|e| e.id == env.id)
    };
    let test_prefix = env
        .test_prefix
        .clone()
        .or_else(|| before.as_ref().map(|b| b.test_prefix.clone()))
        .unwrap_or_else(environments::default_test_prefix);
    let password_expiry_query = env
        .password_expiry_query
        .clone()
        .or_else(|| before.as_ref().map(|b| b.password_expiry_query.clone()))
        .unwrap_or_default();
    let active_session_query = env
        .active_session_query
        .clone()
        .or_else(|| before.as_ref().map(|b| b.active_session_query.clone()))
        .unwrap_or_default();
    let account_lookup = env
        .account_lookup
        .clone()
        .or_else(|| before.as_ref().map(|b| b.account_lookup.clone()))
        .unwrap_or_default();
    let env = Environment {
        id: env.id,
        name: env.name,
        start_url: env.start_url,
        allowed_origins: env.allowed_origins,
        db_id: env.db_id,
        test_environment: env.test_environment,
        test_prefix,
        // Not the form's to set: a save keeps what the file holds
        // (`environments::save_env_with`).
        db_auto_approve: false,
        password_expiry_query,
        active_session_query,
        // Not the form's to set either (`set_reset_expired_passwords`).
        reset_expired_passwords: false,
        account_lookup,
    };
    let unmarked = before.as_ref().is_some_and(|b| b.test_environment && !env.test_environment);
    let address_moved = before.as_ref().is_some_and(|b| b.start_url.trim() != env.start_url.trim());
    let sites_moved = before.as_ref().is_some_and(|b| sites(&b.allowed_origins) != sites(&env.allowed_origins));
    if !address_moved && !sites_moved {
        let id = env.id.clone();
        let file = environments::save_env_with(root, env, || known_db_ids(store))?;
        crate::applog::info(format!("Environments: saved ({} environment(s))", file.environments.len()));
        if unmarked {
            strip_proposed_passwords(root, &id);
        }
        return Ok(view(store, file));
    }
    // Held across the check and the write, exactly as a switch holds them
    // (`set_active_with`) - which also means no switch can make this the
    // active environment halfway through.
    let refused = |e: String| e.replacen(SWITCH_REFUSED, ADDRESS_CHANGE_REFUSED, 1);
    // And across every Auto Test browser: no open begins (`opening()`), and
    // each slot's session is locked in slot order - an assistant's only
    // tried briefly, so an address that is in use is refused at once beside
    // an assistant's replay rather than after it. Another environment's
    // address is in nobody's use, so a taken slot does not refuse that save.
    let _opening = crate::commands::autorun_slots::opening().write().await;
    let slots = crate::commands::autorun::lock_every_slot().await;
    let run_slot = if environments::active_id(root)? == env.id {
        if slots.busy {
            return Err(refused(SWITCH_BUSY.to_string()));
        }
        refuse_switch(slots.any_open()).map_err(refused)?;
        Some(crate::api_templates::runner::claim().ok_or_else(|| refused(SWITCH_TEMPLATE_RUNNING.to_string()))?)
    } else {
        None
    };
    let id = env.id.clone();
    let file = environments::save_env_with(root, env, || known_db_ids(store))?;
    if address_moved {
        // `id` matched an entry the file check passed, so it is a
        // well-formed id and this path stays inside the root.
        match std::fs::remove_dir_all(crate::autorun::accounts::sessions_dir_for(root, &id)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => crate::applog::warn(format!(
                "Environments: the saved sessions of a moved environment stayed behind: {e}"
            )),
            _ => {}
        }
        // The site is another one now: what was held for the old site and
        // what its checks found do not carry over.
        crate::autorun::accounts::forget_pending_for(root, &id);
        crate::autorun::sign_in_check::forget_marks(root, &id);
    }
    drop(run_slot);
    drop(slots);
    if unmarked {
        strip_proposed_passwords(root, &id);
    }
    crate::applog::info(format!(
        "Environments: saved ({} environment(s)), one with a new {}",
        file.environments.len(),
        if address_moved { "website address" } else { "list of allowed sites" }
    ));
    Ok(view(store, file))
}

/// An environment no longer marked as a test environment keeps no proposed
/// password on disk (nor, then, in the next backup). The environment is
/// saved already, so a strip that fails is logged, not fatal: a password
/// left behind is still never used or reported outside a test environment.
fn strip_proposed_passwords(root: &Path, id: &str) {
    // And it resets no expired password any more: the switch only means
    // something on a test environment, and is off again if it is marked
    // one later.
    if let Err(e) = environments::set_reset_expired_passwords(root, id, false) {
        crate::applog::warn(format!("Environments: the reset switch of an unmarked environment stayed on: {e}"));
    }
    match environments::strip_proposed_passwords(root, id) {
        Ok(0) => {}
        Ok(n) => crate::applog::info(format!(
            "Environments: dropped {n} proposed password(s) from an environment no longer marked as a test environment"
        )),
        Err(e) => crate::applog::warn(format!(
            "Environments: the proposed passwords of an environment no longer marked as a test environment stayed behind: {e}"
        )),
    }
}

/// Removes the environment's files and its default password. A password
/// that will not go is logged, not fatal: the environment is gone already,
/// and the id is never handed out again.
pub fn remove_with(root: &Path, store: &dyn SecretStore, id: &str) -> Result<EnvListView, String> {
    let file = environments::remove_env(root, id)?;
    if let Err(e) = store.remove(&environments::password_target(id)) {
        crate::applog::warn(format!("Environments: a removed environment's default password stayed behind: {e}"));
    }
    crate::applog::info("Environments: one removed");
    Ok(view(store, file))
}

/// Whether a switch may happen now. `supervised_open` is passed in because
/// the supervised slot is behind an async lock the caller holds.
pub fn refuse_switch(supervised_open: bool) -> Result<(), String> {
    if crate::commands::autorun_record::recording_is_going() || crate::commands::autorun_replay::replay_is_running() {
        return Err(SWITCH_BUSY.to_string());
    }
    if crate::api_templates::runner::is_running() {
        return Err(SWITCH_TEMPLATE_RUNNING.to_string());
    }
    // A browser signed in to an account belongs to the environment it signed
    // in to; only the person closes it.
    if crate::autorun::lease::a_browser_is_held() {
        return Err(SWITCH_BROWSER_SIGNED_IN.to_string());
    }
    if supervised_open {
        return Err(SWITCH_SUPERVISED_OPEN.to_string());
    }
    Ok(())
}

/// Brings the app in line with an `environments.json` an import replaced:
/// the active environment's folder is selected again from disk (never made
/// from nothing), and the bridge is moved to it as a switch moves it. Answers
/// the active id and whether the bridge's writes are now held (the caller
/// ends the hold with `HeldWrites` once the locks are let go); `None` when
/// there is no environment yet. The caller holds the switch lock for writing.
pub fn settle_after_import(
    shared: &Path,
    store: &dyn SecretStore,
    bridge: Option<&Mutex<BridgeContext>>,
) -> Result<Option<(String, bool)>, String> {
    let Some(env) = environments::read_active(shared)? else { return Ok(None) };
    crate::autorun::store::reselect_from_disk()?;
    if bridge.is_none() {
        return Ok(Some((env.id, false)));
    }
    let file = move_bridge_to(shared, store, &env.id, bridge)?;
    Ok(Some((file.active, true)))
}

pub async fn set_active_with(root: &Path, store: &dyn SecretStore, id: &str) -> Result<EnvListView, String> {
    Ok(switch_with(root, store, id, None, || {}).await?.0)
}

/// What a switch answers: the list as Rust now has it, and what was done to
/// each AI tool's own "always allow" to make it match the new environment
/// (`AutoApproveOutcome`, as the Run changes without asking switch answers).
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct EnvSwitched {
    pub view: EnvListView,
    pub allow: Vec<crate::commands::ai_tools::AutoApproveOutcome>,
}

/// What `env_set_active` runs once the bridge has moved: each AI tool's own
/// "always allow" for db_query made to match the new active environment -
/// written when it runs changes without asking, taken out when it does not
/// - answering what was done for each tool.
pub fn switch_reconciler(
    root: std::path::PathBuf,
    working_dir: Option<String>,
    tools: crate::commands::ai_tools::ToolHome,
) -> impl FnOnce() -> Vec<crate::commands::ai_tools::AutoApproveOutcome> + Send + 'static {
    move || {
        let outcomes = crate::commands::ai_tools::match_allow_files_in(&root, working_dir.as_deref(), &tools);
        crate::commands::ai_tools::log_allow_outcomes(&outcomes);
        outcomes
    }
}

/// Make `id` active and move the bridge with it, in one step: the file and
/// the bridge's context change under the context's lock, which every push
/// from the webview takes too (`commands::ai_bridge::push_context_with`).
/// A request is then judged wholly by the environment before or wholly by
/// the one after - never the new database with the old switches.
///
/// The write switches are the webview's, kept per environment, so Rust
/// cannot know the new environment's: they go OFF here and come back only
/// with a push for this environment. The database and "without asking" are
/// the new environment's own at once.
pub fn move_bridge_to(
    root: &Path,
    store: &dyn SecretStore,
    id: &str,
    bridge: Option<&Mutex<BridgeContext>>,
) -> Result<EnvFile, String> {
    let mut held = bridge.map(|b| b.lock().unwrap_or_else(|e| e.into_inner()));
    let previous = environments::active_id(root)?;
    let file = environments::set_active(root, id)?;
    // Every Auto Test store now reads and writes the new environment's folder.
    // If that cannot be made, the file goes back: the list, the Auto Test
    // folder and the bridge must all name the same environment.
    // (Before app set-up there is no layout to move: tests of the bridge alone.)
    let selected = if crate::autorun::store::shared_root().is_some() {
        crate::autorun::store::select_environment(&file.active).map(|_| ())
    } else {
        Ok(())
    };
    if let Err(e) = selected {
        if let Err(back) = environments::set_active(root, &previous) {
            crate::applog::warn(format!("Environments: the switch could not be undone: {back}"));
        }
        return Err(e);
    }
    if let (Some(ctx), Some(env)) = (held.as_deref_mut(), file.environments.iter().find(|e| e.id == file.active)) {
        // Held until the caller has brought the allow files in line
        // (`release_writes`, which `switch_with` calls once they are).
        super::ai_bridge::hold_writes(ctx);
        // The new environment's switches are the webview's to say, by a
        // push for it: none is wanted until then.
        ctx.gate.wanted = None;
        ctx.gate.env_id = Some(env.id.clone());
        ctx.db_no_ask = env.db_auto_approve;
        // As the webview's switch does: a database that is not set up any
        // more is no database.
        ctx.db_id = Some(env.db_id.clone()).filter(|d| credentials::is_known(store, d));
        // Nor is its tool list known: every tool that has a switch is off
        // until the push for this environment brings its own.
        ctx.disabled_tools = crate::mcp::all_tool_names()
            .into_iter()
            .filter(|t| !crate::ai_tools::CORE_TOOLS.contains(&t.as_str()))
            .collect();
        // The network tools likewise, until their own switch comes with it.
        ctx.network_tools = false;
    }
    Ok(file)
}

/// The switch: refused while something records, runs or holds the
/// supervised browser, then `move_bridge_to`, then `after` - which the app
/// uses to make each AI tool's own "always allow" match the new
/// environment - before the answer goes back. Every write stays held off
/// from the move until `after` has finished, whether it worked or not, so
/// even a push for the new environment that lands meanwhile turns none on
/// before the allow files say what the new environment says. Answers the
/// list and what `after` answered (None when it did not finish).
pub async fn switch_with<T: Send + 'static>(
    root: &Path,
    store: &dyn SecretStore,
    id: &str,
    bridge: Option<&Mutex<BridgeContext>>,
    after: impl FnOnce() -> T + Send + 'static,
) -> Result<(EnvListView, Option<T>), String> {
    // Held across the switch, so no supervised browser, the main one or an
    // assistant's, can open halfway through it: the opening lock for writing
    // first (an open in progress finishes, a new one is refused), then every
    // slot's session in slot order. An assistant's slot is only tried
    // briefly: one that stays taken is a replay, a batch or an open under
    // way, and the switch is refused at once rather than after it.
    let _opening = crate::commands::autorun_slots::opening().write().await;
    let slots = crate::commands::autorun::lock_every_slot().await;
    if slots.busy {
        return Err(SWITCH_BUSY.to_string());
    }
    refuse_switch(slots.any_open())?;
    // The template slot is TAKEN, not just looked at, for the write: a run
    // that started between the look in `refuse_switch` and the write would
    // otherwise sign in to one environment and finish in another.
    let run_slot = crate::api_templates::runner::claim().ok_or_else(|| SWITCH_TEMPLATE_RUNNING.to_string())?;
    // The writes in flight finish first; one that starts after sees the new
    // environment. No store write happens inside this lock (see `switch_lock`).
    let file = {
        let _switching = crate::autorun::roots::switch_lock().write().unwrap_or_else(|e| e.into_inner());
        move_bridge_to(root, store, id, bridge)?
    };
    // The hold `move_bridge_to` took ends when this does, however it ends.
    let release = super::ai_bridge::HeldWrites(bridge);
    // A browser a template run kept signed in belongs to the environment
    // it signed in to. Closed off the async threads (ending a process waits
    // for it to go), and waited for while the run slot is still taken, so
    // no run in the new environment starts beside it.
    let _ = tauri::async_runtime::spawn_blocking(crate::api_templates::held::close_all).await;
    let answered = tauri::async_runtime::spawn_blocking(after).await.ok();
    drop(release);
    drop(run_slot);
    drop(slots);
    if let Some(e) = file.environments.iter().find(|e| e.id == file.active) {
        crate::applog::info(format!("Environments: switched to {}", e.name));
    }
    Ok((view(store, file), answered))
}

pub fn set_default_password_with(root: &Path, store: &dyn SecretStore, id: &str, password: &str) -> Result<(), String> {
    if password.is_empty() {
        return Err("type the default password first".to_string());
    }
    must_exist(root, id)?;
    store.put(&environments::password_target(id), password)
}

/// Switch "Reset expired passwords" for one environment. Only a test
/// environment can have it turned on.
pub fn set_reset_expired_passwords_with(root: &Path, store: &dyn SecretStore, id: &str, on: bool) -> Result<EnvListView, String> {
    must_exist(root, id)?;
    if on && !environments::load_or_init(root, None)?.environments.iter().any(|e| e.id == id && e.test_environment) {
        return Err("mark this environment as a test environment first - an expired password is reset only on one".to_string());
    }
    let file = environments::set_reset_expired_passwords(root, id, on)?;
    crate::applog::info(format!("Environments: resetting expired passwords is now {}", if on { "on" } else { "off" }));
    Ok(view(store, file))
}

pub fn clear_default_password_with(root: &Path, store: &dyn SecretStore, id: &str) -> Result<(), String> {
    must_exist(root, id)?;
    store.remove(&environments::password_target(id))
}

/// One proposed account a person picked to add. `password` is what they
/// typed; empty means "use the password the assistant proposed with it, or
/// else the environment's default password".
#[derive(Clone, serde::Deserialize, specta::Type)]
pub struct AccountInput {
    pub key: String,
    pub label: String,
    pub username: String,
    pub password: String,
}

/// Hand-written so a password cannot reach a log line through `{:?}`.
impl std::fmt::Debug for AccountInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountInput")
            .field("key", &self.key)
            .field("label", &self.label)
            .field("username", &self.username)
            .field("password", &"(hidden)")
            .finish()
    }
}

/// Said for a pick with no password typed while the environment has no
/// default password.
pub const NO_DEFAULT_PASSWORD: &str = "no default password set - type one";

/// The active environment's proposed accounts, as the webview may see
/// them: whether each carries a password, never the password. Outside a
/// test environment none counts as having one (see `usable_proposals`).
pub fn proposals_with(root: &Path) -> Result<Vec<environments::ProposedAccount>, String> {
    let env = environments::active(root)?;
    Ok(usable_proposals(root, &env)?.iter().map(|p| p.view()).collect())
}

/// One environment's proposals, with every stored password dropped unless
/// the environment is marked as a test environment now. Unmarking strips
/// them from the file; this covers one left behind by a strip that failed
/// or by a restored backup.
fn usable_proposals(root: &Path, env: &Environment) -> Result<Vec<environments::StoredProposal>, String> {
    let mut list = environments::load_proposals(root, &env.id)?;
    if !env.test_environment {
        for p in &mut list {
            p.password = None;
        }
    }
    Ok(list)
}

/// Drop the active environment's whole proposal, with any password it held.
pub fn dismiss_proposals_with(root: &Path) -> Result<(), String> {
    environments::save_proposals(root, &environments::active_id(root)?, &[])
}

/// Add the picked proposals to the active environment's accounts.
///
/// Every pick is checked first - a usable key used once, a username, and a
/// password - and one that fails refuses the whole call with nothing
/// written. The password is the one typed; else the one the assistant
/// proposed with that same key and username (it moves from the proposals
/// file into the account, never through the webview); else the
/// environment's default. A pick whose key is already an account is
/// written only when `replace` names it; otherwise it is left as it is and
/// its key returned, for the person to confirm. What was added leaves the
/// proposal, its password with it.
pub fn add_proposals_with(
    root: &Path,
    store: &dyn SecretStore,
    picks: Vec<AccountInput>,
    replace: Vec<String>,
) -> Result<Vec<String>, String> {
    use crate::autorun::accounts::{load_accounts_for, valid_key, Account};
    let env = environments::active(root)?;
    let env_id = env.id.clone();
    let accounts = load_accounts_for(root, &env_id)?;
    let proposed = usable_proposals(root, &env)?;
    let mut default: Option<Option<String>> = None;
    let mut seen = std::collections::HashSet::new();
    let mut ready: Vec<Account> = vec![];
    let mut confirm: Vec<String> = vec![];
    for p in picks {
        let key = p.key.trim().to_string();
        if !valid_key(&key) {
            return Err(format!(
                "\"{key}\" is not a usable account key - use lowercase letters, digits, dot, underscore or hyphen"
            ));
        }
        if !seen.insert(key.clone()) {
            return Err(format!("the account key \"{key}\" appears more than once"));
        }
        let username = p.username.trim().to_string();
        if username.is_empty() {
            return Err(format!("the account \"{key}\" has no username"));
        }
        if accounts.iter().any(|a| a.key == key) && !replace.iter().any(|r| r.trim() == key) {
            confirm.push(key);
            continue;
        }
        let proposal = proposed.iter().find(|q| q.key == key && q.username.trim() == username);
        let from_proposal = proposal.and_then(|q| q.password.clone());
        // Where the password came from, only when it is the proposal's: one
        // typed here, or the default, has no source.
        let mut password_source = None;
        let password = if !p.password.is_empty() {
            p.password
        } else if let Some(pw) = from_proposal {
            password_source = proposal.and_then(|q| q.password_source.clone());
            pw
        } else {
            if default.is_none() {
                let found = store
                    .get(&environments::password_target(&env_id))
                    .map_err(|e| format!("the default password could not be read: {e}"))?;
                default = Some(found.filter(|d| !d.is_empty()));
            }
            match default.as_ref().and_then(|d| d.clone()) {
                Some(d) => d,
                None => return Err(format!("\"{key}\": {NO_DEFAULT_PASSWORD}")),
            }
        };
        let label = match p.label.trim() {
            "" => key.clone(),
            l => l.to_string(),
        };
        ready.push(Account { key, label, username, password, password_source });
    }
    if ready.is_empty() {
        return Ok(confirm);
    }
    let added: Vec<String> = ready.iter().map(|a| a.key.clone()).collect();
    // Merged into the list as it is NOW, under the accounts lock, and by the
    // id it was read by: a reset or a save made meanwhile is kept, and a
    // switch in between must not put this environment's list into the next.
    crate::autorun::accounts::merge_accounts_for(root, &env_id, ready)?;
    crate::applog::info(format!("Environments: added {} proposed account(s)", added.len()));
    // The accounts are saved: a proposal that will not update is logged,
    // not a failure of the add.
    let left = environments::load_proposals(root, &env_id)
        .map(|list| list.into_iter().filter(|p| !added.contains(&p.key)).collect::<Vec<_>>())
        .and_then(|list| environments::save_proposals(root, &env_id, &list));
    if let Err(e) = left {
        crate::applog::warn(format!("Environments: the added accounts stayed among the proposed ones: {e}"));
    }
    Ok(confirm)
}

/// What a "Sync Database" did, in counts and nothing else:
/// no password and no login is in it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct FillCounts {
    /// Logins whose password is now the one in the table (it differed).
    pub updated: u32,
    /// Logins whose password already was the one in the table.
    pub unchanged: u32,
    /// Logins with no row in the table, or a row with no password.
    pub no_row: u32,
    /// Logins whose value in the table is only a hash.
    pub hash_only: u32,
    /// Of the passwords updated, how many came from a different database
    /// than the one this Fill read (the account remembers where its
    /// password came from). A password nobody filled has no source and is
    /// not counted.
    pub other_database: u32,
}

/// What the Fill button asks about when the environment's database is not
/// the one the last Fill read: the two databases' labels.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct FillDbChange {
    /// The database the last Fill read.
    pub from: String,
    /// The database the environment uses now.
    pub to: String,
}

/// The label a database is known by, for a sentence. An id the list no
/// longer has reads as that, never as the id.
fn db_label(store: &dyn SecretStore, id: &str) -> String {
    credentials::label_of(store, id).unwrap_or_else(|| "a database no longer in the list".to_string())
}

/// Whether the active environment's database differs from the one its last
/// Fill read. `None` with no earlier Fill, or the same database.
pub fn fill_db_change_in(root: &Path, store: &dyn SecretStore) -> Result<Option<FillDbChange>, String> {
    let env = environments::active(root)?;
    Ok(match environments::last_fill_db(root, &env.id) {
        Some(was) if was != env.db_id => {
            Some(FillDbChange { from: db_label(store, &was), to: db_label(store, &env.db_id) })
        }
        _ => None,
    })
}

/// Whether the Fill should first ask: the environment's database is not the
/// one its last Fill read.
#[tauri::command]
#[specta::specta]
pub fn env_fill_db_change(app: tauri::AppHandle, secrets: State<'_, DbSecrets>) -> Result<Option<FillDbChange>, String> {
    fill_db_change_in(&super::autorun::root(&app)?, &*secrets.0)
}

/// A fill works only on a test environment.
fn require_test(env: &Environment) -> Result<(), String> {
    if env.test_environment {
        Ok(())
    } else {
        Err(format!("{} - mark it in Edit environments first", crate::autorun::account_lookup::NOT_TEST))
    }
}

/// The environment the fill works on and what the lookup found for `users`,
/// or the sentence for why not: not a test environment, or the lookup did not
/// run (database off, no connection, a table or column missing, an ambiguous
/// password column). Never carries a value.
async fn fill_lookup<S: crate::autorun::setup_find::RowSource>(
    root: &Path,
    rows: Result<&S, String>,
    users: &[String],
) -> Result<(Environment, crate::autorun::account_lookup::LookedUp), String> {
    use crate::autorun::account_lookup::{failure_sentence, look_up, FIX_IN_APP};
    let env = environments::active(root)?;
    require_test(&env)?;
    let looked = match rows {
        Ok(rows) => look_up(rows, &env.account_lookup, users).await,
        Err(why) => Err(why),
    };
    match looked {
        Ok(found) => {
            // The Fill read this database: the next one asks only if the
            // environment's database is not this one.
            environments::set_last_fill_db(root, &env.id, &env.db_id);
            Ok((env, found))
        }
        Err(why) => {
            crate::applog::info(format!("Environments: the account lookup for {} did not run", env.name));
            Err(failure_sentence(&why, FIX_IN_APP))
        }
    }
}

/// Fills the active environment's PROPOSED logins from the account table:
/// a found password replaces what the proposal holds, a hash or an empty
/// value becomes its reason, and a login with no row keeps what it has (or
/// gets "no row" as its reason). Answers counts only.
pub async fn fill_proposals_in<S: crate::autorun::setup_find::RowSource>(
    root: &Path,
    rows: Result<&S, String>,
) -> Result<FillCounts, String> {
    use crate::autorun::account_lookup::{settle_proposal, Found};
    let env = environments::active(root)?;
    // Said before anything is read.
    require_test(&env)?;
    let mut list = environments::load_proposals(root, &env.id)?;
    if list.is_empty() {
        return Err("there are no proposed logins to fill".to_string());
    }
    let users: Vec<String> = list.iter().map(|p| p.username.clone()).collect();
    let (env, found) = fill_lookup(root, rows, &users).await?;
    let mut counts = FillCounts::default();
    let before = list.clone();
    for p in &mut list {
        let seen = found.of(&p.username);
        let (password, reason) = settle_proposal(seen, p.password.clone(), p.no_password.clone(), &found.table);
        match seen {
            Some(Found::Password(_)) if password == p.password => counts.unchanged += 1,
            Some(Found::Password(_)) => {
                counts.updated += 1;
                if p.password_source.as_deref().is_some_and(|was| was != env.db_id) {
                    counts.other_database += 1;
                }
            }
            // The database login's own password cannot be read back: the
            // proposal keeps what it had.
            Some(Found::Masked) => counts.unchanged += 1,
            Some(Found::Hash) => counts.hash_only += 1,
            Some(Found::Empty) | None => counts.no_row += 1,
        }
        // Where the kept password came from: this database when the table
        // gave it, nothing when it was dropped, as it was otherwise.
        match (seen, &password) {
            (Some(Found::Password(_)), _) => p.password_source = Some(env.db_id.clone()),
            (_, None) => p.password_source = None,
            _ => {}
        }
        p.password = password;
        p.no_password = reason;
    }
    if list != before {
        environments::save_proposals(root, &env.id, &list)?;
    }
    crate::applog::info(format!(
        "Environments: filled the proposed logins from the account table ({} updated, {} unchanged, {} with no row, {} hash only)",
        counts.updated, counts.unchanged, counts.no_row, counts.hash_only
    ));
    Ok(counts)
}

/// Fills the active environment's SAVED accounts from the account table:
/// `keys` names the accounts (empty means every one). A password that
/// differs is saved the way a person's change of it is
/// (`accounts::merge_accounts_for`: its saved session goes with it, and any
/// password held pending is let go); an account whose password did not
/// change is not written. Answers counts only.
pub async fn fill_accounts_in<S: crate::autorun::setup_find::RowSource>(
    root: &Path,
    rows: Result<&S, String>,
    keys: &[String],
) -> Result<FillCounts, String> {
    use crate::autorun::account_lookup::Found;
    let env = environments::active(root)?;
    require_test(&env)?;
    let all = crate::autorun::accounts::load_accounts_for(root, &env.id)?;
    let targets: Vec<&crate::autorun::accounts::Account> =
        all.iter().filter(|a| keys.is_empty() || keys.iter().any(|k| k.trim() == a.key)).collect();
    if targets.is_empty() {
        return Err("there are no saved accounts to fill".to_string());
    }
    let users: Vec<String> = targets.iter().map(|a| a.username.clone()).collect();
    let (env, found) = fill_lookup(root, rows, &users).await?;
    let mut counts = FillCounts::default();
    let mut changes = vec![];
    // The accounts whose password came from another database than this one.
    let mut elsewhere: Vec<String> = vec![];
    for a in targets {
        match found.of(&a.username) {
            Some(Found::Password(p)) if *p == a.password => counts.unchanged += 1,
            Some(Found::Password(p)) => {
                if a.password_source.as_deref().is_some_and(|was| was != env.db_id) {
                    elsewhere.push(a.key.clone());
                }
                changes.push(crate::autorun::accounts::PasswordChange {
                    key: a.key.clone(),
                    username: a.username.clone(),
                    was: a.password.clone(),
                    new: p.clone(),
                    source: Some(env.db_id.clone()),
                })
            }
            // The database login's own password cannot be read back: the
            // saved one stays, as if it had not changed.
            Some(Found::Masked) => counts.unchanged += 1,
            Some(Found::Hash) => counts.hash_only += 1,
            Some(Found::Empty) | None => counts.no_row += 1,
        }
    }
    // The accounts are read again just before the write, and only an account
    // still as it was when the lookup began is changed: one changed during
    // the lookup keeps its newer password and counts as unchanged.
    let asked = changes.len();
    let applied = if changes.is_empty() {
        vec![]
    } else {
        crate::autorun::accounts::apply_password_changes_keyed(root, &env.id, &changes)?
    };
    counts.updated += applied.len() as u32;
    counts.unchanged += (asked - applied.len()) as u32;
    counts.other_database += applied.iter().filter(|k| elsewhere.contains(k)).count() as u32;
    crate::applog::info(format!(
        "Environments: filled saved accounts from the account table ({} updated, {} unchanged, {} with no row, {} hash only)",
        counts.updated, counts.unchanged, counts.no_row, counts.hash_only
    ));
    Ok(counts)
}

/// The active environment's database as the place the fill reads, or the
/// sentence for why there is none. `db_read` is the Database Access module as
/// the webview holds it.
fn fill_db(
    root: &Path,
    store: &dyn SecretStore,
    db_read: bool,
) -> Result<crate::api_templates::gate::SqlcmdStageDb<crate::db::RealRunner>, String> {
    if !crate::autorun::preconditions::read_access(db_read) {
        return Err(crate::autorun::account_lookup::DB_OFF.to_string());
    }
    crate::autorun::preconditions::environment_db(root, Some(store))
}

/// "Sync Database" on the Proposed tab.
#[tauri::command]
#[specta::specta]
pub async fn env_fill_proposal_passwords(
    app: tauri::AppHandle,
    secrets: State<'_, DbSecrets>,
    db_read: bool,
) -> Result<FillCounts, String> {
    let root = super::autorun::root(&app)?;
    match fill_db(&root, &*secrets.0, db_read) {
        Ok(db) => fill_proposals_in(&root, Ok(&crate::autorun::active_login::StageRows(&db))).await,
        Err(why) => fill_proposals_in::<crate::autorun::account_lookup::NoRows>(&root, Err(why)).await,
    }
}

/// "Sync Database" on the Accounts tab: the named accounts, or
/// every saved one when `keys` is empty.
#[tauri::command]
#[specta::specta]
pub async fn env_fill_account_passwords(
    app: tauri::AppHandle,
    secrets: State<'_, DbSecrets>,
    keys: Vec<String>,
    db_read: bool,
) -> Result<FillCounts, String> {
    let root = super::autorun::root(&app)?;
    match fill_db(&root, &*secrets.0, db_read) {
        Ok(db) => fill_accounts_in(&root, Ok(&crate::autorun::active_login::StageRows(&db)), &keys).await,
        Err(why) => fill_accounts_in::<crate::autorun::account_lookup::NoRows>(&root, Err(why), &keys).await,
    }
}

/// One login in the environment's account table, for the picker. No password.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct DbLogin {
    pub login: String,
    /// A saved account in the active environment already has this username.
    pub saved: bool,
}

/// The logins the picker lists, and whether the list was cut at the limit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct DbLoginList {
    pub logins: Vec<DbLogin>,
    pub cut_short: bool,
}

/// A login that was asked for and not added, and why (a fixed sentence).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct NotAdded {
    pub login: String,
    pub reason: String,
}

/// What adding logins from the database did: counts and logins, never a password.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct AddedCounts {
    pub added: u32,
    pub already_saved: u32,
    pub not_added: Vec<NotAdded>,
}

fn lookup_failed(env: &Environment, why: String) -> String {
    use crate::autorun::account_lookup::{failure_sentence, FIX_IN_APP};
    crate::applog::info(format!("Environments: the account lookup for {} did not run", env.name));
    failure_sentence(&why, FIX_IN_APP)
}

/// Lists the logins in the active test environment's account table that hold
/// `search`, each marked when an account with that username is saved.
pub async fn list_logins_in<S: crate::autorun::setup_find::RowSource>(
    root: &Path,
    rows: Result<&S, String>,
    search: &str,
) -> Result<DbLoginList, String> {
    let env = environments::active(root)?;
    require_test(&env)?;
    let listed = match rows {
        Ok(rows) => crate::autorun::account_lookup::list_logins(rows, &env.account_lookup, search).await,
        Err(why) => Err(why),
    };
    let (logins, cut_short) = listed.map_err(|why| lookup_failed(&env, why))?;
    let saved = crate::autorun::accounts::load_accounts_for(root, &env.id)?;
    let logins: Vec<DbLogin> = logins
        .into_iter()
        .map(|login| {
            let is_saved = saved.iter().any(|a| a.username.trim().eq_ignore_ascii_case(login.trim()));
            DbLogin { login, saved: is_saved }
        })
        .collect();
    crate::applog::info(format!("Environments: listed {} login(s) from the account table", logins.len()));
    Ok(DbLoginList { logins, cut_short })
}

/// Adds the named logins as accounts of the active test environment, reading
/// each password from the account table here. A login already saved is
/// skipped; one with only a hash, no password, the database login's own
/// password or no row is not added, and says why.
pub async fn add_logins_in<S: crate::autorun::setup_find::RowSource>(
    root: &Path,
    rows: Result<&S, String>,
    logins: &[String],
) -> Result<AddedCounts, String> {
    use crate::autorun::account_lookup::{
        key_for_login, no_row_reason, Found, EMPTY_PASSWORD, HASH_ONLY, SAME_AS_DB_LOGIN,
    };
    let env = environments::active(root)?;
    require_test(&env)?;
    let saved = crate::autorun::accounts::load_accounts_for(root, &env.id)?;
    let mut counts = AddedCounts::default();
    let mut asked: Vec<String> = vec![];
    for l in logins.iter().map(|l| l.trim()).filter(|l| !l.is_empty()) {
        if asked.iter().any(|a| a.eq_ignore_ascii_case(l)) {
            continue;
        }
        if saved.iter().any(|a| a.username.trim().eq_ignore_ascii_case(l)) {
            counts.already_saved += 1;
        } else {
            asked.push(l.to_string());
        }
    }
    if asked.is_empty() {
        return Ok(counts);
    }
    let looked = match rows {
        Ok(rows) => crate::autorun::account_lookup::look_up(rows, &env.account_lookup, &asked).await,
        Err(why) => Err(why),
    };
    let found = looked.map_err(|why| lookup_failed(&env, why))?;
    let mut taken: std::collections::HashSet<String> = saved.iter().map(|a| a.key.clone()).collect();
    let mut ready = vec![];
    for login in asked {
        let why = match found.of(&login) {
            Some(Found::Password(p)) => {
                let key = key_for_login(&login, &taken);
                taken.insert(key.clone());
                ready.push(crate::autorun::accounts::Account {
                    key,
                    label: login.clone(),
                    username: login,
                    password: p.clone(),
                    password_source: Some(env.db_id.clone()),
                });
                continue;
            }
            Some(Found::Hash) => HASH_ONLY.to_string(),
            Some(Found::Empty) => EMPTY_PASSWORD.to_string(),
            Some(Found::Masked) => SAME_AS_DB_LOGIN.to_string(),
            None => no_row_reason(&found.table),
        };
        counts.not_added.push(NotAdded { login, reason: why });
    }
    let appended = crate::autorun::accounts::append_new_accounts_for(root, &env.id, ready.clone())?;
    counts.added = appended.len() as u32;
    // A login saved by someone else meanwhile is not an add.
    counts.already_saved += (ready.len() - appended.len()) as u32;
    crate::applog::info(format!(
        "Environments: added {} account(s) from the account table ({} not added)",
        counts.added,
        counts.not_added.len()
    ));
    Ok(counts)
}

/// "Add accounts from the database": the logins in the account table.
#[tauri::command]
#[specta::specta]
pub async fn env_list_db_logins(
    app: tauri::AppHandle,
    secrets: State<'_, DbSecrets>,
    search: String,
    db_read: bool,
) -> Result<DbLoginList, String> {
    let root = super::autorun::root(&app)?;
    match fill_db(&root, &*secrets.0, db_read) {
        Ok(db) => list_logins_in(&root, Ok(&crate::autorun::active_login::StageRows(&db)), &search).await,
        Err(why) => list_logins_in::<crate::autorun::account_lookup::NoRows>(&root, Err(why), &search).await,
    }
}

/// Adds the ticked logins as accounts; each password is read here and never
/// leaves Rust.
#[tauri::command]
#[specta::specta]
pub async fn env_add_db_logins(
    app: tauri::AppHandle,
    secrets: State<'_, DbSecrets>,
    logins: Vec<String>,
    db_read: bool,
) -> Result<AddedCounts, String> {
    let root = super::autorun::root(&app)?;
    match fill_db(&root, &*secrets.0, db_read) {
        Ok(db) => add_logins_in(&root, Ok(&crate::autorun::active_login::StageRows(&db)), &logins).await,
        Err(why) => add_logins_in::<crate::autorun::account_lookup::NoRows>(&root, Err(why), &logins).await,
    }
}

/// The environments and which is active. `current_db` is the database the
/// Company database card has chosen now: on first use it becomes Default's.
#[tauri::command(async)]
#[specta::specta]
pub fn env_list(
    app: tauri::AppHandle,
    secrets: State<'_, DbSecrets>,
    current_db: Option<String>,
    working_dir: Option<String>,
) -> Result<EnvListView, String> {
    // The shared root: this call is the one that makes Default (with the
    // Company database the person chose), so it cannot need an
    // environment's folder to exist first.
    let root = crate::autorun::store::shared_root().ok_or_else(|| super::autorun::NO_DATA_DIRECTORY.to_string())?;
    let view = env_list_with(&root, &*secrets.0, current_db.as_deref())?;
    // No selection here: a list must never move the Auto Test folder (a
    // stale one would undo a switch) or fail for want of a folder. The
    // first read of the root selects the active environment.
    // Once a run, with the working repository the bridge's start could not
    // know: each AI tool's own "always allow" made to match the active
    // environment, writes held off meanwhile (`repair_allow_files`).
    static REPAIRED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !REPAIRED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        let bridge = super::ai_bridge::running_bridge(&app);
        let outcomes = super::ai_tools::repair_allow_files(
            bridge.as_deref().map(|b| &b.ctx),
            &root,
            working_dir.as_deref(),
            &super::ai_tools::ToolHome::real(),
        );
        super::ai_tools::log_allow_outcomes(&outcomes);
    }
    Ok(view)
}

/// The environments, after older versions' machine-wide "Run database
/// changes without asking" has moved to the active one (`list_adopting`) -
/// and then cleared, so nothing reads it again.
pub fn env_list_with(root: &Path, store: &dyn SecretStore, current_db: Option<&str>) -> Result<EnvListView, String> {
    let legacy = crate::app_settings::current().db_auto_approve;
    let view = list_adopting(root, store, current_db, legacy)?;
    if legacy {
        // It belongs to an environment now (and the file says it has moved,
        // so it never moves twice). Best-effort: a setting left on is read
        // by nothing but this.
        if let Err(e) = crate::app_settings::update(|s| s.db_auto_approve = false) {
            crate::applog::warn(format!("Environments: the old machine-wide database setting stayed behind: {e}"));
        }
    }
    Ok(view)
}

#[tauri::command]
#[specta::specta]
pub async fn env_save(
    app: tauri::AppHandle,
    secrets: State<'_, DbSecrets>,
    env: EnvInput,
) -> Result<EnvListView, String> {
    save_with(&super::autorun::root(&app)?, &*secrets.0, env).await
}

#[tauri::command]
#[specta::specta]
pub fn env_remove(app: tauri::AppHandle, secrets: State<'_, DbSecrets>, id: String) -> Result<EnvListView, String> {
    remove_with(&super::autorun::root(&app)?, &*secrets.0, &id)
}

#[tauri::command]
#[specta::specta]
pub async fn env_set_active(
    app: tauri::AppHandle,
    secrets: State<'_, DbSecrets>,
    id: String,
    working_dir: Option<String>,
) -> Result<EnvSwitched, String> {
    let root = super::autorun::root(&app)?;
    let bridge = super::ai_bridge::running_bridge(&app);
    // Each AI tool's own "always allow" for db_query follows the active
    // environment: written when the new one runs changes without asking,
    // taken out when it does not. What was done comes back with the list.
    let after = switch_reconciler(root.clone(), working_dir, super::ai_tools::ToolHome::real());
    let (view, allow) = switch_with(&root, &*secrets.0, &id, bridge.as_deref().map(|b| &b.ctx), after).await?;
    // Every open screen reads the new environment's data again.
    {
        use tauri_specta::Event as _;
        let _ = crate::events::AutorunEnvironmentChanged { env_id: view.active.clone() }.emit(&app);
    }
    let allow = allow.unwrap_or_else(|| {
        vec![crate::commands::ai_tools::AutoApproveOutcome {
            tool: "Your AI tools".to_string(),
            applied: false,
            note: "their settings could not be brought in line with this environment - see Settings > Logs".to_string(),
        }]
    });
    Ok(EnvSwitched { view, allow })
}

#[tauri::command]
#[specta::specta]
pub fn env_set_default_password(
    app: tauri::AppHandle,
    secrets: State<'_, DbSecrets>,
    id: String,
    password: String,
) -> Result<(), String> {
    set_default_password_with(&super::autorun::root(&app)?, &*secrets.0, &id, &password)
}

#[tauri::command]
#[specta::specta]
pub fn env_set_reset_expired_passwords(
    app: tauri::AppHandle,
    secrets: State<'_, DbSecrets>,
    id: String,
    on: bool,
) -> Result<EnvListView, String> {
    set_reset_expired_passwords_with(&super::autorun::root(&app)?, &*secrets.0, &id, on)
}

#[tauri::command]
#[specta::specta]
pub fn env_clear_default_password(
    app: tauri::AppHandle,
    secrets: State<'_, DbSecrets>,
    id: String,
) -> Result<(), String> {
    clear_default_password_with(&super::autorun::root(&app)?, &*secrets.0, &id)
}

/// The assistant's proposed accounts for the active environment.
#[tauri::command(async)]
#[specta::specta]
pub fn env_proposals(app: tauri::AppHandle) -> Result<Vec<environments::ProposedAccount>, String> {
    proposals_with(&super::autorun::root(&app)?)
}

/// Dismiss the active environment's whole proposal.
#[tauri::command]
#[specta::specta]
pub fn env_dismiss_proposals(app: tauri::AppHandle) -> Result<(), String> {
    dismiss_proposals_with(&super::autorun::root(&app)?)
}

/// Add the picked proposals; returns the keys that are already accounts and
/// need the person's confirmation (resend them in `replace`).
#[tauri::command]
#[specta::specta]
pub fn env_add_proposals(
    app: tauri::AppHandle,
    secrets: State<'_, DbSecrets>,
    picks: Vec<AccountInput>,
    replace: Vec<String>,
) -> Result<Vec<String>, String> {
    add_proposals_with(&super::autorun::root(&app)?, &*secrets.0, picks, replace)
}
