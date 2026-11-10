//! The one sign-in recipe the assistant proposes for an environment, kept
//! until the person approves or rejects it. A proposal is the recipe as it
//! WOULD be saved (the draft's five parts laid over the saved recipe), the
//! hash of the saved recipe it was made against, and a plain-words view for
//! the review. Nothing here signs in, and nothing a person typed is kept:
//! a recipe holds locators and the `{{username}}` / `{{password}}` markers.

use super::recipe::{self, PasswordResetPage, RecipeStep, SignInRecipe};
use crate::browser::actions::Action;
use crate::environments::Environment;
use std::path::{Path, PathBuf};

pub const RESET_PART_WORDS: &str =
    "the change-password part was found on the page but not sent; it is used the next time an expired password is met";
pub const PROPOSAL_STALE: &str = "the sign-in recipe changed after the assistant proposed this one, so it was not saved - ask the assistant to try again";
pub const RESET_SWITCH_NOT_TEST: &str =
    "Reset expired passwords can only be turned on for a test environment - nothing was saved";
const PROPOSAL_UNREADABLE: &str =
    "the proposed sign-in could not be read - reject it and ask the assistant to propose it again";

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoredSignInProposal {
    /// Merged, as it would be saved.
    pub recipe: SignInRecipe,
    pub account: String,
    pub proposed_ms: f64,
    pub summary: String,
    /// `saved_hash` of the recipe file when this was proposed.
    pub saved_hash: String,
    pub changes: Vec<String>,
}

pub fn proposal_path(root: &Path, env_id: &str, org: &str, project: &str) -> PathBuf {
    super::roots::shared_of(root)
        .join("proposals")
        .join(format!("signin-{env_id}-{}.json", recipe::project_slug(org, project)))
}

/// Empty when the project has no recipe file, else a short hash of its text.
pub fn saved_hash(root: &Path, org: &str, project: &str) -> Result<String, String> {
    match std::fs::read_to_string(recipe::recipe_path(root, org, project)) {
        Ok(text) => Ok(format!("{:08x}", recipe::fnv1a(&text))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(_) => Err("the saved sign-in recipe could not be read".to_string()),
    }
}

/// The draft's address, steps, signed-in check and change-password page
/// laid over the saved recipe; everything else stays the saved recipe's. A
/// draft with no change-password page keeps the saved one: an omission is not
/// a removal.
pub fn merge(draft: &SignInRecipe, saved: Option<&SignInRecipe>) -> SignInRecipe {
    match saved {
        None => draft.clone(),
        Some(saved) => SignInRecipe {
            start_url: draft.start_url.clone(),
            steps: draft.steps.clone(),
            signed_in: draft.signed_in.clone(),
            signed_in_in_menu: draft.signed_in_in_menu,
            password_reset: draft.password_reset.clone().or_else(|| saved.password_reset.clone()),
            ..saved.clone()
        },
    }
}

/// Which of the parts the draft supplies differ from the saved recipe. With
/// no saved recipe every part the draft supplies counts.
pub fn changed_parts(merged: &SignInRecipe, saved: Option<&SignInRecipe>) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |changed: bool, name: &str| {
        if changed {
            out.push(name.to_string());
        }
    };
    match saved {
        None => {
            push(true, "address");
            push(true, "steps");
            push(true, "signed-in check");
            push(merged.password_reset.is_some(), "change-password part");
        }
        Some(s) => {
            push(merged.start_url != s.start_url, "address");
            push(merged.steps != s.steps, "steps");
            push(merged.signed_in != s.signed_in || merged.signed_in_in_menu != s.signed_in_in_menu, "signed-in check");
            push(merged.password_reset != s.password_reset, "change-password part");
        }
    }
    out
}

pub fn summary(merged: &SignInRecipe) -> String {
    let n = merged.steps.len();
    let mut s = format!("{n} step{}, signed in when {} shows", if n == 1 { "" } else { "s" }, merged.signed_in.describe());
    if merged.password_reset.is_some() {
        s.push_str(", with a change-password part");
    }
    s
}

/// Replaces any earlier proposal for this environment and project.
pub fn save(root: &Path, env_id: &str, org: &str, project: &str, p: &StoredSignInProposal) -> Result<(), String> {
    let path = proposal_path(root, env_id, org, project);
    super::roots::create_dir_all(root, path.parent().unwrap_or(root)).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(p).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&path, &json)?;
    super::data_changed::notify(root, crate::events::AutorunDataKind::SignInProposal);
    Ok(())
}

pub fn load(root: &Path, env_id: &str, org: &str, project: &str) -> Result<Option<StoredSignInProposal>, String> {
    match std::fs::read_to_string(proposal_path(root, env_id, org, project)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            serde_json::from_str(s).map(Some).map_err(|e| {
                // The parser's words can quote file content; keep them out of both.
                crate::applog::warn(format!(
                    "Auto-run sign-in proposal did not parse ({:?} at line {}, column {})",
                    e.classify(),
                    e.line(),
                    e.column()
                ));
                PROPOSAL_UNREADABLE.to_string()
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(PROPOSAL_UNREADABLE.to_string()),
    }
}

pub fn clear(root: &Path, env_id: &str, org: &str, project: &str) -> Result<(), String> {
    match std::fs::remove_file(proposal_path(root, env_id, org, project)) {
        Ok(()) => {
            super::data_changed::notify(root, crate::events::AutorunDataKind::SignInProposal);
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("the proposed sign-in could not be removed".to_string()),
    }
}

/// What the review shows. No host, no query, no typed text.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct SignInProposalView {
    /// A path only, or "this environment's address".
    pub address: String,
    pub steps: Vec<String>,
    pub signed_in: String,
    pub signed_in_in_menu: bool,
    pub password_reset: Option<String>,
    pub account: String,
    pub changes: Vec<String>,
    pub offer_reset_switch: bool,
}

fn action_in_words(a: &Action) -> String {
    match a {
        Action::Navigate { url } => format!("go to {}", crate::browser::save_guard::path_of(url)),
        Action::Fill { selector, value } => {
            let with = if value.contains(recipe::PASSWORD) {
                "the password"
            } else if value.contains(recipe::USERNAME) {
                "the username"
            } else {
                "fixed text"
            };
            format!("fill in {} with {with}", selector.describe())
        }
        other => super::report::action_words(other),
    }
}

fn step_in_words(s: &RecipeStep) -> String {
    match s {
        RecipeStep::Do(a) => action_in_words(a),
        RecipeStep::WhenVisible(w) => {
            let then: Vec<String> = w.then.iter().map(action_in_words).collect();
            format!("if {} shows, {}", w.selector.describe(), then.join(", then "))
        }
    }
}

fn reset_in_words(page: &PasswordResetPage) -> String {
    let page = super::signin::with_builtin_fields(page.clone());
    let d = |t: &Option<crate::browser::locator::Target>| t.as_ref().map(|t| t.describe()).unwrap_or_default();
    let mut s = format!(
        "page {}: new password {}, confirm {}, send {}",
        page.path_contains,
        d(&page.new),
        d(&page.confirm),
        d(&page.submit)
    );
    if !page.no_current_password {
        s.push_str(&format!(", current password {}", page.field.describe()));
    }
    s
}

pub fn view(p: &StoredSignInProposal, env: &Environment) -> SignInProposalView {
    SignInProposalView {
        address: if env.start_url.trim().is_empty() {
            crate::browser::save_guard::path_of(&p.recipe.start_url)
        } else {
            "this environment's address".to_string()
        },
        steps: p.recipe.steps.iter().map(step_in_words).collect(),
        signed_in: p.recipe.signed_in.describe(),
        signed_in_in_menu: p.recipe.signed_in_in_menu,
        password_reset: p.recipe.password_reset.as_ref().map(reset_in_words),
        account: p.account.clone(),
        changes: p.changes.clone(),
        offer_reset_switch: p.recipe.password_reset.is_some() && env.test_environment && !env.reset_expired_passwords,
    }
}
