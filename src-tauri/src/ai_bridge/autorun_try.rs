//! Trying one action in the Auto Run browser, and describing what a try did.

use super::*;

/// The applog line for a tried action: its kind, what it points at (a
/// locator's own words, or a navigate's url / a check_url's address), and
/// whether it worked. Pure and separate from the route so it can be
/// tested without a browser. A `fill`'s VALUE is never in it - only its
/// selector, the same as every other selector-carrying kind.
pub fn describe_try(action: &crate::browser::actions::Action, ok: bool) -> String {
    format!(
        "AI tried {} in the supervised browser: {}",
        describe_action(action),
        if ok { "ok" } else { "failed" }
    )
}

/// An action in a few words: its kind and what it points at, as
/// `describe_try` says it - never a `fill`'s value, a host or a query
/// string. What the log and the discovery map keep of an action.
pub fn describe_action(action: &crate::browser::actions::Action) -> String {
    use crate::browser::actions::Action;
    // A discovery's screenshot travels as a marked action (`action_help`).
    match crate::autorun::action_help::screenshot_ask(action) {
        Some(true) => return "screenshot full_page".to_string(),
        Some(false) => return "screenshot".to_string(),
        None => {}
    }
    let kind = serde_json::to_value(action)
        .ok()
        .and_then(|v| v["kind"].as_str().map(str::to_string))
        .unwrap_or_default();
    let what = match action {
        // Its path only: a host and a query string never reach a log line
        // or the discovery map, and a query can carry a token.
        Action::Navigate { url } => crate::autorun::discovery_map::path_only(url),
        // The words it looks for (a path or a name), never a noted address.
        Action::CheckUrl { contains, equals, same_as, .. } => equals
            .as_deref()
            .map(crate::autorun::discovery_map::path_only)
            .or_else(|| same_as.clone())
            .unwrap_or_else(|| contains.clone()),
        Action::RememberUrl { name } => name.clone(),
        Action::UseComponent { component, .. } => component.trim().to_string(),
        Action::CheckText { .. }
        | Action::SignIn { .. }
        | Action::Reload
        | Action::ExpireSession
        | Action::ReturnToArea { area: None } => String::new(),
        Action::ReturnToArea { area: Some(area) } => area.trim().to_string(),
        Action::PressKey { key, .. } => key.trim().to_string(),
        Action::ExpectRow { table, .. }
        | Action::ExpectNoRow { table, .. }
        | Action::ExpectSorted { table, .. }
        | Action::ExpectRowCount { table, .. } => table.describe(),
        // The words it checks for are the script's, never a secret.
        Action::ExpectDialog { text, contains, .. } => text.clone().or_else(|| contains.clone()).unwrap_or_default(),
        Action::Drag { from, to, position, .. } => {
            format!("{} {} {}", from.describe(), position.unwrap_or_default().word(), to.describe())
        }
        Action::ExpectDownload { name, .. } => name.trim().to_string(),
        // A tab's name; an opened tab's path, never its host or query.
        Action::ExpectTab { name, .. }
        | Action::SwitchTab { name }
        | Action::CloseTab { name }
        | Action::ExpectTabClosed { name, .. } => name.clone(),
        Action::OpenTab { name, url } => format!("{name} {}", crate::browser::actions::path_only(url)),
        // Never a query string: a fragment or a path can carry a token there.
        Action::ExpectResponse { url_contains: address, .. } | Action::ApiRequest { path: address, .. } => {
            crate::autorun::report::without_query(address.trim()).to_string()
        }
        Action::Click { selector }
        | Action::Hover { selector, .. }
        | Action::Fill { selector, .. }
        | Action::SelectOption { selector, .. }
        | Action::WaitFor { selector, .. }
        | Action::ExpectVisible { selector, .. }
        | Action::ExpectHidden { selector, .. }
        | Action::ExpectText { selector, .. }
        | Action::ExpectTextMoved { selector, .. }
        | Action::RememberText { selector, .. }
        | Action::ExpectContainsText { selector, .. }
        | Action::ExpectCount { selector, .. }
        | Action::ExpectAttribute { selector, .. }
        | Action::ExpectStyle { selector, .. }
        | Action::ExpectEnabled { selector, .. }
        | Action::ExpectDisabled { selector, .. }
        | Action::ExpectChecked { selector, .. }
        | Action::ExpectFocused { selector, .. }
        | Action::ExpectEach { selector, .. }
        | Action::ExpectOrder { selector, .. }
        | Action::ExpectPosition { selector, .. }
        | Action::ExpectNoChange { selector, .. }
        | Action::Upload { selector, .. }
        | Action::WhenVisible { selector, .. }
        | Action::WhenHidden { selector, .. }
        | Action::RepeatWhileVisible { selector, .. } => selector.describe(),
        // The control that pages: what the walk is about.
        Action::PageUntil { next, .. } => next.describe(),
    };
    let target = if what.is_empty() { String::new() } else { format!(" {what}") };
    format!("{kind}{target}")
}

/// What no action an assistant runs may be: one read from its body by
/// `/autorun-try` or `/autorun-discover-action`.
pub(crate) fn refuse_as_a_tried_action(action: &crate::browser::actions::Action) -> Result<(), (u16, String)> {
    // Signing in needs the tester's own accounts and the project's
    // recipe. It is theirs to drive, and a script never carries a login.
    if matches!(action, crate::browser::actions::Action::SignIn { .. }) {
        return Err((400, "sign_in is not a thing an assistant does - the person signs in".to_string()));
    }
    action.validate().map_err(|why| (400, why))?;
    // `validate()` allows `file://` (a saved script may need it for the
    // live fixture, a development-only tab) but a TRIED action runs
    // against whatever page the person actually has open - sending their
    // browser to a local file is never something a rehearsal should do -
    // nor is one guarded inside a `when_visible`.
    if tried_addresses(action).iter().any(|url| url.trim().to_ascii_lowercase().starts_with("file:")) {
        return Err((400, "a tried navigate goes to http or https only".to_string()));
    }
    Ok(())
}

/// Every address an action would send the browser to: a `navigate`'s or an
/// `open_tab`'s, inside a `when_visible` too.
pub(crate) fn tried_addresses(action: &crate::browser::actions::Action) -> Vec<&str> {
    use crate::browser::actions::Action;
    action
        .each()
        .into_iter()
        .filter_map(|a| match a {
            Action::Navigate { url } | Action::OpenTab { url, .. } => Some(url.as_str()),
            _ => None,
        })
        .collect()
}

/// One action of a try or a discovery, read from the assistant's JSON. A
/// `wait_for` with no `timeout_ms` gets the discovery's own wait, and a wait
/// above the ceiling is lowered (`action_help`); `notes` say which. What is
/// not an action is refused with the field it expected and an example, and
/// so is a key its kind does not have; `repeat` on a `press_key` is read as
/// `times`. A saved script is read elsewhere and never gets any of it (a
/// save refuses unknown keys on what it is sent, `refuse_raw_saved_action`).
pub(crate) fn tried_action_from(mut raw: serde_json::Value) -> Result<(crate::browser::actions::Action, Vec<String>), String> {
    // A discovery's own picture, read before any action is (never a script's).
    if let Some(shot) = crate::autorun::action_help::screenshot_from(&raw) {
        return shot.map(|action| (action, Vec::new()));
    }
    let default_ms = u32::try_from(crate::browser::timing::Timing::supervised().expect_ms).unwrap_or(u32::MAX);
    let mut notes = crate::autorun::action_help::prepare_repeat(&mut raw)?;
    crate::autorun::action_help::refuse_unknown_keys(&raw)?;
    notes.extend(crate::autorun::action_help::prepare_waits(&mut raw, default_ms));
    match serde_json::from_value::<crate::browser::actions::Action>(raw.clone()) {
        Ok(action) => Ok((action, notes)),
        Err(e) => Err(crate::autorun::action_help::refusal_for(&raw, &e.to_string())),
    }
}

/// `answer`, with what was done to the action's waits said in it: a `waits`
/// field of a JSON answer, else a line before the text.
pub(crate) fn with_wait_notes(answer: (u16, String), notes: &[String]) -> (u16, String) {
    if notes.is_empty() {
        return answer;
    }
    let (status, text) = answer;
    let said = notes.join("; ");
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(mut v @ serde_json::Value::Object(_)) => {
            v["waits"] = serde_json::json!(said);
            (status, v.to_string())
        }
        _ => (status, format!("Waits: {said}.\n{text}")),
    }
}

/// Run ONE action against the open page, so an assistant can find out
/// whether a repair works before it writes the repair down.
pub(crate) async fn autorun_try(
    ctx: &BridgeContext,
    body: &str,
    seen: &mut Option<crate::autorun::overrides::Overrides>,
    n: SlotNo,
) -> (u16, String) {
    let raw = match body_field(body, "action", "{ \"action\": <one script action> }") {
        Ok(v) => v,
        Err(refused) => return refused,
    };
    if let Some(r) = crate::autorun::forbidden_fixes::forbidden_in(&raw) {
        return (400, r);
    }
    let (action, wait_notes) = match tried_action_from(raw) {
        Ok(read) => read,
        Err(refused) => return (400, refused),
    };
    if let Err(refused) = refuse_as_a_tried_action(&action) {
        return refused;
    }
    // The case the try is for: its no-save guard applies, exactly as it
    // does to a step of that case, so a try can never send the save the
    // case itself would have stopped.
    let case_id = match try_case_id(body) {
        Ok(id) => id,
        Err(refused) => return refused,
    };
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    if let Some(going) = replay_is_holding_the_browser(n) {
        return going;
    }
    // Everything that can fail without the browser is settled BEFORE the
    // lock: whoever holds it holds the browser the person is using, and
    // a session held open to look up a path is a session held for no
    // reason.
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    let Some(session) = slot.as_mut() else {
        return (409, NO_SUPERVISED_BROWSER.to_string());
    };
    // A try uses the browser: no replay carries on from where it stood, and
    // no watched step names what happened before it.
    session.used_otherwise();
    crate::commands::autorun::note_session_use_in(n);
    // Another case's tabs go first, before its unreported stop is read by
    // the guard below (`try_for_case` does this too).
    crate::autorun::runner::tabs_for_case(&mut session.cdp, &mut session.tabs_case, case_id).await;
    // A try adds its case's guard and never lifts one (`guard_for_case`).
    if let Err(why) =
        crate::commands::autorun::guard_supervised(session, &root, &ctx.org, &ctx.project, case_id, false).await
    {
        return (409, why);
    }
    let discovery_area = session.discovery.as_ref().and_then(|s| s.area.clone());
    // An explore session files nothing (`files_finds` is false), and notes the
    // saves the try sent, as its actions do.
    let explore_session = session.discovery.as_ref().and_then(|s| s.explore_session()).map(str::to_string);
    // An action that never answers ends inside `try_action`, as a failed
    // action; the call as a whole is bounded by `CALL_BUDGET_MS`.
    let answer = crate::autorun::action_help::CALL_STARTED
        .scope(
            std::time::Instant::now(),
            try_in_session(
                &mut session.cdp,
                &mut session.tabs_case,
                &mut session.account,
                &mut session.lease,
                &root,
                &ctx.org,
                &ctx.project,
                case_id,
                discovery_area.as_deref(),
                explore_session.as_deref(),
                &action,
            ),
        )
        .await;
    let answer = with_wait_notes(answer, &wait_notes);
    let answer = let_go_if_silent(&mut slot, answer).await;
    crate::commands::autorun::publish_discovery_in(n, &slot);
    *seen = Some(crate::commands::autorun::overrides_of(&slot));
    answer
}

/// `try_in` for a case in the supervised browser, whose tabs belong to the
/// case it last ran (`tabs_case`). A try for another case starts as that
/// case's first step would: every tab but `main` is closed and `main` is
/// current (`runner::tabs_for_case`), so it never acts in a tab another
/// case left current. `discovery_area` is the area of a discovery under way
/// in that browser: what a try that worked acted on is filed there first.
#[allow(clippy::too_many_arguments)]
pub async fn try_for_case<D: crate::browser::cdp::Driver>(
    d: &mut D,
    tabs_case: &mut Option<i32>,
    account: &mut Option<String>,
    lease: &mut crate::autorun::lease::Held,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    case_id: i32,
    discovery_area: Option<&str>,
    action: &crate::browser::actions::Action,
) -> (u16, String) {
    crate::autorun::runner::tabs_for_case(d, tabs_case, case_id).await;
    try_in_area(d, account, lease, root, organization, project, case_id, discovery_area, true, action).await
}

/// `try_for_case` for a try in a discovery's browser: in an explore session
/// (`explore_session`) nothing the try acted on is filed in the discovery
/// map, and the saves it sent are noted on the session's list of saves (by
/// method and path, with the `handler`) and named at the end of the answer, as
/// the session's actions do. Any other try is `try_for_case`.
#[allow(clippy::too_many_arguments)]
pub async fn try_in_session<D: crate::browser::cdp::Driver>(
    d: &mut D,
    tabs_case: &mut Option<i32>,
    account: &mut Option<String>,
    lease: &mut crate::autorun::lease::Held,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    case_id: i32,
    discovery_area: Option<&str>,
    explore_session: Option<&str>,
    action: &crate::browser::actions::Action,
) -> (u16, String) {
    crate::autorun::runner::tabs_for_case(d, tabs_case, case_id).await;
    let mark = explore_session.map(|_| crate::browser::cdp::Driver::net_mark(d));
    let mut answer =
        try_in_area(d, account, lease, root, organization, project, case_id, discovery_area, explore_session.is_none(), action)
            .await;
    if let (Some(mark), Some(session)) = (mark, explore_session) {
        let sent = crate::browser::cdp::Driver::net_since(d, mark);
        crate::browser::cdp::Driver::net_unmark(d, mark);
        let saves: Vec<(String, String)> = sent
            .iter()
            .filter(|s| crate::autorun::explore::counts_as_save(root, &s.method, &s.path_query))
            .filter(|s| !crate::browser::save_guard::stopped_by_guard(s))
            .map(|s| (s.method.clone(), s.path_query.clone()))
            .collect();
        if let Err(why) = crate::autorun::explore::note_writes(root, session, &saves) {
            crate::applog::warn(format!("Auto Run explore session: its saves could not be noted: {why}"));
        }
        if !saves.is_empty() {
            let lines: Vec<String> =
                saves.iter().map(|(m, p)| crate::autorun::explore::request_line(m, p)).collect();
            answer.1.push_str(&format!("\nSaves it sent: {}.", lines.join(", ")));
        }
    }
    answer
}

// ------------------------------------------------------------ discovery

/// Said to a try that does not name its case.
pub const TRY_NEEDS_CASE: &str = "name the case this try is for (case_id), so its no-save guard applies";

/// The `case_id` a try body must carry.
pub fn try_case_id(body: &str) -> Result<i32, (u16, String)> {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    match v.get("case_id") {
        None | Some(serde_json::Value::Null) => Err((400, TRY_NEEDS_CASE.to_string())),
        Some(id) => id
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .ok_or((400, "case_id must be a number".to_string())),
    }
}

/// A component's try as one outcome, from each of its actions' (`steps`):
/// ok when every one passed; otherwise the first failure's detail, named by
/// its place and action, and its picture.
pub(crate) fn component_outcome(steps: &[(String, crate::browser::actions::ActionOutcome)]) -> crate::browser::actions::ActionOutcome {
    use crate::browser::actions::ActionOutcome;
    match steps.iter().enumerate().find(|(_, (_, o))| !o.ok) {
        None => {
            let n = steps.len();
            ActionOutcome::passed(if n == 1 { "its 1 action passed".to_string() } else { format!("all {n} of its actions passed") })
        }
        Some((i, (what, o))) => {
            // An action the failure stopped was never carried out.
            let failed = steps.iter().filter(|(_, o)| !o.ok && !o.detail.starts_with("not run:")).count();
            let more = if failed > 1 { format!(" ({failed} of {} failed)", steps.len()) } else { String::new() };
            let mut out = ActionOutcome::failed(format!("action {} ({what}) failed: {}{more}", i + 1, o.detail));
            out.screenshot = o.screenshot.clone();
            out.harness = o.harness;
            out
        }
    }
}

/// What a discovery try files for its `repeat_while_visible` with `except`
/// (`seen_check::loop_rows_review`, newest wins): `Some(None)` when the try
/// went past the rows check, `Some(Some(why))` when the check refused it,
/// `None` when it never ran the check.
pub fn loop_rows_verdict(outcome: &crate::browser::actions::ActionOutcome) -> Option<Option<String>> {
    let guard_said = outcome.detail.contains(crate::browser::repeat::EXCEPT_GUARD_MARK);
    if guard_said {
        return Some(Some(outcome.detail.clone()));
    }
    (outcome.ok && outcome.rows_told_apart).then_some(None)
}

/// The loop-rows records a discovery try of `action` files, at `at`: one per
/// `repeat_while_visible` with `except` in it, when the try has a verdict
/// (`loop_rows_verdict`); none otherwise.
pub fn loop_rows_to_file(action: &crate::browser::actions::Action, outcome: &crate::browser::actions::ActionOutcome, at: u64) -> Vec<crate::autorun::discovery_map::LoopRows> {
    let Some(refused) = loop_rows_verdict(outcome) else { return Vec::new() };
    action
        .each()
        .into_iter()
        .filter_map(|each| match each {
            crate::browser::actions::Action::RepeatWhileVisible { selector, except, .. } if !except.is_empty() => {
                Some(crate::autorun::discovery_map::LoopRows { selector: selector.clone(), except: except.clone(), refused: refused.clone(), at })
            }
            _ => None,
        })
        .collect()
}

/// One tried action in a browser already guarded for its case
/// (`commands::autorun::guard_for_case`), as the route answers it.
/// `case_id` is the case the try is for: a tried `return_to_area` goes to
/// the area that case's saved script names. A try that worked files every
/// locator it acted on in the discovery map, under that case's area.
#[allow(clippy::too_many_arguments)]
pub async fn try_in<D: crate::browser::cdp::Driver>(
    d: &mut D,
    account: &mut Option<String>,
    lease: &mut crate::autorun::lease::Held,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    case_id: i32,
    action: &crate::browser::actions::Action,
) -> (u16, String) {
    try_in_area(d, account, lease, root, organization, project, case_id, None, true, action).await
}

/// `try_in`, its sightings filed under `discovery_area` when one is given
/// (`recording_area`), and what it acted on filed in the discovery map
/// unless `files_finds` is false (an explore session).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn try_in_area<D: crate::browser::cdp::Driver>(
    d: &mut D,
    account: &mut Option<String>,
    lease: &mut crate::autorun::lease::Held,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    case_id: i32,
    discovery_area: Option<&str>,
    files_finds: bool,
    action: &crate::browser::actions::Action,
) -> (u16, String) {
    let guarded = d.is_guarding_saves();
    let mark = if guarded { Some(crate::browser::cdp::Driver::net_mark(d)) } else { None };
    let tried = try_action(d, account, lease, root, organization, project, case_id, discovery_area, false, files_finds, action, None).await;
    // The writes a guarded try sent that the guard let through, by method and path.
    let let_through: Vec<(String, String)> = match mark {
        Some(mark) => {
            let sent = crate::browser::cdp::Driver::net_since(d, mark);
            crate::browser::cdp::Driver::net_unmark(d, mark);
            sent.iter()
                .filter(|s| crate::browser::save_guard::SAVE_METHODS.contains(&s.method.as_str()))
                .filter(|s| !crate::browser::save_guard::stopped_by_guard(s))
                .map(|s| (s.method.clone(), crate::browser::save_guard::path_of(&s.path_query)))
                .collect()
        }
        None => Vec::new(),
    };
    let (outcome, steps, answered) = match tried {
        Ok(tried) => (tried.outcome, tried.steps, tried.answered),
        Err(refused) => return refused,
    };
    if files_finds {
        remember_tried(root, organization, project, discovery_area, case_id, &answered);
    }
    let mut text = format!("{}: {}", if outcome.ok { "ok" } else { "failed" }, outcome.detail);
    if let Some(shot) = &outcome.screenshot {
        text.push_str(&format!(" (picture: {})", crate::autorun::store::shot_path(root, shot).display()));
    }
    // A component's try answers each of its actions, as a discovery's does.
    for (i, (what, o)) in steps.iter().enumerate() {
        let detail = o.detail.trim();
        // One the failure before it stopped says so, and why, not "failed".
        if let Some(why) = detail.strip_prefix("not run: ").filter(|_| !o.ok) {
            text.push_str(&format!("\n{}. not run: {what} ({why})", i + 1));
            continue;
        }
        let said = if detail.is_empty() { what.clone() } else { format!("{what}: {}", one_short_line(detail)) };
        text.push_str(&format!("\n{}. {}: {said}", i + 1, if o.ok { "ok" } else { "failed" }));
    }
    if let Some(line) = crate::browser::save_guard::let_through_line(&let_through) {
        text.push_str(&format!("\nlet_through: {line}"));
    }
    (200, text)
}

/// What one action an assistant ran did: its outcome, and the dialog the
/// page raised while it ran, if any.
pub(crate) struct Tried {
    /// A `use_component`'s actions, each described with its outcome; empty
    /// for any other action.
    pub(crate) steps: Vec<(String, crate::browser::actions::ActionOutcome)>,
    pub(crate) outcome: crate::browser::actions::ActionOutcome,
    pub(crate) dialog: Option<crate::autorun::StepDialog>,
    /// Each action the try carried out - a component's own, or the one -
    /// described, with its own outcome: what the page answered each.
    pub(crate) answered: Vec<(String, crate::browser::actions::ActionOutcome)>,
}

/// What the picture a discovery's `screenshot` asks for did: taken and
/// saved (the answer carries its name as the action's `screenshot`), or
/// failed in words with no protocol text in them. Never a failure of the
/// page: a picture changes nothing.
pub(crate) async fn take_picture<D: crate::browser::cdp::Driver>(
    d: &mut D,
    root: &std::path::Path,
    full_page: bool,
) -> crate::browser::actions::ActionOutcome {
    use crate::browser::actions::ActionOutcome;
    let wanted = if full_page { "the whole page" } else { "the page" };
    let bytes = match crate::browser::page::screenshot_of(d, full_page).await {
        Ok(bytes) => bytes,
        Err(e) => {
            crate::applog::warn(format!("Auto Run: a screenshot could not be taken: {e}"));
            return ActionOutcome::failed(NO_PICTURE.to_string());
        }
    };
    match crate::autorun::store::save_shot(root, &bytes) {
        Ok(name) => {
            let mut outcome = ActionOutcome::passed(format!("took a picture of {wanted}"));
            outcome.screenshot = Some(name);
            outcome
        }
        Err(why) => {
            crate::applog::warn(format!("Auto Run: a screenshot could not be saved: {why}"));
            ActionOutcome::failed(NO_PICTURE.to_string())
        }
    }
}

/// Said when a discovery's `screenshot` could not be taken or kept.
pub const NO_PICTURE: &str = "the picture could not be taken - see Settings, Logs";

/// What a try teaches of the application's rules, kept as a discovery's
/// (`discovery_map::rules_from`) under the area its sightings go to
/// (`recording_area`): every request the application refused its actions
/// up to the first that failed, and what the page showed after the nearest
/// one before that failure that changed the page. With no area known
/// nothing is kept, and a map that could not be written fails nothing.
pub(crate) fn remember_tried(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    discovery_area: Option<&str>,
    case_id: i32,
    answered: &[(String, crate::browser::actions::ActionOutcome)],
) {
    use crate::autorun::discovery_map::{remember_refusals_logged, rules_from, ActionAnswer, RuleSource};
    // What the first failure stopped was never carried out.
    let ran = answered.iter().position(|(_, o)| !o.ok).map_or(answered.len(), |i| i + 1);
    let answers: Vec<ActionAnswer> = answered[..ran]
        .iter()
        .map(|(what, o)| ActionAnswer {
            page: o.page_path.clone().unwrap_or_default(),
            step: o.page_step.clone(),
            after: what.clone(),
            said: o.page_said.clone(),
            refused: o.refused.clone(),
            changed: o.page_path.is_some(),
            failed: !o.ok && !crate::autorun::runner::tcm_made(o),
        })
        .collect();
    let case = (case_id != 0).then_some(case_id);
    let rules = rules_from(&answers, RuleSource::Discovery, case, crate::autorun::sessions::now_ms());
    if rules.is_empty() || project.trim().is_empty() {
        return;
    }
    let area = recording_area(root, discovery_area, case);
    remember_refusals_logged(root, organization, project, area.as_deref(), rules);
}

/// One action, carried out by the runner's own step loop as a step of one
/// numbered 0 (`try_in_area` says why), with what it acted on filed when it
/// worked. `saves_only_counted` is a mapping run's: a stopped save never
/// fails it (`runner::InRun::saves_only_counted`).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn try_action<D: crate::browser::cdp::Driver>(
    d: &mut D,
    account: &mut Option<String>,
    lease: &mut crate::autorun::lease::Held,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    case_id: i32,
    discovery_area: Option<&str>,
    saves_only_counted: bool,
    files_finds: bool,
    action: &crate::browser::actions::Action,
    noted: Option<&mut crate::autorun::runner::Noted>,
) -> Result<Tried, (u16, String)> {
    use crate::autorun::runner::{area_route, area_routes, named_areas, AreaRoute, InRun, NEEDS_SCRIPT_AREA};
    // A discovery's `screenshot` takes a picture and acts on nothing.
    if let Some(full_page) = crate::autorun::action_help::screenshot_ask(action) {
        let outcome = take_picture(d, root, full_page).await;
        let answered = vec![(describe_action(action), outcome.clone())];
        return Ok(Tried { outcome, dialog: None, steps: Vec::new(), answered });
    }
    // A check that times a request from the action before it, or compares the page with the one its step began
    // on, has nothing to measure from when tried alone: it says so, and where to try it.
    if crate::autorun::api_checks::needs_its_step(action) {
        let outcome = crate::browser::actions::ActionOutcome::failed(format!(
            "{}{}",
            crate::browser::actions::CANNOT_RUN,
            crate::autorun::api_checks::TRY_IN_ITS_STEP
        ));
        let answered = vec![(describe_action(action), outcome.clone())];
        return Ok(Tried { outcome, dialog: None, steps: Vec::new(), answered });
    }
    // A bare `return_to_area` goes to the case's own area - in a discovery
    // (case 0, which has no script), the discovery's own area; one that
    // names an area goes to that one.
    // A `when_visible` may hold one in its `then`, to recover from a bounce.
    let bare = |a: &crate::browser::actions::Action| {
        matches!(a, crate::browser::actions::Action::ReturnToArea { .. }) && a.area_named().is_none()
    };
    let resolved = action.each().into_iter().any(bare)
        .then(|| {
            if case_id == 0 {
                crate::autorun::runner::discovery_area_route(root, organization, project, discovery_area)
            } else {
                area_route(root, organization, project, case_id)
            }
        });
    let area = match &resolved {
        Some(Ok(r)) => AreaRoute::To(r),
        Some(Err(why)) => AreaRoute::Unknown(why),
        None => AreaRoute::Unknown(NEEDS_SCRIPT_AREA),
    };
    let areas = area_routes(root, organization, project, &named_areas(std::iter::once(action)));
    // A step of one, numbered 0 - it belongs to no case, and nothing
    // records it. The runner's step loop still carries it out, so a tried
    // action behaves exactly as it will inside a script - the runner's own
    // kinds included: a tried `expect_response` takes its mark as the try
    // starts, and checks a request the page makes while it waits - and a
    // save the guard stops fails it with the run's own sentence.
    //
    // `{{username}}` and `{{password}}` are refused where a script is
    // SAVED, not here: a tried `fill` is not on its way into a file, and
    // it types the literal text it was given rather than standing in for
    // anything a recipe would have substituted.
    // The page the try starts on: a click that navigates was matched on
    // this page, not the one it leads to.
    // A `wait_for`, and any legacy string target, passes on one match or
    // several and does not say which, so once the try worked each is
    // probed, as `probe_autorun_locator` does, and filed with the count the
    // probe gives (`visible_matches`). The probe never clicks or types.
    // A legacy string target is probed BEFORE the try, on the page it acts
    // on: a probe is read-only, and after a click that navigates it would
    // count the wrong page. A structured target that is one plain css link
    // matched one visible element, but its hidden matches are not known
    // (`NOT_ONE`), so it never vouches for a legacy use of its key.
    let (known, legacy): (Vec<_>, Vec<_>) = matched_targets(action).into_iter().partition(|(t, _)| !t.is_legacy());
    let mut matched: Vec<Matched> = known
        .into_iter()
        .map(|(t, n)| {
            let css = crate::autorun::discovery_map::plain_target(t) && t.links().first().is_some_and(|l| l.css.is_some());
            (t, n, if css { crate::autorun::discovery_map::NOT_ONE as usize } else { n })
        })
        .collect();
    for (target, _) in legacy {
        let text = crate::browser::snapshot::probe_exact(d, target).await.ok();
        match text {
            Some(text) => match counted(d, target, &text).await {
                Some((n, all)) => matched.push((target, n, all)),
                None => crate::applog::info("Auto Run try: a tried locator was not filed, as the probe before it saw nothing visible"),
            },
            None => crate::applog::info("Auto Run try: a tried locator was not filed, as the probe before it got no answer"),
        }
    }
    // A `wait_for` passes on one match or several and does not say which,
    // and what it waits for may not be there yet when the try starts: it is
    // probed once the try worked, and filed as found, its count not known,
    // when what it waited for is already gone. Waiting changes no page.
    let waited = match action {
        crate::browser::actions::Action::WaitFor { selector, .. } => Some(selector),
        _ => None,
    };
    // (A list check changes no page either, so its list is filed at the page
    // it is probed on, after the try.)
    let started_on = if matched.is_empty() && waited.is_none() && list_targets(action).is_empty() {
        String::new()
    } else {
        page_address(d).await.0
    };
    let step = crate::autorun::StepScript { step_number: 0, actions: vec![action.clone()], unchecked: None, cleanup: false, because: Vec::new() };
    // A try fails on a request the application refused: the assistant learns
    // the reason now, not from a wait that times out later.
    // A discovery's `noted` carries its last action that changed the page
    // to the next action, as a run's steps carry it.
    let mut run = InRun { areas: Some(&areas), saves_only_counted, noted, ..Default::default() };
    // An action that never answers is a failed action, so everything that
    // follows (what its writes and finds filed) still runs. It is given its
    // own wait and a margin, and no more than the call has left.
    let timing = crate::browser::timing::Timing::supervised();
    let own = crate::autorun::action_help::wait_budget_ms(action, timing.nav_ms);
    let budget = crate::autorun::action_help::action_budget(own, crate::autorun::action_help::call_elapsed_ms());
    // With no time left the action is not started at all.
    let mut timed_out = false;
    let outcomes = match crate::autorun::action_help::time_to_run(budget) {
        Err(spent) => {
            timed_out = true;
            vec![crate::browser::actions::ActionOutcome::failed(spent)]
        }
        Ok(ms) => {
            let ran = crate::autorun::action_help::within_budget(
                ms,
                crate::autorun::runner::run_step_in_run(d, root, organization, project, &step, &timing, account, lease, None, area, &mut run),
            )
            .await;
            match ran {
                Ok(Ok(v)) => v,
                Ok(Err(why)) => return Err((500, why)),
                Err(seconds) => {
                    timed_out = true;
                    vec![crate::browser::actions::ActionOutcome::failed(crate::autorun::action_help::did_not_answer(seconds))]
                }
            }
        }
    };
    // What a component's waits and reads found is filed though a later
    // action of it failed: each passed (`component_finds`).
    if files_finds && matches!(action, crate::browser::actions::Action::UseComponent { .. }) {
        let recorded_in = recording_area(root, discovery_area, Some(case_id));
        file_component_finds(d, root, organization, project, recorded_in, std::slice::from_ref(action), &outcomes).await;
    }
    // A `use_component` runs as its component's actions, one outcome each:
    // every one is answered, and the try as a whole is ok only when all are.
    let steps: Vec<(String, crate::browser::actions::ActionOutcome)> = match action {
        // A timeout is one outcome for the whole component, labelled as the
        // component, not as its first action.
        crate::browser::actions::Action::UseComponent { .. } if timed_out => {
            vec![(describe_action(action), outcomes[0].clone())]
        }
        crate::browser::actions::Action::UseComponent { .. } => {
            let expanded = crate::autorun::components::expand_step(root, organization, project, std::slice::from_ref(action))
                .map(|(x, _)| x.into_iter().map(|(a, _)| describe_action(&a)).collect::<Vec<_>>())
                .unwrap_or_default();
            outcomes
                .iter()
                .enumerate()
                .map(|(i, o)| (expanded.get(i).cloned().unwrap_or_else(|| format!("action {}", i + 1)), o.clone()))
                .collect()
        }
        _ => Vec::new(),
    };
    let answered = match (&steps[..], outcomes.first()) {
        ([], Some(o)) => vec![(describe_action(action), o.clone())],
        _ => steps.clone(),
    };
    let outcome = if steps.is_empty() {
        let Some(outcome) = outcomes.into_iter().next() else {
            return Err((500, "the action produced no outcome".to_string()));
        };
        outcome
    } else {
        component_outcome(&steps)
    };
    // Shown to a person reading Settings -> Logs, never returned to the
    // assistant - and never a `fill`'s VALUE, which `describe_try` never
    // even looks at.
    crate::applog::info(describe_try(action, outcome.ok));
    // A discovery try of a `repeat_while_visible` with `except` files whether
    // its rows could be told apart, so the save can judge the same loop
    // (`seen_check::loop_rows_review`). Only a try that went past the guard
    // (it passed having read its rows) or was stopped by it says anything.
    if files_finds && !project.trim().is_empty() {
        for rows in loop_rows_to_file(action, &outcome, crate::autorun::sessions::now_ms()) {
            let area = recording_area(root, discovery_area, Some(case_id));
            if let Err(why) =
                crate::autorun::discovery_map::record_loop_rows_in(root, organization, project, area.as_deref(), rows)
            {
                unrecorded(&why);
            }
        }
    }
    // Only a try that worked: a locator that failed was never seen working.
    // (An explore session files no locator: `files_finds` is false.)
    if outcome.ok && files_finds {
        let at = Sighting {
            root: root.to_path_buf(),
            org: organization.to_string(),
            project: project.to_string(),
            area: recording_area(root, discovery_area, Some(case_id)),
            account: None,
            discovering: None,
            // Only the application's own pages count (`may_record`).
            policy: recording_policy(root, organization, project),
        };
        // A list check's outcome does not say how many items it read, and a
        // passed one has proved its list was there: the list is probed once
        // the try worked, and an `expect_each`'s child as the chain inside
        // the list (the whole the save asks to have seen), each filed as
        // found. Nothing visible now files nothing: the check alone does not
        // say what was there.
        let list_chain = list_chain_of(action);
        let mut matched = matched;
        for target in list_targets(action).into_iter().chain(list_chain.as_ref()) {
            if let Ok(text) = crate::browser::snapshot::probe_exact(d, target).await {
                if let Some((n, all)) = counted(d, target, &text).await {
                    matched.push((target, n, all));
                }
            }
        }
        if let Some(target) = waited {
            let found = match crate::browser::snapshot::probe_exact(d, target).await {
                Ok(text) => counted(d, target, &text).await,
                Err(_) => None,
            };
            // Nothing visible now, or the browser did not answer: the wait
            // found it, so it was there (a toast is gone a few seconds
            // later), filed with its count not known.
            let unknown = crate::autorun::discovery_map::NOT_ONE as usize;
            matched.push(match found {
                Some((n, all)) => (target, n, all),
                None => (target, unknown, unknown),
            });
        }
        record_matched_targets(&at, &started_on, &matched);
    }
    Ok(Tried { outcome, dialog: run.dialog, steps, answered })
}

