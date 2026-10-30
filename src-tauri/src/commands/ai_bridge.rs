//! AI bridge lifecycle: start-on-demand + context push from the frontend.
//! The bridge itself is read/validate only (see ai_bridge.rs).

use std::sync::Arc;

use crate::ai_bridge::{BridgeContext, BridgeState, SharedBridge};
use crate::state::get_fresh_token;

/// Managed state: the running bridge, if any.
#[derive(Default)]
pub struct BridgeHandle {
    pub running: std::sync::Mutex<Option<(SharedBridge, u16)>>,
    /// Held for the whole of a start. The `running` lock cannot be, because
    /// binding the listener is async and a std mutex must not be held across
    /// an await - so without this, two callers both saw None and both bound
    /// a port. See `bridge_status`.
    starting: tokio::sync::Mutex<()>,
}

#[derive(serde::Serialize, specta::Type)]
pub struct BridgeStatus {
    pub port: u16,
    /// Absolute path to this app's own exe, run with `--mcp` (what the user
    /// registers in their AI tool - the MCP proxy is folded into the main
    /// binary, not a separate file).
    pub mcp_exe: String,
}

#[tauri::command]
#[specta::specta]
pub async fn bridge_status(app: tauri::AppHandle) -> Result<BridgeStatus, String> {
    use tauri::Manager;
    let handle = app.state::<BridgeHandle>();
    // One start at a time. The frontend pushes context from an effect that
    // can fire twice in a row (org and the field refs resolving in adjacent
    // commits), and both calls used to get past the check below while the
    // other was still awaiting the bind - leaving a second listener on a
    // loopback port that no handshake file names, serving a context that is
    // never updated again.
    let _starting = handle.starting.lock().await;
    if let Some((_, port)) = handle.running.lock().unwrap().as_ref() {
        return Ok(BridgeStatus { port: *port, mcp_exe: mcp_exe_path() });
    }
    // Writes start held: each AI tool's own "always allow" is first made to
    // match the active environment (an app that stopped halfway through a
    // switch left the last one's), and only then can a push turn one on.
    let mut first = BridgeContext::default();
    hold_writes(&mut first);
    let shared = BridgeState::new(first, app.package_info().version.to_string());
    crate::ai_bridge::set_live_bridge(&shared);
    if let Ok(root) = super::autorun::root(&app) {
        let repairing = Arc::clone(&shared);
        tauri::async_runtime::spawn_blocking(move || {
            let tools = super::ai_tools::ToolHome::real();
            // The hold taken above is this repair's: it ends with it.
            let _release = HeldWrites(Some(&repairing.ctx));
            super::ai_tools::log_allow_outcomes(&super::ai_tools::match_allow_files_in(&root, None, &tools));
        });
    } else {
        release_writes(&shared.ctx);
    }
    let app_for_client = app.clone();
    let factory: crate::ai_bridge::ClientFactory = Arc::new(move || {
        let app = app_for_client.clone();
        Box::pin(async move {
            get_fresh_token(&app).await.ok().map(crate::ado::AdoClient::new)
        })
    });
    // Let `/begin` tell the UI where the assistant is about to write, so the
    // Import tab can watch that path before the file exists. Set once - the
    // bridge only ever starts once per process.
    let app_for_intake = app.clone();
    crate::ai_bridge::set_intake_sink(Box::new(move |path| {
        use tauri_specta::Event as _;
        let _ = crate::events::IntakeOutputPath { path }.emit(&app_for_intake);
    }));

    // A prove or run of an API template tells the API Templates tab, so it
    // reloads when the template or its history changed.
    let app_for_templates = app.clone();
    crate::ai_bridge::set_templates_sink(Box::new(move |id| {
        use tauri_specta::Event as _;
        let _ = crate::events::ApiTemplatesChanged { id }.emit(&app_for_templates);
    }));

    // An assistant opening, editing or previewing a report tells the Crystal
    // Reports screen, which then reads the open report again.
    let app_for_crystal = app.clone();
    crate::ai_bridge::set_crystal_sink(Box::new(move |path| {
        use tauri_specta::Event as _;
        let _ = crate::events::CrystalReportChanged { path }.emit(&app_for_crystal);
    }));

    // In-place bridge writes are allowed on the files the app follows.
    let app_for_watch = app.clone();
    crate::ai_bridge::set_watch_source(Box::new(move || {
        use tauri::Manager;
        crate::filewatch::watched_paths(&app_for_watch.state::<crate::filewatch::FileWatchState>())
    }));

    // An assistant's replay to a step runs in the supervised browser this
    // app holds, and a must-not-save script asks the person in its window.
    crate::ai_bridge::set_replay_host(Box::new(AppReplayHost(app.clone())));

    let (port, _token) = crate::ai_bridge::start_listener(
        Arc::clone(&shared),
        Some(factory),
        Some(crate::ai_bridge::handshake_path()),
    )
    .await?;
    *handle.running.lock().unwrap() = Some((shared, port));
    spawn_agent_sweeper();
    Ok(BridgeStatus { port, mcp_exe: mcp_exe_path() })
}

/// Every `agents::SWEEP_EVERY`, ask whether each assistant's process still
/// runs (`agents::sweep`), on a blocking thread because the check is a system
/// call per agent. Each agent found gone is logged by label only, and one
/// past its grace gives up the browser it was bound to (an extra one is
/// released, the main one only unbound), and then an extra browser whose
/// assistant made no call for 30 minutes is released (`release_idle`).
/// Started once, where the bridge starts; a second call does nothing.
pub fn spawn_agent_sweeper() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        tauri::async_runtime::spawn(async {
            loop {
                tokio::time::sleep(crate::autorun::agents::SWEEP_EVERY).await;
                let swept = tauri::async_runtime::spawn_blocking(|| {
                    crate::autorun::agents::sweep(
                        crate::autorun::sessions::now_ms(),
                        &crate::browser::tree::process_running,
                    )
                })
                .await;
                let Ok(swept) = swept else { continue };
                for key in &swept.newly_gone {
                    if let Some(info) = crate::autorun::agents::info(key) {
                        crate::applog::info(crate::autorun::agents::ended_line(&info.label));
                    }
                }
                // An agent past its grace holds no browser any more; an
                // extra one is released (each wait is bounded).
                crate::commands::autorun_slots::on_sweep(&swept).await;
                // An extra browser whose assistant made no call for 30
                // minutes is released too, gone or not (never the main one).
                crate::commands::autorun_slots::release_idle(crate::autorun::sessions::now_ms()).await;
            }
        });
    });
}

/// The app behind the bridge's `/autorun-replay`. The replay takes the
/// person's own path, as the assistant's.
struct AppReplayHost(tauri::AppHandle);

impl crate::ai_bridge::ReplayHost for AppReplayHost {
    fn replay(
        &self,
        slot: crate::autorun::slots::SlotNo,
        holder: Option<crate::autorun::agents::AgentKey>,
        organization: String,
        project: String,
        req: crate::autorun::replay_to::ReplayRequest,
    ) -> crate::ai_bridge::HostFuture<'_, Result<crate::ai_bridge::AssistantReplay, String>> {
        Box::pin(async move {
            use crate::commands::autorun::{replay_supervised, ReplayBy};
            replay_supervised(&self.0, slot, &organization, &project, req, ReplayBy::Assistant, holder.as_ref())
                .await
                .map(|(end, _tabs, discovery, left_open, from, reuse, overrides)| crate::ai_bridge::AssistantReplay {
                    end,
                    discovery,
                    left_open,
                    from,
                    reuse,
                    // Read under the lock, the moment the replay ended.
                    overrides,
                })
        })
    }

    fn page(
        &self,
        slot: crate::autorun::slots::SlotNo,
        organization: String,
        project: String,
    ) -> crate::ai_bridge::HostFuture<'_, (u16, String)> {
        Box::pin(async move {
            crate::ai_bridge::supervised_page_in(slot, &organization, &project, crate::browser::snapshot::PAGE_LINES_DEFAULT)
                .await
        })
    }
}

/// The AI Bridge tab's on/off switches, pushed together - one argument, so
/// `set_bridge_context` stays inside the ten a specta command may take.
#[derive(Debug, Clone, Default, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSwitches {
    /// Company database create/update/delete (see `BridgeContext::db_writes`).
    pub db_writes: bool,
    /// Proving and running API templates (see `BridgeContext::api_writes`).
    pub api_writes: bool,
    /// The network tools (see `BridgeContext::network_tools`).
    #[serde(default)]
    pub network_tools: bool,
    /// The environment these switches are the webview's for - each keeps
    /// its own. Empty while the webview does not know it yet. A push for any
    /// environment but the active one turns no write on
    /// (`push_context_with`).
    #[serde(default)]
    pub env_id: String,
    /// The webview's "Treat all projects as one" switch (see
    /// `BridgeContext::all_projects`). Not a write switch.
    #[serde(default)]
    pub all_projects: bool,
    /// The PBI selected in the top bar, or none (see
    /// `BridgeContext::current_pbi`). Not a switch: it rides here so
    /// `set_bridge_context` stays inside the ten arguments a specta command
    /// may take.
    #[serde(default)]
    pub pbi: Option<crate::ai_bridge::CurrentPbi>,
}

/// Take the selected PBI off the bridge's context: the webview calls it when
/// the org changes or the person signs out, when nothing else is pushed.
pub fn clear_current_pbi(slot: &std::sync::Mutex<BridgeContext>) {
    slot.lock().unwrap_or_else(|e| e.into_inner()).current_pbi = None;
}

#[tauri::command]
#[specta::specta]
pub fn clear_bridge_pbi(app: tauri::AppHandle) {
    if let Some(shared) = running_bridge(&app) {
        clear_current_pbi(&shared.ctx);
    }
}

/// The running bridge, if it has started.
pub fn running_bridge(app: &tauri::AppHandle) -> Option<SharedBridge> {
    use tauri::Manager;
    let handle = app.state::<BridgeHandle>();
    let guard = handle.running.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().map(|(shared, _)| Arc::clone(shared))
}

/// Put a push from the webview into the bridge's context, judged against
/// the active environment under the context's own lock - the lock a switch
/// holds while it moves the active environment (`move_bridge_to`).
///
/// A push for the active environment (`env_id`) brings its database, tool
/// list and write switches - the writes waiting, if a hold is on, until the
/// allow files match (`release_writes`). Any other push - one that was on
/// its way when a switch landed, one from a webview that does not know the
/// environment yet, one with no environments at all - turns no write on and
/// leaves the context's database and tool list as they are; only before any
/// environment is the context's own is there none to keep, and then the
/// push's are taken (writes still off). "Without asking" is never the
/// webview's to say: it is the active environment's own, read here.
pub fn push_context_with(
    slot: &std::sync::Mutex<BridgeContext>,
    root: Option<&std::path::Path>,
    mut next: BridgeContext,
    env_id: &str,
) {
    let mut ctx = slot.lock().unwrap_or_else(|e| e.into_inner());
    let active = root.and_then(|r| match crate::environments::read_active(r) {
        Ok(env) => env,
        Err(e) => {
            crate::applog::warn(format!("AI bridge: the active environment could not be read, so no write is allowed: {e}"));
            None
        }
    });
    let its_own = !env_id.is_empty() && active.as_ref().is_some_and(|e| e.id == env_id);
    next.gate = ctx.gate.clone();
    if its_own {
        next.gate.env_id = Some(env_id.to_string());
        if next.gate.holds > 0 {
            next.gate.wanted = Some((next.db_writes, next.api_writes));
            next.db_writes = false;
            next.api_writes = false;
        }
    } else {
        next.db_writes = false;
        next.api_writes = false;
        // Like the tool list: kept as the context's own environment has it,
        // and never turned on here. Fail closed, though: an OFF push for the
        // environment the context already holds is always honoured, even
        // when the environments file cannot be read right now.
        let held = ctx.gate.env_id.as_deref();
        let off_for_held = held.is_some() && held == Some(env_id) && !next.network_tools;
        next.network_tools = held.is_some() && ctx.network_tools && !off_for_held;
        if ctx.gate.env_id.is_some() {
            next.db_id = ctx.db_id.clone();
            next.disabled_tools = ctx.disabled_tools.clone();
        }
    }
    next.db_no_ask = active.is_some_and(|e| e.db_auto_approve);
    *ctx = next;
}

/// Hold every write off - for a switch or a repair that is making each AI
/// tool's own "always allow" match the environment. What was on is kept,
/// to come back when the last hold ends; nothing off is ever turned on.
pub fn hold_writes(ctx: &mut BridgeContext) {
    if ctx.gate.holds == 0 && ctx.gate.wanted.is_none() {
        ctx.gate.wanted = Some((ctx.db_writes, ctx.api_writes));
    }
    ctx.gate.holds += 1;
    ctx.db_writes = false;
    ctx.api_writes = false;
}

/// End one hold. When it was the last, the write switches a push for the
/// context's environment asked for meanwhile (or that were on before) apply.
pub fn release_writes(slot: &std::sync::Mutex<BridgeContext>) {
    let mut ctx = slot.lock().unwrap_or_else(|e| e.into_inner());
    ctx.gate.holds = ctx.gate.holds.saturating_sub(1);
    if ctx.gate.holds == 0 {
        if let Some((db, api)) = ctx.gate.wanted.take() {
            ctx.db_writes = db;
            ctx.api_writes = api;
        }
    }
}

/// Ends a hold when dropped, so a hold ends however the work under it does.
pub struct HeldWrites<'a>(pub Option<&'a std::sync::Mutex<BridgeContext>>);

impl Drop for HeldWrites<'_> {
    fn drop(&mut self) {
        if let Some(slot) = self.0 {
            release_writes(slot);
        }
    }
}

/// Run `f` with every write held off.
pub fn hold_writes_while<T>(bridge: Option<&std::sync::Mutex<BridgeContext>>, f: impl FnOnce() -> T) -> T {
    if let Some(slot) = bridge {
        hold_writes(&mut slot.lock().unwrap_or_else(|e| e.into_inner()));
    }
    let _release = HeldWrites(bridge);
    f()
}

/// The frontend pushes its current org/project + detected field refs so
/// bridge routes have defaults the AI never has to guess.
///
/// The slash commands, agent files and "always allow" entries that follow the
/// pushed state are files (and, for the tools found, a PATH search), so they
/// are written on a blocking thread, with no bridge lock held.
#[tauri::command]
#[specta::specta]
pub async fn set_bridge_context(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
    disabled_tools: Vec<String>,
    working_dir: Option<String>,
    db_id: Option<String>,
    switches: BridgeSwitches,
) {
    let (running, root) = push_context(
        &app,
        organization,
        project,
        module_ref,
        preconditions_ref,
        disabled_tools.clone(),
        working_dir.clone(),
        db_id,
        switches,
    );
    let job = tauri::async_runtime::spawn_blocking(move || {
        sync_files_after_push(running.as_ref(), root.as_deref(), disabled_tools, working_dir)
    });
    if let Err(e) = job.await {
        crate::applog::warn(format!("syncing the AI tools' files after a push stopped early: {e}"));
    }
}

/// The part of `set_bridge_context` that changes the bridge's context: quick,
/// no file work. Answers the running bridge (if any) and the app's data folder.
#[allow(clippy::too_many_arguments)]
fn push_context(
    app: &tauri::AppHandle,
    organization: String,
    project: String,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
    disabled_tools: Vec<String>,
    working_dir: Option<String>,
    db_id: Option<String>,
    switches: BridgeSwitches,
) -> (Option<SharedBridge>, Option<std::path::PathBuf>) {
    use tauri::Manager;
    let secrets = std::sync::Arc::clone(&app.state::<crate::db::DbSecrets>().0);
    let root = super::autorun::root(app).ok();
    let handle = app.state::<BridgeHandle>();
    let guard = handle.running.lock().unwrap();
    let mut running: Option<SharedBridge> = None;
    if let Some((shared, _)) = guard.as_ref() {
        let next = BridgeContext {
            org: organization,
            project,
            module_ref,
            preconditions_ref,
            disabled_tools: disabled_tools.clone(),
            working_dir: working_dir.clone(),
            // Only the id comes from the webview; the login it stands for
            // stays in the store and is resolved when a database tool
            // runs. See `BridgeContext`.
            db_id,
            db_secrets: Some(secrets),
            db_writes: switches.db_writes,
            api_writes: switches.api_writes,
            network_tools: switches.network_tools,
            all_projects: switches.all_projects,
            current_pbi: switches.pbi.clone(),
            // Set from the active environment, never from the webview.
            db_no_ask: false,
            gate: Default::default(),
        };
        push_context_with(&shared.ctx, root.as_deref(), next, &switches.env_id);
        running = Some(Arc::clone(shared));
    }
    drop(guard);
    (running, root)
}

/// The files that follow the state the bridge holds now: the slash commands
/// and agent files, and the "always allow" entries. Runs on a blocking
/// thread, and reads the tool list and repository from the bridge's context
/// when it runs, not from the push that started it - so two pushes landing out
/// of order still leave the files as the latest one says. The context's lock
/// is let go before any file is touched.
fn sync_files_after_push(
    running: Option<&SharedBridge>,
    root: Option<&std::path::Path>,
    pushed_disabled: Vec<String>,
    pushed_working_dir: Option<String>,
) {
    // The tool list the bridge now holds: a push for another environment
    // than the active one leaves the context's own list in place.
    let (disabled_tools, working_dir) = match running {
        Some(shared) => {
            let ctx = shared.ctx.lock().unwrap_or_else(|e| e.into_inner());
            (ctx.disabled_tools.clone(), ctx.working_dir.clone())
        }
        None => (pushed_disabled, pushed_working_dir),
    };
    // A tool switched off in the AI Bridge tab loses its slash command too.
    // The tool side already refuses a disabled tool twice - filtered from
    // tools/list, refused again on call - but the picker was still offering
    // it, and the picker is where a person actually looks.
    //
    // Only when the set has really moved: this command also fires on an
    // org or project change, and rewriting ten files for that would be ten
    // pointless writes.
    //
    // Keyed on the REPOSITORY too, not just the set: the command files live
    // per repository now, so switching repository with the same toggles has
    // to re-sync - into the new repository's dir. Still a no-op unless that
    // dir already exists (see `sync_commands`).
    //
    // And on the modules: a module switched off takes its commands with it
    // (`ai_tools::effective_disabled`), so the next push after a switch
    // re-syncs even with the same toggles.
    type SyncKey = (Vec<String>, Option<String>, std::collections::BTreeSet<crate::modules::Module>);
    static LAST: std::sync::Mutex<Option<SyncKey>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let key: SyncKey = (disabled_tools.clone(), working_dir.clone(), crate::modules::current());
    if last.as_ref() != Some(&key) {
        *last = Some(key);
        crate::commands::ai_tools::sync_commands(&disabled_tools, working_dir.as_deref());
    }
    drop(last);

    // The "always allow" the app writes for the Auto Run and API template
    // tools follows the same things - the tools switched off, the
    // repository, the modules - and is written wherever the server is
    // registered, whether or not the command folder exists. Judged on the
    // list the bridge holds, and only when that, the repository or the
    // modules have moved.
    type AllowKey = (Vec<String>, Option<String>, std::collections::BTreeSet<crate::modules::Module>);
    static LAST_ALLOW: std::sync::Mutex<Option<AllowKey>> = std::sync::Mutex::new(None);
    let Some(root) = root else { return };
    let mut last_allow = LAST_ALLOW.lock().unwrap_or_else(|e| e.into_inner());
    let key: AllowKey = (disabled_tools.clone(), working_dir.clone(), crate::modules::current());
    if last_allow.as_ref() != Some(&key) {
        *last_allow = Some(key);
        crate::commands::ai_tools::log_allow_outcomes(&crate::commands::ai_tools::sync_allow_rules_in(
            root,
            working_dir.as_deref(),
            &crate::commands::ai_tools::ToolHome::real(),
            Some(disabled_tools),
        ));
    }
}

fn mcp_exe_path() -> String {
    std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

