//! The review of a sign-in the assistant proposed: show it, approve it
//! (saved the way a recorded one is, never replacing an environment's own
//! address) or reject it; and the assistant's two ways in: look at a sign-in
//! page (`inspect_in`) and propose a recipe that signed in on a trial
//! (`propose_in`). Nothing here calls Azure DevOps; a trial never sends a
//! change-password form, and no typed value or password is in any answer.

use super::autorun_check_signin::{claim_check_browser, refusal};
use super::autorun_record_signin::for_saving;
use crate::autorun::accounts::{find_account, Account};
use crate::autorun::lease::Held;
use crate::autorun::recipe::{effective_recipe, load_effective_recipe_if_any, load_recipe, save_recipe, SignInRecipe, NO_SITE_ADDRESS};
use crate::autorun::replay::Browsers;
use crate::autorun::signin::{is_password_expired, redact, reset_page, sign_in_trial};
use crate::autorun::signin_attempts;
use crate::autorun::signin_inspect::{read_sign_in_page, reset_fields_showing};
use crate::autorun::signin_proposal::{self as sp, SignInProposalView, StoredSignInProposal};
use crate::autorun::signin_recorder::{check_marker_for, refuse_check_on_start_page, CHECK_BROWSER_SILENT, CHECK_START_DID_NOT_OPEN};
use crate::browser::actions::{execute_in, Action, Policy};
use crate::browser::cdp::Driver;
use crate::browser::timing::Timing;
use std::path::Path;

const NOTHING_PROPOSED: &str = "there is no proposed sign-in to approve";

pub fn proposal_view_with(root: &Path, org: &str, project: &str) -> Result<Option<SignInProposalView>, String> {
    let env = crate::environments::active(root)?;
    Ok(sp::load(root, &env.id, org, project)?.map(|p| sp::view(&p, &env)))
}

pub const SWITCH_NOT_TURNED_ON: &str = "the sign-in was saved, but Reset expired passwords could not be turned on - turn it on in the Environments dialog";

pub fn approve_with(root: &Path, org: &str, project: &str, turn_on_reset: bool) -> Result<(), String> {
    approve_turning_on(root, org, project, turn_on_reset, |id| {
        crate::environments::set_reset_expired_passwords(root, id, true).map(|_| ())
    })
}

/// `approve_with`, with the switch's write passed in so a failure of it can be tried.
pub fn approve_turning_on(
    root: &Path,
    org: &str,
    project: &str,
    turn_on_reset: bool,
    turn_on: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
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
    let switch = if turn_on_reset && p.recipe.password_reset.is_some() { turn_on(&env.id) } else { Ok(()) };
    sp::clear(root, &env.id, org, project)?;
    crate::applog::info("Auto-run sign-in recipe approved from the assistant's proposal");
    if let Err(e) = switch {
        crate::applog::warn(format!("Auto-run: Reset expired passwords could not be turned on after approval: {e}"));
        return Err(SWITCH_NOT_TURNED_ON.to_string());
    }
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

// ---- The assistant's tools ------------------------------------------------------

pub const INSPECT_SHAPE: &str =
    "{ \"account\"?: <a saved account key>, \"recipe\"?: <a draft sign-in recipe> } - both or neither";
pub const PROPOSE_SHAPE: &str = "{ \"recipe\": <the whole sign-in recipe>, \"account\": <a saved account key> }";

const INSPECT_ONE_OF: &str = "inspect_sign_in_page takes an account and a recipe together (the page is then read after a trial sign-in with that recipe) or neither (the sign-in page alone is opened, nobody is signed in)";
const PROPOSAL_NOT_KEPT: &str = "the proposal could not be kept - see Settings, Logs for the details";
const NOTHING_PROPOSED_PREFIX: &str = "the trial did not sign in - nothing was proposed";

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectBody {
    #[serde(default)]
    account: Option<String>,
    #[serde(default)]
    recipe: Option<SignInRecipe>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeBody {
    recipe: SignInRecipe,
    account: String,
}

/// The body's own sentence for one that does not parse: serde's words can
/// quote what the body held, so they are never passed on.
fn not_readable(tool: &str, shape: &str) -> (u16, String) {
    (400, format!("that is not what {tool} reads. Expected {shape}."))
}

fn saved_account(root: &Path, key: &str) -> Result<Account, (u16, String)> {
    match find_account(root, key) {
        Ok(Some(a)) => Ok(a),
        Ok(None) => Err((400, format!("no account named \"{key}\" is saved on this machine - get_accounts lists the keys"))),
        Err(e) => Err((500, e)),
    }
}

const TRIAL_OFF_SITE: &str = "the proposed sign-in starts on a site this environment does not use - nothing was tried";
const TRIAL_NO_BASIS: &str = "set this environment's address or record a sign-in first - a trial never signs in on a site the app has not been given";
const BROWSER_WOULD_NOT_OPEN: &str = "the browser would not open - see Settings, Logs for the details";

/// The recipe a trial signs in with, and the draft laid over the saved one.
/// A trial types a stored password, so it only goes to a site the app was
/// given: the environment's own address and allowed sites when it has an
/// address, else the SAVED recipe's. The draft's allowed sites are never used.
fn trial_recipe(
    root: &Path,
    org: &str,
    project: &str,
    draft: &SignInRecipe,
    env: &crate::environments::Environment,
) -> Result<(SignInRecipe, SignInRecipe, Option<SignInRecipe>), (u16, String)> {
    let saved = load_recipe(root, org, project).map_err(|e| (500, e))?;
    let merged = sp::merge(draft, saved.as_ref());
    if env.start_url.trim().is_empty() {
        let Some(saved) = &saved else { return Err((400, TRIAL_NO_BASIS.to_string())) };
        let allowed = saved.origins();
        if !crate::autorun::recipe::origin_of(&merged.start_url).is_some_and(|o| allowed.contains(&o)) {
            return Err((400, TRIAL_OFF_SITE.to_string()));
        }
    }
    merged.validate().map_err(|why| (400, why))?;
    let to_save = for_saving(root, org, project, merged.clone()).map_err(|e| (500, e))?;
    let mut effective = effective_recipe(&to_save, env);
    if env.start_url.trim().is_empty() {
        // `merge` keeps the saved sites; with no saved recipe there is no way here.
        effective.allowed_origins = saved.as_ref().map(|s| s.allowed_origins.clone()).unwrap_or_default();
    }
    Ok((merged, effective, saved))
}

/// Why a trial did not sign in, in words with no address in them.
fn trial_failure(detail: &str, harness: bool, who: &Account) -> String {
    if harness {
        return CHECK_BROWSER_SILENT.to_string();
    }
    if detail.starts_with(crate::autorun::signin::PAGE_DID_NOT_OPEN) {
        return CHECK_START_DID_NOT_OPEN.to_string();
    }
    redact(detail, who)
}

/// `inspect_sign_in_page`: what the sign-in page shows (`read_sign_in_page`).
/// With an `account` and a draft `recipe` the draft is tried once first (a
/// trial, counted) and the page read is where it ended; with neither the
/// site's sign-in page is opened and read, nobody signed in and nothing
/// counted. Answers `200 {"page": ..., "sign_in"?: <the trial's words>}`.
pub async fn inspect_in<B: Browsers>(
    browsers: &mut B,
    root: &Path,
    org: &str,
    project: &str,
    body: &str,
    timing: &Timing,
) -> (u16, String) {
    if let Some(busy) = refusal() {
        return (409, busy);
    }
    let Ok(asked) = serde_json::from_str::<InspectBody>(if body.trim().is_empty() { "{}" } else { body }) else {
        return not_readable("inspect_sign_in_page", INSPECT_SHAPE);
    };
    let key = asked.account.as_deref().map(str::trim).filter(|k| !k.is_empty()).map(str::to_string);
    let trial = match (key, asked.recipe) {
        (None, None) => None,
        (Some(key), Some(draft)) => Some((key, draft)),
        _ => return (400, format!("{INSPECT_ONE_OF}. Expected {INSPECT_SHAPE}.")),
    };
    let env = match crate::environments::active(root) {
        Ok(e) => e,
        Err(e) => return (500, e),
    };
    let account = match &trial {
        Some((key, draft)) => {
            let who = match saved_account(root, key) {
                Ok(a) => a,
                Err(refused) => return refused,
            };
            if let Err(why) = draft.validate() {
                return (400, why);
            }
            match trial_recipe(root, org, project, draft, &env) {
                Ok((_, effective, _)) => Some((who, effective)),
                Err(refused) => return refused,
            }
        }
        None => None,
    };
    let open_at = match &account {
        Some(_) => None,
        None => match load_effective_recipe_if_any(root, org, project) {
            Ok(Some(r)) => Some(r),
            Ok(None) => return (400, NO_SITE_ADDRESS.to_string()),
            Err(e) => return (500, e),
        },
    };
    let _claim = match claim_check_browser() {
        Ok(c) => c,
        Err(why) => return (409, why),
    };
    // Dropped (the account let go) after the browser is closed.
    let mut lease = Held::setup();
    if let Some((who, _)) = &account {
        if let Err(why) = lease.hold(root, &who.key).await {
            return (409, redact(&why, who));
        }
        if let Err(why) = signin_attempts::take(root, &who.key) {
            return (409, why);
        }
    }
    let mut d = match browsers.open().await {
        Ok(d) => d,
        Err(why) => {
            crate::applog::warn(format!("inspect_sign_in_page: the browser would not open: {why}"));
            return (422, BROWSER_WOULD_NOT_OPEN.to_string());
        }
    };
    let answer = match (&account, &open_at) {
        (Some((who, effective)), _) => {
            let out = sign_in_trial(&mut d, root, effective, who, timing).await;
            let said = if out.ok { redact(&out.detail, who) } else { trial_failure(&out.detail, out.harness, who) };
            read_the_page(&mut d, Some(said)).await
        }
        (None, Some(effective)) => {
            let go = Action::Navigate { url: effective.start_url.clone() };
            let out = execute_in(&mut d, &go, timing, &Policy::only(effective.origins())).await;
            if out.ok {
                read_the_page(&mut d, None).await
            } else {
                crate::applog::warn(format!("inspect_sign_in_page: the sign-in address did not open: {}", out.detail));
                (422, if out.harness { CHECK_BROWSER_SILENT } else { CHECK_START_DID_NOT_OPEN }.to_string())
            }
        }
        (None, None) => (500, "there is nothing to open".to_string()),
    };
    browsers.close(d).await;
    drop(lease);
    answer
}

async fn read_the_page<D: Driver>(d: &mut D, sign_in: Option<String>) -> (u16, String) {
    match read_sign_in_page(d).await {
        Ok(page) => {
            let mut v = serde_json::json!({ "page": page });
            if let Some(words) = sign_in {
                v["sign_in"] = serde_json::json!(words);
            }
            (200, v.to_string())
        }
        Err(why) => (422, match sign_in {
            Some(words) => format!("{words} - {why}"),
            None => why,
        }),
    }
}

/// `propose_sign_in_recipe`: the draft is laid over the saved recipe
/// (`sp::merge`), tried once as `account` (a trial: it never sends a
/// change-password form), and kept for the person to approve only when it
/// signed in - or, with a change-password part, when it reached a page that
/// part describes. Nothing is saved to the project here.
pub async fn propose_in<B: Browsers>(
    browsers: &mut B,
    root: &Path,
    org: &str,
    project: &str,
    body: &str,
    timing: &Timing,
) -> (u16, String) {
    if let Some(busy) = refusal() {
        return (409, busy);
    }
    let Ok(asked) = serde_json::from_str::<ProposeBody>(body) else {
        return not_readable("propose_sign_in_recipe", PROPOSE_SHAPE);
    };
    let who = match saved_account(root, asked.account.trim()) {
        Ok(a) => a,
        Err(refused) => return refused,
    };
    if let Err(why) = asked.recipe.validate() {
        return (400, why);
    }
    let env = match crate::environments::active(root) {
        Ok(e) => e,
        Err(e) => return (500, e),
    };
    let (merged, effective, saved) = match trial_recipe(root, org, project, &asked.recipe, &env) {
        Ok(t) => t,
        Err(refused) => return refused,
    };
    if let Err(why) = check_marker_for(&merged.signed_in, &who) {
        return (400, why);
    }
    // Taken before the trial: what the proposal was made against.
    let hash = match sp::saved_hash(root, org, project) {
        Ok(h) => h,
        Err(e) => return (500, e),
    };
    let _claim = match claim_check_browser() {
        Ok(c) => c,
        Err(why) => return (409, why),
    };
    let mut lease = Held::setup();
    if let Err(why) = lease.hold(root, &who.key).await {
        return (409, redact(&why, &who));
    }
    if let Err(why) = signin_attempts::take(root, &who.key) {
        return (409, why);
    }
    let mut d = match browsers.open().await {
        Ok(d) => d,
        Err(why) => {
            crate::applog::warn(format!("propose_sign_in_recipe: the browser would not open: {why}"));
            return (422, format!("{NOTHING_PROPOSED_PREFIX}: {BROWSER_WOULD_NOT_OPEN}"));
        }
    };
    let tried = async {
        if let Err(why) = refuse_check_on_start_page(&mut d, &effective, timing).await {
            return Err(format!("nothing was proposed: {why}"));
        }
        let out = sign_in_trial(&mut d, root, &effective, &who, timing).await;
        if out.ok {
            return Ok(false);
        }
        let on_described_page = merged.password_reset.is_some()
            && is_password_expired(&out.detail)
            && match reset_page(&effective) {
                Some(page) => reset_fields_showing(&mut d, &page, timing).await,
                None => false,
            };
        if on_described_page {
            return Ok(true);
        }
        let why = trial_failure(&out.detail, out.harness, &who);
        crate::applog::warn(format!("a proposed sign-in as {} did not sign in: {why}", who.key));
        Err(format!("{NOTHING_PROPOSED_PREFIX}: {why}"))
    }
    .await;
    browsers.close(d).await;
    drop(lease);
    let reset_found = match tried {
        Ok(found) => found,
        Err(said) => return (422, said),
    };
    let mut said = sp::summary(&merged);
    if reset_found {
        said.push_str(". ");
        said.push_str(sp::RESET_PART_WORDS);
    }
    let mut changes = sp::changed_parts(&merged, saved.as_ref());
    if !env.start_url.trim().is_empty() {
        // The environment's own address is kept on approval: not a change.
        changes.retain(|c| c != "address");
    }
    let stored = StoredSignInProposal {
        recipe: merged,
        account: who.key.clone(),
        proposed_ms: crate::autorun::sessions::now_ms() as f64,
        summary: said.clone(),
        saved_hash: hash,
        changes,
    };
    if let Err(e) = sp::save(root, &env.id, org, project, &stored) {
        crate::applog::warn(format!("a proposed sign-in could not be kept: {e}"));
        return (500, PROPOSAL_NOT_KEPT.to_string());
    }
    crate::applog::info(format!("Auto-run sign-in proposed by the assistant (as {})", who.key));
    (200, serde_json::json!({ "proposed": true, "summary": said }).to_string())
}
