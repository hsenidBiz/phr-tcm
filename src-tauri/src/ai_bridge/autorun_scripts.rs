//! Saving and retiring Auto Run components, and reading and deleting a case's script.

use super::*;

/// `/autorun-component-save`: a component, under every save rule
/// (`components::save_tried`), against the open discovery: its area and
/// the components it tried that worked. With no discovery going, nothing
/// was tried, and the save is refused. The test cases of the saved scripts
/// that use it are read first (a read), so each of this project's is
/// checked against the new version; when they cannot be read, a save that
/// has users is refused.
pub(crate) async fn autorun_component_save(
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
    body: &str,
    n: SlotNo,
) -> (u16, String) {
    use crate::autorun::components::{users_of, Component, UserCases};
    const SHAPE: &str = "{ \"name\": <its name>, \"description\": <what it does>, \"inputs\": [{ \"name\", \"kind\": \"text\" or \"target\", \"description\" }], \"actions\": [<script actions>], \"why\": <why it changes, for a saved one> }";
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {SHAPE}.")),
    };
    if let Some(r) = v.get("actions").and_then(crate::autorun::forbidden_fixes::forbidden_in) {
        return (400, r);
    }
    let draft: Component = match serde_json::from_value(v.clone()) {
        Ok(c) => c,
        Err(e) => return (400, format!("that is not a component: {e}. Expected {SHAPE}.")),
    };
    let why = match v.get("why") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(_) => return (400, "\"why\" is one sentence".to_string()),
    };
    let dry_run = match v.get("dry_run") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(_) => return (400, "\"dry_run\" is true or false.".to_string()),
    };
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    // The scripts that use it, by case: this project's with their text,
    // and the rest. `save_tried` reads the users again under its lock.
    let users = users_of(&root, &draft.name).cases;
    let cases: Option<UserCases> = if users.is_empty() {
        Some(UserCases::default())
    } else {
        match client {
            None => None,
            Some(client) => match client.get_case_texts_with_projects(&ctx.org, &users).await {
                Ok(found) => {
                    let mut cases = UserCases::default();
                    for c in found {
                        if c.project.trim().eq_ignore_ascii_case(ctx.project.trim()) {
                            cases.here.insert(c.id, c.text);
                        }
                    }
                    cases.elsewhere = users.iter().copied().filter(|id| !cases.here.contains_key(id)).collect();
                    Some(cases)
                }
                Err(_) => {
                    // The error itself can carry the request's address; the
                    // line names only what could not be read.
                    crate::applog::warn(format!(
                        "Auto Run component {}: the test cases of the scripts that use it could not be read",
                        draft.name.trim()
                    ));
                    None
                }
            },
        }
    };
    let now = crate::autorun::sessions::now_ms();
    if unattended_run_is_using_the_browser().is_some() {
        // No discovery can be going while an unattended run has the
        // browser, so the save is refused for that, as it always was.
        let mut none: Option<crate::commands::autorun::Session> = None;
        if dry_run {
            return dry_run_component_in(&mut none, &root, &ctx.org, &ctx.project, draft, why.as_deref(), now, cases.as_ref());
        }
        return save_component_in(&mut none, &root, &ctx.org, &ctx.project, draft, why.as_deref(), now, cases.as_ref())
            .await;
    }
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    if dry_run {
        return dry_run_component_in(&mut slot, &root, &ctx.org, &ctx.project, draft, why.as_deref(), now, cases.as_ref());
    }
    save_component_in(&mut slot, &root, &ctx.org, &ctx.project, draft, why.as_deref(), now, cases.as_ref()).await
}

/// A component save's dry run (`"dry_run": true`) against the discovery
/// going in `slot`: every rule a save makes (`components::check_tried`),
/// answered as the save would answer it, or `would_save` with the version
/// it would become. Nothing is written, nothing is probed or recorded, and
/// it is not a try of the component.
#[allow(clippy::too_many_arguments)]
pub fn dry_run_component_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    draft: crate::autorun::components::Component,
    why: Option<&str>,
    now: u64,
    cases: Option<&crate::autorun::components::UserCases>,
) -> (u16, String) {
    use crate::autorun::components::{check_tried, TriedIn};
    let held = match slot.as_mut() {
        Some(b) => b.parts().discovery.as_ref().map(|d| (d.area.clone(), d.tried.clone())),
        None => None,
    };
    let session = held.as_ref().map(|(area, tried)| TriedIn { area: area.as_deref(), tried });
    match check_tried(root, organization, project, draft, why, session, now, cases) {
        Ok(would) => (
            200,
            serde_json::json!({
                "would_save": would.saved,
                "version": would.version,
                "changes": would.changes,
            })
            .to_string(),
        ),
        Err(why) => component_answer(Err(why)),
    }
}

/// A component save (`components::save_tried`) against the discovery going
/// in `slot`: its area and the components it tried that worked. Refused
/// only because some of its own locators were never seen, while that
/// discovery is open, the refused locators are checked on the discovery's
/// current page first (`record_refused_in`), and the save is tried once
/// more; the answer then says what was recorded.
#[allow(clippy::too_many_arguments)]
pub async fn save_component_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    draft: crate::autorun::components::Component,
    why: Option<&str>,
    now: u64,
    cases: Option<&crate::autorun::components::UserCases>,
) -> (u16, String) {
    use crate::autorun::components::{save_tried, TriedIn};
    let held = |slot: &mut Option<B>| match slot.as_mut() {
        Some(b) => b.parts().discovery.as_ref().map(|d| (d.area.clone(), d.tried.clone())),
        None => None,
    };
    let save = |held: &Option<(Option<String>, Vec<String>)>, draft: crate::autorun::components::Component| {
        let session = held.as_ref().map(|(area, tried)| TriedIn { area: area.as_deref(), tried });
        save_tried(root, organization, project, draft, why, session, now, cases)
    };
    let first = held(slot);
    let saved = save(&first, draft.clone());
    let Err(refused) = &saved else {
        return with_delete_notes(component_answer(saved), &draft);
    };
    // Refused by the check of its own locators, for nothing but locators
    // never seen: what that check alone says, recomputed here.
    let Some((area, _)) = first.as_ref() else {
        return component_answer(saved);
    };
    let area = area.as_deref().map(str::trim).filter(|a| !a.is_empty());
    let targets = crate::autorun::seen_check::load_checked_map(root, organization, project).ok().and_then(|map| {
        let own = crate::autorun::seen_check::check_component_seen(&map, area, &draft.actions);
        if own.as_ref().err() != Some(refused) {
            return None;
        }
        crate::autorun::seen_check::unseen_component_targets(&map, area, &draft.actions)
    });
    let Some(targets) = targets.filter(|t| !t.is_empty()) else {
        return component_answer(saved);
    };
    let Some(recorded) = record_refused_in(slot, root, organization, project, &targets).await else {
        return component_answer(saved);
    };
    if recorded.is_empty() {
        return after_recording(&recorded, component_answer(saved));
    }
    let again = save(&held(slot), draft.clone());
    after_recording(&recorded, component_answer(again))
}

/// A component save's answer with `notes` added when its actions delete by
/// position (a warning, never a refusal).
pub(crate) fn with_delete_notes(answer: (u16, String), draft: &crate::autorun::components::Component) -> (u16, String) {
    let notes = crate::autorun::script_notes::positional_delete_notes_for_actions(&draft.actions);
    if answer.0 != 200 || notes.is_empty() {
        return answer;
    }
    match serde_json::from_str::<serde_json::Value>(&answer.1) {
        Ok(mut v) if v.is_object() => {
            v["notes"] = serde_json::json!(notes);
            (answer.0, v.to_string())
        }
        _ => answer,
    }
}

/// A component save's answer: the saved name, version and changes as
/// JSON, or the refusal with its status.
pub(crate) fn component_answer(saved: Result<crate::autorun::components::Saved, String>) -> (u16, String) {
    use crate::autorun::components::{SIGN_IN_TO_CHECK_USERS, TRY_IT_FIRST};
    match saved {
        Ok(saved) => {
            crate::applog::info(format!("Auto Run component {} saved as version {}", saved.saved, saved.version));
            (200, serde_json::to_string(&saved).unwrap_or_default())
        }
        Err(why) if why == TRY_IT_FIRST => (409, why),
        Err(why) if why == SIGN_IN_TO_CHECK_USERS => (503, why),
        Err(why) => (400, why),
    }
}

/// The case id a script route names: a positive whole number.
pub(crate) fn script_case_id(found: Option<serde_json::Value>, shape: &str) -> Result<i32, (u16, String)> {
    let id = match found {
        Some(serde_json::Value::Number(n)) => n.as_i64(),
        Some(serde_json::Value::String(s)) => s.trim().parse::<i64>().ok(),
        _ => None,
    };
    match id.and_then(|n| i32::try_from(n).ok()).filter(|n| *n > 0) {
        Some(id) => Ok(id),
        None => Err((400, format!("\"case_id\" is a test case's id, a positive whole number. Expected {shape}."))),
    }
}

/// `GET /autorun-script?case_id=N`: case N's saved script, whole, as the
/// JSON `save_autorun_script` takes for one entry. 404 when the case has
/// none on this machine.
pub(crate) fn autorun_script_read(target: &str) -> (u16, String) {
    match autorun_root() {
        Ok(root) => autorun_script_read_at(&root, target),
        Err(refused) => refused,
    }
}

/// `autorun_script_read` against the store at `root`.
pub fn autorun_script_read_at(root: &std::path::Path, target: &str) -> (u16, String) {
    const SHAPE: &str = "?case_id=<the test case id>";
    let case_id = match script_case_id(q(target, "case_id").map(serde_json::Value::String), SHAPE) {
        Ok(id) => id,
        Err(refused) => return refused,
    };
    match crate::autorun::store::load_script(root, case_id) {
        Ok(Some(script)) => (200, serde_json::to_string_pretty(&script).unwrap_or_default()),
        Ok(None) => (404, crate::autorun::store::no_script(case_id)),
        Err(e) => {
            crate::applog::warn(format!("Auto Run: the script of case {case_id} could not be read: {e}"));
            (500, format!("the script of case {case_id} could not be read - see Settings, Logs"))
        }
    }
}

/// `POST /autorun-script-delete`, body `{ case_id }`: the script is moved
/// aside, not removed (`store::delete_script`), and scripts kept aside more
/// than 30 days are purged first. Refused while the case is being replayed.
/// A local file move: nothing reaches Azure DevOps.
pub(crate) fn autorun_script_delete(body: &str) -> (u16, String) {
    match autorun_root() {
        Ok(root) => autorun_script_delete_at(&root, body, crate::autorun::sessions::now_ms()),
        Err(refused) => refused,
    }
}

/// `autorun_script_delete` against the store at `root`, at `now` (epoch ms).
pub fn autorun_script_delete_at(root: &std::path::Path, body: &str, now: u64) -> (u16, String) {
    const SHAPE: &str = "{ \"case_id\": <the test case id> }";
    let case_id = match body_field(body, "case_id", SHAPE).and_then(|v| script_case_id(Some(v), SHAPE)) {
        Ok(id) => id,
        Err(refused) => return refused,
    };
    crate::autorun::store::purge_deleted_scripts(root, now);
    match crate::autorun::store::delete_script(root, case_id, now) {
        Ok(_) => {
            crate::applog::info(format!("Auto Run: the script of case {case_id} was deleted by the assistant (kept aside for 30 days)"));
            (
                200,
                serde_json::json!({
                    "deleted": case_id,
                    "sentence": format!("The script of case {case_id} is deleted. It is kept aside on this machine for {} days, then purged.", crate::autorun::store::DELETED_KEPT_DAYS),
                })
                .to_string(),
            )
        }
        Err(why) if why == crate::autorun::store::no_script(case_id) => (404, why),
        Err(why) if why == crate::autorun::store::replaying_refusal(case_id) => (409, why),
        Err(why) => {
            crate::applog::warn(format!("Auto Run: the script of case {case_id} could not be deleted: {why}"));
            (500, format!("the script of case {case_id} could not be deleted - see Settings, Logs"))
        }
    }
}

/// `/autorun-component-retire`: a component no saved script uses
/// (`components::remove_unused`). One in use stays, and the refusal names
/// the cases that use it.
pub(crate) fn autorun_component_retire(ctx: &BridgeContext, body: &str) -> (u16, String) {
    let name = match body_field(body, "name", "{ \"name\": <the component's name> }") {
        Ok(serde_json::Value::String(s)) if !s.trim().is_empty() => s,
        Ok(_) => return (400, "\"name\" is a saved component's name".to_string()),
        Err(refused) => return refused,
    };
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    match crate::autorun::components::remove_unused(&root, &ctx.org, &ctx.project, &name) {
        Ok(removed) => {
            crate::applog::info(format!("Auto Run component {removed} removed"));
            (200, serde_json::json!({ "removed": removed }).to_string())
        }
        Err(why) if why == crate::autorun::components::not_saved(&name) => (404, why),
        Err(why) => (409, why),
    }
}

/// A discovery's `navigate` or `open_tab` to a site outside `policy`'s,
/// refused. A relative address names no site: the runner makes it absolute
/// on the page and holds it to the same list.
pub(crate) fn refuse_outside_the_recipe(
    policy: &crate::browser::actions::Policy,
    action: &crate::browser::actions::Action,
) -> Result<(), (u16, String)> {
    for url in tried_addresses(action) {
        if let Some(origin) = crate::autorun::recipe::origin_of(url) {
            if !policy.allows(url) {
                return Err((
                    400,
                    format!(
                        "{origin} is not one of {} - a discovery goes only where the sign-in recipe does",
                        crate::browser::actions::ALLOWED_ORIGINS
                    ),
                ));
            }
        }
    }
    Ok(())
}

