//! What an assistant records about runs: failures, quirks, validation rules, defects, divergences and the run order.

use super::*;

/// Before a replay or a failures answer: a case whose divergence waits for a
/// rewrite that has since been saved in Azure DevOps runs again (read only;
/// nothing when signed out).
pub(crate) async fn release_rewritten_cases(ctx: &BridgeContext, client: Option<&crate::ado::AdoClient>) {
    let (Some(client), Ok(root)) = (client, autorun_root()) else {
        return;
    };
    crate::commands::autorun_divergence::refresh_rewritten(client, &root, &ctx.org, &ctx.project, None).await;
}

/// A run's failed cases, as text an assistant can act on - and, when it
/// must not touch the script at all, the reason why.
pub(crate) fn autorun_failures(ctx: &BridgeContext, target: &str) -> (u16, String) {
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    // A `case_id` that was sent but is not a number is a mistake worth
    // naming: read as "no case given" it would quietly answer about the
    // newest run instead, which is a different question.
    let case_id = match q(target, "case_id") {
        None => None,
        Some(raw) => match raw.trim().parse::<i32>() {
            Ok(id) => Some(id),
            Err(_) => return (400, "case_id must be a number".to_string()),
        },
    };
    let run = match q(target, "run_id") {
        Some(id) => match crate::autorun::store::load_run(&root, &id) {
            Ok(Some(run)) => run,
            Ok(None) => return (404, format!("no run {id}")),
            // The store's own message names the file's path, and a path
            // is nothing the reader can act on.
            Err(_) => return (400, "the run file could not be read".to_string()),
        },
        // A case's newest word may be a replay that stopped on a failure,
        // when it is newer than the case's newest run or no run has it.
        // With no case named, a replay that stopped on a failure counts as
        // the newest word too, when it is newer than the newest run.
        None => match case_id.map_or_else(
            || crate::autorun::replay_record::latest(&root),
            |id| crate::autorun::replay_record::latest_for_case(&root, id),
        ) {
            Some(run) => run,
            None => {
                return match case_id {
                    Some(id) => (404, format!("no run on this machine has case {id}")),
                    None => (404, "no run on this machine yet".to_string()),
                }
            }
        },
    };
    // The scripts are what turn "action 2 failed" into the action's own
    // JSON. A case with no script on disk is reported as such rather than
    // dropped, so this gathers whatever is there and lets the describer
    // say what is missing.
    let scripts: Vec<crate::autorun::CaseScript> = run
        .cases
        .iter()
        .filter_map(|c| crate::autorun::store::load_script(&root, c.case_id).ok().flatten())
        .collect();
    // The project's components, so an action one ran is shown as it
    // expands. A file that does not read leaves those actions named by
    // their component only.
    let components = crate::autorun::components::load_components(&root, &ctx.org, &ctx.project).unwrap_or_default();
    let mut said = crate::autorun::failures::describe_failures_in(Some(&root), &run, &scripts, &components);
    // The divergences reported for the run's failed or blocked cases: what
    // a person has yet to rule on, what waits for a repair, what was ruled.
    let stopped: Vec<i32> =
        run.cases.iter().filter(|c| crate::autorun::failures::is_failed(c)).map(|c| c.case_id).collect();
    if let Ok(list) = crate::autorun::divergences::load(&root, &ctx.org, &ctx.project) {
        if let Some(section) =
            crate::autorun::divergences::failures_section(&list, &stopped, crate::autorun::sessions::now_ms())
        {
            said.push_str("\n\n");
            said.push_str(&section);
        }
    }
    // The bounds of a repair end the answer whenever a case failed or was
    // Blocked, so an assistant that reads only this still sees them.
    let bounds = if said.starts_with("no failed case") {
        None
    } else {
        Some(format!("\n\n{}", crate::autorun::failures::REPAIR_BOUNDS))
    };
    if case_id.is_some() || q(target, "run_id").is_some() {
        if let Some(b) = &bounds {
            said.push_str(b);
        }
        return (200, said);
    }
    // Asked of no case and no run: say which run that was, and name the
    // other recent runs here with failed or blocked cases, so failures a
    // run or two back are not missed.
    let all_scripts = crate::autorun::store::list_scripts(&root);
    let runs = crate::autorun::store::list_runs(&root);
    let now = crate::autorun::sessions::now_ms();
    let (head, others) = crate::autorun::failures::newest_run_context(&run, &all_scripts, &runs, now);
    let mut out = format!("{head}\n\n{said}");
    if let Some(others) = others {
        out.push_str("\n\n");
        out.push_str(&others);
    }
    if let Some(b) = &bounds {
        out.push_str(b);
    }
    (200, out)
}

/// Record something learned about the application, attributed, so the
/// next script does not rediscover the same surprise.
///
/// `from` says which assistant's work it came out of: "autorun" (the
/// default) or "api", for one building API templates. Both read the same
/// list - one per project.
pub(crate) fn autorun_quirk(ctx: &BridgeContext, body: &str) -> (u16, String) {
    use crate::autorun::quirks::{record_quirk_classed, QuirkClass, Recorded, FROM_API, FROM_AUTORUN};
    let text = match body_field(body, "text", "{ \"text\": \"one line about this application\" }") {
        Ok(serde_json::Value::String(s)) => s,
        Ok(_) => return (400, "\"text\" is one line of text".to_string()),
        Err(refused) => return refused,
    };
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let from = match v.get("from") {
        None | Some(serde_json::Value::Null) => FROM_AUTORUN,
        Some(serde_json::Value::String(s)) if s == FROM_AUTORUN => FROM_AUTORUN,
        Some(serde_json::Value::String(s)) if s == FROM_API => FROM_API,
        Some(_) => return (400, format!("\"from\" is \"{FROM_AUTORUN}\" or \"{FROM_API}\"")),
    };
    let class = match v.get("class") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => match QuirkClass::parse(s) {
            Some(c) => Some(c),
            None => return (400, QUIRK_CLASS_REFUSED.to_string()),
        },
        Some(_) => return (400, QUIRK_CLASS_REFUSED.to_string()),
    };
    // The cases and steps it is about, when the assistant names them -
    // each step checked against the newest run of its case, so a note can
    // only be tied to steps that really failed. That tie is what later
    // runs count evidence against.
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct NamedCase {
        case_id: i32,
        steps: Vec<i32>,
    }
    let named: Vec<NamedCase> = match v.get("cases") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(c) => match serde_json::from_value(c.clone()) {
            Ok(list) => list,
            Err(e) => {
                return (400, format!("\"cases\" is a list of {{ case_id, steps: [number] }}: {e}"))
            }
        },
    };
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let mut sources: Vec<crate::autorun::quirks::QuirkSource> = Vec::with_capacity(named.len());
    for c in &named {
        let run = crate::autorun::failures::latest_run(&root, Some(c.case_id));
        let script = crate::autorun::store::load_script(&root, c.case_id).ok().flatten();
        match crate::autorun::quirks::source_from_run(run.as_ref(), script.as_ref(), c.case_id, &c.steps) {
            Ok(s) => sources.push(s),
            Err(why) => return (400, why),
        }
    }
    match record_quirk_classed(
        &root,
        &ctx.org,
        &ctx.project,
        &text,
        "assistant",
        from,
        sources,
        class,
        crate::autorun::sessions::now_ms(),
    ) {
        Ok((recorded, kept)) => {
            let mut answer = match &recorded {
                Recorded::Added(id) => format!("recorded as {id}"),
                Recorded::AlreadyKnown(id) => format!("already known, as {id}"),
                Recorded::Reactivated(id, reason) => reactivated_reply(id, reason.as_deref()),
            };
            if let (Some(asked), Some(kept)) = (class, kept) {
                if !matches!(recorded, Recorded::Added(_)) && asked != kept {
                    answer.push_str(&format!(" (kept its class: {})", kept.label()));
                }
            }
            if kept == Some(QuirkClass::ProductBug) {
                answer.push_str(PRODUCT_BUG_ANSWER);
            }
            (200, answer)
        }
        Err(why) => (400, why),
    }
}

/// Said when a quirk is sent a `class` that is not one of the three.
pub(crate) const QUIRK_CLASS_REFUSED: &str = "\"class\" is \"app quirk\", \"environment flake\" or \"product bug\"";

/// Added to the answer for a product bug.
pub(crate) const PRODUCT_BUG_ANSWER: &str = " A product bug is shown on every result of the cases it names and at the top of the run report: tell the person, never script around it.";

/// Remember, or take out, one rule the application enforced in an area -
/// the words its page showed after an action it refused - as the
/// assistant judged it (`record_validation_rule`). Reads the open
/// discovery's area and the page the browser is on, the way `try` does,
/// without waiting for a browser an unattended run holds.
pub(crate) async fn record_validation_rule(ctx: &BridgeContext, body: &str, n: SlotNo) -> (u16, String) {
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    if unattended_run_is_using_the_browser().is_some() || crate::autorun::replay_to::state(n).is_running() {
        return rule_in_area(&root, &ctx.org, &ctx.project, body, None, "");
    }
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    record_validation_rule_in(&mut slot, &root, &ctx.org, &ctx.project, body).await
}

/// `record_validation_rule` in the browser `slot` holds: its discovery's
/// area is where a rule goes when the body names none, and the page it is
/// on is the rule's page when the body names none. No browser, or one open
/// for no discovery, leaves the body to name them.
pub async fn record_validation_rule_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    body: &str,
) -> (u16, String) {
    let Some(browser) = slot.as_mut() else {
        return rule_in_area(root, organization, project, body, None, "");
    };
    let p = browser.parts();
    let area = p.discovery.as_ref().and_then(|d| d.area.clone());
    // Only an add that names no page takes the browser's.
    let page = if takes_the_browsers_page(body) { current_page(p.driver).await.0 } else { String::new() };
    rule_in_area(root, organization, project, body, area.as_deref(), &page)
}

/// Whether a `record_validation_rule` body is an add that names no page,
/// the one kind filed under the page the browser is on. A body that is not
/// a rule is left to `rule_in_area` to refuse.
pub(crate) fn takes_the_browsers_page(body: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    let removal = v["remove"].as_bool().unwrap_or(false);
    let named = v["page"].as_str().is_some_and(|p| !p.trim().is_empty());
    !removal && !named
}

/// The rule a body tells, kept in (or taken out of) the map. `discovery_area`
/// and `browser_page` (a path, empty when not known) fill in what the body
/// leaves out; an area from neither is asked for rather than guessed, and
/// one the body names that is neither recorded nor in the map is refused,
/// so a typo makes no stray area. An add without a page is on the page the
/// browser is on; a removal without one is from every page of the area,
/// whatever the browser shows: the words are the rule.
pub(crate) fn rule_in_area(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    body: &str,
    discovery_area: Option<&str>,
    browser_page: &str,
) -> (u16, String) {
    use crate::autorun::discovery_map::{canonical_area, forget_refusal_saved, page_path, remember_refusals, RefusalRule, RuleSource};
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct RuleBody {
        message: String,
        after: Option<String>,
        area: Option<String>,
        page: Option<String>,
        step: Option<String>,
        #[serde(default)]
        remove: bool,
    }
    let shape = "{ \"message\": \"the words the page showed\", \"after\": \"the action that caused it\" }";
    let rule: RuleBody = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => return (400, format!("that is not a rule: {e}. Expected {shape}.")),
    };
    // One line, as the page's messages are read.
    let words = |text: &str| -> String {
        let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
        one.chars().take(crate::browser::page_messages::MAX_MESSAGE_CHARS).collect()
    };
    let named = |text: &Option<String>| text.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
    let message = words(&rule.message);
    if message.is_empty() {
        return (400, "\"message\" is the application's own words, as the page showed them".to_string());
    }
    if organization.trim().is_empty() || project.trim().is_empty() {
        return (400, RULE_NEEDS_A_PROJECT.to_string());
    }
    let named_area = named(&rule.area);
    let Some(area) = named_area.clone().or_else(|| discovery_area.map(str::trim).filter(|a| !a.is_empty()).map(str::to_string)) else {
        return (400, "Name the area: no discovery is running.".to_string());
    };
    let area = canonical_area(root, organization, project, &area);
    if named_area.is_some() {
        // The discovery's own area is one the assistant started; a name
        // typed into the body must already be known.
        let key = crate::autorun::discovery_map::area_key(&area);
        let recorded = crate::autorun::nav::load_nav(root, organization, project)
            .ok()
            .is_some_and(|nav| crate::autorun::nav::find_area(&nav, &area).is_some());
        let mapped = crate::autorun::discovery_map::load_map(root, organization, project)
            .ok()
            .is_some_and(|map| map.areas.iter().any(|a| crate::autorun::discovery_map::area_key(&a.area) == key));
        if !recorded && !mapped {
            return (400, format!("No area named \"{area}\" is recorded."));
        }
    }
    // Only an add takes the page the browser is on.
    let page = named(&rule.page)
        .or_else(|| Some(browser_page.trim().to_string()).filter(|p| !p.is_empty() && !rule.remove))
        .map(|p| page_path(&p));
    if rule.remove {
        return match forget_refusal_saved(root, organization, project, &area, page.as_deref(), &message) {
            Ok(true) => (200, format!("Removed from {area}.")),
            Ok(false) => (200, format!("No such rule in {area}.")),
            Err(_) => rule_not_kept(),
        };
    }
    let after = words(rule.after.as_deref().unwrap_or(""));
    if after.is_empty() {
        return (400, "\"after\" is the action that caused it, in a few words (needed unless you remove the rule)".to_string());
    }
    let Some(page) = page else {
        return (400, "Name the page: the Auto Run browser is not on one.".to_string());
    };
    let now = crate::autorun::sessions::now_ms();
    let step = named(&rule.step);
    let rule = RefusalRule { page, step, after, message, source: RuleSource::Assistant, case_id: None, first_seen: now, last_seen: now, count: 1 };
    match remember_refusals(root, organization, project, &area, vec![rule]) {
        Ok(()) => (200, format!("Remembered for {area}.")),
        Err(_) => rule_not_kept(),
    }
}

/// Said by `record_validation_rule` with no project open: a rule belongs to one.
pub(crate) const RULE_NEEDS_A_PROJECT: &str = "open a project in the app first - a validation rule belongs to a project";

/// Said when the map could not be written: how, never in the rule's words
/// or a file's path.
pub(crate) fn rule_not_kept() -> (u16, String) {
    crate::applog::warn("Discovery map: a rule the assistant gave could not be kept".to_string());
    (500, "the discovery map could not be written, so the rule was not kept".to_string())
}

/// Mark a case as a suspected application defect: the script is right, the
/// application did not do what the case expects. The newest run of the
/// case decides whether the step really failed there (and not for a
/// `STOP:` reason) - `defects::check_mark`. Answers with the stored mark.
/// Never touches the script's actions, its repairs or its last repair.
pub(crate) fn autorun_defect(body: &str) -> (u16, String) {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct MarkBody {
        case_id: i32,
        step_number: i32,
        note: String,
    }
    let shape = "{ \"case_id\": 501, \"step_number\": 3, \"note\": \"what the application did, against what the case expects\" }";
    let m: MarkBody = match serde_json::from_str(body) {
        Ok(m) => m,
        Err(e) => return (400, format!("that is not a mark: {e}. Expected {shape}.")),
    };
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let script = match crate::autorun::store::load_script(&root, m.case_id) {
        Ok(s) => s,
        Err(e) => return (500, e),
    };
    // The newest word on the case: a replay that stopped on a failure
    // counts when it is newer than the case's newest run, or none has it.
    let run = crate::autorun::replay_record::latest_for_case(&root, m.case_id);
    let mark = match crate::autorun::defects::check_mark(
        run.as_ref(),
        script.as_ref(),
        m.case_id,
        m.step_number,
        &m.note,
        crate::autorun::sessions::now_ms(),
    ) {
        Ok(mark) => mark,
        Err(why) => return (400, why),
    };
    if let Err(e) = crate::autorun::store::set_suspected_defect(&root, m.case_id, Some(mark.clone())) {
        return (500, format!("could not save the mark: {e}"));
    }
    crate::applog::info(format!(
        "AI marked case {} step {} as a suspected application defect",
        m.case_id, mark.step_number
    ));
    (200, serde_json::to_string(&mark).unwrap_or_default())
}

/// `POST /autorun-divergence`: where a test case and the live application
/// disagree, reported instead of fitting a check to the page. The case is
/// read from Azure DevOps and must hold the words the assistant quotes
/// (`case_says`), at a step it has; the divergence is stored for a person
/// to rule on (`divergences::report_in`). Never touches a script.
pub(crate) async fn autorun_divergence(ctx: &BridgeContext, client: Option<&crate::ado::AdoClient>, body: &str) -> (u16, String) {
    use crate::autorun::divergences::{self, Reported};
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct DivergenceBody {
        case_id: i32,
        step: i32,
        case_says: String,
        app_shows: String,
        kind: divergences::DivergenceKind,
        #[serde(default)]
        proposal: Option<String>,
    }
    let shape = "{ \"case_id\": 147556, \"step\": 2, \"case_says\": \"exactly 4 levels\", \"app_shows\": \"6 levels\", \"kind\": \"data\" }";
    let b: DivergenceBody = match serde_json::from_str(body) {
        Ok(b) => b,
        Err(e) => {
            return (
                400,
                format!("that is not a divergence: {e}. Expected {shape} - kind is data, app or wording."),
            )
        }
    };
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let Some(client) = client else {
        return (
            503,
            "sign in to Test Case Manager first - a divergence is checked against its test case".to_string(),
        );
    };
    let case = match client
        .get_test_cases_by_ids(&ctx.org, &[b.case_id], ctx.module_ref.as_deref(), ctx.preconditions_ref.as_deref())
        .await
    {
        Ok(cases) => cases.into_iter().find(|c| c.id == b.case_id),
        Err(crate::ado::AdoError::NotFound) => None,
        Err(e) => {
            // The raw error goes to the log; the reader gets the sentence
            // that names no address (CLAUDE.md, user-facing errors).
            crate::applog::warn(format!("Auto Run: a divergence's test case could not be read: {e:?}"));
            return (502, format!("Azure DevOps error: {}", e.user_text()));
        }
    };
    let Some(case) = case else {
        return (400, format!("case {} is not a test case in this organization", b.case_id));
    };
    if b.step < 1 || b.step as usize > case.steps.len() {
        return (400, format!("case {} has {} steps - \"step\" is one of them", b.case_id, case.steps.len()));
    }
    if !crate::autorun::case_text::CaseText::from_case(&case).holds_quote(&b.case_says) {
        return (
            400,
            format!(
                "\"case_says\" is the case's own words, quoted from step {}, another expected result or the preconditions - case {} does not hold \"{}\"",
                b.step, b.case_id, b.case_says
            ),
        );
    }
    let (case_says, app_shows) =
        (crate::autorun::defects::scrub_note(&b.case_says), crate::autorun::defects::scrub_note(&b.app_shows));
    let proposal = b.proposal.as_deref().map(crate::autorun::defects::scrub_note);
    let now = crate::autorun::sessions::now_ms();
    let mut refused: Option<String> = None;
    let reported = divergences::update(&root, &ctx.org, &ctx.project, |list| {
        let r = divergences::report_with_proposal_in(list, b.case_id, b.step, &case_says, &app_shows, b.kind, proposal.as_deref(), now)
            .inspect_err(|why| refused = Some(why.clone()))?;
        let id = match &r {
            Reported::New(id) | Reported::Updated(id) => id,
        };
        let id = id.clone();
        divergences::note_pbi_in(list, &id, ctx.current_pbi.as_ref().map(|p| p.id));
        let stored = list.iter().find(|d| d.id == id).cloned().expect("the divergence just reported");
        Ok((stored, matches!(r, Reported::Updated(_))))
    });
    match reported {
        Ok((stored, updated)) => {
            crate::applog::info(format!(
                "AI reported a {} divergence on case {} step {}",
                stored.kind.word(),
                b.case_id,
                b.step
            ));
            (200, divergences::tool_answer(b.case_id, &stored, updated))
        }
        // A rule's refusal is the assistant's to read; a file that could
        // not be read or written is the machine's.
        Err(e) if refused.is_some() => (400, e),
        Err(e) => (500, e),
    }
}

/// `POST /autorun-divergence-ruling`: the assistant applies the ruling the person gave in the chat, on one
/// divergence or on several (all or none), quoting their words (`divergences::rule_by_assistant_in`, by
/// `commands::autorun_divergence::rule_by_assistant_at`). A rewrite ruling takes the case's print at once, so a
/// later rewrite is seen as a difference. The person's words are stored and shown, never written to the log.
pub(crate) async fn autorun_divergence_ruling(ctx: &BridgeContext, client: Option<&crate::ado::AdoClient>, body: &str) -> (u16, String) {
    use crate::autorun::divergences::{self, AssistantRuling};
    #[derive(serde::Deserialize)]
    struct Pick {
        case_id: i32,
        divergence_id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct RulingBody {
        case_id: Option<i32>,
        divergence_id: Option<String>,
        divergences: Option<Vec<Pick>>,
        ruling: AssistantRuling,
        said: String,
        note: Option<String>,
    }
    let shape = "{ \"case_id\": 141701, \"divergence_id\": \"d3\", \"ruling\": \"case_right\", \"said\": \"a different cycle name is fine\" } - ruling is case_right, data_fixed or rewrite_case";
    let b: RulingBody = match serde_json::from_str(body) {
        Ok(b) => b,
        Err(e) => return (400, format!("that is not a ruling: {e}. Expected {shape}.")),
    };
    let single = b.case_id.is_some() || b.divergence_id.is_some();
    let picks: Vec<(i32, String)> = match (single, b.divergences) {
        (true, Some(_)) => {
            return (
                400,
                "name one divergence (case_id and divergence_id) or several (divergences), not both".to_string(),
            )
        }
        (true, None) => match (b.case_id, b.divergence_id) {
            (Some(c), Some(d)) => vec![(c, d)],
            _ => return (400, divergences::NO_PICK.to_string()),
        },
        (false, Some(list)) => list.into_iter().map(|p| (p.case_id, p.divergence_id)).collect(),
        (false, None) => return (400, divergences::NO_PICK.to_string()),
    };
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let ruling = b.ruling.ruling();
    let now = crate::autorun::sessions::now_ms();
    let answer = match crate::commands::autorun_divergence::rule_by_assistant_at(
        &root,
        &ctx.org,
        &ctx.project,
        &picks,
        ruling,
        &b.said,
        b.note.as_deref(),
        now,
    ) {
        Ok(answer) => answer,
        Err(crate::commands::autorun_divergence::RulingFailure::Refused(why)) => return (400, why),
        Err(crate::commands::autorun_divergence::RulingFailure::Machine(e)) => return (500, e),
    };
    if ruling == crate::autorun::divergences::Ruling::Rewrite {
        if let Some(client) = client {
            let mut cases: Vec<i32> = picks.iter().map(|(c, _)| *c).collect();
            cases.sort_unstable();
            cases.dedup();
            crate::commands::autorun_divergence::refresh_rewritten(client, &root, &ctx.org, &ctx.project, Some(&cases)).await;
        }
    }
    (200, answer)
}

/// `POST /autorun-defect-clear`: the assistant clears the suspected-defect mark it set, once the person said so
/// in the chat. Their words are kept with the cleared mark (`store::clear_suspected_defect_with_words`) so the
/// case card can show them; there is no Undo. Never written to the log: only the case and the step.
pub(crate) fn autorun_defect_clear(body: &str) -> (u16, String) {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ClearBody {
        case_id: i32,
        said: String,
        note: Option<String>,
    }
    let shape = "{ \"case_id\": 501, \"said\": \"the person's own words from this chat\" }";
    let b: ClearBody = match serde_json::from_str(body) {
        Ok(b) => b,
        Err(e) => return (400, format!("that is not a clearing: {e}. Expected {shape}.")),
    };
    let said = match crate::autorun::divergences::check_said(&b.said) {
        Ok(s) => s,
        Err(why) => return (400, why),
    };
    let note = match crate::autorun::divergences::check_note(b.note.as_deref()) {
        Ok(n) => n,
        Err(why) => return (400, why),
    };
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let script = match crate::autorun::store::load_script(&root, b.case_id) {
        Ok(s) => s,
        Err(e) => return (500, e),
    };
    if script.as_ref().and_then(|s| s.suspected_defect.as_ref()).is_none() {
        return (400, format!("case {} has no suspected-defect mark", b.case_id));
    }
    let now = crate::autorun::sessions::now_ms();
    match crate::autorun::store::clear_suspected_defect_with_words(&root, b.case_id, &said, note.as_deref(), now) {
        Ok(Some(step)) => {
            crate::applog::info(format!("AI cleared the suspected defect on case {} step {step} on the person's words", b.case_id));
            (
                200,
                format!(
                    "Cleared the suspected-defect mark on case {} step {step}, on the person's words. A failure there is no longer labelled as an application defect.",
                    b.case_id
                ),
            )
        }
        Ok(None) => (400, format!("case {} has no suspected-defect mark", b.case_id)),
        Err(e) => (500, e),
    }
}

/// What `set_autorun_order` says after the order it saved: with
/// `mode: "replace"` (the default) the call replaced the PBI's whole saved
/// order, and the PBI's other cases run after the listed ones.
pub const ORDER_REPLACED: &str = "This replaced the PBI's whole Auto Run order: its other cases run after these, in list order. To add cases ahead of the saved order and keep the rest of it, send mode: \"append\".";

/// `POST /autorun-order`, body `{ pbi_id, case_ids, mode? }`: Auto Run's own
/// execution order for the PBI on this machine (`store::save_order`). Run
/// Tests' order is not touched. Every id must be one of the PBI's test
/// cases, read from Azure DevOps the way the Auto Run tab lists them; a
/// listed case with no saved script is saved in the order and named back.
pub(crate) async fn autorun_order(ctx: &BridgeContext, client: Option<&crate::ado::AdoClient>, body: &str) -> (u16, String) {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct OrderBody {
        pbi_id: i32,
        case_ids: Vec<i32>,
        #[serde(default)]
        mode: Option<String>,
    }
    let shape = "{ \"pbi_id\": 100, \"case_ids\": [503, 501, 502], \"mode\": \"replace\" | \"append\" (optional) }";
    let o: OrderBody = match serde_json::from_str(body) {
        Ok(o) => o,
        Err(e) => return (400, format!("that is not an order: {e}. Expected {shape}.")),
    };
    let append = match o.mode.as_deref().map(str::trim) {
        None | Some("replace") => false,
        Some("append") => true,
        Some(other) => return (400, format!("mode \"{other}\" is not one: send \"replace\" (the default) or \"append\". Expected {shape}.")),
    };
    if o.case_ids.is_empty() {
        return (400, "case_ids is empty - list the PBI's cases in the order Auto Run should run them".to_string());
    }
    for (i, id) in o.case_ids.iter().enumerate() {
        if o.case_ids[..i].contains(id) {
            return (400, format!("case {id} is listed twice"));
        }
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let Some(client) = client else {
        return (503, "sign in to Test Case Manager first".to_string());
    };
    let on_pbi: Vec<i32> = match client.get_pbi_test_cases(&ctx.org, o.pbi_id).await {
        Ok(cases) => cases.into_iter().map(|c| c.id).collect(),
        Err(crate::ado::AdoError::NotFound) => {
            return (404, format!("Azure DevOps has no work item #{} - check the PBI id", o.pbi_id))
        }
        Err(e) => return (502, format!("Azure DevOps error: {}", e.user_text())),
    };
    let not_on_pbi: Vec<String> = o
        .case_ids
        .iter()
        .filter(|id| !on_pbi.contains(id))
        .map(|id| format!("case {id} is not in PBI {}", o.pbi_id))
        .collect();
    if !not_on_pbi.is_empty() {
        return (400, not_on_pbi.join("; "));
    }
    // The Azure DevOps read is done: only the file's read and write hold the
    // order lock (never across an await), so another call's append cannot
    // be read past and then overwritten here.
    let (kept, saved): (Vec<i32>, Vec<i32>) = match crate::autorun::store::with_order_locked(|| {
        // Appending keeps the saved order's other cases after the given ones.
        let kept: Vec<i32> = match (append, crate::autorun::store::load_order(&root, o.pbi_id)) {
            (true, Some(existing)) => crate::autorun::store::append_order(&existing, &o.case_ids)
                .into_iter()
                .filter(|id| !o.case_ids.contains(id))
                .collect(),
            _ => Vec::new(),
        };
        let saved: Vec<i32> = o.case_ids.iter().chain(&kept).copied().collect();
        crate::autorun::store::save_order(&root, o.pbi_id, &saved).map(|()| (kept, saved))
    }) {
        Ok(both) => both,
        Err(e) => return (500, format!("could not save the order: {e}")),
    };
    let how = if append { "appended" } else { "replaced" };
    crate::applog::info(format!("AI set Auto Run's order for PBI {} ({how}): {} case(s)", o.pbi_id, saved.len()));
    let ids = |list: &[i32]| list.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(", ");
    let mut lines = vec![format!("saved Auto Run's order for PBI {}: {}", o.pbi_id, ids(&o.case_ids))];
    if append {
        lines.push(match kept.is_empty() {
            true => "Appended: the saved order had no other cases.".to_string(),
            false => format!("Appended: the saved order's other cases run after these, as they were: {}", ids(&kept)),
        });
    } else {
        lines.push(ORDER_REPLACED.to_string());
    }
    for id in &o.case_ids {
        if !matches!(crate::autorun::store::load_script(&root, *id), Ok(Some(_))) {
            lines.push(format!("case {id} has no saved script"));
        }
    }
    (200, lines.join("\n"))
}

/// What an assistant is told when its line brought a retired note back.
pub(crate) fn reactivated_reply(id: &str, reason: Option<&str>) -> String {
    match reason {
        Some(r) => format!("{id} had been retired (\"{r}\") - it is back on the list; check that reason no longer holds"),
        None => format!("{id} had been retired - it is back on the list"),
    }
}

/// Retire one of the assistant's own quirks, with a reason - and, when
/// `replacement` is given, file the better note in the same call, keeping
/// the old one's sources. A person's note is refused: it is theirs to
/// remove, in the app.
pub(crate) fn autorun_quirk_retire(ctx: &BridgeContext, body: &str) -> (u16, String) {
    use crate::autorun::quirks::{retire_in, update_quirks, Recorded};
    let shape = "{ \"id\": \"q1a2b3c\", \"reason\": \"why it no longer helps\", \"replacement\": \"optional better note\" }";
    let id = match body_field(body, "id", shape) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(_) => return (400, "\"id\" is a quirk's id, as the guide shows it".to_string()),
        Err(refused) => return refused,
    };
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let reason = match v.get("reason") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(_) => return (400, "\"reason\" is one sentence".to_string()),
    };
    let replacement = match v.get("replacement") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(_) => return (400, "\"replacement\" is one line of text".to_string()),
    };
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let now = crate::autorun::sessions::now_ms();
    match update_quirks(&root, &ctx.org, &ctx.project, |list| {
        retire_in(list, &id, reason.as_deref(), replacement.as_deref(), true, now)
    }) {
        Ok((None, _)) => (200, format!("retired {}", id.trim())),
        Ok((Some(Recorded::Added(new)), _)) => (200, format!("retired {}, replaced by {new}", id.trim())),
        Ok((Some(Recorded::AlreadyKnown(new) | Recorded::Reactivated(new, _)), _)) => {
            (200, format!("retired {} - the replacement is already on the list as {new}", id.trim()))
        }
        Err(why) => (400, why),
    }
}

// ------------------------------------------------- the environment's accounts

