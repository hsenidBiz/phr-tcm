//! Starting a discovery and driving it: one action, or a batch of them.

use super::*;

/// Said to a discovery action, or a page read for one, with no discovery
/// going.
pub const NO_DISCOVERY: &str = "no discovery is going - start one with start_autorun_discovery";

/// Said to a mapping run started with no module named.
pub const MAPPING_NEEDS_A_MODULE: &str = "Name at least one module to map.";

/// Said to a start that asks to explore and to map at once.
pub const EXPLORE_NOT_MAPPING: &str = "explore and mapping are two different runs - send one of them";

/// Said when an explore session's change list could not be made at its
/// start: the session did not start.
pub const EXPLORE_NOT_STARTED: &str = "the explore session's change list could not be started (see Settings -> Logs), so the session did not start and its browser is closed";

/// Whether an argument is true, as a boolean or as the text "true".
pub(crate) fn said_true(v: &serde_json::Value) -> bool {
    v.as_bool() == Some(true) || v.as_str().map(|s| s.trim().eq_ignore_ascii_case("true")) == Some(true)
}

/// Said when the browser a discovery opened was replaced (the person
/// pressed Open browser) before its sign-in.
pub(crate) const DISCOVERY_TAKEN_OVER: &str =
    "the Auto Run browser was taken over before the discovery signed in - start the discovery again";

/// The words of the page's live messages: alerts, statuses and live
/// regions. Read before and after a discovery action, so the ones the
/// action brought up can be told apart.
pub(crate) const STATUS_TEXT_JS: &str = "Array.from(document.querySelectorAll('[role=alert],[role=status],[aria-live=assertive],[aria-live=polite]')).map(e => (e.innerText || '').trim()).filter(t => t.length > 0)";

/// A name the body gave, trimmed; blank is none.
pub(crate) fn named(text: Option<&str>) -> Option<String> {
    text.map(str::trim).filter(|t| !t.is_empty()).map(str::to_string)
}

/// The page's live messages right now (`STATUS_TEXT_JS`), each on one short
/// line. None when the page cannot say.
pub(crate) async fn status_texts<D: crate::browser::cdp::Driver>(d: &mut D) -> Vec<String> {
    match crate::browser::page::eval_value(d, STATUS_TEXT_JS).await {
        Ok(serde_json::Value::Array(items)) => items.iter().filter_map(|v| v.as_str()).map(one_short_line).collect(),
        _ => Vec::new(),
    }
}

/// `text` with every value a `fill` in `action` typed taken out: the map
/// keeps what an action led to, never what was typed to get there.
pub(crate) fn without_typed(text: &str, action: &crate::browser::actions::Action) -> String {
    let mut out = text.to_string();
    for a in action.each() {
        if let crate::browser::actions::Action::Fill { value, .. } = a {
            if !value.trim().is_empty() {
                out = out.replace(value.as_str(), "(typed)");
            }
        }
    }
    out
}

/// Sign the browser a discovery just opened in as `account_key` (a KEY,
/// never a login), and hand back where it landed: `{signed_in, detail,
/// path, page}`, the path with no host or query. A browser it took over
/// from the assistant's replay that is still signed in as that account -
/// the same checks a replay makes (`replay_to::signed_in_here`: the lease,
/// the sign-in's cookies, a session not refused, the signed-in marker) -
/// keeps its sign-in and goes home (`went_home`,
/// `discovery_signed_in_already`); any other is signed in as it. The discovery takes the
/// account and `area` once the sign-in arrives, and the landing page is
/// filed under that area, explored by that account. A sign-in that does
/// not arrive - or an account that cannot be signed in at all - closes
/// the browser and says why. A browser the person opened is never signed
/// in or closed here.
pub async fn discover_start_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    account_key: &str,
    area: Option<&str>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    let key = account_key.trim();
    let failed = {
        let Some(browser) = slot.as_mut() else {
            return (409, NO_SUPERVISED_BROWSER.to_string());
        };
        let p = browser.parts();
        if p.discovery.is_none() {
            return (409, DISCOVERY_TAKEN_OVER.to_string());
        }
        let kept = crate::autorun::replay_to::signed_in_here(
            p.driver,
            root,
            organization,
            project,
            key,
            &*p.signed_in,
            &*p.lease,
            timing,
        )
        .await
            && went_home(p.driver, root, organization, project, timing).await
            // Going home may have re-issued a cookie of the same sign-in.
            && crate::autorun::replay_to::retake_sign_in_for(p.driver, root, organization, project, p.lease, "the home page")
                .await
                .is_ok();
        lift_a_calls_guard(p.driver, p.guarded_case, p.guard_owner).await;
        let signed = if kept {
            crate::applog::info(format!("Auto Run discovery as {key}: signed in already, the sign-in is kept"));
            Ok(discovery_signed_in_already(key))
        } else {
            match crate::autorun::signin::sign_in_leased(
                p.driver,
                root,
                organization,
                project,
                key,
                p.lease,
                p.signed_in,
                timing,
            )
            .await
            {
                Err(why) => Err(why),
                Ok(out) if !out.ok => Err(out.detail),
                Ok(out) => Ok(out.detail),
            }
        };
        let signed_at = std::time::Instant::now();
        match signed {
            Err(why) => why,
            Ok(detail) => {
                let area =
                    named(area).map(|a| crate::autorun::discovery_map::canonical_area(root, organization, project, &a));
                let started_at =
                    p.discovery.as_ref().map_or_else(crate::autorun::sessions::now_ms, |s| s.started_at);
                let tried = p.discovery.as_mut().map(|s| std::mem::take(&mut s.tried)).unwrap_or_default();
                // A mapping run's summary is kept under the project it signed in to.
                let mapping = p.discovery.as_mut().and_then(|s| s.mapping.take()).map(|run| {
                    crate::commands::autorun::MappingRun {
                        place: Some(crate::commands::autorun::MappingPlace {
                            root: root.to_path_buf(),
                            organization: organization.to_string(),
                            project: project.to_string(),
                        }),
                        ..run
                    }
                });
                let is_mapping = mapping.is_some();
                // An explore session keeps its change list in a file of its
                // own, made now that the sign-in is known: one that cannot be
                // made closes the browser, as a sign-in that failed does.
                let (explore, explore_failed) = match p.discovery.as_mut().and_then(|s| s.explore.take()) {
                    None => (None, None),
                    Some(mut run) => match crate::autorun::explore::begin(root, organization, project, key, started_at) {
                        Ok(id) => {
                            run.session = Some(id);
                            // After the sign-in: its own answer is never
                            // what verifies a create.
                            run.began_ms = crate::autorun::sessions::now_ms();
                            (Some(run), None)
                        }
                        Err(why) => {
                            crate::applog::warn(format!("Auto Run explore session could not start: {why}"));
                            (None, Some(EXPLORE_NOT_STARTED.to_string()))
                        }
                    },
                };
                let (case_id, no_save) = p.discovery.as_ref().map_or((None, false), |s| (s.case_id, s.no_save));
                let state = crate::commands::autorun::DiscoveryState {
                    area,
                    account: Some(key.to_string()),
                    started_at,
                    tried,
                    mapping,
                    case_id,
                    no_save,
                    call_no_save: false,
                    call_case_id: None,
                    explore,
                    carried: Vec::new(),
                    kept: Default::default(),
                    signed_out: false,
                };
                let must_not_save = discovery_must_not_save(root, &state);
                *p.discovery = Some(crate::commands::autorun::DiscoveryState { no_save: must_not_save, ..state });
                // A mapping run saves nothing, and neither does a discovery
                // for a case that must not save: the guard goes on once the
                // sign-in (which itself sends a form) has arrived, and before
                // the page is touched again. A guard that will not go on
                // closes the browser: neither ever goes unguarded.
                let guarded = match (explore_failed, guard_a_mapping_run(p.driver, is_mapping, root, organization, project).await) {
                    (Some(why), _) => Err(why),
                    (None, Ok(())) => guard_a_no_save_discovery(p.driver, must_not_save, root, organization, project).await,
                    (None, Err(why)) => Err(why),
                };
                if let Err(why) = guarded {
                    why
                } else {
                    // Both guards are the discovery's, on until it ends.
                    if is_mapping || must_not_save {
                        crate::commands::autorun::claim_guard(p.guard_owner, crate::commands::autorun::GuardOwner::Discovery);
                    }
                    // Landed: at the area when it names a saved one, else home.
                    let named_area = p.discovery.as_ref().and_then(|s| s.area.clone());
                    let landing = land_at_area(
                        p.driver,
                        root,
                        organization,
                        project,
                        key,
                        named_area.as_deref(),
                        signed_at,
                        timing,
                    )
                    .await;
                    // The landing page is filed, but does not mark the area
                    // explored: nothing of the area itself has been seen yet.
                    let at =
                        discovery_sighting(root, organization, project, p.discovery.as_ref(), None, p.signed_in.as_deref())
                            .map(|at| Sighting { discovering: None, ..at });
                    let (status, page) =
                        read_page(p.driver, crate::browser::snapshot::DEFAULT_LIMIT, at.as_ref()).await;
                    let (path, _) = current_page(p.driver).await;
                    let mut answer = serde_json::json!({
                        "signed_in": true,
                        "detail": detail,
                        "path": path,
                        "page": if status == 200 { page.as_str() } else { "" },
                    });
                    if status != 200 {
                        answer["page_unavailable"] = serde_json::json!(page);
                    }
                    if let Some(at_area) = landing.at_area {
                        answer["at_area"] = serde_json::json!(at_area);
                        if at_area {
                            if let Some(name) = named_area.as_deref() {
                                answer["detail"] = serde_json::json!(format!("{detail}, then to {name}"));
                            }
                        }
                    }
                    if let Some(walk) = &landing.walk {
                        answer["area_walk"] = serde_json::json!(walk);
                    }
                    if !landing.notes.is_empty() {
                        answer["area_notes"] = serde_json::json!(landing.notes);
                    }
                    if must_not_save && !is_mapping {
                        answer["no_save"] = serde_json::json!(true);
                        let blocked = take_no_save_blocks(p.driver, &[]);
                        if !blocked.is_empty() {
                            answer["blocked"] = serde_json::json!(blocked.len());
                            answer["blocked_writes"] = serde_json::json!(blocked);
                        }
                    }
                    let session = p.discovery.as_ref().and_then(|s| s.explore_session()).map(str::to_string);
                    if let Some(session) = &session {
                        answer["explore"] = serde_json::json!({ "session": session });
                    }
                    let what = if is_mapping {
                        "mapping run"
                    } else if session.is_some() {
                        "explore session"
                    } else {
                        "discovery"
                    };
                    crate::applog::info(format!("Auto Run {what} started as {key}"));
                    return (200, answer.to_string());
                }
            }
        }
    };
    if let Some(browser) = slot.take() {
        browser.close();
    }
    crate::applog::info(format!("Auto Run discovery as {key} did not start; its browser is closed"));
    (409, failed)
}

/// Said in a discovery start's answer (`area_walk`) when `area` is no saved
/// area: the discovery starts at home and files what it sees under that name.
pub fn no_saved_area_to_start_at(area: &str) -> String {
    format!("no saved area is called {area}; the discovery starts at home and files what it sees under that name")
}

/// Where a discovery start landed and what it says about the area.
#[derive(Default)]
pub(crate) struct AreaLanding {
    /// `Some` when an area was named: whether the browser now stands at it.
    pub(crate) at_area: Option<bool>,
    /// Why it does not, when an area was named.
    pub(crate) walk: Option<String>,
    /// What the walk did on the way besides clicking the path.
    pub(crate) notes: Vec<String>,
}

/// After a discovery's sign-in: walk to the saved area `area` as a run
/// does (`nav::reach_module_noted`, from the sign-in), and close the
/// project's known pop-ups - waiting a moment for one to come up - so the
/// first action is never covered by a cookie banner. A walk that fails
/// leaves the browser where it stopped and the discovery going; an area that
/// is not saved leaves it at home. Never closes the browser.
pub(crate) async fn land_at_area<D: crate::browser::cdp::Driver>(
    d: &mut D,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    account: &str,
    area: Option<&str>,
    signed_at: std::time::Instant,
    timing: &crate::browser::timing::Timing,
) -> AreaLanding {
    /// How long, from when the sign-in finished, a banner is waited for.
    const BANNER_WAIT_MS: u64 = 3_000;
    let recipe = crate::autorun::recipe::load_effective_recipe_if_any(root, organization, project).ok().flatten();
    let mut landing = AreaLanding::default();
    let route = area.map(str::trim).filter(|a| !a.is_empty()).map(|a| {
        match crate::autorun::nav::load_nav(root, organization, project) {
            Err(why) => Err(why),
            Ok(nav) if crate::autorun::nav::find_area(&nav, a).is_some() => {
                crate::autorun::runner::discovery_area_route(root, organization, project, Some(a))
            }
            Ok(_) => Err(no_saved_area_to_start_at(a)),
        }
    });
    let mut walked = false;
    match route {
        None => {}
        Some(Err(why)) => {
            landing.at_area = Some(false);
            landing.walk = Some(why);
        }
        Some(Ok(route)) => {
            walked = true;
            // A banner up over the menu would take the first click.
            if let Some(recipe) = &recipe {
                crate::autorun::popups::close_shown_within(d, recipe, 0, timing).await;
            }
            let (out, notes) = crate::autorun::nav::reach_module_noted(
                d,
                &route,
                crate::autorun::nav::TripFrom::SignIn,
                timing,
                "Auto Run discovery",
                Some(account),
            )
            .await;
            landing.at_area = Some(out.ok);
            landing.notes = notes;
            if !out.ok {
                crate::applog::info(format!("Auto Run discovery: the walk to its area did not arrive ({})", out.detail));
                landing.walk = Some(out.detail);
            }
        }
    }
    // The walk's own time counts toward the wait: a banner not up by then is not coming.
    let remaining = BANNER_WAIT_MS.saturating_sub(signed_at.elapsed().as_millis() as u64);
    if let Some(recipe) = &recipe {
        crate::autorun::popups::close_shown_within(d, recipe, remaining, timing).await;
    }
    if walked {
        settle_landing(d, timing).await;
    }
    landing
}

/// After a walk to an area: let the page finish what it does on arriving,
/// as a run lets it before a click (`actions::settle`), then wait for its
/// address to hold still for `SETTLE_QUIET` (at most `SETTLE_CAP`), since a
/// screen can redirect itself a moment after it loads. The answer that
/// follows then describes where the browser is. Never fails: a page that
/// will not hold still is answered as it stands.
pub(crate) async fn settle_landing<D: crate::browser::cdp::Driver>(d: &mut D, timing: &crate::browser::timing::Timing) {
    use crate::browser::actions::{SETTLE_CAP, SETTLE_QUIET};
    crate::browser::actions::settle(d, timing).await;
    let began = std::time::Instant::now();
    let mut seen = String::new();
    let mut since = began;
    loop {
        match crate::browser::page::eval_value(d, "location.href").await {
            Ok(v) => {
                let at = v.as_str().map(crate::autorun::nav::path_of).unwrap_or_default();
                if at != seen {
                    seen = at;
                    since = std::time::Instant::now();
                }
            }
            // Between two documents the page refuses: that is a change.
            Err(e) if e.is_transient() => since = std::time::Instant::now(),
            Err(_) => return,
        }
        if since.elapsed() >= SETTLE_QUIET || began.elapsed() >= SETTLE_CAP {
            return;
        }
        d.idle(std::time::Duration::from_millis(timing.poll_ms.max(10))).await;
    }
}

/// A discovery start's `detail` when the browser it took over was signed in
/// as its account already: nothing was signed in again.
pub fn discovery_signed_in_already(key: &str) -> String {
    format!("signed in already as {key}; went to the home page")
}

/// Switch a mapping run's save guard on, as a Must not save case's is
/// (`commands::autorun::guard_for_case`): the built-in save words and the
/// project's own. Nothing to do for an ordinary discovery.
pub(crate) async fn guard_a_mapping_run<D: crate::browser::cdp::Driver>(
    d: &mut D,
    is_mapping: bool,
    root: &std::path::Path,
    organization: &str,
    project: &str,
) -> Result<(), String> {
    if !is_mapping {
        return Ok(());
    }
    let nav = crate::autorun::nav::load_nav(root, organization, project)
        .map_err(|why| crate::browser::save_guard::setup_failed(&why))?;
    d.guard_saves(&nav.save_words).await.map_err(|e| crate::browser::save_guard::setup_failed(&e.to_string()))
}

/// Put a discovery's save guard on when it must not save
/// (`discovery_must_not_save`) and the guard is not on already, as a Must
/// not save case's is: the built-in save words and the project's own.
/// Asked before every action, so a guard something lifted between two
/// actions is back on before the next. `Err` is the guard's own sentence:
/// such a discovery never acts unguarded.
pub(crate) async fn guard_a_no_save_discovery<D: crate::browser::cdp::Driver>(
    d: &mut D,
    must_not_save: bool,
    root: &std::path::Path,
    organization: &str,
    project: &str,
) -> Result<(), String> {
    if !must_not_save || d.is_guarding_saves() {
        return Ok(());
    }
    guard_a_mapping_run(d, true, root, organization, project).await
}

/// The saves the guard stopped for a discovery that must not save since
/// this was last asked, each as `METHOD /path` (never a host, a query or a
/// value an action in `ran` typed). The stop's pending sentence goes with
/// them: a save stopped between two actions is reported here, and never
/// fails the next action.
pub(crate) fn take_no_save_blocks<D: crate::browser::cdp::Driver>(d: &mut D, ran: &[&crate::browser::actions::Action]) -> Vec<String> {
    let _ = crate::browser::cdp::Driver::take_save_blocked(d);
    crate::browser::cdp::Driver::take_saves_stopped(d)
        .into_iter()
        .map(|(method, path)| {
            let path = ran.iter().fold(crate::autorun::discovery_map::path_only(&path), |p, a| without_typed(&p, a));
            format!("{method} {path}")
        })
        .collect()
}

/// The saves the guard stopped since the last time this was asked, for a
/// mapping run: added to the run's count and logged by method and path
/// only, with nothing an action in `ran` typed. How many, or `None` for an
/// ordinary discovery.
pub(crate) fn count_blocked_writes<D: crate::browser::cdp::Driver>(
    d: &mut D,
    discovery: Option<&mut crate::commands::autorun::DiscoveryState>,
    ran: &[&crate::browser::actions::Action],
) -> Option<usize> {
    let run = discovery.and_then(|s| s.mapping.as_mut())?;
    let stopped = crate::browser::cdp::Driver::take_saves_stopped(d);
    for (method, path) in &stopped {
        let path = ran.iter().fold(crate::autorun::discovery_map::path_only(path), |p, a| without_typed(&p, a));
        crate::applog::warn(format!("Auto Run mapping run blocked a write: {method} {path}"));
    }
    run.blocked_writes = run.blocked_writes.saturating_add(u32::try_from(stopped.len()).unwrap_or(u32::MAX));
    Some(stopped.len())
}

/// What one action a discovery ran did: its outcome, the dialog it raised,
/// the writes the page sent and the path it ended on.
pub(crate) struct Discovered {
    pub(crate) outcome: crate::browser::actions::ActionOutcome,
    pub(crate) dialogs: Vec<String>,
    pub(crate) writes: Vec<serde_json::Value>,
    /// Saves the guard stopped, each `METHOD /path`: not writes.
    pub(crate) blocked: Vec<String>,
    pub(crate) after: String,
}

/// What the page answered each action of one discovery call - one action,
/// a component's actions, or a batch - kept across them. One `Noted`
/// carries the call's last action that changed the page on to the next, as
/// a run's steps carry it: a message or a refusal that came after an
/// action's own reads is still put on it, and a later failure names it.
/// Each action runs as a step of its own, so the steps `noted` numbers are
/// mapped back to the call's actions.
///
/// It also keeps, for each of those steps, what the application's rules
/// are read from (`discovery_map::rules_from`): the action, the page its
/// read after it was on, and what the page answered it. The call keeps them once,
/// when it is over (`remember`).
#[derive(Default)]
pub(crate) struct Heard {
    pub(crate) noted: crate::autorun::runner::Noted,
    /// The call's action each step `noted` numbered belongs to.
    pub(crate) owners: Vec<usize>,
    /// Everything the page answered each of the call's actions so far.
    pub(crate) answers: Vec<Answered>,
    /// What the page answered each step `noted` numbered, by that number.
    pub(crate) spots: Vec<crate::autorun::discovery_map::ActionAnswer>,
    /// The discovery's area and case, as the call found them.
    pub(crate) area: Option<String>,
    pub(crate) case_id: Option<i32>,
    /// The discovery is an explore session: nothing it learns is filed.
    pub(crate) explore: bool,
}

/// What the page answered one action of a discovery call.
#[derive(Default, Clone)]
pub(crate) struct Answered {
    pub(crate) said: Vec<crate::browser::page_messages::PageMessage>,
    pub(crate) refused: Vec<crate::autorun::refusals::RefusedRequest>,
}

impl Heard {
    /// The call's action `at` is about to run one step.
    pub(crate) fn begin(&mut self, at: usize) {
        self.noted.step = self.owners.len();
        self.owners.push(at);
        self.spots.push(Default::default());
    }

    pub(crate) fn add(
        &mut self,
        at: usize,
        said: Vec<crate::browser::page_messages::PageMessage>,
        refused: Vec<crate::autorun::refusals::RefusedRequest>,
    ) {
        if self.answers.len() <= at {
            self.answers.resize_with(at + 1, Answered::default);
        }
        let a = &mut self.answers[at];
        a.said.extend(said);
        a.said.truncate(crate::browser::page_messages::MAX_MESSAGES);
        a.refused.extend(refused);
    }

    /// What the step action `at` just ran answered it, and what came late
    /// for the call's earlier actions. `one` is the action the step ran (a
    /// component's own).
    pub(crate) fn ran(
        &mut self,
        at: usize,
        outcome: &crate::browser::actions::ActionOutcome,
        one: &crate::browser::actions::Action,
    ) {
        self.add(at, outcome.page_said.clone(), outcome.refused.clone());
        if let Some(spot) = self.spots.get_mut(self.noted.step) {
            spot.page = outcome.page_path.clone().unwrap_or_default();
            spot.step = outcome.page_step.clone();
            spot.after = describe_action(one);
            spot.said.extend(outcome.page_said.iter().cloned());
            spot.refused.extend(outcome.refused.iter().cloned());
            spot.changed = outcome.page_path.is_some();
            spot.failed = !outcome.ok && !crate::autorun::runner::tcm_made(outcome);
        }
        self.take_late();
    }

    pub(crate) fn take_late(&mut self) {
        for late in self.noted.take_late() {
            if let Some(spot) = self.spots.get_mut(late.step) {
                spot.said.extend(late.page_said.iter().cloned());
                spot.refused.extend(late.refused.iter().cloned());
            }
            if let Some(&at) = self.owners.get(late.step) {
                self.add(at, late.page_said, late.refused);
            }
        }
    }

    /// The call is over: what it taught of the application's rules is kept
    /// under the discovery's area, in one write - a map that could not be
    /// written fails nothing.
    pub(crate) fn remember(&self, root: &std::path::Path, organization: &str, project: &str) {
        use crate::autorun::discovery_map::{remember_refusals_logged, rules_from, RuleSource};
        // An explore session keeps nothing in the discovery map.
        if self.explore {
            return;
        }
        let rules = rules_from(&self.spots, RuleSource::Discovery, self.case_id, crate::autorun::sessions::now_ms());
        remember_refusals_logged(root, organization, project, self.area.as_deref(), rules);
    }

    /// The call's last action has no next one to read the page for it: the
    /// messages are read once more, before the call's closing page read.
    pub(crate) async fn look_again<D: crate::browser::cdp::Driver>(&mut self, d: &mut D) {
        self.noted.look_again(d).await;
        self.take_late();
    }

    /// Before a batch's next action: the requests the application refused
    /// the last action that changed the page since its own reads.
    pub(crate) async fn look_for_refusals<D: crate::browser::cdp::Driver>(&mut self, d: &mut D) {
        self.noted.look_for_refusals(d).await;
        self.take_late();
    }

    pub(crate) fn of(&self, at: usize) -> Answered {
        self.answers.get(at).cloned().unwrap_or_default()
    }

    /// Put what the page answered action `at` on its JSON answer:
    /// `page_said` and `app_refused`, each only when there is one.
    pub(crate) fn put_on(&self, at: usize, v: &mut serde_json::Value) {
        let Some(o) = v.as_object_mut() else {
            return;
        };
        let a = self.of(at);
        o.remove("page_said");
        o.remove("app_refused");
        if !a.said.is_empty() {
            o.insert("page_said".into(), serde_json::json!(a.said));
        }
        if !a.refused.is_empty() {
            o.insert("app_refused".into(), serde_json::json!(a.refused));
        }
    }

    /// The call is over: the mark its last action kept is let go.
    pub(crate) fn forget<B: DiscoveryBrowser>(&mut self, slot: &mut Option<B>) {
        if let Some(b) = slot.as_mut() {
            self.noted.forget(&*b.parts().driver);
        }
    }
}

/// One action in the discovery's browser, run as `/autorun-try` runs one
/// (`try_action`, case 0), with what it set off: `{ok, detail, path,
/// dialogs, writes, page}`. Every write request the page sent (`SAVE_METHODS`)
/// is logged in the area's map by method and path - never a query string -
/// the page it ended on is filed, and a line saying what the action led to
/// goes into the area's outcomes. Neither ever holds a value a `fill` typed.
/// A non-blank `area` moves the discovery to that area first.
///
/// A `use_component` is expanded before the browser is touched: from
/// `draft` when one is given, so a component can be tried before it is
/// saved, else from the saved one. Each of its actions then runs as a
/// single action does, and the try stops at the first that fails, as a
/// script step does. Its answer adds `steps: [{action, ok, detail}]`, with
/// `ok` and `detail` the try's as a whole, and a try that worked is kept
/// on the discovery by `components::draft_fingerprint`.
///
/// In a mapping run the answer adds `blocked`: how many saves the guard
/// stopped since the last action (`count_blocked_writes`).
///
/// What the page newly showed after it is `page_said`, and the requests the
/// application refused it `app_refused`, each only when there is one: the
/// messages are read once more before the closing page read, as nothing
/// comes after this action to read them for it.
pub async fn discover_action_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    action: &crate::browser::actions::Action,
    draft: Option<&crate::autorun::components::Component>,
    area: Option<&str>,
) -> (u16, String) {
    let mut heard = Heard::default();
    let was_guarding = guarding_saves(slot);
    let stood = sign_in_stands_now(slot, root, organization, project).await;
    let answer = crate::autorun::action_help::CALL_STARTED
        .scope(
            std::time::Instant::now(),
            discover_action_heard(slot, root, organization, project, action, draft, area, &mut heard, 0, true),
        )
        .await;
    heard.remember(root, organization, project);
    heard.forget(slot);
    end_call_guard(slot, root, was_guarding).await;
    retake_print_after_call(slot, root, organization, project, stood, std::slice::from_ref(action)).await;
    answer
}

/// Whether the browser in `slot` is signed in as its session's account and
/// still holds that sign-in's cookies as they were printed
/// (`replay_to::sign_in_stands`): looked at before a discovery call.
pub(crate) async fn sign_in_stands_now<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
) -> bool {
    let Some(b) = slot.as_mut() else { return false };
    let p = b.parts();
    let Some(key) = p.signed_in.clone() else { return false };
    crate::autorun::replay_to::sign_in_stands(p.driver, root, organization, project, &key, &*p.signed_in, &*p.lease)
        .await
        .is_ok()
}

/// After a discovery call that began with the sign-in standing (`stood`) and
/// did not sign in itself: when the page still shows it is signed in, the
/// sign-in's cookies are printed again. The app's own pages re-issue the
/// session cookie as they load (`replay_to::retake_sign_in`); without this a
/// long discovery leaves a stale print, and a later reset or replay takes the
/// same sign-in for a changed one and signs in again.
pub(crate) async fn retake_print_after_call<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    stood: bool,
    actions: &[crate::browser::actions::Action],
) {
    if !stood {
        return;
    }
    let Some(b) = slot.as_mut() else { return };
    let p = b.parts();
    let Some(key) = p.signed_in.clone() else { return };
    let Ok(recipe) = crate::autorun::recipe::load_effective_recipe(root, organization, project) else { return };
    // The page cannot say who is signed in, so a call that could have swapped
    // the login is never taken as the same sign-in.
    if let Some(why) = crate::autorun::replay_to::touches_sign_in(actions, &recipe) {
        crate::applog::info(format!("Auto Run: a discovery call {why}, so the sign-in's cookies were not printed again"));
        return;
    }
    let timing = crate::browser::timing::Timing::supervised();
    let shows = crate::autorun::signin::already_signed_in_why(
        p.driver,
        &recipe,
        Some(&key),
        &key,
        &timing,
        crate::autorun::signin::SIGNED_IN_LOOK_MS,
    )
    .await;
    if shows.is_ok() {
        let _ = crate::autorun::replay_to::retake_sign_in(p.driver, &recipe.origins(), &mut *p.lease, "a discovery call").await;
    }
}

/// Whether the browser in `slot` guards saves now.
pub(crate) fn guarding_saves<B: DiscoveryBrowser>(slot: &mut Option<B>) -> bool {
    slot.as_mut().is_some_and(|b| crate::browser::cdp::Driver::is_guarding_saves(&*b.parts().driver))
}

/// A discovery call is over: its own `no_save` (`DiscoveryState::call_no_save`)
/// is let go, and the guard it put on is lifted (`commands::autorun::lift_guard_of`)
/// unless the discovery itself must not save (`discovery_must_not_save`) - a
/// mapping run, or one started with `no_save` or for a case marked so. A guard
/// already on before the call (`was_guarding`) was not the call's, and stays.
pub(crate) async fn end_call_guard<B: DiscoveryBrowser>(slot: &mut Option<B>, root: &std::path::Path, was_guarding: bool) {
    let Some(browser) = slot.as_mut() else {
        return;
    };
    let p = browser.parts();
    let Some(state) = p.discovery.as_mut() else {
        return;
    };
    let said = std::mem::take(&mut state.call_no_save);
    state.call_case_id = None;
    if !said || was_guarding || state.mapping.is_some() || discovery_must_not_save(root, state) {
        return;
    }
    if !matches!(p.guard_owner, None | Some(crate::commands::autorun::GuardOwner::Call { .. })) {
        return;
    }
    crate::commands::autorun::lift_guard_of(p.driver, p.guarded_case, p.guard_owner).await;
}

/// `discover_action_in` as action `at` of a discovery call that `heard`
/// keeps; `closing` when it is the call's last, which reads the page's
/// messages once more for it.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn discover_action_heard<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    action: &crate::browser::actions::Action,
    draft: Option<&crate::autorun::components::Component>,
    area: Option<&str>,
    heard: &mut Heard,
    at: usize,
    closing: bool,
) -> (u16, String) {
    let Some(browser) = slot.as_mut() else {
        return (409, NO_DISCOVERY.to_string());
    };
    let p = browser.parts();
    let Some(state) = p.discovery.as_mut() else {
        return (409, NO_DISCOVERY.to_string());
    };
    let component = match action {
        crate::browser::actions::Action::UseComponent { component, inputs } => {
            match component_to_try(root, organization, project, component, inputs, draft) {
                Ok(found) => Some(found),
                Err(refused) => return refused,
            }
        }
        _ => None,
    };
    if let Some(moved) = named(area) {
        stand_in_area(state, root, organization, project, &moved);
    }
    let area_name = state.area.clone();
    heard.area = area_name.clone();
    heard.case_id = state.case_id;
    let exploring = state.explore.is_some();
    heard.explore = exploring;
    let explore_session = state.explore_session().map(str::to_string);
    let setup_running = state.explore.as_ref().is_some_and(|e| e.setup_running);
    let mapping = state.mapping.is_some();
    let began_ms = crate::autorun::sessions::now_ms();
    // Once a discovery must not save, it stays so (`DiscoveryState::no_save`).
    // A call that said `no_save` itself is guarded too, for that call only
    // (`end_call_guard` lifts it): it never makes the discovery one.
    let discovery_guarded = discovery_must_not_save(root, state);
    state.no_save = discovery_guarded;
    let must_not_save = discovery_guarded || state.call_no_save;
    // A call's own guard is for the case that call named, not the
    // discovery's.
    let call_case_id = state.call_case_id;
    let d = p.driver;
    let runs: Vec<&crate::browser::actions::Action> = match &component {
        Some((_, actions)) => actions.iter().collect(),
        None => vec![action],
    };
    // Its guard is on before the action, or the action does not run; a save
    // it stopped between calls is reported with this action, never failing it.
    if let Err(why) = guard_a_no_save_discovery(d, must_not_save, root, organization, project).await {
        return (409, why);
    }
    if discovery_guarded {
        crate::commands::autorun::claim_guard(p.guard_owner, crate::commands::autorun::GuardOwner::Discovery);
    } else if must_not_save {
        crate::commands::autorun::claim_guard(p.guard_owner, crate::commands::autorun::GuardOwner::Call { case_id: call_case_id });
    }
    let reports_blocks = must_not_save && !mapping;
    let mut blocked_writes = if reports_blocks { take_no_save_blocks(d, &runs) } else { Vec::new() };
    let (mut steps, mut writes, mut dialogs) = (Vec::new(), Vec::new(), Vec::new());
    let mut last = None;
    let mut after = String::new();
    for (i, one) in runs.iter().copied().enumerate() {
        heard.begin(at);
        let ran = discover_one(
            d,
            p.signed_in,
            p.lease,
            root,
            organization,
            project,
            area_name.as_deref(),
            mapping,
            exploring,
            one,
            &mut heard.noted,
        )
        .await;
        let mut ran = match ran {
            Ok(ran) => ran,
            Err(refused) => return refused,
        };
        heard.ran(at, &ran.outcome, one);
        if !ran.outcome.ok {
            ran.outcome.detail = crate::browser::input::with_offscreen_hint(&ran.outcome.detail);
        }
        // A component that fails on its first action says where the
        // browser is: most often it stands on another screen.
        if component.is_some() && i == 0 && !ran.outcome.ok {
            ran.outcome.detail = with_page_where(&ran.outcome.detail, &ran.after);
        }
        steps.push(serde_json::json!({
            "action": describe_action(one),
            "ok": ran.outcome.ok,
            "detail": ran.outcome.detail,
        }));
        writes.extend(ran.writes);
        if reports_blocks {
            blocked_writes.extend(ran.blocked);
        }
        dialogs.extend(ran.dialogs);
        after = ran.after;
        let failed = !ran.outcome.ok;
        last = Some(ran.outcome);
        if failed {
            break;
        }
    }
    let Some(outcome) = last else {
        return (500, "the action produced no outcome".to_string());
    };
    if let (Some((c, _)), true) = (&component, outcome.ok) {
        let print = crate::autorun::components::draft_fingerprint(c);
        if let Some(state) = p.discovery.as_mut() {
            if !state.tried.contains(&print) {
                state.tried.push(print);
            }
        }
    }
    if closing {
        heard.look_again(d).await;
    }
    let sighting = discovery_sighting(root, organization, project, p.discovery.as_ref(), None, p.signed_in.as_deref());
    let (status, page) = read_page(d, crate::browser::snapshot::PAGE_LINES_DEFAULT, sighting.as_ref()).await;
    let mut blocked = count_blocked_writes(d, p.discovery.as_mut(), &runs);
    if reports_blocks {
        blocked_writes.extend(take_no_save_blocks(d, &runs));
        // A save seen both ways (in the request record and as a stop) is
        // said once, in the order it was first seen.
        let mut seen = std::collections::HashSet::new();
        blocked_writes.retain(|w| seen.insert(w.clone()));
        blocked = Some(blocked_writes.len());
    }
    let mut answer = serde_json::json!({
        "ok": outcome.ok,
        "detail": outcome.detail,
        "path": after,
        "dialogs": dialogs,
        "writes": writes,
        "page": if status == 200 { page.as_str() } else { "" },
    });
    heard.put_on(at, &mut answer);
    if let Some(n) = blocked {
        answer["blocked"] = serde_json::json!(n);
    }
    if reports_blocks {
        answer["no_save"] = serde_json::json!(true);
        answer["blocked_writes"] = serde_json::json!(blocked_writes);
        let posted: Vec<(String, String)> = writes
            .iter()
            .filter_map(|w| Some((w["method"].as_str()?.to_string(), w["path"].as_str()?.to_string())))
            .collect();
        if let Some(line) = crate::browser::save_guard::let_through_line(&posted) {
            answer["let_through"] = serde_json::json!(line);
        }
    }
    if component.is_some() {
        answer["steps"] = serde_json::json!(steps);
    }
    if status != 200 {
        answer["page_unavailable"] = serde_json::json!(page);
    }
    if let Some(shot) = &outcome.screenshot {
        answer["picture"] = serde_json::json!(crate::autorun::store::shot_path(root, shot).display().to_string());
    }
    // An explore session keeps an action that worked for the assistant that
    // ran it (`autorun::kept`), unless a setup is driving it, a save it sent
    // was stopped, or it typed a login. A keep never fails the action.
    // (A picture changes nothing, so there is nothing to keep of it.)
    let kept_kind = crate::autorun::action_help::screenshot_ask(action).is_none();
    if let (Some(session), true) =
        (explore_session.as_deref(), outcome.ok && kept_kind && !setup_running && !mapping && blocked.unwrap_or(0) == 0)
    {
        let steps: Option<Vec<crate::browser::actions::Action>> = component.as_ref().map(|(_, actions)| actions.clone());
        let credential = match crate::autorun::accounts::load_accounts(root) {
            Ok(accounts) => crate::autorun::kept::types_a_credential(action, steps.as_deref(), &accounts),
            Err(_) => true,
        };
        if !credential {
            crate::autorun::kept::keep(
                root,
                session,
                action,
                steps,
                area_name.as_deref(),
                began_ms,
                crate::autorun::sessions::now_ms(),
            );
        }
    }
    // An explore session keeps the saves the call reported, by method and
    // path, so its end can name the ones no change names. A change list
    // that will not take them is logged and never fails the action.
    if let Some(session) = explore_session.as_deref() {
        let sent: Vec<(String, String)> = answer["writes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|w| {
                let path = w["path"].as_str()?;
                let path = match w["handler"].as_str() {
                    Some(handler) => format!("{path}?handler={handler}"),
                    None => path.to_string(),
                };
                Some((w["method"].as_str()?.to_string(), path))
            })
            .collect();
        if let Err(why) = crate::autorun::explore::note_writes(root, session, &sent) {
            crate::applog::warn(format!("Auto Run explore session: its saves could not be noted: {why}"));
        }
    }
    (200, answer.to_string())
}

/// The batch cap as a literal, so `MAX_BATCH` and the sentences that name
/// it are built from the one number.
macro_rules! max_batch {
    () => {
        20
    };
}

/// The most actions `discover_autorun_actions` runs in one call.
pub const MAX_BATCH: usize = max_batch!();

/// Said to a batch of more than `MAX_BATCH` actions.
pub const BATCH_TOO_LONG: &str = concat!("at most ", max_batch!(), " actions in one call");

/// Said to a batch with no actions in it.
pub const BATCH_EMPTY: &str = "send at least one action in \"actions\"";

/// A batch's size, refused when it holds nothing or more than `MAX_BATCH`.
pub fn refuse_batch_size(n: usize) -> Result<(), (u16, String)> {
    match n {
        0 => Err((400, BATCH_EMPTY.to_string())),
        n if n > MAX_BATCH => Err((400, BATCH_TOO_LONG.to_string())),
        _ => Ok(()),
    }
}

/// The draft a batch's action is tried with: `draft` for a `use_component`
/// that names it, else none (a saved component, or not a component).
pub(crate) fn draft_for<'a>(
    action: &crate::browser::actions::Action,
    draft: Option<&'a crate::autorun::components::Component>,
) -> Option<&'a crate::autorun::components::Component> {
    let key = crate::autorun::nav::module_key;
    match (action, draft) {
        (crate::browser::actions::Action::UseComponent { component, .. }, Some(c)) if key(&c.name) == key(component) => {
            Some(c)
        }
        _ => None,
    }
}

/// One line of a batch's answer for an action that ran, from its single
/// answer (`discover_action_in`'s JSON): ok or failed with the detail, the
/// dialogs it raised, the writes it set off (method and path only), the
/// page path when it changed, whether its own page read failed, where its
/// picture is, and what the page answered it (`page_said`, `app_refused`).
/// A failed action's detail already names its refusal, so only what the
/// page showed it and the detail does not say is added.
pub(crate) fn batch_line(n: usize, action: &crate::browser::actions::Action, v: &serde_json::Value, last_path: &mut Option<String>) -> String {
    let ok = v["ok"].as_bool().unwrap_or(false);
    let detail = v["detail"].as_str().map(str::trim).filter(|d| !d.is_empty());
    let mut said = batch_detail(&detail.map(str::to_string).unwrap_or_else(|| describe_action(action)));
    for dialog in v["dialogs"].as_array().into_iter().flatten().filter_map(|d| d.as_str()) {
        said.push_str(&format!("; dialog {}", one_short_line(dialog)));
    }
    for write in v["writes"].as_array().into_iter().flatten() {
        let (method, path) = (write["method"].as_str().unwrap_or(""), write["path"].as_str().unwrap_or(""));
        said.push_str(&format!("; wrote {method} {path}"));
    }
    for stopped in v["blocked_writes"].as_array().into_iter().flatten().filter_map(|w| w.as_str()) {
        said.push_str(&format!("; blocked {stopped}"));
    }
    if let Some(path) = v["path"].as_str().filter(|p| !p.trim().is_empty()) {
        if last_path.as_deref() != Some(path) {
            said.push_str(&format!("; page {path}"));
            *last_path = Some(path.to_string());
        }
    }
    if v.get("page_unavailable").is_some() {
        said.push_str("; page unreadable");
    }
    if let Some(picture) = v["picture"].as_str() {
        said.push_str(&format!("; picture {picture}"));
    }
    if ok {
        let refused: Vec<crate::autorun::refusals::RefusedRequest> =
            serde_json::from_value(v["app_refused"].clone()).unwrap_or_default();
        if let Some(first) = refused.first() {
            said.push_str(&format!("; {}", crate::autorun::refusals::app_refused_line(first)));
        }
    }
    let shown: Vec<crate::browser::page_messages::PageMessage> =
        serde_json::from_value(v["page_said"].clone()).unwrap_or_default();
    if !shown.is_empty() {
        let note = crate::autorun::runner::page_showed_note(&shown);
        if ok || !detail.is_some_and(|d| d.contains(&note)) {
            said.push_str(&format!("; {note}"));
        }
    }
    format!("{n}. {}: {said}", if ok { "ok" } else { "failed" })
}

/// An action's detail on its batch line: one line of at most 200
/// characters, unless it carries what the page or the application answered
/// - the reason the assistant needs whole - which is kept on one line, as
/// long as the runner made it.
pub(crate) fn batch_detail(detail: &str) -> String {
    use crate::autorun::{refusals::APP_REFUSED, runner::PAGE_SHOWED};
    if detail.contains(APP_REFUSED) || detail.contains(PAGE_SHOWED) {
        detail.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        one_short_line(detail)
    }
}

/// Whether a failed action's detail already names what the page answered
/// an earlier action (`runner::AFTER_EARLIER`).
pub(crate) fn names_an_earlier_answer(detail: &str) -> bool {
    use crate::autorun::{refusals::APP_REFUSED, runner::{AFTER_EARLIER, PAGE_SHOWED}};
    detail.find(AFTER_EARLIER).is_some_and(|i| detail[i..].contains(PAGE_SHOWED) || detail[i..].contains(APP_REFUSED))
}

/// Said after a batch stopped at action `failed` (from 0): the last action
/// before it that the page answered with a message, as the likelier cause.
/// Nothing when the failed action has a refusal of its own, or its detail
/// already names an earlier action's answer.
pub(crate) fn points_back(actions: &[crate::browser::actions::Action], heard: &Heard, failed: usize, detail: &str) -> Option<String> {
    if !heard.of(failed).refused.is_empty() || names_an_earlier_answer(detail) {
        return None;
    }
    let (k, said) = (0..failed).rev().map(|k| (k, heard.of(k).said)).find(|(_, said)| !said.is_empty())?;
    let what = actions.get(k).map(describe_action).unwrap_or_default();
    Some(format!("After action {} ({what}) {}.", k + 1, crate::autorun::runner::page_showed_note(&said)))
}

/// The first action of a batch that worked and was refused once it had
/// ended (from 0): a refusal found within its own reads would have failed it.
pub(crate) fn refused_once_ended(rows: &[Row], heard: &Heard) -> Option<usize> {
    rows.iter().find_map(|row| match row {
        Row::Ran(k, v) if v["ok"].as_bool() == Some(true) && !heard.of(*k).refused.is_empty() => Some(*k),
        _ => None,
    })
}

/// One line of a batch's answer, before the batch is done: said as it is,
/// or the action (from 0) that ran with its single answer, put into words
/// once everything the page answered it is known.
pub(crate) enum Row {
    Said(String),
    Ran(usize, serde_json::Value),
}

/// A batch's rows as its answer's lines (`batch_line`), each action with
/// what the page answered it (`Heard::put_on`).
pub(crate) fn batch_rows(actions: &[crate::browser::actions::Action], rows: &[Row], heard: &Heard) -> Vec<String> {
    let mut last_path = None;
    rows.iter()
        .map(|row| match row {
            Row::Said(line) => line.clone(),
            Row::Ran(i, v) => {
                let mut v = v.clone();
                heard.put_on(*i, &mut v);
                match actions.get(*i) {
                    Some(action) => batch_line(i + 1, action, &v, &mut last_path),
                    None => String::new(),
                }
            }
        })
        .collect()
}

/// Said when a batch stopped at a failure: how many actions were not run.
pub(crate) fn not_run_line(n: usize) -> String {
    let what = if n == 1 { "action was" } else { "actions were" };
    format!("Stopped at the failure: {n} {what} not run.")
}

/// Said when a batch gave way to End discovery, Close browser or a
/// release: how many actions were not run, and what may have asked. A
/// release that timed out still asked, so the line never claims the
/// discovery ended.
pub fn ended_line(n: usize) -> String {
    format!("Stopped: {n} not run - End discovery, Close browser, a release or a reset asked the batch to stop.")
}

/// Each browser slot's batch stop control: set by End discovery, Close
/// browser, both releases and the assistant's reset before they wait for the
/// browser, so a batch holding it gives way before its next action instead
/// of keeping them waiting. Cleared when a batch hears it, and before each
/// batch starts. Coordination only.
pub(crate) static BATCH_STOPS: [std::sync::atomic::AtomicBool; SLOT_COUNT] =
    [const { std::sync::atomic::AtomicBool::new(false) }; SLOT_COUNT];

/// Slot `n`'s batch stop control (`BATCH_STOPS`).
pub fn batch_stop(n: SlotNo) -> &'static std::sync::atomic::AtomicBool {
    &BATCH_STOPS[n.index()]
}

/// How many batches of discovery actions are going in each slot (waiting for
/// the browser or driving it). Batches in one slot can overlap (two callers
/// sharing it, or parallel calls of one proxy), so this counts. Kept so the
/// Assistants card can offer to stop them without taking the browser's lock.
/// Coordination only.
pub(crate) static BATCHES_GOING: [std::sync::atomic::AtomicUsize; SLOT_COUNT] =
    [const { std::sync::atomic::AtomicUsize::new(0) }; SLOT_COUNT];

/// Whether a batch of discovery actions is going in slot `n` (`BATCHES_GOING`).
pub fn batch_going(n: SlotNo) -> bool {
    BATCHES_GOING[n.index()].load(std::sync::atomic::Ordering::SeqCst) > 0
}

/// Counts slot `n`'s batch as going for as long as it lives; dropped at the
/// batch's end, however it ends.
pub struct BatchGoing(SlotNo);

impl BatchGoing {
    pub fn begin(n: SlotNo) -> BatchGoing {
        BATCHES_GOING[n.index()].fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        BatchGoing(n)
    }
}

impl Drop for BatchGoing {
    fn drop(&mut self) {
        BATCHES_GOING[self.0.index()].fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Ask the batch going in slot `n`, if any, to stop before its next action.
pub fn stop_batch_in(n: SlotNo) {
    batch_stop(n).store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Ask the batch going in the main slot, if any, to stop before its next
/// action.
pub fn stop_batch() {
    stop_batch_in(SlotNo::MAIN);
}

/// Ask the batch going in every slot, if any, to stop before its next action.
pub fn stop_all_batches() {
    for n in SlotNo::all() {
        stop_batch_in(n);
    }
}

/// A batch's answer when the browser failed under it: `lines` so far, and
/// when the browser has gone (`let_go_if_silent` lets it go right here)
/// `BROWSER_GONE` after them, with 409. A browser still there keeps the
/// failure's own status.
pub(crate) async fn batch_ended_by_the_browser<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    failure: (u16, String),
    mut lines: Vec<String>,
) -> (u16, String) {
    let (status, said) = let_go_if_silent(slot, failure).await;
    if said == BROWSER_GONE {
        lines.push(BROWSER_GONE.to_string());
        return (409, lines.join("\n"));
    }
    (status, lines.join("\n"))
}

/// Several actions in the discovery's browser, in order, each exactly as
/// `discover_action_in` runs one: its own blocking and counting in a
/// mapping run, its own page read and the sightings that read records, and
/// a `use_component` naming `draft` tried with it. `area`, when named,
/// moves the discovery there once, before the first action, so even a
/// first action refused before the browser is touched leaves the batch in
/// the named area. The caller holds the browser for the whole batch, so
/// nothing comes between its actions; `stop` (`batch_stop` from the route)
/// is checked before each action, and when it is set the batch answers what
/// ran and lets the browser go at once.
///
/// It stops at the first action that fails unless `stop_on_failure` is
/// false; a browser that fails stops it either way, and a browser that has
/// gone is let go here, keeping the lines of the actions that ran. The
/// answer is one line per action that ran (`N. ok: ...` or `N. failed:
/// ...`), a line for the actions not run, the saves a mapping run blocked
/// when there were any, and then the page once, as `get_autorun_page`
/// answers it after the last action (`page_read_in`). As a whole read, that
/// last read replaces the page's elements just as `get_autorun_page` after
/// a single action does: something only the last action's own read saw (a
/// message that has since gone) is dropped.
///
/// The batch's actions share one `Heard`: what the page showed only after
/// an action's own reads is found by the next action's first read (or, for
/// the last, by one more read before the page's) and said on that action's
/// line. When the batch stopped at a failure whose own line cannot name it,
/// the stop line points back to the last action the page answered with a
/// message (`points_back`).
#[allow(clippy::too_many_arguments)]
pub async fn discover_actions_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    actions: &[crate::browser::actions::Action],
    draft: Option<&crate::autorun::components::Component>,
    area: Option<&str>,
    stop_on_failure: bool,
    stop: &std::sync::atomic::AtomicBool,
) -> (u16, String) {
    discover_actions_paged(slot, root, organization, project, actions, draft, area, stop_on_failure, stop, true).await
}

/// [`discover_actions_in`], with the page after the last action left out
/// of the answer when `page` is false (`"page": false`): it is still read,
/// and what it shows recorded as seen, and a last line says where to read
/// it (`PAGE_LEFT_OUT`).
#[allow(clippy::too_many_arguments)]
pub async fn discover_actions_paged<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    actions: &[crate::browser::actions::Action],
    draft: Option<&crate::autorun::components::Component>,
    area: Option<&str>,
    stop_on_failure: bool,
    stop: &std::sync::atomic::AtomicBool,
    page: bool,
) -> (u16, String) {
    let mut heard = Heard::default();
    let was_guarding = guarding_saves(slot);
    let stood = sign_in_stands_now(slot, root, organization, project).await;
    let answer = crate::autorun::action_help::CALL_STARTED
        .scope(
            std::time::Instant::now(),
            discover_actions_heard(
                slot,
                root,
                organization,
                project,
                actions,
                draft,
                area,
                stop_on_failure,
                stop,
                page,
                &mut heard,
            ),
        )
        .await;
    heard.remember(root, organization, project);
    heard.forget(slot);
    end_call_guard(slot, root, was_guarding).await;
    retake_print_after_call(slot, root, organization, project, stood, actions).await;
    answer
}

/// `discover_actions_in`, with what the page answered its actions kept in
/// `heard`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn discover_actions_heard<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    actions: &[crate::browser::actions::Action],
    draft: Option<&crate::autorun::components::Component>,
    area: Option<&str>,
    stop_on_failure: bool,
    stop: &std::sync::atomic::AtomicBool,
    page_wanted: bool,
    heard: &mut Heard,
) -> (u16, String) {
    if let Err(refused) = refuse_batch_size(actions.len()) {
        return refused;
    }
    let Some(state) = slot.as_mut().and_then(|b| b.parts().discovery.as_mut()) else {
        return (409, NO_DISCOVERY.to_string());
    };
    if let Some(moved) = named(area) {
        stand_in_area(state, root, organization, project, &moved);
    }
    let blocked_by = if state.mapping.is_some() { "the mapping run" } else { "the no-save guard" };
    let mut rows = Vec::new();
    let mut blocked = 0usize;
    let mut ran = 0;
    // The action the batch stopped at, and its detail.
    let mut stopped_at = None;
    // An action that worked, refused once it had ended, and how many were
    // then not run.
    let mut refused_after = None;
    for (i, action) in actions.iter().enumerate() {
        if stop.swap(false, std::sync::atomic::Ordering::SeqCst) {
            let mut lines = batch_rows(actions, &rows, heard);
            lines.push(ended_line(actions.len() - i));
            return (409, lines.join("\n"));
        }
        // A refused request stops the batch even when it was answered after
        // the action that sent it had ended (past the answer wait, or sent
        // after its own reads): the next action does not run.
        if stop_on_failure && i > 0 {
            if let Some(b) = slot.as_mut() {
                heard.look_for_refusals(b.parts().driver).await;
            }
            if let Some(k) = refused_once_ended(&rows, heard) {
                refused_after = Some((k, actions.len() - i));
                break;
            }
        }
        // The call has a time of its own (`CALL_BUDGET_MS`); when it has run
        // out, nothing more is started and the page is not read.
        if crate::autorun::action_help::action_budget(1, crate::autorun::action_help::call_elapsed_ms()).is_none() {
            let secs = crate::autorun::action_help::CALL_BUDGET_MS / 1000;
            rows.push(Row::Said(format!("{}. failed: {}", i + 1, crate::autorun::action_help::no_answer(i + 1, secs))));
            let mut lines = batch_rows(actions, &rows, heard);
            if i + 1 < actions.len() {
                lines.push(not_run_line(actions.len() - i - 1));
            }
            return (504, lines.join("
"));
        }
        let (status, body) =
            discover_action_heard(slot, root, organization, project, action, draft_for(action, draft), None, heard, i, false)
                .await;
        ran = i + 1;
        if status == 200 && body.contains(crate::autorun::action_help::DID_NOT_ANSWER) {
            // An action that did not answer: the browser may not answer the
            // next either, so the batch stops here and does not read the page.
            let v: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
            if v["detail"].as_str().is_some_and(|d| d.starts_with(crate::autorun::action_help::DID_NOT_ANSWER)) {
                rows.push(Row::Ran(i, v));
                let mut lines = batch_rows(actions, &rows, heard);
                if ran < actions.len() {
                    lines.push(not_run_line(actions.len() - ran));
                }
                return (504, lines.join("
"));
            }
        }
        if status != 200 {
            // The browser failed or the discovery is gone: nothing after
            // it can run.
            let stopped = said_browser_failed(&(status, body.clone())) || body == NO_DISCOVERY;
            rows.push(Row::Said(format!("{}. failed: {}", i + 1, one_short_line(&body))));
            if stopped {
                let mut lines = batch_rows(actions, &rows, heard);
                if ran < actions.len() {
                    lines.push(not_run_line(actions.len() - ran));
                }
                return batch_ended_by_the_browser(slot, (status, body), lines).await;
            }
            if stop_on_failure {
                break;
            }
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
        blocked += v["blocked"].as_u64().map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX));
        let failed = !v["ok"].as_bool().unwrap_or(false);
        let detail = v["detail"].as_str().unwrap_or("").to_string();
        rows.push(Row::Ran(i, v));
        if stop_on_failure && failed {
            stopped_at = Some((i, detail));
            break;
        }
    }
    // The last action has no next one to read the page's messages for it.
    if let Some(b) = slot.as_mut() {
        heard.look_again(b.parts().driver).await;
    }
    let mut lines = batch_rows(actions, &rows, heard);
    let back = stopped_at.and_then(|(i, detail)| points_back(actions, heard, i, &detail));
    match (refused_after, ran < actions.len(), back) {
        (Some((k, n)), _, _) => {
            let what = actions.get(k).map(describe_action).unwrap_or_default();
            lines.push(format!("Stopped: the app refused action {} ({what}) after it ended: {n} not run.", k + 1));
        }
        (None, true, Some(back)) => lines.push(format!("{} {back}", not_run_line(actions.len() - ran))),
        (None, true, None) => lines.push(not_run_line(actions.len() - ran)),
        (None, false, Some(back)) => lines.push(back),
        (None, false, None) => {}
    }
    if blocked > 0 {
        lines.push(format!("Saves blocked by {blocked_by}: {blocked}."));
    }
    let Some(browser) = slot.as_mut() else {
        return (409, NO_DISCOVERY.to_string());
    };
    let filing = if project.trim().is_empty() { None } else { Some(root) };
    let (status, page) =
        page_read_in(browser, filing, organization, project, None, crate::browser::snapshot::PAGE_LINES_DEFAULT).await;
    if status != 200 {
        lines.push(format!("The page could not be read: {}", one_short_line(&page)));
        return batch_ended_by_the_browser(slot, (status, page), lines).await;
    }
    if !page_wanted {
        return (200, format!("{}\n\nPage: {PAGE_LEFT_OUT}", lines.join("\n")));
    }
    (200, format!("{}\n\n{page}", lines.join("\n")))
}

/// Said to a discovery's `use_component` naming a component the project
/// has not saved, sent without a `draft`: how a new one is tried.
pub fn not_saved_try_draft(name: &str) -> String {
    format!("\"{}\" is not saved in this project - to try a new one, send it as draft", name.trim())
}

/// A component's first action that failed in a discovery, with where the
/// browser is: the page's path only, never its host or query. A failure
/// there is often the browser on another screen than the component starts
/// on, not a wrong locator.
pub fn with_page_where(detail: &str, path: &str) -> String {
    // Unknown (the address could not be read): no hint at all, rather
    // than the "/" `path_only` makes of nothing.
    if path.trim().is_empty() {
        return detail.to_string();
    }
    let path = crate::autorun::discovery_map::path_only(path);
    format!("{detail} (the page is {path})")
}

/// The component a discovery's `use_component` tries, and its actions with
/// the inputs put in: `draft` when given (it must be the component the
/// action names), else the saved one. Each expanded action is refused as a
/// tried action is, and one that would go to a site outside the sign-in
/// recipe's is refused too, all before the browser is touched.
pub(crate) fn component_to_try(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    name: &str,
    inputs: &serde_json::Map<String, serde_json::Value>,
    draft: Option<&crate::autorun::components::Component>,
) -> Result<(crate::autorun::components::Component, Vec<crate::browser::actions::Action>), (u16, String)> {
    use crate::autorun::components;
    let key = crate::autorun::nav::module_key;
    let c = match draft {
        Some(c) if key(&c.name) == key(name) => c.clone(),
        Some(c) => {
            return Err((400, format!("the draft is {}, but the action uses {}", c.name.trim(), name.trim())));
        }
        None => {
            let file = components::load_components(root, organization, project).map_err(|why| (409, why))?;
            components::find(&file, name).cloned().ok_or_else(|| (400, not_saved_try_draft(name)))?
        }
    };
    let actions = components::expand(&c, inputs).map_err(|why| (400, why))?;
    if actions.is_empty() {
        return Err((400, format!("{} has no actions to try", c.name.trim())));
    }
    for a in &actions {
        refuse_as_a_tried_action(a)?;
    }
    if actions.iter().any(|a| !tried_addresses(a).is_empty()) {
        let recipe =
            crate::autorun::recipe::load_effective_recipe(root, organization, project).map_err(|why| (409, why))?;
        let policy = crate::autorun::runner::policy_for(Some(&recipe));
        for a in &actions {
            refuse_outside_the_recipe(&policy, a)?;
        }
    }
    Ok((c, actions))
}

/// One action of a discovery, with the writes it set off logged and the
/// line saying what it led to filed (`discover_action_in`). In a mapping
/// run (`mapping`) a save the guard stopped never fails the action: it is
/// counted (`count_blocked_writes`), and the action keeps its own outcome.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn discover_one<D: crate::browser::cdp::Driver>(
    d: &mut D,
    signed_in: &mut Option<String>,
    lease: &mut crate::autorun::lease::Held,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    area_name: Option<&str>,
    mapping: bool,
    exploring: bool,
    action: &crate::browser::actions::Action,
    noted: &mut crate::autorun::runner::Noted,
) -> Result<Discovered, (u16, String)> {
    // An explore session files no write, outcome or locator in the map.
    let recording = !project.trim().is_empty() && !exploring;
    let bucket = area_name.unwrap_or("");
    let before = current_page(d).await.0;
    let notes_before = status_texts(d).await;
    let mark = crate::browser::cdp::Driver::net_mark(d);
    let tried = try_action(
        d,
        signed_in,
        lease,
        root,
        organization,
        project,
        0,
        area_name,
        mapping,
        !exploring,
        action,
        Some(noted),
    )
    .await;
    // The writes are read once and the mark let go, however the action
    // went: on the whole browser a mark kept pushes out the ones still in
    // use (`MAX_NET_MARKS`).
    let sent = crate::browser::cdp::Driver::net_since(d, mark);
    crate::browser::cdp::Driver::net_unmark(d, mark);
    let tried = tried?;
    let what = describe_action(action);
    let now = crate::autorun::sessions::now_ms();
    let mut writes = Vec::new();
    let mut blocked = Vec::new();
    for sent in sent {
        if !crate::autorun::explore::counts_as_save(root, &sent.method, &sent.path_query) {
            continue;
        }
        let path = without_typed(&crate::autorun::discovery_map::path_only(&sent.path_query), action);
        // A save the guard failed in the browser never reached the server:
        // it is blocked, not written, and the map does not log it.
        if crate::browser::save_guard::stopped_by_guard(&sent) {
            blocked.push(format!("{} {path}", sent.method));
            continue;
        }
        if recording {
            let entry = crate::autorun::discovery_map::WriteEntry {
                method: sent.method.clone(),
                path: path.clone(),
                at: now,
                step: what.clone(),
            };
            if let Err(why) = crate::autorun::discovery_map::record_write(root, organization, project, bucket, entry) {
                unrecorded(&why);
            }
        }
        let mut write = serde_json::json!({ "method": sent.method, "path": path });
        // Which save a page's address is: the one part of the query worth
        // keeping (`explore::handler_of`).
        if let Some(handler) = crate::autorun::explore::handler_of(&sent.path_query) {
            write["handler"] = serde_json::json!(handler);
        }
        writes.push(write);
    }
    let after = current_page(d).await.0;
    let new_notes: Vec<String> =
        status_texts(d).await.into_iter().filter(|note| !notes_before.contains(note)).collect();
    let dialogs: Vec<String> = tried.dialog.iter().map(|x| format!("{}: {}", x.kind, x.message)).collect();
    if tried.outcome.ok && recording {
        let led_to = match (&tried.dialog, new_notes.first()) {
            (Some(x), _) => format!("{} \"{}\"", x.kind, one_short_line(&x.message)),
            (None, Some(note)) => format!("\"{note}\""),
            (None, None) if !after.is_empty() && after != before => format!("moved to {after}"),
            (None, None) => "nothing visible changed".to_string(),
        };
        let line = one_short_line(&without_typed(&format!("{what}: {led_to}"), action));
        if let Err(why) = crate::autorun::discovery_map::record_outcome(root, organization, project, bucket, &line) {
            unrecorded(&why);
        }
    }
    Ok(Discovered { outcome: tried.outcome, dialogs, writes, blocked, after })
}

/// `/autorun-discover-start`: open the Auto Run browser for the assistant
/// and sign in as `account` (`discover_start_in`). Refused, before
/// anything opens, while anything else holds the browser, and for an
/// account or recipe that is not there. `holder` is the assistant whose call
/// it is: the browser opened in slot `n` stays its own through any browser it
/// replaces.
pub(crate) async fn autorun_discover_start(
    ctx: &BridgeContext,
    body: &str,
    seen: &mut Option<crate::autorun::overrides::Overrides>,
    n: SlotNo,
    holder: Option<&AgentKey>,
) -> (u16, String) {
    const SHAPE: &str = "{ \"account\": <an account key>, \"area\": <an area name, optional>, \"browser\": \"edge\" | \"chrome\", optional, \"mapping\": true | false, optional, \"modules\": [<a module name>], for a mapping run, \"case_id\": <the case explored for, optional>, \"no_save\": true | false, optional, \"explore\": true | false, optional }";
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {SHAPE}.")),
    };
    let Some(account) = named(v.get("account").and_then(|a| a.as_str())) else {
        return (
            400,
            format!("this call needs an \"account\": the key of an account saved under Auto Run, Accounts. Expected {SHAPE}."),
        );
    };
    let area = named(v.get("area").and_then(|a| a.as_str()));
    let browser = match v.get("browser") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(b)) if matches!(b.trim().to_ascii_lowercase().as_str(), "edge" | "chrome") => {
            Some(b.trim().to_ascii_lowercase())
        }
        Some(_) => return (400, "browser is \"edge\" or \"chrome\"".to_string()),
    };
    let mapping = match v.get("mapping") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(_) => return (400, format!("mapping is true or false. Expected {SHAPE}.")),
    };
    let explore = match v.get("explore") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::String(s)) if matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "false") => {
            s.trim().eq_ignore_ascii_case("true")
        }
        Some(_) => return (400, format!("explore is true or false. Expected {SHAPE}.")),
    };
    if explore && mapping {
        return (400, EXPLORE_NOT_MAPPING.to_string());
    }
    let modules: Vec<String> = match v.get("modules") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(serde_json::Value::Array(items)) => {
            match items.iter().map(|m| m.as_str().map(str::to_string)).collect::<Option<Vec<_>>>() {
                Some(names) => names,
                None => return (400, format!("modules is a list of module names. Expected {SHAPE}.")),
            }
        }
        Some(_) => return (400, format!("modules is a list of module names. Expected {SHAPE}.")),
    };
    let mapping = if mapping {
        let run = crate::commands::autorun::MappingRun::new(&modules);
        if run.modules.is_empty() {
            return (400, MAPPING_NEEDS_A_MODULE.to_string());
        }
        Some(run)
    } else {
        None
    };
    // One mapping run at a time, whoever runs it and in whichever browser:
    // claimed before anything opens, refused at once naming whose browser it
    // is going in, and given back when this start returns (failed or not).
    let _mapping_start = match mapping.is_some().then(|| crate::commands::autorun::claim_mapping_start(n)) {
        Some(Err(going)) => return (409, mapping_refusal(n, going).unwrap_or_else(|| mapping_going("an Auto Run browser"))),
        Some(Ok(start)) => Some(start),
        None => None,
    };
    if let Err(why) = crate::commands::autorun::refuse_discovery_while_busy(n) {
        return (409, why);
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    // A discovery for a case that must not save is guarded from its
    // sign-in on: settled before a browser opens.
    let asked = match discovery_for(&root, body) {
        Ok(asked) => crate::commands::autorun::DiscoveryFor { explore, ..asked },
        Err(refused) => return refused,
    };
    // No account, no recipe: said before a browser opens for nothing.
    let (username, session_minutes) = match crate::autorun::signin::prepare(&root, &ctx.org, &ctx.project, &account) {
        Ok((recipe, acct)) => (acct.username, recipe.session_minutes),
        Err(why) => return (409, why),
    };
    // Someone already signed in as the account is logged out by this
    // sign-in: refused before a browser opens. An expired or nearly expired
    // password only warns. A database that is off never blocks.
    let gate = sign_in_gate(ctx, &root, &account, &username, session_minutes).await;
    if let Some(refused) = gate.refused {
        return (409, refused);
    }
    // None named: a browser the replay left is kept, whichever it is, and a
    // new one is the one last chosen (`open_for_discovery`). A guarded
    // discovery's guard is on in a browser it took over before the lock goes.
    let from = GuardFrom { root: &root, organization: &ctx.org, project: &ctx.project };
    let made_way = match crate::commands::autorun::open_for_discovery(browser.as_deref(), mapping, asked, from, n, holder).await {
        Ok(made_way) => made_way,
        Err(why) => return (409, why),
    };
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    let answer = discover_start_in(
        &mut slot,
        &root,
        &ctx.org,
        &ctx.project,
        &account,
        area.as_deref(),
        &crate::browser::timing::Timing::supervised(),
    )
    .await;
    // A sign-in that failed closed the browser, and the discovery with it.
    crate::commands::autorun::publish_discovery_in(n, &slot);
    // A mapping run's guard pauses every request: the browser is answered
    // between the assistant's calls too.
    if slot.as_ref().is_some_and(|s| s.cdp.is_guarding_saves()) {
        crate::commands::autorun::answer_between_commands_in(n);
    }
    *seen = Some(crate::commands::autorun::overrides_of(&slot));
    said_warnings(said_made_way(made_way, answer), &gate.warnings)
}

/// The two environment queries' checks of an account before a discovery
/// signs in as it (`active_login::discovery_gate`). Nothing is asked when
/// neither query is set; a database that is off is said once, never a
/// refusal.
pub(crate) async fn sign_in_gate(
    ctx: &BridgeContext,
    root: &std::path::Path,
    key: &str,
    username: &str,
    session_minutes: u32,
) -> crate::autorun::active_login::Gate {
    use crate::autorun::active_login::{discovery_gate, Gate, StageRows, NOT_CHECKED_NOTE};
    let Ok(env) = crate::environments::active(root) else { return Gate::default() };
    if env.password_expiry_query.trim().is_empty() && env.active_session_query.trim().is_empty() {
        return Gate::default();
    }
    let Ok(db) = stage_db(ctx) else {
        return Gate { refused: None, warnings: vec![NOT_CHECKED_NOTE.to_string()] };
    };
    let ours = crate::autorun::active_login::session_ownership(
        root,
        &env.id,
        key,
        session_minutes,
        crate::autorun::sessions::now_ms(),
    );
    discovery_gate(
        &StageRows(&db),
        &env.password_expiry_query,
        &env.active_session_query,
        key,
        username,
        ours,
        &crate::autorun::password_expiry::today(),
    )
    .await
}

/// `answer` with the checks' warnings beside it: a `warnings` list in a JSON
/// object answer, or lines after plain text. Nothing is added for no
/// warnings or a refused call.
pub fn said_warnings((status, text): (u16, String), warnings: &[String]) -> (u16, String) {
    if warnings.is_empty() || status != 200 {
        return (status, text);
    }
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(mut v @ serde_json::Value::Object(_)) => {
            v["warnings"] = serde_json::json!(warnings);
            (status, v.to_string())
        }
        _ => (status, format!("{text}
Warning: {}", warnings.join("
Warning: "))),
    }
}

/// The case a discovery call names (`case_id`, optional) and whether it
/// said `no_save` (optional). `no_save` is also true when the case's saved
/// script on this machine is marked `no_save`, so naming such a case is
/// enough. A script that cannot be read refuses the call: a case that may
/// be marked `no_save` is never explored unguarded.
pub fn discovery_for(root: &std::path::Path, body: &str) -> Result<crate::commands::autorun::DiscoveryFor, (u16, String)> {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let case_id = match v.get("case_id") {
        None | Some(serde_json::Value::Null) => None,
        Some(id) => Some(
            id.as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .filter(|n| *n > 0)
                .ok_or((400, "case_id must be a test case id: a positive number".to_string()))?,
        ),
    };
    let said = match v.get("no_save") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(_) => return Err((400, "no_save is true or false".to_string())),
    };
    let marked = match case_id {
        None => false,
        Some(id) => crate::autorun::store::load_script(root, id).map_err(|why| (409, why))?.is_some_and(|s| s.no_save),
    };
    Ok(crate::commands::autorun::DiscoveryFor { case_id, no_save: said || marked, explore: false })
}

/// Put what a discovery call named (`discovery_for`) on the discovery in
/// `slot`, for that call: when it said `no_save` (or named a case marked
/// so), that call is guarded (`DiscoveryState::call_no_save`), for the case
/// the call named (`DiscoveryState::call_case_id`) - the discovery does not
/// become one that must not save. The first case named is the one it
/// explores for, and a case marked `no_save` keeps its discovery guarded
/// from then on (`discovery_must_not_save`). Nothing to do with no
/// discovery going.
pub fn mark_discovery_for<B: DiscoveryBrowser>(slot: &mut Option<B>, asked: crate::commands::autorun::DiscoveryFor) {
    let Some(state) = slot.as_mut().and_then(|b| b.parts().discovery.as_mut()) else {
        return;
    };
    state.call_no_save = asked.no_save;
    state.call_case_id = asked.case_id;
    if state.case_id.is_none() {
        state.case_id = asked.case_id;
    }
}

/// Whether a discovery must not save: it was told so, or the case it
/// explores for is marked `no_save` now (read again each time, so a mark
/// added on the Setup tab counts from the next action). A script that
/// cannot be read counts as marked.
pub fn discovery_must_not_save(root: &std::path::Path, state: &crate::commands::autorun::DiscoveryState) -> bool {
    state.no_save
        || state
            .case_id
            .is_some_and(|id| crate::autorun::store::load_script(root, id).map_or(true, |s| s.is_some_and(|s| s.no_save)))
}

/// A discovery start's answer, saying first what it made of the browser the
/// assistant's replay left open (`MadeWay`): took it over (a `"took_over"`
/// field) or closed it (a `"closed"` field) beside the JSON of one that
/// started, the sentence before the text of one that did not.
pub fn said_made_way(made_way: MadeWay, (status, text): (u16, String)) -> (u16, String) {
    let (field, sentence) = match made_way {
        MadeWay::Free => return (status, text),
        MadeWay::TookOver => ("took_over", TOOK_OVER_LEFT_OPEN),
        MadeWay::Closed => ("closed", CLOSED_LEFT_OPEN),
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(mut v @ serde_json::Value::Object(_)) => {
            v[field] = serde_json::json!(sentence);
            (status, v.to_string())
        }
        _ => (status, format!("{sentence} {text}")),
    }
}

/// `/autorun-discover-action`: one action in the discovery's browser
/// (`discover_action_in`). Refused as a tried action is (`sign_in`, a local
/// file), and a `navigate` or `open_tab` to a site outside the sign-in
/// recipe's, before the browser is touched. A `use_component` may carry a
/// `draft` component, tried in place of the saved one.
pub(crate) async fn autorun_discover_action(
    ctx: &BridgeContext,
    body: &str,
    seen: &mut Option<crate::autorun::overrides::Overrides>,
    n: SlotNo,
) -> (u16, String) {
    const SHAPE: &str = "{ \"action\": <one script action>, \"draft\": <a component, optional, for a use_component>, \"area\": <an area name, optional>, \"case_id\": <the case explored for, optional>, \"no_save\": true | false, optional }";
    let raw = match body_field(body, "action", SHAPE) {
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
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let area = named(v.get("area").and_then(|a| a.as_str()));
    // A component not saved yet, tried by the `use_component` that names it.
    let draft: Option<crate::autorun::components::Component> = match v.get("draft") {
        None | Some(serde_json::Value::Null) => None,
        Some(raw) => match serde_json::from_value(raw.clone()) {
            Ok(c) => Some(c),
            Err(e) => return (400, format!("that draft is not a component: {e}. Expected {SHAPE}.")),
        },
    };
    if draft.is_some() && !matches!(action, crate::browser::actions::Action::UseComponent { .. }) {
        return (400, format!("a draft is tried by a use_component action that names it. Expected {SHAPE}."));
    }
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let recipe = match crate::autorun::recipe::load_effective_recipe(&root, &ctx.org, &ctx.project) {
        Ok(r) => r,
        Err(why) => return (409, why),
    };
    let policy = crate::autorun::runner::policy_for(Some(&recipe));
    if let Err(refused) = refuse_outside_the_recipe(&policy, &action) {
        return refused;
    }
    let asked = match discovery_for(&root, body) {
        Ok(asked) => asked,
        Err(refused) => return refused,
    };
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    mark_discovery_for(&mut slot, asked);
    let answer = discover_action_in(&mut slot, &root, &ctx.org, &ctx.project, &action, draft.as_ref(), area.as_deref()).await;
    let answer = with_wait_notes(answer, &wait_notes);
    let answer = let_go_if_silent(&mut slot, answer).await;
    crate::commands::autorun::publish_discovery_in(n, &slot);
    *seen = Some(crate::commands::autorun::overrides_of(&slot));
    answer
}

/// `/autorun-discover-actions`: several actions in the discovery's browser,
/// in order, under one hold of it (`discover_actions_in`). Every action is
/// checked as `/autorun-discover-action` checks one, all before the
/// browser is touched: a batch with one refused action runs none. A
/// `draft` is tried by each `use_component` that names it; `area` moves the
/// discovery before the first action.
pub(crate) async fn autorun_discover_actions(
    ctx: &BridgeContext,
    body: &str,
    seen: &mut Option<crate::autorun::overrides::Overrides>,
    n: SlotNo,
) -> (u16, String) {
    const SHAPE: &str = concat!("{ \"actions\": [<script actions, at most ", max_batch!(), ">], \"stop_on_failure\": <true or false, optional, true by default>, \"draft\": <a component, optional, for a use_component>, \"area\": <an area name, optional>, \"case_id\": <the case explored for, optional>, \"no_save\": true | false, optional }");
    let raw = match body_field(body, "actions", SHAPE) {
        Ok(v) => v,
        Err(refused) => return refused,
    };
    let Some(raw) = raw.as_array() else {
        return (400, format!("\"actions\" is a list of actions. Expected {SHAPE}."));
    };
    if let Err(refused) = refuse_batch_size(raw.len()) {
        return refused;
    }
    let mut actions = Vec::with_capacity(raw.len());
    let mut wait_notes = Vec::new();
    for (i, one) in raw.iter().enumerate() {
        if let Some(r) = crate::autorun::forbidden_fixes::forbidden_in(one) {
            return (400, format!("action {}: {r}", i + 1));
        }
        match tried_action_from(one.clone()) {
            Ok((a, notes)) => {
                wait_notes.extend(notes.into_iter().map(|n| format!("action {}: {n}", i + 1)));
                actions.push(a);
            }
            Err(refused) => return (400, format!("action {}: {refused}", i + 1)),
        }
    }
    for (i, action) in actions.iter().enumerate() {
        if let Err((status, why)) = refuse_as_a_tried_action(action) {
            return (status, format!("action {}: {why}", i + 1));
        }
    }
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let stop_on_failure = match v.get("stop_on_failure") {
        None | Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(_) => return (400, format!("\"stop_on_failure\" is true or false. Expected {SHAPE}.")),
    };
    let area = named(v.get("area").and_then(|a| a.as_str()));
    let draft: Option<crate::autorun::components::Component> = match v.get("draft") {
        None | Some(serde_json::Value::Null) => None,
        Some(raw) => match serde_json::from_value(raw.clone()) {
            Ok(c) => Some(c),
            Err(e) => return (400, format!("that draft is not a component: {e}. Expected {SHAPE}.")),
        },
    };
    if let Some(c) = &draft {
        if !actions.iter().any(|a| draft_for(a, Some(c)).is_some()) {
            return (400, format!("a draft is tried by a use_component action that names it. Expected {SHAPE}."));
        }
    }
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let recipe = match crate::autorun::recipe::load_effective_recipe(&root, &ctx.org, &ctx.project) {
        Ok(r) => r,
        Err(why) => return (409, why),
    };
    let policy = crate::autorun::runner::policy_for(Some(&recipe));
    for (i, action) in actions.iter().enumerate() {
        if let Err((status, why)) = refuse_outside_the_recipe(&policy, action) {
            return (status, format!("action {}: {why}", i + 1));
        }
    }
    // Every component expanded as its action would expand it, so one that
    // is not saved, misses an input or leads outside the recipe refuses
    // the batch before any earlier action drives the browser.
    for (i, action) in actions.iter().enumerate() {
        if let crate::browser::actions::Action::UseComponent { component, inputs } = action {
            if let Err((status, why)) =
                component_to_try(&root, &ctx.org, &ctx.project, component, inputs, draft_for(action, draft.as_ref()))
            {
                return (status, format!("action {}: {why}", i + 1));
            }
        }
    }
    let asked = match discovery_for(&root, body) {
        Ok(asked) => asked,
        Err(refused) => return refused,
    };
    // A stop asked for before this batch is not this batch's.
    batch_stop(n).store(false, std::sync::atomic::Ordering::SeqCst);
    let _going = BatchGoing::begin(n);
    // Held for the whole batch: nothing else drives the browser between
    // its actions. End discovery, Close browser or a release asks it to
    // give way (`BATCH_STOPS`). A gone browser is let go inside, keeping
    // the lines of the actions that ran.
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    mark_discovery_for(&mut slot, asked);
    let answer = discover_actions_paged(
        &mut slot,
        &root,
        &ctx.org,
        &ctx.project,
        &actions,
        draft.as_ref(),
        area.as_deref(),
        stop_on_failure,
        batch_stop(n),
        page_wanted(body),
    )
    .await;
    let answer = with_wait_notes(answer, &wait_notes);
    crate::commands::autorun::publish_discovery_in(n, &slot);
    *seen = Some(crate::commands::autorun::overrides_of(&slot));
    answer
}

