//! What is waiting on the person in the current project: the assistant's
//! proposed logins, its proposed sign-in and the divergences nobody has
//! ruled on yet. Each item says what it is and where it leads; none carries
//! a login, a password or an address.

use crate::autorun::divergences::{self, DivergenceStatus};
use crate::autorun::signin_proposal;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WaitingTarget {
    Accounts,
    SignIn,
    Divergence { id: String, case_id: i32, step: i32, pbi: Option<i32> },
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

/// The active environment's proposals plus the project's sign-in proposal
/// and open divergences, newest first (ties keep accounts, sign-in,
/// divergences).
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
    for d in divergences::load(root, organization, project)? {
        if d.status != DivergenceStatus::Open {
            continue;
        }
        items.push(WaitingItem {
            key: d.id.clone(),
            title: format!("Case {}: step {} differs ({})", d.case_id, d.step, d.kind.word()),
            proposed_ms: d.reported_at.trim().parse::<u64>().unwrap_or(0) as f64,
            target: WaitingTarget::Divergence { id: d.id, case_id: d.case_id, step: d.step, pbi: d.pbi },
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
