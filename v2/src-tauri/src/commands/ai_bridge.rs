//! AI bridge lifecycle: start-on-demand + context push from the frontend.
//! The bridge itself is read/validate only (see ai_bridge.rs).

use std::sync::Arc;

use crate::ai_bridge::{BridgeContext, BridgeState, SharedBridge};
use crate::state::get_fresh_token;

/// Managed state: the running bridge, if any.
#[derive(Default)]
pub struct BridgeHandle(pub std::sync::Mutex<Option<(SharedBridge, u16)>>);

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
    // Already running? Reuse it.
    {
        let handle = app.state::<BridgeHandle>();
        let guard = handle.0.lock().unwrap();
        if let Some((_, port)) = guard.as_ref() {
            return Ok(BridgeStatus { port: *port, mcp_exe: mcp_exe_path() });
        }
    }
    let shared = BridgeState::new(BridgeContext::default(), app.package_info().version.to_string());
    let app_for_client = app.clone();
    let factory: crate::ai_bridge::ClientFactory = Arc::new(move || {
        let app = app_for_client.clone();
        Box::pin(async move {
            get_fresh_token(&app).await.ok().map(crate::ado::AdoClient::new)
        })
    });
    let (port, _token) = crate::ai_bridge::start_listener(Arc::clone(&shared), Some(factory))
        .await?;
    let handle = app.state::<BridgeHandle>();
    *handle.0.lock().unwrap() = Some((shared, port));
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
) {
    use tauri::Manager;
    let handle = app.state::<BridgeHandle>();
    let guard = handle.0.lock().unwrap();
    if let Some((shared, _)) = guard.as_ref() {
        *shared.ctx.lock().unwrap() = BridgeContext {
            org: organization,
            project,
            module_ref,
            preconditions_ref,
        };
    }
}

fn mcp_exe_path() -> String {
    std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}
