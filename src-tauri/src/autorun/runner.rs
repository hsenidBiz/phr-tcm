//! The step loop `auto_run_step` runs: one action at a time, `sign_in`
//! carried out here (it needs the tester's accounts and the project's
//! recipe, which the executor has neither of), and a failure screenshot
//! taken the same way regardless of which of the two produced it.
//!
//! Pulled out of the Tauri command so it can be exercised directly, the
//! same way `autorun::store` and `autorun::signin` already are.

use super::lease::Held;
use super::nav::{self, Route};
use super::api_checks;
use super::recipe::{self, SignInRecipe};
use super::signin::{self, SignInOutcome};
use super::{store, PageErrors, StepDialog, StepScript};
use crate::browser::dialogs;
use crate::browser::actions::{
    execute_in, expectation, in_missing_tab, shows_up, upload_in, Action, ActionOutcome, Policy, CANNOT_RUN, NOT_SHOWN,
    WHEN_VISIBLE_MS,
};
use crate::browser::cdp::{Driver, MAIN_TAB};
use crate::browser::page;
use crate::browser::timing::{Timing, SHOT_TIMEOUT_MS};
use crate::browser::downloads::{DownloadEntry, DownloadState};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const AFTER_FAILED_SIGN_IN: &str = "not run: the sign-in before this action failed";
const AFTER_UNREACHED: &str = "not run: the module screen was not reached after the sign-in";
const AFTER_REFUSED_ADDRESS: &str = "not run: this step opened a page by address, which this project does not allow";
/// The rest of a step after the page tried to save in a no-save script.
pub const AFTER_SAVE_BLOCKED: &str = "not run: the page tried to save, and this script must not";

/// The rest of a step after `return_to_area` could not reach the area.
const AFTER_AREA_UNREACHED: &str = "not run: the case's area was not reached";

/// The rest of a step after a `return_to_area` that names an area could
/// not reach it.
pub const AFTER_NAMED_AREA_UNREACHED: &str = "not run: the area the step went to was not reached";

/// The routes to the areas a script's `return_to_area` actions name, keyed
/// by `nav::module_key`: each the area's recorded path from the recipe's
/// home, or why there is none. A name missing from it was not resolved
/// (the area was removed since the script was saved), and blocks the rest
/// of its step the same way.
pub type NamedAreas = BTreeMap<String, Result<Route, String>>;

/// Every area a `return_to_area` in `actions` names, a `when_visible`'s
/// guarded actions included, trimmed and once each by `nav::module_key`.
pub fn named_areas<'a>(actions: impl IntoIterator<Item = &'a Action>) -> Vec<&'a str> {
    let mut seen = std::collections::BTreeSet::new();
    actions
        .into_iter()
        .flat_map(Action::each)
        .filter_map(Action::area_named)
        .filter(|name| seen.insert(nav::module_key(name)))
        .collect()
}

/// The route to each area in `names`, from this project's recorded areas
/// (`nav::find_area`) and the recipe's home - the same parts the case's
/// own route is made of. Read once, before the steps that use it.
pub fn area_routes(root: &Path, organization: &str, project: &str, names: &[&str]) -> NamedAreas {
    if names.is_empty() {
        return NamedAreas::new();
    }
    let nav_file = nav::load_nav(root, organization, project);
    let home = recipe::load_effective_recipe_if_any(root, organization, project);
    names
        .iter()
        .map(|name| {
            let route = match (&nav_file, &home) {
                (Err(why), _) | (_, Err(why)) => Err(why.clone()),
                (Ok(nav_file), Ok(home)) => match nav::find_area(nav_file, name) {
                    None => Err(nav::unrecorded_area(name)),
                    Some(path) => {
                        home.as_ref().map(|h| Route::new(h, path.clone())).ok_or_else(|| NO_HOME_FOR_AREA.to_string())
                    }
                },
            };
            (nav::module_key(name), route)
        })
        .collect()
}

/// Where a `return_to_area` naming `name` goes, from `areas`: its route, or
/// the sentence that fails it - an area not in `areas` at all is one not
/// recorded when the run read them.
fn named_route<'a>(areas: Option<&'a NamedAreas>, name: &str) -> Result<&'a Route, String> {
    match areas.and_then(|m| m.get(&nav::module_key(name))) {
        Some(Ok(rt)) => Ok(rt),
        Some(Err(why)) => Err(why.clone()),
        None => Err(nav::unrecorded_area(name)),
    }
}

/// Where a `return_to_area` goes: the case's route, or why there is none.
pub enum AreaRoute<'a> {
    To(&'a Route),
    Unknown(&'a str),
}

/// Said when the run has no route for the case: this project records no
/// menu paths, so there is no area to go back to.
pub const NO_AREA_IN_RUN: &str =
    "return_to_area has nowhere to go: this project has no areas recorded - record them on the Auto Run Setup tab";

/// Said in a watched run, or a try, of a script that names no area: only an
/// unattended run knows the case's Module, from which a default area comes.
pub const NEEDS_SCRIPT_AREA: &str =
    "return_to_area in a watched run needs the script's own area - set it on the script, or run the case unattended";

/// Said when there is no sign-in recipe to start the trip from.
pub const NO_HOME_FOR_AREA: &str =
    "return_to_area needs the project's sign-in recipe or site address to start from - set one on the Auto Run Setup tab";

/// The route a watched run's (or a try's) `return_to_area` takes: the area
/// the case's saved script names, from the recipe's home - the same parts
/// an unattended run puts together before step 1. Only the script's own
/// area: the case's Module is not known here, so a default area is not
/// chosen for it.
pub fn area_route(root: &Path, organization: &str, project: &str, case_id: i32) -> Result<Route, String> {
    let script = store::load_script(root, case_id)?
        .ok_or_else(|| format!("return_to_area: case {case_id} has no saved script to name its area"))?;
    let area = script.area.as_deref().map(str::trim).filter(|a| !a.is_empty()).ok_or(NEEDS_SCRIPT_AREA)?;
    let nav_file = nav::load_nav(root, organization, project)?;
    let path = nav::find_area(&nav_file, area).ok_or_else(|| nav::unrecorded_area(area))?;
    let home = recipe::load_effective_recipe_if_any(root, organization, project)?.ok_or(NO_HOME_FOR_AREA)?;
    Ok(Route::new(&home, path.clone()))
}

/// Where an authored `navigate` may go for this project: everywhere when
/// there is no recipe to run (no saved recipe and no site address), else
/// only the origins of the recipe that runs - the project's own, or the
/// built-in one at the site address.
pub fn policy_for(recipe: Option<&SignInRecipe>) -> Policy {
    match recipe {
        Some(r) => Policy::only(r.origins()),
        None => Policy::open(),
    }
}

/// A sign-in reads as the one action outcome it stands in for. `harness`
/// carries over too, so a `sign_in` that failed because the browser itself
/// stopped answering does not then get asked for a screenshot below.
pub fn as_action_outcome(out: &SignInOutcome) -> ActionOutcome {
    let mut outcome = if out.ok {
        ActionOutcome::passed(out.detail.clone())
    } else {
        ActionOutcome::failed(out.detail.clone())
    };
    outcome.harness = out.harness;
    outcome
}

/// A picture of the page at the moment an action failed, or (from the
/// replay engine) at the moment an executed step ended. Best effort: a
/// browser that cannot take one (it has gone away, or is too busy to
/// answer within `SHOT_TIMEOUT_MS`) just means no picture - the failure is
/// already reported in words. Never called for a harness failure (asking a
/// browser that has already failed to answer for a picture is exactly the
/// stall this guards against) and never for an action that was never run.
pub(crate) async fn picture<D: Driver>(d: &mut D, root: &Path) -> Option<String> {
    let bytes = tokio::time::timeout(Duration::from_millis(SHOT_TIMEOUT_MS), page::screenshot(d))
        .await
        .ok()?
        .ok()?;
    store::save_shot(root, &bytes).ok()
}

/// Run one step's actions in order and report every outcome.
///
/// Actions after an ORDINARY failure still run: the watcher learns more
/// from "the click worked, the check did not" than from a run that stops
/// at the first red. A failed `sign_in` is the one exception - what
/// follows would run as the wrong person, or as nobody, and report
/// results that mean nothing - so once a `sign_in` action fails, every
/// remaining action of that step is not executed and is reported as such.
/// The outcomes list still has one entry per action, in order.
///
/// A `sign_in` here takes its account's lease (`lease`) only for the step:
/// a browser that stays signed in between steps keeps its lease in its own
/// session and runs them through `run_step_routed`.
pub async fn run_step<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    step: &StepScript,
    timing: &Timing,
    account: &mut Option<String>,
) -> Result<Vec<ActionOutcome>, String> {
    let mut lease = Held::supervised();
    run_step_routed(d, root, organization, project, step, timing, account, &mut lease, None, AreaRoute::Unknown(NEEDS_SCRIPT_AREA))
        .await
}

/// `run_step` with the browser's own account lease: a `sign_in` is first
/// cleared with `lease` (`Held::hold`), and one that is refused signs
/// nobody in and fails the step with the sentence that says who has the
/// account. With a `route` - an unattended run in a project with module
/// paths - a `sign_in` lands on the home page, so the runner takes the
/// browser back to the case's module screen before the next action; the
/// `sign_in`'s one outcome then says both halves. When the module is not
/// reached, the rest of the step is not run: it would act on the wrong
/// screen.
#[allow(clippy::too_many_arguments)]
pub async fn run_step_routed<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    step: &StepScript,
    timing: &Timing,
    account: &mut Option<String>,
    lease: &mut Held,
    route: Option<&Route>,
    area: AreaRoute<'_>,
) -> Result<Vec<ActionOutcome>, String> {
    let mut run = InRun::default();
    run_step_in_run(d, root, organization, project, step, timing, account, lease, route, area, &mut run).await
}

/// What an unattended run shares with each step it runs, and learns back.
#[derive(Default)]
pub struct InRun<'a> {
    /// The run's Stop: a wait inside the step (an `expect_download`'s) ends
    /// at its next look once it is set. `None` in a watched run or a try,
    /// where nothing waits that long unasked.
    pub cancel: Option<&'a AtomicBool>,
    /// Set by the step: the moment it began, as its `expect_download` takes
    /// it (after reading what the browser had already sent). The step's
    /// record dates its downloads from the same moment.
    pub began: Option<Instant>,
    /// With no Stop to hear, how long an `expect_download` may wait at most,
    /// whatever its `within_ms` says: `WATCHED_DOWNLOAD_WAIT_MS` when `None`.
    /// A test sets a shorter one.
    pub watched_cap_ms: Option<u32>,
    /// Learned back: the tab the step ran in, when that was not `main` -
    /// the tab it ended in, or else the last other tab one of its actions
    /// (other than a tab action) acted in. `None` for a step that stayed in
    /// `main`.
    pub tab: Option<String>,
    /// The routes to the areas the steps' `return_to_area` actions name
    /// (`area_routes`). `None` reads as none resolved: a named area then
    /// fails as not recorded.
    pub areas: Option<&'a NamedAreas>,
    /// The case's `fail_on_unexpected_dialog`: a dialog no `expect_dialog`
    /// claimed fails the step it appeared in.
    pub fail_on_unexpected_dialog: bool,
    /// Learned back: the dialog the step met, for its record.
    pub dialog: Option<StepDialog>,
    /// The case's `page_errors`: what the step does with the page's own
    /// errors (`page_errors`). `None` looks at none.
    pub page_errors: Option<PageErrors>,
    /// The case's `ignore_page_errors`.
    pub ignore_page_errors: Vec<String>,
    /// Learned back: how many page errors a `flag` counted in the step.
    pub page_errors_seen: u32,
}

/// The longest an `expect_download` waits in a watched run or a try, which
/// has no Stop to end it: the person, or the assistant, is waiting on the
/// answer, and a step must never hold the browser for two minutes unasked.
pub const WATCHED_DOWNLOAD_WAIT_MS: u32 = 30_000;

/// What an action interrupted by the run's Stop, and every action after it
/// in the step, records: what a stopped case records for what it never
/// ran.
pub const AFTER_STOP: &str = "not run: the run was stopped";

/// `run_step_routed` inside an unattended run: the run's Stop reaches the
/// step's waits through `run`, and the step says when it began.
#[allow(clippy::too_many_arguments)]
pub async fn run_step_in_run<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    step: &StepScript,
    timing: &Timing,
    account: &mut Option<String>,
    lease: &mut Held,
    route: Option<&Route>,
    area: AreaRoute<'_>,
    run: &mut InRun<'_>,
) -> Result<Vec<ActionOutcome>, String> {
    // No recipe to run (none saved, no site address): navigation is open,
    // as before the built-in existed; the sign-in itself is what refuses.
    let recipe = recipe::load_effective_recipe_if_any(root, organization, project)?;
    let policy = policy_for(recipe.as_ref());
    // Read per step, like the recipe: a person may flip the switch between
    // two steps of a supervised run.
    let nav_file = nav::load_nav(root, organization, project)?;
    // Where the step began in the browser's network record: an
    // `expect_response` looks only at requests that started after it. What
    // the browser has already sent is read first, so a request the page
    // made before this step is not taken for one of the step's own.
    // The same for downloads: one an earlier step started is read (and
    // dated) before the step's own time is noted, so it is never taken for
    // one this step started.
    if step.actions.iter().any(|a| matches!(a, Action::ExpectResponse { .. } | Action::ExpectDownload { .. })) {
        api_checks::settle(d, timing).await;
    }
    // A tab opened from now on, or during the step before, is one this
    // step's `expect_tab` may claim.
    d.step_began();
    // A dialog from before the step began is not the step's. Every
    // `expect_dialog` the step holds is armed now, in order, so one that an
    // earlier action of the step opens is claimed and answered as asked.
    let plans: Vec<dialogs::DialogPlan> = step
        .actions
        .iter()
        .filter_map(expectation)
        .enumerate()
        .map(|(i, want)| dialogs::plan_of(i as u32, want.answer, &want.prompt_text.map(str::to_string)))
        .collect();
    if let Some(book) = d.dialog_book() {
        book.take_seen();
        book.arm(plans);
    }
    let mut dialogs_read = 0usize;
    let fail_on_unexpected = run.fail_on_unexpected_dialog;
    let mut deferred: Vec<(usize, u32)> = Vec::new();
    let mark = d.net_mark();
    let began = Instant::now();
    run.began = Some(began);
    run.tab = None;
    run.dialog = None;
    let mut ran_in: Option<String> = None;
    let areas = run.areas;
    let here =
        Here { root, organization, project, policy: &policy, direct_urls: nav_file.direct_urls, step: step.step_number };
    let mut out = Vec::with_capacity(step.actions.len());
    let mut blocked: Option<&'static str> = None;
    for action in &step.actions {
        if let Some(why) = blocked {
            out.push(ActionOutcome::failed(why));
            continue;
        }
        // A save stopped before this action began - while the page loaded,
        // or between two steps - fails the step here, before it acts.
        if let Some(sentence) = d.take_save_blocked() {
            let mut stopped = ActionOutcome::failed(sentence);
            stopped.screenshot = picture(d, root).await;
            out.push(stopped);
            blocked = Some(AFTER_SAVE_BLOCKED);
            continue;
        }
        // An `expect_dialog` judges once the step's other actions are done:
        // its place is kept, and filled then.
        if matches!(action, Action::ExpectDialog { .. }) {
            deferred.push((out.len(), deferred.len() as u32));
            out.push(ActionOutcome::failed("not run yet"));
            continue;
        }
        // The tab an action acted in. A tab action acts on the tabs, not in
        // the current one, so it does not count.
        let in_tab = d.tab_name();
        if in_tab != MAIN_TAB && !action.is_tab_action() {
            ran_in = Some(in_tab);
        }
        // The current tab closed by itself: an action that acts in it says
        // so at once, and the tab actions still run.
        if let Some(gone) = in_missing_tab(d, action) {
            out.push(gone);
            continue;
        }
        let mut outcome = match action {
            Action::SignIn { account: key } => match signin::prepare(root, organization, project, key) {
                Err(why) => {
                    *account = None;
                    blocked = Some(AFTER_FAILED_SIGN_IN);
                    ActionOutcome::failed(why)
                }
                Ok((r, who)) => match lease.hold(root, &who.key).await {
                    // Someone else is signed in as this account: nobody is
                    // signed in here, and the browser stays as it was.
                    Err(why) => {
                        blocked = Some(AFTER_FAILED_SIGN_IN);
                        ActionOutcome::failed(why)
                    }
                    // The account is this browser's from here, whichever
                    // way the sign-in goes.
                    Ok(()) => {
                        let signed = signin::sign_in(d, root, &r, &who, timing).await;
                        *account = signed.ok.then(|| who.key.clone());
                        match route {
                            _ if !signed.ok => {
                                blocked = Some(AFTER_FAILED_SIGN_IN);
                                as_action_outcome(&signed)
                            }
                            None => as_action_outcome(&signed),
                            Some(rt) => {
                                let who = format!("Auto Run, step {}", step.step_number);
                                let went = nav::reach_module(d, rt, nav::TripFrom::SignIn, timing, &who).await;
                                if !went.ok {
                                    blocked = Some(AFTER_UNREACHED);
                                }
                                signed_then_went(&signed, went)
                            }
                        }
                    }
                },
            },
            // The run's own trip to the case's area, from wherever the page
            // is now (a reload that went home, a page the case moved off).
            // Not reaching it stops the step: what follows would act on the
            // wrong screen.
            // A named area: its own route, read before the steps. Not
            // reaching it stops the step the same way.
            Action::ReturnToArea { .. } if action.area_named().is_some() => {
                let name = action.area_named().unwrap_or_default();
                match named_route(areas, name) {
                    Ok(rt) => {
                        let who = format!("Auto Run, step {}", step.step_number);
                        let went = nav::reach_module(d, rt, nav::TripFrom::Elsewhere, timing, &who).await;
                        if !went.ok {
                            blocked = Some(AFTER_NAMED_AREA_UNREACHED);
                        }
                        went
                    }
                    Err(why) => {
                        blocked = Some(AFTER_NAMED_AREA_UNREACHED);
                        ActionOutcome::failed(format!("return_to_area: {why}"))
                    }
                }
            }
            Action::ReturnToArea { .. } => match &area {
                AreaRoute::To(rt) => {
                    let who = format!("Auto Run, step {}", step.step_number);
                    let went = nav::reach_module(d, rt, nav::TripFrom::Elsewhere, timing, &who).await;
                    if !went.ok {
                        blocked = Some(AFTER_AREA_UNREACHED);
                    }
                    went
                }
                AreaRoute::Unknown(why) => {
                    blocked = Some(AFTER_AREA_UNREACHED);
                    ActionOutcome::failed(*why)
                }
            },
            // Only the runner knows where the step began.
            Action::ExpectResponse { .. } => api_checks::expect_response(d, action, mark, timing).await,
            Action::ApiRequest { path, .. } => {
                // Its fetch is the run's own request: a 5xx it gets back is
                // never the page's error.
                let before = d.net_mark();
                let outcome = api_checks::api_request(d, action, timing).await;
                let own: Vec<String> = d
                    .net_since(before)
                    .into_iter()
                    .filter(|e| e.method == "GET" && e.path_query.split(['?', '#']).next() == Some(path.as_str()))
                    .map(|e| e.id)
                    .collect();
                if let Some(book) = d.page_error_book() {
                    book.exclude(own);
                }
                outcome
            }
            Action::ExpectDownload { .. } => {
                // No Stop to hear: the wait is capped instead.
                let cap = match run.cancel {
                    Some(_) => None,
                    None => Some(run.watched_cap_ms.unwrap_or(WATCHED_DOWNLOAD_WAIT_MS)),
                };
                let outcome = expect_download(d, action, began, run.cancel, cap).await;
                if outcome.detail == AFTER_STOP {
                    blocked = Some(AFTER_STOP);
                }
                outcome
            }
            Action::WhenVisible { .. } => {
                let (outcome, stop) = when_visible(d, &here, action, timing).await;
                blocked = stop;
                outcome
            }
            other => {
                let (outcome, stop) = plain(d, &here, other, timing).await;
                blocked = stop;
                outcome
            }
        };
        // A save the page tried while this action ran is the step's
        // failure, whatever the action itself made of the page.
        if let Some(sentence) = d.take_save_blocked() {
            outcome = ActionOutcome::failed(sentence);
            blocked = Some(AFTER_SAVE_BLOCKED);
        }
        unexpected_dialog(d, &mut dialogs_read, fail_on_unexpected, &mut outcome);
        // A Stop is no failure to picture.
        if !outcome.ok && !outcome.harness && outcome.detail != AFTER_STOP {
            outcome.screenshot = picture(d, root).await;
        }
        out.push(outcome);
    }
    // Each `expect_dialog`, in order, now the rest of the step has run.
    for (slot, id) in deferred {
        let mut outcome = match (blocked, step.actions.get(slot).and_then(expectation)) {
            (Some(why), _) => ActionOutcome::failed(why),
            (None, None) => ActionOutcome::failed("expect_dialog could not be read"),
            (None, Some(want)) => dialogs::judge(d, id, &want, timing).await,
        };
        unexpected_dialog(d, &mut dialogs_read, fail_on_unexpected, &mut outcome);
        if !outcome.ok && !outcome.harness && blocked.is_none() {
            outcome.screenshot = picture(d, root).await;
        }
        out[slot] = outcome;
    }
    // What no dialog claimed goes no further than this step.
    let seen = match d.dialog_book() {
        Some(book) => {
            book.disarm();
            book.take_seen()
        }
        None => Vec::new(),
    };
    // One that opened after the step's last look: still this step's.
    if let Some(last) = out.last_mut() {
        if last.ok {
            if let Some(s) = seen.iter().skip(dialogs_read).find(|s| s.claimed_by.is_none()) {
                if fail_on_unexpected {
                    *last = ActionOutcome::failed(dialogs::unexpected(&s.kind, &s.message));
                }
            }
        }
    }
    run.dialog = seen
        .iter()
        .find(|s| s.claimed_by.is_some())
        .or_else(|| seen.first())
        .map(|s| StepDialog { kind: s.kind.clone(), message: s.message.clone() });
    // The page's own errors since the previous step was judged. An outcome
    // they failed is pictured, as any failed action is.
    let judged = super::page_errors::judge_step(d, run.page_errors, &run.ignore_page_errors, &mut out);
    run.page_errors_seen = judged.seen;
    if let Some(i) = judged.failed {
        out[i].screenshot = picture(d, root).await;
    }
    let ended_in = d.tab_name();
    run.tab = if ended_in != MAIN_TAB { Some(ended_in) } else { ran_in };
    Ok(out)
}

/// A dialog nobody expected, seen since the last look (`read` counts how
/// many of the book's dialogs have been looked at): with
/// `fail_on_unexpected_dialog` it fails `outcome`, when nothing else did,
/// with its sentence. Without, the action already said it was accepted.
fn unexpected_dialog<D: Driver>(d: &mut D, read: &mut usize, fail: bool, outcome: &mut ActionOutcome) {
    let Some(book) = d.dialog_book() else {
        return;
    };
    let seen = book.seen();
    let first = seen.iter().skip(*read).find(|s| s.claimed_by.is_none()).cloned();
    *read = seen.len();
    if let Some(s) = first {
        if fail && outcome.ok {
            *outcome = ActionOutcome::failed(dialogs::unexpected(&s.kind, &s.message));
        }
    }
}

/// The supervised browser keeps one set of tabs across the cases a person
/// runs in it: a step of another case than the last one (`case`) is a new
/// case, so every tab but `main` is closed first - and the page errors met
/// so far were another case's, so they are dropped too. `tabs_case` is the
/// case the browser's tabs belong to.
pub async fn tabs_for_case<D: Driver>(d: &mut D, tabs_case: &mut Option<i32>, case: i32) {
    if *tabs_case != Some(case) {
        d.close_other_tabs().await;
        super::page_errors::drop_all(d);
    }
    *tabs_case = Some(case);
}

/// A supervised step is about to run: the case's tabs (`tabs_for_case`),
/// and, for the case's first step, a clean page-error slate - whatever the
/// page met before it (opening, a sign-in, a step run earlier) is no
/// step's. Between two later steps of one case, errors still count against
/// the next.
pub async fn supervised_step_begins<D: Driver>(d: &mut D, tabs_case: &mut Option<i32>, case: i32, first_step: bool) {
    tabs_for_case(d, tabs_case, case).await;
    if first_step {
        super::page_errors::drop_all(d);
    }
}

/// What a plain action needs from the step it is in.
struct Here<'a> {
    root: &'a Path,
    organization: &'a str,
    project: &'a str,
    policy: &'a Policy,
    direct_urls: bool,
    step: i32,
}

/// A plain action - one a `when_visible` may guard - carried out, and why
/// the rest of the step must not run, when it must not.
async fn plain<D: Driver>(
    d: &mut D,
    here: &Here<'_>,
    action: &Action,
    timing: &Timing,
) -> (ActionOutcome, Option<&'static str>) {
    match action {
        // A script saved before the switch was turned off. The runner's
        // own trip home never comes through here.
        Action::Navigate { .. } | Action::OpenTab { .. } if !here.direct_urls => {
            (ActionOutcome::failed(nav::no_address(here.step)), Some(AFTER_REFUSED_ADDRESS))
        }
        Action::Upload { selector, file } => {
            (upload(d, here.root, here.organization, here.project, action, selector, file, timing).await, None)
        }
        other => (execute_in(d, other, timing, here.policy).await, None),
    }
}

/// A `when_visible`: one look for its target, then its `then` actions in
/// order if it showed, as the ONE outcome the action stands for - so the
/// step still has one outcome per action. A target that never appeared
/// passes with `NOT_SHOWN`. The guard stops at its first failure, and that
/// failure is the step's, said after what already ran.
async fn when_visible<D: Driver>(
    d: &mut D,
    here: &Here<'_>,
    action: &Action,
    timing: &Timing,
) -> (ActionOutcome, Option<&'static str>) {
    if let Err(why) = action.validate() {
        return (ActionOutcome::failed(format!("{CANNOT_RUN}{why}")), None);
    }
    let Action::WhenVisible { selector, within_ms, then } = action else {
        return (ActionOutcome::failed(format!("{CANNOT_RUN}this is not a when_visible")), None);
    };
    let target = selector.describe();
    match shows_up(d, selector, within_ms.unwrap_or(WHEN_VISIBLE_MS), timing).await {
        Err(silent) => return (silent, None),
        Ok(false) => return (ActionOutcome::passed(format!("{target} {NOT_SHOWN}")), None),
        Ok(true) => {}
    }
    let mut said: Vec<String> = Vec::with_capacity(then.len());
    for guarded in then {
        let (out, stop) = plain(d, here, guarded, timing).await;
        said.push(out.detail);
        if !out.ok {
            let mut failed = ActionOutcome::failed(format!("{target} showed: {}", said.join("; ")));
            failed.harness = out.harness;
            return (failed, stop);
        }
    }
    (ActionOutcome::passed(format!("{target} showed: {}", said.join("; "))), None)
}

/// An `upload`, carried out here because only the runner knows the
/// project, and so where its Test files are. The file is checked - there,
/// and within the cap - before anything in the page is touched; a missing
/// one fails the action with the sentence saying where to add it.
#[allow(clippy::too_many_arguments)]
async fn upload<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    action: &Action,
    selector: &crate::browser::locator::Target,
    file: &str,
    timing: &Timing,
) -> ActionOutcome {
    if let Err(why) = action.validate() {
        return ActionOutcome::failed(format!("this action cannot run: {why}"));
    }
    let folder = crate::test_files::folder(root, organization, project);
    match crate::test_files::check_for_run(&folder, file, "this step") {
        Err(why) => ActionOutcome::failed(why),
        Ok((path, size)) => {
            let shown = format!("\"{file}\" ({})", crate::test_files::human_size(size));
            upload_in(d, selector, &path, &shown, timing).await
        }
    }
}

/// How often an `expect_download` looks at the browser's downloads.
const DOWNLOAD_POLL: Duration = Duration::from_millis(100);

/// `ms` as a person reads a wait: `15 s`, `1.5 s`.
fn seconds(ms: u32) -> String {
    if ms % 1000 == 0 {
        format!("{} s", ms / 1000)
    } else {
        format!("{:.1} s", f64::from(ms) / 1000.0)
    }
}

/// An `expect_download`: the first download that started after `began`
/// (the step's start), waited for until it completes - both within
/// the action's one `within_ms` - then checked by
/// `autorun::downloads::check_file`. A download an earlier step started is
/// never this step's, finished or not. The file is read off the async
/// thread: a workbook can be up to the 50 MB cap. The run's Stop (`cancel`)
/// ends either wait at its next look, as `AFTER_STOP`; with no Stop, `cap`
/// bounds the whole wait, and its sentences name the capped time.
async fn expect_download<D: Driver>(
    d: &mut D,
    action: &Action,
    began: Instant,
    cancel: Option<&AtomicBool>,
    cap: Option<u32>,
) -> ActionOutcome {
    use crate::autorun::downloads::{check_file, CellCheck, DownloadCheck, HeaderCheck};
    use crate::browser::actions::{CellMatch, HeadersSpec, DOWNLOAD_WAIT_MS};
    if let Err(why) = action.validate() {
        return ActionOutcome::failed(format!("{CANNOT_RUN}{why}"));
    }
    let Action::ExpectDownload { name, within_ms, sheet, headers, cells, contains_text, .. } = action else {
        return ActionOutcome::failed(format!("{CANNOT_RUN}this is not an expect_download"));
    };
    let asked = within_ms.unwrap_or(DOWNLOAD_WAIT_MS);
    let within = cap.map_or(asked, |c| asked.min(c));
    let deadline = Instant::now() + Duration::from_millis(u64::from(within));
    let stopped = || cancel.is_some_and(|c| c.load(Ordering::SeqCst));
    let guid = loop {
        if let Some(e) = d.downloads().into_iter().find(|e| e.started_at > began) {
            break e.guid;
        }
        if stopped() {
            return ActionOutcome::failed(AFTER_STOP);
        }
        if Instant::now() >= deadline {
            return ActionOutcome::failed(format!("no download started within {}", seconds(within)));
        }
        d.idle(DOWNLOAD_POLL).await;
    };
    let entry = loop {
        let Some(e) = d.downloads().into_iter().find(|e| e.guid == guid) else {
            return ActionOutcome::failed("the download this step started is no longer followed by the browser");
        };
        match e.state {
            DownloadState::Completed => break e,
            DownloadState::Canceled => return ActionOutcome::failed(format!("the download \"{}\" was canceled", e.name)),
            DownloadState::InProgress if Instant::now() >= deadline => {
                return ActionOutcome::failed(format!(
                    "the download \"{}\" did not finish within {}",
                    e.name,
                    seconds(within)
                ))
            }
            DownloadState::InProgress if stopped() => return ActionOutcome::failed(AFTER_STOP),
            DownloadState::InProgress => d.idle(DOWNLOAD_POLL).await,
        }
    };
    let check = DownloadCheck {
        name: name.trim().to_string(),
        sheet: sheet.clone(),
        headers: headers.as_ref().map(|h| match h {
            HeadersSpec::Exact(v) => HeaderCheck::Exact(v.clone()),
            HeadersSpec::Contains(v) => HeaderCheck::Contains(v.clone()),
        }),
        cells: cells
            .iter()
            .flatten()
            .map(|c| CellCheck { r#ref: c.at.clone(), text: c.text.clone(), contains: c.how == CellMatch::Contains })
            .collect(),
        contains_text: contains_text.clone().unwrap_or_default(),
    };
    let (path, shown) = (entry.path.clone(), entry.name.clone());
    let checked = tokio::task::spawn_blocking(move || check_file(&path, &shown, &check)).await;
    match checked {
        Ok(Ok(sentence)) => ActionOutcome::passed(sentence),
        Ok(Err(sentence)) => ActionOutcome::failed(sentence),
        Err(_) => ActionOutcome::failed(format!("\"{}\" could not be read", entry.name)),
    }
}

/// The files a step saved: the downloads that started after `began`
/// (and before `until`, the next step's start, when there is one) and
/// completed, by the names they are kept under on disk - numbered when
/// their own name was taken.
pub fn saved_between(all: &[DownloadEntry], began: Instant, until: Option<Instant>) -> Vec<String> {
    all.iter()
        .filter(|e| e.state == DownloadState::Completed)
        .filter(|e| e.started_at > began && until.map_or(true, |u| e.started_at <= u))
        .filter_map(|e| e.path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect()
}

/// A sign-in and the trip back to the module that follows it, as the one
/// outcome the `sign_in` action stands for. A failed trip puts the runner's
/// sentence first: `nav::unreached_after_sign_in` reads it by position.
fn signed_then_went(signed: &SignInOutcome, went: ActionOutcome) -> ActionOutcome {
    let detail = if went.ok {
        format!("{}{}{}", signed.detail, nav::THEN, went.detail)
    } else {
        format!("{}{}{})", went.detail, nav::AFTER_SIGN_IN, signed.detail)
    };
    let mut out = if went.ok { ActionOutcome::passed(detail) } else { ActionOutcome::failed(detail) };
    out.harness = went.harness;
    out
}
