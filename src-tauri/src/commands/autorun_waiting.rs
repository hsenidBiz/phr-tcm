//! What is waiting on the person in the current project: the active
//! environment's proposed logins and its proposed sign-in. Each item says
//! what it is and where it leads; none carries a login, a password or an
//! address. Divergences are not here: they are ruled on in each case's card.

use crate::autorun::signin_proposal;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WaitingTarget {
    Accounts,
    SignIn,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct WaitingItem {
    pub key: String,
    pub title: String,
    pub proposed_ms: f64,
    pub target: WaitingTarget,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct WaitingItems {
    pub env_id: String,
    pub items: Vec<WaitingItem>,
}

/// The active environment's proposals plus the project's sign-in proposal,
/// newest first (ties keep accounts, then sign-in).
pub fn waiting_items_with(root: &Path, organization: &str, project: &str) -> Result<WaitingItems, String> {
    let env = crate::environments::active(root)?;
    let mut items: Vec<WaitingItem> = Vec::new();
    for p in crate::environments::load_proposals(root, &env.id)? {
        items.push(WaitingItem {
            key: p.key.clone(),
            title: p.key,
            proposed_ms: p.proposed_ms.unwrap_or(0) as f64,
            target: WaitingTarget::Accounts,
        });
    }
    if let Some(p) = signin_proposal::load(root, &env.id, organization, project)? {
        items.push(WaitingItem {
            key: "sign-in".to_string(),
            title: format!("Sign-in for {}", env.name),
            proposed_ms: p.proposed_ms,
            target: WaitingTarget::SignIn,
        });
    }
    // Stable: equal times keep the order they were pushed in.
    items.sort_by(|a, b| b.proposed_ms.total_cmp(&a.proposed_ms));
    Ok(WaitingItems { env_id: env.id, items })
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_waiting_items(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<WaitingItems, String> {
    waiting_items_with(&super::autorun::root(&app)?, &organization, &project)
}
