//! What the Auto Run routes share: the no-save and page-compaction wrappers, the names of who holds a browser or an account, case claims, and the Auto Run guide.

use super::*;

/// Said on every answer of a discovery that must not save
/// (`with_no_save_note`): the flag, once set, holds until the discovery
/// ends.
pub const NO_SAVE_ON: &str = "(must not save: on for this discovery)";

/// `answer` saying `NO_SAVE_ON` when `on`: a JSON object gets it as
/// `must_not_save`, any other answer after its text.
pub fn with_no_save_note(answer: (u16, String), on: bool) -> (u16, String) {
    if !on {
        return answer;
    }
    let (status, text) = answer;
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(mut map)) => {
            map.insert("must_not_save".to_string(), serde_json::json!(NO_SAVE_ON));
            (status, serde_json::Value::Object(map).to_string())
        }
        _ => (status, format!("{text} {NO_SAVE_ON}")),
    }
}

/// Whether the discovery holding slot `n`'s Auto Run browser must not save. Only
/// looks: a browser someone else holds right now (a run, a replay) is
/// never waited for, and then there is no discovery to speak of.
pub(crate) fn discovery_no_save(n: SlotNo) -> bool {
    match crate::commands::autorun::session_slot(n).try_lock() {
        Ok(slot) => slot.as_ref().and_then(|s| s.discovery.as_ref()).is_some_and(|d| d.no_save),
        Err(_) => false,
    }
}

/// What a template or fixture run refused its account says when the
/// assistant's own discovery is what holds it: end the discovery first,
/// or, for a template, run as another account. A fixture always signs in
/// as its own account.
pub fn discovery_holds_hint(fixture: bool) -> String {
    let other = if fixture {
        "a fixture always signs in as its own account"
    } else {
        "or run the template as another account no browser holds"
    };
    format!(
        "Your discovery holds this account in the Auto Run browser: end it with end_autorun_discovery first (what it mapped is kept) and run again; {other}."
    )
}

/// `answer` with `discovery_holds_hint` added when a discovery is going
/// (`discovering`) and the run was refused its account because the Auto
/// Run browser holds it: as `discovery` on a JSON object, else on a line
/// after the text.
pub fn with_discovery_hint(answer: (u16, String), discovering: bool, fixture: bool) -> (u16, String) {
    let held = format!(" was in use by {}", crate::autorun::lease::BROWSER_WORDS);
    if !discovering || answer.0 < 400 || !answer.1.contains(&held) {
        return answer;
    }
    let (status, text) = answer;
    let hint = discovery_holds_hint(fixture);
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(mut map)) => {
            map.insert("discovery".to_string(), serde_json::json!(hint));
            (status, serde_json::Value::Object(map).to_string())
        }
        _ => (status, format!("{text}\n{hint}")),
    }
}

/// Said in place of the page dump an answer leaves out (`without_page`).
pub const PAGE_LEFT_OUT: &str = "left out (page: false) - get_autorun_page reads it";

/// Whether a call's body wants the page in its answer: every body but one
/// that sent `"page": false`.
pub fn page_wanted(body: &str) -> bool {
    // `page` takes text too (`"full"`), which `coerce_args` then leaves as
    // text: the text `false` is the boolean.
    let asked = serde_json::from_str::<serde_json::Value>(body).ok().and_then(|v| match v.get("page")? {
        serde_json::Value::Bool(b) => Some(*b),
        serde_json::Value::String(s) if s.trim().eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    });
    asked != Some(false)
}

/// Whether a call's body asked for the whole page tree (`"page": "full"`)
/// rather than the compact one (`page_compact`).
pub fn page_full(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .is_some_and(|v| v.get("page").and_then(|p| p.as_str()).is_some_and(|p| p.trim().eq_ignore_ascii_case("full")))
}

/// Who reads the pages of a call: the caller as `kept::owner_of` names it.
pub fn page_owner(caller: &crate::autorun::agents::Caller) -> String {
    crate::autorun::kept::owner_of(caller)
}

/// The page an answer carries: the `page` of a JSON answer, or the text of a
/// plain one (a batch's lines and its page).
pub(crate) fn page_of(text: &str) -> Option<String> {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Object(map)) => map.get("page").and_then(|p| p.as_str()).map(str::to_string),
        Ok(_) => None,
        Err(_) => Some(text.to_string()),
    }
}

/// `read` (a call that reads the page) for slot `n`'s reader `owner`: the
/// landmarks that reader already has are one line each in the page it
/// reads (`page_compact`), unless `full`; and what the answer printed is what
/// the reader is remembered to have. A failed answer remembers nothing.
pub async fn compacted<F: std::future::Future<Output = (u16, String)>>(
    n: SlotNo,
    owner: &str,
    full: bool,
    read: F,
) -> (u16, String) {
    use crate::autorun::page_compact::{known_for, remember, scoped};
    let answer = scoped(known_for(n, owner, full), read).await;
    if answer.0 == 200 {
        if let Some(page) = page_of(&answer.1) {
            remember(n, owner, &page);
        }
    }
    answer
}

/// `agents::note_call`, and when the call is the proxy's own (no subagent
/// name) for a tool that has no `agent` property to carry one, it also counts
/// as a call of the proxy's named subagents (`agents::touch_named_of`): only
/// the Auto Run and API template tools can name their caller, and a subagent
/// busy with lookups must not lose its browser and cases for it.
pub fn note_call_for_tool(
    caller: &crate::autorun::agents::Caller,
    headers: &crate::autorun::agents::AgentHeaders,
    tool: Option<&str>,
    now_ms: u64,
) -> Option<String> {
    use crate::ai_tools::{API_TEMPLATE_TOOLS, AUTORUN_TOOLS};
    let label = crate::autorun::agents::note_call(caller, headers, now_ms);
    if let (crate::autorun::agents::Caller::Agent(key), Some(tool)) = (caller, tool) {
        if key.name.is_none() && !AUTORUN_TOOLS.contains(&tool) && !API_TEMPLATE_TOOLS.contains(&tool) {
            crate::autorun::agents::touch_named_of(&key.id, now_ms);
        }
    }
    label
}

/// A reset's or a release's answer, ending with the other assistants' live
/// holds (`autorun_slots::live_holds_except`) so the coordinator sees who has
/// which browser, account and cases and for how long it has been quiet. With
/// none held it is the answer unchanged.
pub fn with_other_holds(answer: (u16, String), holder: Option<&AgentKey>) -> (u16, String) {
    let holds = crate::commands::autorun_slots::live_holds_except(crate::autorun::sessions::now_ms(), holder);
    let note = crate::commands::autorun_slots::holds_note(&holds);
    if note.is_empty() {
        return answer;
    }
    // A JSON object answer gets a `holds` field; any other, the text.
    if let Ok(serde_json::Value::Object(mut map)) = serde_json::from_str::<serde_json::Value>(&answer.1) {
        map.insert("holds".to_string(), serde_json::json!(holds));
        return (answer.0, serde_json::Value::Object(map).to_string());
    }
    (answer.0, format!("{}\n\n{note}", answer.1))
}

/// The assistant `holder` is done: every case it claimed is free again, when
/// its call (`status`) was answered, not refused. The default caller holds none.
pub(crate) fn end_claims_of(holder: Option<&AgentKey>, status: u16) {
    if let (Some(key), true) = (holder, status < 300) {
        crate::autorun::claims::end_for(key);
    }
}

/// `answer` with its `page` dump left out when the call's body sent
/// `"page": false` (`page_wanted`): start_autorun_discovery,
/// discover_autorun_actions and replay_autorun_to_step answer the whole
/// page, which a caller that only wants the outcome can do without. The
/// page is still read, so what it shows is still recorded as seen;
/// `page_left_out` says where to read it. Any other body, an answer that
/// is not a JSON object, or one with no page, is unchanged. The batch's
/// text answer is asked for it instead (`discover_actions_paged`).
pub fn without_page(answer: (u16, String), body: &str) -> (u16, String) {
    if page_wanted(body) {
        return answer;
    }
    let (status, text) = answer;
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(mut map)) if map.contains_key("page") => {
            map.remove("page");
            map.insert("page_left_out".to_string(), serde_json::json!(PAGE_LEFT_OUT));
            (status, serde_json::Value::Object(map).to_string())
        }
        _ => (status, text),
    }
}

/// Said when a page route arrives with no browser behind it. The person
/// opens one; an assistant cannot, and telling it which button to name
/// is the difference between a dead end and a sentence it can pass on.
pub(crate) const NO_SUPERVISED_BROWSER: &str =
    "no supervised browser is open - the person opens one with Open browser on the Auto Run tab";

/// What every page route checks before it reaches for the browser at all:
/// an unattended run owns the browser while it is going, and the two
/// kinds of session are never open at once.
pub(crate) fn unattended_run_is_using_the_browser() -> Option<(u16, String)> {
    if crate::commands::autorun_replay::replay_is_running() {
        Some((409, crate::autorun::sign_in_check::going_sentence("an unattended run is going - wait for it")))
    } else {
        None
    }
}

/// Where scripts, runs and quirks live, or the refusal to guess.
/// The refusal is the store's own sentence: no data directory, no
/// environment yet, or an unreadable list each tell the assistant (and so
/// the person) what is actually wrong.
pub fn autorun_root() -> Result<std::path::PathBuf, (u16, String)> {
    crate::autorun::store::resolve_root().map_err(|e| (503, e))
}

/// The active environment's live guide section, or None when the list
/// cannot be read (said in the log; a guide without it still teaches the
/// format).
pub(crate) fn active_environment_section(root: &std::path::Path, ctx: &BridgeContext) -> Option<String> {
    match crate::environments::active(root) {
        Ok(env) => {
            let database = environment_database(ctx, &env);
            Some(crate::autorun::guide::active_environment_section(&env, database.as_ref()))
        }
        Err(e) => {
            crate::applog::warn(&format!("Guide: the active environment could not be read: {e}"));
            None
        }
    }
}

/// The environment's database as the app lists it (`DbDatabase`: label,
/// server and database, never a password), or None when it names none the
/// app knows (the guide then says it is not set up any more, or that there
/// is none when no id is set) - or no store is set up to look it up in,
/// which only a context nobody set up lacks.
pub(crate) fn environment_database(ctx: &BridgeContext, env: &crate::environments::Environment) -> Option<crate::db::DbDatabase> {
    if env.db_id.trim().is_empty() {
        return None;
    }
    let store = ctx.db_secrets.as_deref()?;
    crate::db::credentials::databases(store).into_iter().find(|d| d.id == env.db_id)
}

/// The guide's own text, plus this project's sections when it has any: the
/// module-screen rule while "Scripts may open pages by address" is off,
/// the recorded areas, the areas with no map or a stale one, this
/// project's components, then the recorded quirks. The constant (`autorun::guide::autorun_guide`)
/// only says a quirks section exists; this reads what is actually on file,
/// so the guide can never go stale on a live project.
///
/// `quick` answers the Quick rules section in place of the full text, with
/// the same live sections after it. `n` is the asker's browser, when it has
/// one: the rules listed are those of the area its discovery is exploring.
pub(crate) fn autorun_guide_with_quirks(ctx: &BridgeContext, quick: bool, n: Option<SlotNo>) -> String {
    let mut out =
        if quick { crate::autorun::guide::quick_rules() } else { crate::autorun::guide::autorun_guide() };
    let Some(root) = crate::autorun::store::configured_root() else {
        return out;
    };
    // The environment is the app's, not a project's: named even with no
    // project open.
    if let Some(section) = active_environment_section(&root, ctx) {
        out.push('\n');
        out.push_str(&section);
    }
    if ctx.project.trim().is_empty() {
        return out;
    }
    let nav = crate::autorun::nav::load_nav(&root, &ctx.org, &ctx.project).unwrap_or_default();
    let quirks = crate::autorun::quirks::load_quirks(&root, &ctx.org, &ctx.project).unwrap_or_default();
    let files = project_test_files(&root, ctx);
    let map = crate::autorun::discovery_map::load_map(&root, &ctx.org, &ctx.project).unwrap_or_else(|e| {
        crate::applog::warn(&format!("Guide: {e}"));
        Default::default()
    });
    let components = crate::autorun::components::load_components(&root, &ctx.org, &ctx.project).unwrap_or_else(|e| {
        crate::applog::warn(&format!("Guide: {e}"));
        Default::default()
    });
    let areas: Vec<&str> = nav.modules.iter().map(|m| m.name()).collect();
    for section in [
        crate::autorun::nav::guide_section(&nav),
        crate::autorun::discovery_map::explore_section(&areas, &map, crate::autorun::sessions::now_ms()),
        crate::autorun::discovery_map::rules_section(&map, running_discovery_area(n).as_deref()),
        crate::autorun::components::guide_section(&components),
        crate::autorun::quirks::quirks_section(&quirks),
        crate::test_files::guide_section(&files),
    ] {
        if !section.is_empty() {
            out.push('\n');
            out.push_str(&section);
        }
    }
    out
}

/// The area of the discovery going on now in slot `n`'s browser, for the
/// guide's rules; none for a caller with no browser (`n` is `None`). It never
/// waits: a call that holds the browser is busy, and the guide then lists
/// every area's rules, which is still true.
pub fn running_discovery_area(n: Option<SlotNo>) -> Option<String> {
    let n = n?;
    let slot = crate::commands::autorun::session_slot(n).try_lock().ok()?;
    slot.as_ref()?.discovery.as_ref()?.area.clone()
}

/// What `/autorun-network-wait` answers. The registry's view carries no
/// host or port, and nothing is added here.
pub(crate) fn autorun_network_wait_answer() -> String {
    match crate::autorun::network::waits().current() {
        None => serde_json::json!({ "waiting": false }).to_string(),
        Some(w) => serde_json::json!({
            "waiting": true,
            "run_id": w.run_id,
            "before_case_id": w.before_case_id,
            "waited_s": w.waited_s,
            "give_up_in_s": w.give_up_in_s,
        })
        .to_string(),
    }
}

/// The caller works on `case_id` from now, or is told who does. An assistant
/// claims it (`claims::claim`: refused, 409, while another live assistant
/// holds it). The default caller (an older proxy, a test, the person's own
/// tools) never claims, and is refused only by a LIVE assistant's claim, so
/// with no assistant connected nothing is ever refused. The answer names the
/// holder by label only.
pub fn claim_for(caller: &crate::autorun::agents::Caller, case_id: i32) -> Result<Option<i32>, (u16, String)> {
    use crate::autorun::{agents, claims};
    let now = crate::autorun::sessions::now_ms();
    match caller {
        agents::Caller::Agent(key) => claims::claim(key, case_id, now).map(|made| made.then_some(case_id)).map_err(|why| (409, why)),
        agents::Caller::Default => {
            let live = claims::holder(case_id, now).and_then(|h| agents::info(&h)).filter(|i| i.gone_since_ms.is_none());
            match live {
                Some(held_by) => Err((409, claims::case_claimed(case_id, &held_by.label, claims::held_for(case_id, now)))),
                None => Ok(None),
            }
        }
    }
}

/// A hold a discovery start made ends with that start when it is answered
/// with anything but 200: no work began. A hold the agent had before stays.
pub(crate) fn release_failed_start(newly_claimed: Option<i32>, caller: &crate::autorun::agents::Caller, path: &str, status: u16) {
    if let (Some(case), crate::autorun::agents::Caller::Agent(key), "/autorun-discover-start") = (newly_claimed, caller, path) {
        if status != 200 && crate::autorun::claims::release_case(key, case) {
            crate::applog::info(format!("Auto Run: case {case}'s hold ended - its discovery did not start"));
        }
    }
}

/// The `case_id` a request body names, when it names one.
pub(crate) fn named_case_id(body: &str) -> Option<i32> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get("case_id")?.as_i64().and_then(|n| i32::try_from(n).ok())
}

/// `claim_for` the case a try, a discovery start or a replay body names; a
/// body that names none claims nothing (the call answers its own 400).
pub(crate) fn claim_named_case(caller: &crate::autorun::agents::Caller, body: &str) -> Result<Option<i32>, (u16, String)> {
    match named_case_id(body) {
        Some(case_id) => claim_for(caller, case_id),
        None => Ok(None),
    }
}

/// `claim_for` every case a save bundle names (a list of scripts, or an
/// object with `scripts`), in the bundle's order, before any of the save's
/// gates: the first refusal is the answer.
pub(crate) fn claim_bundle_cases(caller: &crate::autorun::agents::Caller, body: &str) -> Result<Option<i32>, (u16, String)> {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let scripts = if v.is_array() { Some(&v) } else { v.get("scripts") };
    let mut seen: Vec<i32> = Vec::new();
    for script in scripts.and_then(|s| s.as_array()).into_iter().flatten() {
        let Some(case_id) = script.get("case_id").and_then(|n| n.as_i64()).and_then(|n| i32::try_from(n).ok()) else {
            continue;
        };
        if !seen.contains(&case_id) {
            seen.push(case_id);
            claim_for(caller, case_id)?;
        }
    }
    Ok(None)
}

/// The refusal for a second mapping run: whose browser it is going in.
pub fn mapping_going(whose: &str) -> String {
    format!("a mapping run is going in {whose} - one mapping run at a time; wait for it to end")
}

/// The refusal for a mapping start in `caller_slot` while a mapping run goes
/// in another slot (`mapping`, per slot: `commands::autorun::mapping_in`), or
/// None. It names whose browser that is: "{label}'s browser" when the slot is
/// bound to a live assistant, else the main browser (the person's) or an
/// Auto Run browser. Never an id, a pid or a login.
pub fn mapping_refusal(caller_slot: SlotNo, mapping: [bool; SLOT_COUNT]) -> Option<String> {
    let going = SlotNo::all().into_iter().find(|m| *m != caller_slot && mapping[m.index()])?;
    let whose = match label_of_slot(going) {
        Some(label) => format!("{label}'s browser"),
        None if going.is_main() => "the main Auto Run browser".to_string(),
        None => "an Auto Run browser".to_string(),
    };
    Some(mapping_going(&whose))
}

// ------------------------------------------------------- the company database

