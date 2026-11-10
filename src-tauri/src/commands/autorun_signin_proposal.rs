//! The review of a sign-in the assistant proposed: show it, approve it
//! (saved the way a recorded one is, never replacing an environment's own
//! address) or reject it. Nothing here signs in or calls Azure DevOps.

use super::autorun_record_signin::for_saving;
use crate::autorun::recipe::save_recipe;
use crate::autorun::signin_proposal::{self as sp, SignInProposalView};
use std::path::Path;

const NOTHING_PROPOSED: &str = "there is no proposed sign-in to approve";

pub fn proposal_view_with(root: &Path, org: &str, project: &str) -> Result<Option<SignInProposalView>, String> {
    let env = crate::environments::active(root)?;
    Ok(sp::load(root, &env.id, org, project)?.map(|p| sp::view(&p, &env)))
}

pub fn approve_with(root: &Path, org: &str, project: &str, turn_on_reset: bool) -> Result<(), String> {
    let env = crate::environments::active(root)?;
    let p = sp::load(root, &env.id, org, project)?.ok_or_else(|| NOTHING_PROPOSED.to_string())?;
    if sp::saved_hash(root, org, project)? != p.saved_hash {
        return Err(sp::PROPOSAL_STALE.to_string());
    }
    if turn_on_reset && !env.test_environment {
        return Err(sp::RESET_SWITCH_NOT_TEST.to_string());
    }
    p.recipe.validate()?;
    save_recipe(root, org, project, &for_saving(root, org, project, p.recipe.clone())?)?;
    if turn_on_reset && p.recipe.password_reset.is_some() {
        crate::environments::set_reset_expired_passwords(root, &env.id, true)?;
    }
    sp::clear(root, &env.id, org, project)?;
    crate::applog::info("Auto-run sign-in recipe approved from the assistant's proposal");
    Ok(())
}

pub fn reject_with(root: &Path, org: &str, project: &str) -> Result<(), String> {
    let env = crate::environments::active(root)?;
    sp::clear(root, &env.id, org, project)
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_sign_in_proposal(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Option<SignInProposalView>, String> {
    proposal_view_with(&super::autorun::root(&app)?, &organization, &project)
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_approve_sign_in_proposal(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    turn_on_reset: bool,
) -> Result<(), String> {
    approve_with(&super::autorun::root(&app)?, &organization, &project, turn_on_reset)
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_reject_sign_in_proposal(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<(), String> {
    reject_with(&super::autorun::root(&app)?, &organization, &project)
}
