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
    let shared = BridgeState::new(BridgeContext::default(), app.package_info().version.to_string());
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

    let (port, _token) = crate::ai_bridge::start_listener(
        Arc::clone(&shared),
        Some(factory),
        Some(crate::ai_bridge::handshake_path()),
    )
    .await?;
    *handle.running.lock().unwrap() = Some((shared, port));
    Ok(BridgeStatus { port, mcp_exe: mcp_exe_path() })
}

/// The frontend pushes its current org/project + detected field refs so
/// bridge routes have defaults the AI never has to guess.
#[tauri::command]
#[specta::specta]
pub fn set_bridge_context(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
    disabled_tools: Vec<String>,
    working_dir: Option<String>,
) {
    use tauri::Manager;
    let handle = app.state::<BridgeHandle>();
    let guard = handle.running.lock().unwrap();
    if let Some((shared, _)) = guard.as_ref() {
        *shared.ctx.lock().unwrap() = BridgeContext {
            org: organization,
            project,
            module_ref,
            preconditions_ref,
            disabled_tools: disabled_tools.clone(),
            working_dir: working_dir.clone(),
        };
    }
    // A tool switched off in the AI Bridge tab loses its slash command too.
    // The tool side already refuses a disabled tool twice - filtered from
    // tools/list, refused again on call - but the picker was still offering
    // it, and the picker is where a person actually looks.
    //
    // Only when the set has really moved: this command also fires on an
    // org or project change, and rewriting ten files for that would be ten
    // pointless writes.
    static LAST: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().unwrap();
    if last.as_deref() != Some(disabled_tools.as_slice()) {
        *last = Some(disabled_tools.clone());
        crate::commands::ai_tools::sync_commands(&disabled_tools);
    }
}

fn mcp_exe_path() -> String {
    std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

