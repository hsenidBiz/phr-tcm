//! Explore sessions: the change list, setups, `send_api_request` and the undo of a session's own creates.

use super::*;

/// An explore change list refusal as the route says it: a file that does
/// not read is a conflict, a change that could not be kept a server error,
/// an id with no session unknown, and anything else (an entry number the
/// list lacks, a verified create) a request that cannot be done.
pub(crate) fn explore_refusal(why: String) -> (u16, String) {
    use crate::autorun::explore as ex;
    if why == ex::CHANGE_NOT_KEPT {
        (500, why)
    } else if why == ex::CHANGES_UNREADABLE {
        (409, why)
    } else if ex::is_unknown_session(&why) {
        (404, why)
    } else {
        (400, why)
    }
}

/// `/autorun-explore-change`: one change the going explore session made,
/// onto its change list - `{ "change": "created" | "changed" | "deleted",
/// ... }` - or, for any saved session, going or ended, the entries the
/// assistant set back by hand: `{ "session": <id>, "undone": [<n>, ...] }`.
/// A create is checked against the saves the session's own browser made
/// (`explore::record_change`), never taken on the caller's word.
///
/// A session of another organisation or project than `org` and `project` is
/// refused, as the undo refuses it.
pub fn record_change_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    org: &str,
    project: &str,
    body: &str,
) -> (u16, String) {
    use crate::autorun::explore as ex;
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    if let Some(undone) = v.get("undone") {
        const SHAPE: &str = "{ \"session\": <the session id>, \"undone\": [<entry numbers>] }";
        let entries: Option<Vec<u32>> = undone
            .as_array()
            .and_then(|a| a.iter().map(|n| n.as_u64().and_then(|n| u32::try_from(n).ok())).collect());
        let (Some(session), Some(entries)) = (v.get("session").and_then(|s| s.as_str()), entries) else {
            return (400, format!("that is not an undone list. Expected {SHAPE}."));
        };
        let session = session.trim();
        match ex::load(root, session) {
            Ok(s) if !ex::ran_in(&s, org, project) => return (400, ex::other_project(&s)),
            Ok(_) => {}
            Err(why) if ex::is_unknown_session(&why) => return (404, why),
            Err(why) => {
                crate::applog::warn(format!("Auto Run explore session: {why}"));
                return (409, ex::CHANGES_UNREADABLE.to_string());
            }
        }
        return match ex::mark_undone(root, session, &entries) {
            Ok(()) => (200, serde_json::json!({ "session": session, "undone": entries }).to_string()),
            Err(why) => explore_refusal(why),
        };
    }
    // A route marked as one that only reads (or one that writes) is the
    // project's, not the session's: it needs no going session.
    let mut marked = None;
    if let Some(mark) = v.get("route_kind").filter(|m| !m.is_null()) {
        let route = mark.get("route").and_then(|r| r.as_str());
        let kind = match mark.get("kind").and_then(|k| k.as_str()).map(|k| k.trim().to_ascii_lowercase()).as_deref() {
            Some("reads") => Some(ex::RouteKind::Reads),
            Some("writes") => Some(ex::RouteKind::Writes),
            _ => None,
        };
        let (Some(route), Some(kind)) = (route, kind) else {
            return (400, ex::ROUTE_SHAPE.to_string());
        };
        if let Err(why) = ex::check_route_kind(route, kind) {
            return (400, why);
        }
        let said = format!(
            "marked {} as {} in this environment",
            route.trim(),
            if kind == ex::RouteKind::Reads { "reads" } else { "writes" }
        );
        if v.get("change").is_none() {
            return match ex::set_route_kind(root, route, kind) {
                Ok(()) => (200, serde_json::json!(said).to_string()),
                Err(why) => (400, why),
            };
        }
        // With a change, the mark is kept only once the change is taken.
        marked = Some((route.to_string(), kind, said));
    }
    let Some(browser) = slot.as_mut() else {
        return (409, ex::NO_EXPLORE_SESSION.to_string());
    };
    let parts = browser.parts();
    let Some(state) = parts.discovery.as_ref() else {
        return (409, ex::NO_EXPLORE_SESSION.to_string());
    };
    let Some(session) = state.explore_session().map(str::to_string) else {
        return (409, ex::NO_EXPLORE_SESSION.to_string());
    };
    // After the sign-in: the time the change list began, not the discovery's.
    let since = state.explore.as_ref().map_or(0, |run| run.began_ms);
    let change = match ex::change_from_args(&v) {
        Ok(change) => change,
        Err(why) => return (400, why),
    };
    let request = v.get("request").and_then(|r| r.as_str()).map(str::to_string);
    let covers = match ex::covers_from_args(&v) {
        Ok(covers) => covers,
        Err(why) => return (400, why),
    };
    let captured: Vec<&crate::browser::net_capture::Captured> = crate::browser::cdp::Driver::capture(&*parts.driver)
        .map(|c| c.list(0, usize::MAX).0)
        .unwrap_or_default();
    match ex::record_change_covering(root, &session, change, request, covers, &captured, since) {
        Ok(recorded) => {
            let mut out = serde_json::to_value(&recorded).unwrap_or_default();
            if let Some((route, kind, said)) = marked {
                match ex::set_route_kind(root, &route, kind) {
                    Ok(()) => out["route_kind"] = serde_json::json!(said),
                    Err(why) => out["route_kind"] = serde_json::json!(format!("not marked: {why}")),
                }
            }
            (200, out.to_string())
        }
        Err(why) => explore_refusal(why),
    }
}

/// `get_kept_actions`: the actions the owner's explore session kept, or why
/// there are none.
pub fn kept_actions_for(root: &std::path::Path, owner: &str) -> (u16, String) {
    match crate::autorun::kept::last_of(root, owner) {
        Ok(list) => (200, crate::autorun::kept::kept_answer(&list).to_string()),
        Err(why) => (409, why),
    }
}

/// `list_setups`: this project's saved setups, without actions, SQL or values.
pub fn setups_list_for(root: &std::path::Path, org: &str, project: &str) -> (u16, String) {
    match crate::autorun::setups::load_setups(root, org, project) {
        Ok(f) => (200, crate::autorun::setups::list_answer(&f).to_string()),
        Err(why) => (409, why),
    }
}

/// `save_setup`: a range of the owner's kept actions saved as a named setup
/// with a find rule. Everything is checked against files and memory on this
/// machine; the one thing that runs is the database find rule, once, as a
/// SELECT, when `rows` is given (Database Read Access is on). A body that is
/// not a save ask is 400 in serde's words.
pub async fn setup_save_for<S: crate::autorun::setup_find::RowSource>(
    root: &std::path::Path,
    org: &str,
    project: &str,
    owner: &str,
    body: &str,
    rows: Option<&S>,
    now: u64,
) -> (u16, String) {
    use crate::autorun::{explore as ex, kept, nav, seen_check, setup_find, setups};
    let asked: setups::SaveAsked = match serde_json::from_str(body) {
        Ok(a) => a,
        Err(e) => return (400, e.to_string()),
    };
    // A removal needs no explore session and no reason.
    if asked.remove {
        if let Err(why) = crate::autorun::needs_records::refuse_while_named(root, org, project, &asked.name, "removed") {
            return (409, why);
        }
        return match setups::remove(root, org, project, &asked.name) {
            Ok(name) => {
                crate::applog::info(format!("Auto Run setup {name}: removed"));
                (200, serde_json::json!({ "removed": name }).to_string())
            }
            Err(why) if why == setups::no_such_setup(&asked.name) => (404, why),
            Err(why) => (409, why),
        };
    }
    let list = match kept::last_of(root, owner) {
        Ok(l) => l,
        Err(why) => return (409, why),
    };
    let session = match ex::load(root, &list.session) {
        Ok(s) => s,
        Err(why) if ex::is_unknown_session(&why) => return (404, why),
        Err(why) => {
            crate::applog::warn(format!("Auto Run explore session: {why}"));
            return (409, ex::CHANGES_UNREADABLE.to_string());
        }
    };
    let prefix = crate::environments::active(root)
        .map(|e| e.test_prefix)
        .unwrap_or_else(|_| crate::environments::default_test_prefix());
    let recipe = match crate::autorun::recipe::load_effective_recipe(root, org, project) {
        Ok(r) => r,
        Err(why) => return (409, why),
    };
    let policy = crate::autorun::runner::policy_for(Some(&recipe));
    let allowed = |url: &str| policy.allows(url);
    // The map and the areas are read only when the ask needs them.
    let seen = |area: Option<&str>, actions: &[crate::browser::actions::Action]| {
        let map = seen_check::load_checked_map(root, org, project)?;
        seen_check::check_component_seen(&map, area, actions)
    };
    let area_known = |area: &str| nav::load_nav(root, org, project).is_ok_and(|n| nav::find_area(&n, area).is_some());
    let accounts = match crate::autorun::accounts::load_accounts(root) {
        Ok(a) => a,
        Err(_) => return (409, "The saved accounts could not be read, so nothing was saved.".to_string()),
    };
    let built = {
        let from = setups::SaveFrom {
            accounts: &accounts,
            kept: &list,
            session: &session,
            prefix: &prefix,
            allowed: &allowed,
            seen: &seen,
            area_known: &area_known,
            now,
        };
        match setups::build_setup(&asked, &from) {
            Ok(b) => b,
            Err(problems) => return (400, problems.join(" ")),
        }
    };
    let database_checked = match &built.setup.find.database {
        None => None,
        Some(db) => Some(match rows {
            None => "not run: Database Read Access is off",
            Some(rows) => match setup_find::find_in_database(rows, db, &prefix).await {
                setup_find::DbOutcome::Found(_) => "found",
                setup_find::DbOutcome::NotFound => "not found",
                setup_find::DbOutcome::CouldNotRun(why) => {
                    return (400, format!("The database find rule did not run: {why}"))
                }
            },
        }),
    };
    if built.setup.find.database.is_none() {
        if let Err(why) = crate::autorun::needs_records::refuse_while_named(
            root,
            org,
            project,
            &built.setup.name,
            "saved without a database find rule",
        ) {
            return (409, why);
        }
    }
    let setup = built.setup.clone();
    let (replaced, cut) = (built.replaced.clone(), built.cut);
    let saved = match setups::save(root, org, project, built, asked.why.as_deref()) {
        Ok(s) => s,
        Err(why) => return (409, why),
    };
    crate::applog::info(format!("Auto Run setup {}: saved, version {}", saved.saved, saved.version));
    let mut answer = serde_json::json!({
        "saved": saved.saved,
        "version": saved.version,
        "changes": saved.changes,
        "actions": setup.actions.len(),
        "area": setup.area,
        "inputs": setup.inputs.iter().map(|i| i.name.clone()).collect::<Vec<_>>(),
        "replaced": replaced,
        "creates": setup
            .creates
            .iter()
            .map(|c| serde_json::json!({ "kind": c.kind, "request": c.request }))
            .collect::<Vec<_>>(),
        "returns": setup.creates.get(setup.returns).map(|c| c.kind.clone()),
        "cut": cut,
    });
    if let (Some(checked), Some(map)) = (database_checked, answer.as_object_mut()) {
        map.insert("database_checked".to_string(), serde_json::Value::String(checked.to_string()));
    }
    (200, answer.to_string())
}

/// A discovery started in `slot` for `caller`: its previous kept actions are
/// dropped, and an explore session with a change list gets a list of its own.
pub(crate) async fn kept_started(slot: SlotNo, caller: &crate::autorun::agents::Caller) {
    let Ok(root) = autorun_root() else {
        return;
    };
    let mut held = crate::commands::autorun::session_slot(slot).lock().await;
    kept_started_in(&root, &mut held, caller);
}

/// `kept_started` over the browser in `held`: the discovery it holds decides
/// whether `caller` gets a kept list (an explore session with a change list)
/// or none.
pub fn kept_started_in<B: DiscoveryBrowser>(
    root: &std::path::Path,
    held: &mut Option<B>,
    caller: &crate::autorun::agents::Caller,
) {
    let (session, area) = held
        .as_mut()
        .and_then(|b| b.parts().discovery.as_ref().map(|s| (s.explore_session().map(str::to_string), s.area.clone())))
        .unwrap_or((None, None));
    crate::autorun::kept::started(root, &crate::autorun::kept::owner_of(caller), session.as_deref(), area.as_deref());
}

/// `/autorun-explore-changes`: `{}` lists this project's explore sessions,
/// newest first, each counted; `{ "session": <id> }` gives one session's
/// whole change list, the saves its browser sent that no change accounts for,
/// and its `verified_creates` (kind, id, name and the request that proved
/// each: what an undo would delete). A session of another project is refused.
pub fn explore_changes(root: &std::path::Path, org: &str, project: &str, body: &str) -> (u16, String) {
    use crate::autorun::explore as ex;
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    if let Some(id) = v.get("session").and_then(|s| s.as_str()) {
        return match ex::load(root, id.trim()) {
            Ok(s) if !ex::ran_in(&s, org, project) => (400, ex::other_project(&s)),
            Ok(s) => {
                let mut out = serde_json::to_value(&s).unwrap_or_default();
                out["saves_not_on_the_change_list"] = serde_json::json!(ex::unaccounted_in(root, &s));
                // What an undo would delete, as the person is told it first.
                out["verified_creates"] = serde_json::json!(ex::verified_creates(&s));
                (200, out.to_string())
            }
            Err(why) if ex::is_unknown_session(&why) => (404, why),
            Err(why) => {
                crate::applog::warn(format!("Auto Run explore session: {why}"));
                (409, ex::CHANGES_UNREADABLE.to_string())
            }
        };
    }
    let sessions: Vec<serde_json::Value> = ex::list(root, org, project)
        .iter()
        .map(|s| {
            serde_json::json!({
                "session": s.session,
                "started_at": s.started_at,
                "ended_at": s.ended_at,
                "account": s.account,
                "environment": s.environment_name,
                "changes": s.changes.len(),
                "undone": s.changes.iter().filter(|c| c.undone).count(),
            })
        })
        .collect();
    (200, serde_json::json!({ "sessions": sessions }).to_string())
}

/// `/api-template-send`: one GET or POST from the page the discovery's
/// browser is on, in the account it signed in as - no sign-in of its own, no
/// lease of its own. A POST also needs an explore session (so what it
/// changes goes on a change list) and the person's API templates switch.
/// Every refusal comes before anything is sent: the body, then the switch,
/// then the session, the no-save guard, the account the browser still holds,
/// the one-at-a-time slot and the run's own checks. A POST that was sent
/// without being a read is noted on the session's list of saves
/// (`explore::note_writes`); a create that worked goes on the change list
/// from the response's own id (`explore::record_send_create`); an edit or a
/// delete says to record what it changed.
pub async fn send_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    ctx: &BridgeContext,
    root: &std::path::Path,
    body: &str,
    timing: &crate::browser::timing::Timing,
    limit: std::time::Duration,
    retry_pauses: &[std::time::Duration],
) -> (u16, String) {
    use crate::api_templates::one_off as oo;
    use crate::api_templates::runner::{claim, preflight, run_in_page, Mode, RunRequest, TokenPage};
    use crate::api_templates::{Effect, Method};
    use crate::autorun::explore as ex;
    let one = match oo::parse(body) {
        Ok(one) => one,
        Err(refused) => return refused,
    };
    let posts = one.step.method() == Method::Post;
    if posts && !ctx.api_writes {
        return (400, API_WRITES_OFF.to_string());
    }
    let path = oo::filled_path(&one);
    let Some(browser) = slot.as_mut() else {
        return (409, oo::NO_SESSION_FOR_SEND.to_string());
    };
    let parts = browser.parts();
    let Some(state) = parts.discovery.as_ref() else {
        return (409, oo::NO_SESSION_FOR_SEND.to_string());
    };
    // An earlier call found this discovery's account signed out; nothing
    // more is sent on it, however long this call waited for the slot.
    if state.signed_out {
        return (409, oo::SIGNED_OUT_BEFORE.to_string());
    }
    let session = state.explore_session().map(str::to_string);
    let floor = state.explore.as_ref().map_or(0, |run| run.began_ms);
    let kept_before = state.kept.clone();
    if posts && session.is_none() {
        return (409, oo::POST_NEEDS_EXPLORE.to_string());
    }
    if posts && discovery_must_not_save(root, state) {
        return (409, crate::browser::save_guard::blocked("POST", &path));
    }
    let account = match state.account.clone() {
        Some(account) if parts.lease.holds(root, &account) => account,
        _ => return (409, oo::SESSION_NOT_SIGNED_IN.to_string()),
    };
    let Some(_claim) = claim() else {
        return (409, API_TEMPLATE_BUSY.to_string());
    };
    if posts {
        if let Err(moved) = assistant_may_still_write(ctx, root) {
            return (409, moved);
        }
    }
    let page = one.antiforgery_page.clone();
    // Each kept value the call uses goes to the run as a param named
    // `var.<name>`; the saved template never sees it (`one` is untouched).
    let mut template = oo::as_template(&one, oo::ONE_OFF_ID, page.as_deref().unwrap_or("/"));
    let mut vars: std::collections::BTreeMap<String, serde_json::Value> = Default::default();
    for full in oo::vars_used(&one) {
        let Some(value) = kept_before.get(&full[crate::api_templates::exec::VAR_PREFIX.len()..]) else {
            return (400, oo::var_not_kept(&full));
        };
        vars.insert(full, value.clone());
    }
    let mut run_values = serde_json::Map::new();
    for (k, v) in &one.values {
        run_values.insert(k.clone(), crate::api_templates::exec::substitute(v, &vars));
    }
    for (full, value) in &vars {
        use crate::api_templates::ParamType;
        let kind = match value {
            serde_json::Value::Number(_) => ParamType::Number,
            serde_json::Value::Bool(_) => ParamType::Boolean,
            serde_json::Value::Array(_) => ParamType::List,
            _ => ParamType::String,
        };
        template.params.push(crate::api_templates::Param {
            name: full.clone(),
            kind,
            required: true,
            description: None,
            lookup: None,
            default: None,
        });
        run_values.insert(full.clone(), value.clone());
    }
    let req = RunRequest {
        org: ctx.org.clone(),
        project: ctx.project.clone(),
        account,
        values: run_values,
        mode: Mode::Explore,
        template,
        step_limit: one.timeout_s.map(|s| std::time::Duration::from_secs(u64::from(s))),
        keep: one.keep.clone(),
    };
    if let Err(problems) = preflight(root, &req, None) {
        return (400, problems.join("\n"));
    }
    let how = match page {
        Some(p) => TokenPage::Open(p),
        None => TokenPage::Here,
    };
    // What the capture saw before this call was sent is what its answer is
    // judged against.
    let sent_at = crate::autorun::sessions::now_ms();
    // A call that asked for longer steps gets a run that can hold one.
    let limit = crate::api_templates::runner::run_limit_for(limit, req.step_limit);
    let named_a_page = matches!(how, TokenPage::Open(_));
    let mut out = run_in_page(parts.driver, root, &req, timing, how.clone(), limit, retry_pauses).await;
    // A POST from a page with no token, naming no page to read one on: the
    // run stopped before anything was sent, so it is safe to say where to
    // look. One page used by the project's templates is opened and tried
    // once; with none or several, the caller is told which there are.
    let mut token_page_used: Option<String> = None;
    let mut no_token_detail: Option<String> = None;
    let no_token_stop = posts
        && !one.step.is_postback()
        && !named_a_page
        && out.answer.is_none()
        && out.report.failed.as_deref() == Some("Anti-forgery token")
        && out.report.steps.iter().any(|s| s.detail.contains(crate::api_templates::runner::NO_TOKEN_HERE));
    if no_token_stop {
        let listed = crate::api_templates::store::list(root, &ctx.org, &ctx.project);
        if let Err(why) = &listed {
            crate::applog::warn(format!("Auto Run explore session: the saved templates could not be read for a token page: {why}"));
        }
        let pages = listed.as_ref().map(|saved| oo::token_pages(saved)).unwrap_or_default();
        if listed.is_err() {
            no_token_detail = Some(oo::NO_TOKEN_UNREADABLE.to_string());
        } else if let [page] = pages.as_slice() {
            token_page_used = Some(format!("{page} (the page this project's templates use)"));
            out = run_in_page(parts.driver, root, &req, timing, TokenPage::Open(page.clone()), limit, retry_pauses).await;
        } else {
            no_token_detail = Some(oo::no_token_here(&pages));
        }
    }

    let report = &out.report;
    let request = oo::request_of(&one);
    // A postback that timed out, or whose browser stopped answering, after the
    // post went out has no status but may have saved: it counts as sent.
    // A plain step that ran out of time may have been processed too
    // (`SENT_NO_ANSWER`, `MAYBE_SENT_NO_ANSWER`), so it counts the same.
    let timed_out = report.steps.iter().any(|s| {
        s.detail.contains(crate::api_templates::runner::SENT_NO_ANSWER)
            || s.detail.contains(crate::api_templates::runner::MAYBE_SENT_NO_ANSWER)
            // A postback that ran out of time after it may have gone out.
            || (s.detail.contains(crate::api_templates::runner::STEP_TOO_LONG)
                && s.detail.contains(crate::api_templates::postback::MAY_HAVE_SAVED))
    });
    let sent = timed_out
        || report.steps.iter().any(|s| s.status.is_some() || s.detail.contains(crate::api_templates::postback::MAY_HAVE_SAVED));
    // Every POST that was sent is a save, whatever the call said it does: a
    // "read" that wrote must not leave the session's saves looking clean.
    if posts && sent {
        if let Some(session) = &session {
            if let Err(why) = ex::note_writes(root, session, &[("POST".to_string(), oo::request_path(&one))]) {
                crate::applog::warn(format!("Auto Run explore session: a save could not be noted: {why}"));
            }
        }
    }
    // A GET to a route the project marked as one that changes data is a save
    // too (`record_explore_change` with `route_kind`).
    let writing_get = !posts && sent && ex::counts_as_save(root, "GET", &oo::request_path(&one));
    if writing_get {
        if let Some(session) = &session {
            if let Err(why) = ex::note_writes(root, session, &[("GET".to_string(), oo::request_path(&one))]) {
                crate::applog::warn(format!("Auto Run explore session: a save could not be noted: {why}"));
            }
        }
    }
    // The answer is the app's sign-in page: this discovery's account is signed
    // out. The answer is not shown (it is the sign-in page, not data), the
    // saved session is forgotten so the next discovery signs in afresh, and
    // nothing more is sent on this one.
    let start_path = crate::autorun::recipe::load_effective_recipe_if_any(root, &ctx.org, &ctx.project)
        .ok()
        .flatten()
        .map(|recipe| oo::sign_in_path(&recipe.start_url));
    // A postback is judged by the postback rule (`postback.rs`); when that
    // rule refused it as signed out, later sends on this discovery are
    // refused too.
    let postback_signed_out = one.step.is_postback()
        && report.steps.iter().any(|s| s.detail.contains(crate::api_templates::runner::SENT_ELSEWHERE));
    if postback_signed_out {
        if let Some(d) = parts.discovery.as_mut() {
            d.signed_out = true;
        }
        parts.signed_in.take();
        crate::autorun::sessions::forget_session(root, &req.account);
        crate::applog::warn("Auto Run explore session: a postback was answered with the sign-in page, so its account is signed out");
    }
    let signed_out = !one.step.is_postback()
        && out.answer.as_ref().is_some_and(|a| {
            oo::is_sign_in_answer(
                a.final_path.as_deref(),
                a.content_type.as_deref(),
                &a.body,
                &oo::request_path(&one),
                start_path.as_deref(),
            )
        });
    if signed_out {
        if let Some(d) = parts.discovery.as_mut() {
            d.signed_out = true;
        }
        parts.signed_in.take();
        crate::autorun::sessions::forget_session(root, &req.account);
        crate::applog::warn("Auto Run explore session: a call was answered with the sign-in page, so its account is signed out");
        let v = serde_json::json!({ "ok": false, "signed_out": true, "detail": oo::SIGNED_OUT, "request": request });
        return (502, v.to_string());
    }
    // What the discovery keeps now: what it kept, and what this call found
    // when it worked. A value that was found but not kept (a failed call) is
    // still hidden below.
    let mut kept_lines = Vec::new();
    let mut keep_failed = Vec::new();
    let mut kept_now = kept_before;
    if report.ok {
        for (name, path) in &one.keep {
            match out.kept.get(name) {
                Some(value) => {
                    let len = match value {
                        serde_json::Value::String(s) => s.chars().count(),
                        other => other.to_string().chars().count(),
                    };
                    kept_lines.push(format!("{name} ({len} characters)"));
                    kept_now.insert(name.clone(), value.clone());
                }
                None => keep_failed.push(format!("{name}: found nothing at {path}")),
            }
        }
        if let Some(d) = parts.discovery.as_mut() {
            d.kept = kept_now.clone();
        }
    }
    let hide = {
        let mut all = serde_json::Map::new();
        for (name, value) in kept_now.iter().chain(out.kept.iter()) {
            all.insert(format!("{}{name}", crate::api_templates::exec::VAR_PREFIX), value.clone());
        }
        crate::api_templates::exec::hidden_values(&all)
    };
    // The browser's capture saw this call's fetch, so it hides these values
    // for as long as the browser lives (not only while this discovery does).
    if let Some(cap) = crate::browser::cdp::Driver::capture_mut(parts.driver) {
        cap.mask(&hide);
    }
    let mut v = serde_json::json!({
        "ok": report.ok,
        "status": out.answer.as_ref().map(|a| a.status),
        "captured": report.created,
        "request": request,
    });
    if let Some(page) = &token_page_used {
        v["token_page"] = serde_json::json!(page);
    }
    if !kept_lines.is_empty() {
        v["kept"] = serde_json::json!(kept_lines);
    }
    if !keep_failed.is_empty() {
        v["keep_failed"] = serde_json::json!(keep_failed);
    }
    if let Some(a) = &out.answer {
        v["response"] = serde_json::json!({
            "content_type": a.content_type,
            "body": crate::autorun::capture_redact::redact_body(
                a.content_type.as_deref(),
                &crate::api_templates::exec::hide_text(&a.body, &hide),
            ),
            "cut": a.cut,
        });
    }
    if let Some(carried) = out.postback_carried {
        v["postback"] = serde_json::json!(if carried { oo::CARRIED_ON } else { oo::LOADED_FRESH });
    }
    if postback_signed_out {
        v["signed_out"] = serde_json::json!(true);
    }
    if !report.ok {
        v["failed"] = serde_json::json!(report.failed);
        v["detail"] = serde_json::json!(no_token_detail.unwrap_or_else(|| report.message()));
        if timed_out {
            v["timed_out"] = serde_json::json!(true);
        }
    }
    match (one.effect, &one.created, &session) {
        (Effect::Create, Some(made), Some(session)) if report.ok => {
            // The id is verified, and so can be deleted by an undo, only when
            // it is new to this session (`explore::send_create_is_new`): the
            // call did not send it, nothing the page showed or sent before
            // carried it, and no change on the list names it. With no capture
            // to prove that it is kept, not verified.
            let captured: Option<Vec<&crate::browser::net_capture::Captured>> =
                crate::browser::cdp::Driver::capture(&*parts.driver).map(|c| c.list(0, usize::MAX).0);
            let answered_2xx = out.answer.as_ref().is_some_and(|a| (200..300).contains(&a.status));
            let is_new = |id: &str, earlier: &[ex::ChangeEntry]| {
                answered_2xx
                    && ex::send_create_is_new(captured.as_deref(), floor, sent_at, id, earlier, oo::sends_id(&one, id))
            };
            let kept = report
                .created
                .get(&made.id)
                .ok_or_else(|| ex::CHANGE_NOT_KEPT.to_string())
                .and_then(|id| ex::record_send_create(root, session, &made.kind, id, &made.name, &request, &is_new));
            match kept {
                Ok((n, verified)) => {
                    v["change"] = serde_json::json!(n);
                    v["verified"] = serde_json::json!(verified);
                    if !verified {
                        v["note"] = serde_json::json!(ex::NOT_VERIFIED);
                    }
                }
                Err(why) => {
                    crate::applog::warn(format!("Auto Run explore session: a create could not be kept: {why}"));
                    v["change_not_kept"] = serde_json::json!(why);
                }
            }
        }
        (Effect::Edit, _, _) if sent => v["record_change"] = serde_json::json!(oo::edit_reminder(&request)),
        _ if writing_get => v["record_change"] = serde_json::json!(oo::edit_reminder(&request)),
        (Effect::Delete, _, _) if sent => v["record_change"] = serde_json::json!(oo::delete_reminder(&request)),
        _ => {}
    }
    if report.ok {
        match &session {
            // The call said not to keep it.
            _ if !one.save => {
                v["template"] = serde_json::json!({ "not_saved": oo::NOT_SAVED_ASKED });
            }
            // A saved template cannot reach a kept value.
            _ if oo::step_uses_var(&one) => {
                v["template"] = serde_json::json!({ "not_saved": oo::VAR_NOT_SAVED });
            }
            // A postback may have carried on from the call before it, so it
            // cannot stand alone as a template.
            _ if one.step.is_postback() => {
                v["template"] = serde_json::json!({ "not_saved": crate::api_templates::postback::POSTBACK_NOT_SAVED });
            }
            Some(session) => {
                let status = out.answer.as_ref().map(|a| a.status);
                let token = (out.token_page.as_deref(), out.token_found);
                // What this session's earlier one-offs saved, which a later working
                // call of the same route may replace.
                let mut none = Vec::new();
                let auto_saved = parts.discovery.as_mut().and_then(|d| d.explore.as_mut()).map_or(&mut none, |run| &mut run.auto_saved);
                v["template"] = save_or_match_one_off(root, ctx, &req.account, session, &one, status, token, report, auto_saved);
            }
            // A plain discovery has no session to name as the proof.
            None => v["template"] = serde_json::json!({ "not_saved": oo::PLAIN_DISCOVERY_NOT_SAVED }),
        }
    }
    crate::api_templates::exec::hide_json(&mut v, &hide);
    (if report.ok { 200 } else { 502 }, v.to_string())
}

/// Said, as `ended_session`, when an undo ended the going discovery first.
pub const ENDED_FOR_UNDO: &str =
    "Ended your explore session and closed its browser first: Clean up signs in as the delete template's account.";

/// What `/api-template-explore-undo` expects.
pub(crate) const UNDO_SHAPE: &str = "{ \"session\": \"<the session id>\", \"ids\": [\"<id>\", ...], \"person_said_yes\": true, \"kind\": \"<kind, only when an id is two kinds>\" }";

/// Said to an undo the person has not said yes to in the chat.
pub const UNDO_NEEDS_YES: &str = "Tell the person exactly what will be deleted (each record's kind, id and name, from get_explore_changes) and wait for their yes in the chat first.";

/// Said when the account of a delete template is held by a discovery or
/// mapping run that is not the assistant's own explore session.
pub const UNDO_ACCOUNT_IN_USE: &str = "A discovery or mapping run that is not your explore session is using that account's browser: end it with end_autorun_discovery, then ask again.";

/// `/api-template-explore-undo`: delete, through the proven delete templates,
/// the creates `ids` of one explore session - and only those: an id must be
/// one the test-made list holds as this session's verified create, made in
/// the active environment and in the project open now. The person's ask is the gate, so it also
/// needs the API templates switch. Every refusal comes before anything is
/// deleted: the switch, the body, the session, the one-at-a-time slot, then
/// the plan, which refuses the whole undo for one id that is not the
/// session's.
///
/// Clean up signs in as the delete template's proven account, and an account
/// is held by one browser at a time. A going discovery whose browser holds
/// the account of a template this undo needs is therefore ended and its
/// browser closed first (`ended_session`); one on another account is left
/// alone. Each delete that worked marks its create undone on the change
/// list. The answer is `{ results, total, stopped, ended_session? }`.
pub async fn explore_undo_in<S: DiscoveryBrowser, B: crate::autorun::replay::Browsers>(
    slot: &mut Option<S>,
    browsers: &mut B,
    ctx: &BridgeContext,
    root: &std::path::Path,
    body: &str,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    use crate::autorun::explore as ex;
    use serde_json::Value;
    if !ctx.api_writes {
        return (400, API_WRITES_OFF.to_string());
    }
    let shape = || (400, format!("that is not an undo. Expected {UNDO_SHAPE}."));
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return shape();
    };
    let Some(session) = v.get("session").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) else {
        return shape();
    };
    let ids: Option<Vec<String>> = v.get("ids").and_then(Value::as_array).and_then(|a| {
        a.iter()
            .map(|i| match i {
                Value::String(s) => Some(s.trim().to_string()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect()
    });
    let Some(ids) = ids else {
        return shape();
    };
    let kind = match v.get("kind") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.trim().to_string()),
        Some(_) => return shape(),
    };
    // Like a delete call, an undo deletes only after the person said yes in
    // the chat to a message naming what will go.
    if v.get("person_said_yes").and_then(Value::as_bool) != Some(true) {
        return (400, UNDO_NEEDS_YES.to_string());
    }
    let session_file = match ex::load(root, session) {
        Ok(file) => file,
        Err(why) => {
            return if ex::is_unknown_session(&why) {
                (404, why)
            } else {
                crate::applog::warn(format!("Auto Run explore session: {why}"));
                (409, ex::CHANGES_UNREADABLE.to_string())
            };
        }
    };
    // The slot first, as every run does: holding it is what refuses an
    // environment switch while the plan and the deletes are going.
    let Some(_claim) = crate::api_templates::runner::claim() else {
        return (409, API_TEMPLATE_BUSY.to_string());
    };
    if let Err(moved) = assistant_may_still_write(ctx, root) {
        return (409, moved);
    }
    let plan = match crate::autorun::cleanup::undo_plan(root, &ctx.org, &ctx.project, session, &ids, kind.as_deref()) {
        Ok(plan) => plan,
        Err(why) => return (400, why),
    };

    let accounts: Vec<String> = plan
        .iter()
        .filter_map(|(_, t)| t.as_ref().and_then(|t| t.proven.as_ref()).map(|p| p.account.clone()))
        .collect();
    // (holds an account of the plan, and is the assistant's own explore session)
    let held = slot.as_mut().and_then(|b| {
        let p = b.parts();
        let explore = p.discovery.as_ref()?.explore.is_some();
        accounts.iter().any(|a| p.lease.holds(root, a)).then_some(explore)
    });
    // Only the assistant's own explore session is ended for an undo. Any
    // other discovery or mapping run that holds the account is the person's
    // to end: refused before anything is deleted.
    if held == Some(false) {
        return (409, UNDO_ACCOUNT_IN_USE.to_string());
    }
    let mut ended = None;
    let mut explore_end = None;
    if held == Some(true) {
        explore_end = close_discovery(slot).and_then(|(_, explore)| explore);
        ended = Some(ENDED_FOR_UNDO);
    }

    let cancel = std::sync::atomic::AtomicBool::new(false);
    let report = match crate::autorun::cleanup::undo_explore_session(
        browsers,
        root,
        &ctx.org,
        &ctx.project,
        session,
        &ids,
        kind.as_deref(),
        timing,
        &cancel,
        |_| {},
    )
    .await
    {
        Ok(report) => report,
        Err(why) => {
            // The session was closed above whatever came of the deletes.
            let mut said = ended.map_or(why.clone(), |said| format!("{why}\n{said}"));
            if let Some(end) = explore_end.as_ref().filter(|e| !e.saves_not_on_the_change_list.is_empty()) {
                said.push_str(&format!(
                    "\nSaves not on the change list: {}. {}",
                    end.saves_not_on_the_change_list.join(", "),
                    ex::UNRECORDED_SAVES
                ));
            }
            return (400, said);
        }
    };

    let mut not_marked = Vec::new();
    for r in report.results.iter().filter(|r| r.ok) {
        match ex::mark_create_undone(root, session, &r.kind, &r.id) {
            Ok(true) => {}
            Ok(false) => not_marked.push(r.id.clone()),
            Err(why) => {
                crate::applog::warn(format!("Auto Run explore session: an undone create could not be marked: {why}"));
                not_marked.push(r.id.clone());
            }
        }
    }
    let mut answer = serde_json::json!({
        "results": report.results,
        "total": report.total,
        "stopped": report.stopped,
    });
    // What was deleted, as the person was told it: each verified create the
    // undo was asked for, with the request that proved it.
    answer["creates"] = serde_json::json!(ex::verified_creates(&session_file)
        .into_iter()
        .filter(|c| c["id"].as_str().is_some_and(|id| ids.iter().any(|i| i == id)))
        .collect::<Vec<_>>());
    if let Some(said) = ended {
        answer["ended_session"] = serde_json::json!(said);
    }
    if let Some(end) = explore_end {
        if !end.saves_not_on_the_change_list.is_empty() {
            answer["note"] = serde_json::json!(ex::UNRECORDED_SAVES);
        }
        answer["explore"] = serde_json::json!(end);
    }
    if !not_marked.is_empty() {
        answer["not_marked_undone"] = serde_json::json!(not_marked);
    }
    (200, answer.to_string())
}

/// `POST /api-template-explore-undo` with slot `n`'s own session and real
/// browsers.
pub(crate) async fn api_template_explore_undo(ctx: &BridgeContext, body: &str, n: SlotNo) -> (u16, String) {
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    let root = match autorun_root() {
        Ok(root) => root,
        Err(refused) => return refused,
    };
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    // Looked at again now the lock is held: a run may have started while
    // this call waited for it.
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    let mut browsers = real_template_browsers(crate::browser::launch::Browser::Edge);
    explore_undo_in(&mut slot, &mut browsers, ctx, &root, body, &crate::commands::autorun_replay::replay_timing(false)).await
}

/// The proof a working explore call gives the template it saves.
pub(crate) fn explore_proof(
    root: &std::path::Path,
    org: &str,
    project: &str,
    account: &str,
    report: &crate::api_templates::runner::RunReport,
) -> crate::api_templates::Proven {
    crate::api_templates::Proven {
        at: crate::applog::stamp(),
        origin: recipe_origin(root, org, project).unwrap_or_default(),
        account: account.to_string(),
        outputs: report.outputs.clone(),
        environment: crate::environments::active(root).ok().map(|e| e.name),
        via: Some(crate::api_templates::EXPLORE_PROOF.to_string()),
    }
}

/// Adds a working explore call to template `id`'s run history.
pub(crate) fn log_explore_run(
    root: &std::path::Path,
    org: &str,
    project: &str,
    account: &str,
    report: &crate::api_templates::runner::RunReport,
    id: &str,
) {
    let record = crate::api_templates::store::RunRecord {
        at: crate::applog::stamp(),
        mode: crate::api_templates::runner::Mode::Explore.label().to_string(),
        account: account.to_string(),
        ok: true,
        failed_step: None,
        detail: None,
        outputs: report.outputs.clone(),
    };
    if let Err(e) = crate::api_templates::store::append_run(root, org, project, id, record) {
        crate::applog::warn(format!("api template {id}: the run could not be added to its history: {e}"));
    }
}

/// What `send_in` answers under `template` for a call that worked: it is
/// not saved (a delete, a secret written out, a page with no token), or it
/// is the same call as a template already saved (`reused`), or it replaces
/// the template an earlier call of this session auto-saved (`replaced`), or it is saved
/// now as a one-step template proven by the session that made it. A save
/// that fails is logged and answered without a path.
#[allow(clippy::too_many_arguments)]
pub(crate) fn save_or_match_one_off(
    root: &std::path::Path,
    ctx: &BridgeContext,
    account: &str,
    session: &str,
    one: &crate::api_templates::one_off::OneOff,
    status: Option<u16>,
    token: (Option<&str>, bool),
    report: &crate::api_templates::runner::RunReport,
    auto_saved: &mut Vec<String>,
) -> serde_json::Value {
    use crate::api_templates::one_off as oo;
    use crate::api_templates::store;
    use crate::api_templates::{Effect, Expect, EXPLORE_PROOF};
    let not_saved = |why: &str| serde_json::json!({ "not_saved": why });
    if one.effect == Effect::Delete {
        return not_saved(oo::DELETES_NOT_SAVED);
    }
    if oo::sends_a_secret(&one.step) {
        return not_saved(oo::SECRET_NOT_SAVED);
    }
    let token_page = match token {
        (Some(page), true) => page,
        _ => return not_saved(oo::NO_TOKEN_NOT_SAVED),
    };
    if oo::page_sends_a_secret(token_page) {
        return not_saved(oo::SECRET_NOT_SAVED);
    }
    let could_not = || not_saved("the template could not be saved - see Settings, Logs");
    let (org, project) = (ctx.org.as_str(), ctx.project.as_str());
    let saved = match store::list(root, org, project) {
        Ok(saved) => saved,
        Err(e) => {
            crate::applog::warn(format!("Auto Run explore session: the saved templates could not be read: {e}"));
            return could_not();
        }
    };
    let mut one = one.clone();
    // A POST that said it only reads is saved as a read only when the change
    // list shows it changed nothing: one the session has recorded a change
    // for (or whose list cannot be read) is saved as an edit.
    if one.step.method() == crate::api_templates::Method::Post && one.effect == Effect::Read {
        let changed = crate::autorun::explore::load(root, session)
            .map_or(true, |s| crate::autorun::explore::names_request(&s, &oo::request_of(&one)));
        if changed {
            one.effect = Effect::Edit;
        }
    }
    // A path segment that is a record id becomes a param, so one template
    // serves every record, whether it is being matched or saved.
    let (step, params) = oo::with_id_params(&one.step, &one.params);
    one.step = step;
    one.params = params;
    // A call that did not say what it expects keeps what it got, so a later
    // run checks exactly that.
    if one.step.expect == Expect::default() {
        if let Some(status) = status {
            one.step.expect.status = status;
        }
    }
    // A later working call of the same route replaces what an earlier call of
    // this session auto-saved, instead of leaving it beside a `-2` copy - but
    // only a template this session saved, still proven by an explore call, and
    // named by no fixture, flow or setup. The very same call is matched below.
    let mine = auto_saved.iter().rev().find_map(|mine| {
        let t = &saved.iter().find(|s| s.template.id == *mine)?.template;
        let proven_here = t.proven.as_ref().is_some_and(|p| p.via.as_deref() == Some(EXPLORE_PROOF));
        let same_step = t.steps.as_slice() == std::slice::from_ref(&one.step);
        (proven_here && oo::same_route(t, &one.step) && !same_step).then(|| t.clone())
    });
    if let Some(old) = mine {
        match oo::is_referenced(root, org, project, &old.id) {
            Ok(false) => {
                let mut t = oo::as_template(&one, &old.id, token_page);
                (t.title, t.module, t.description, t.sources) = (old.title, old.module, old.description, old.sources);
                if crate::api_templates::check(&t).is_empty() {
                    t.proven = Some(explore_proof(root, org, project, account, report));
                    if let Err(e) = store::save_proven(root, org, project, &t) {
                        crate::applog::warn(format!("Auto Run explore session: the template could not be replaced: {e}"));
                        return could_not();
                    }
                    crate::applog::info(format!("api template {} replaced by a later working call of the same route", t.id));
                    log_explore_run(root, org, project, account, report, &t.id);
                    templates_changed(&t.id);
                    return serde_json::json!({ "id": t.id, "replaced": true });
                }
            }
            Ok(true) => {}
            Err(e) => crate::applog::warn(format!(
                "Auto Run explore session: a template was not replaced, the saved fixtures, flows and setups could not be read: {e}"
            )),
        }
    }
    if let Some(same) = oo::matching(&saved, &one.step) {
        return serde_json::json!({ "id": same.id, "reused": true });
    }
    let taken = |id: &str| !matches!(store::load(root, org, project, id), Ok(None));
    let id = match one.id.as_deref().filter(|id| crate::api_templates::valid_id(id)) {
        Some(id) => oo::unique_id(id, &taken),
        None => oo::derived_id(&one.step, &taken),
    };
    let mut t = oo::as_template(&one, &id, token_page);
    t.title = one.name.clone().unwrap_or_else(|| oo::derived_title(&one.step));
    t.module = one.module.clone().unwrap_or_else(|| oo::EXPLORE_MODULE.to_string());
    t.description = one.description.clone().unwrap_or_else(|| {
        format!("Saved from an explore session on {}.", crate::applog::stamp().split(' ').next().unwrap_or(""))
    });
    t.sources = vec![format!("explore session {session}")];
    let problems = crate::api_templates::check(&t);
    if !problems.is_empty() {
        crate::applog::warn(format!("Auto Run explore session: the call is not a template: {}", problems.join("; ")));
        return could_not();
    }
    t.proven = Some(explore_proof(root, org, project, account, report));
    if let Err(e) = store::save_proven(root, org, project, &t) {
        crate::applog::warn(format!("Auto Run explore session: the template could not be saved: {e}"));
        return could_not();
    }
    log_explore_run(root, org, project, account, report, &id);
    templates_changed(&id);
    auto_saved.push(id.clone());
    serde_json::json!({ "id": id, "saved": true })
}

/// Close out the explore session in `slot`, if one is going, however it
/// ends (the same ways as `finish_mapping`): its change list file gets its
/// end time, and the answer counts its changes and names the saves its
/// browser sent that no change names (`explore::end`). The run is taken off
/// the discovery, so closing it twice ends it once. `None` with no explore
/// session going, with one whose change list was never made, or whose file
/// does not read (the log says so, without a path).
pub fn finish_explore<B: DiscoveryBrowser>(slot: &mut Option<B>) -> Option<crate::autorun::explore::ExploreEnd> {
    let browser = slot.as_mut()?;
    let run = browser.parts().discovery.as_mut()?.explore.take()?;
    let session = run.session?;
    let Some(root) = crate::autorun::store::configured_root() else {
        crate::applog::warn("Auto Run explore session: there is no data folder, so its change list is not closed");
        return None;
    };
    match crate::autorun::explore::end(&root, &session) {
        Ok(end) => {
            crate::applog::info(format!(
                "Auto Run explore session ended: {} changes listed, {} saves not on the list",
                end.changes,
                end.saves_not_on_the_change_list.len()
            ));
            Some(end)
        }
        Err(why) => {
            crate::applog::warn(format!("Auto Run explore session: its change list could not be closed: {why}"));
            None
        }
    }
}

