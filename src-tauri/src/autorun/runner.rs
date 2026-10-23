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
use super::popups;
use super::recipe::{self, SignInRecipe};
use super::run_files::RunFiles;
use super::signin::{self, SignInOutcome};
use super::components::{self, ComponentUse};
use super::{store, PageErrors, StepDialog, StepScript};
use crate::browser::dialogs;
use crate::browser::actions::{
    execute_in, expectation, in_missing_tab, shows_up_to_click, upload_in, Action, ActionOutcome, Policy, CANNOT_RUN, NOT_SHOWN,
    WHEN_VISIBLE_MS,
};
use super::refusals::{self, app_refused_line, refused_with_ids, RefusedRequest};
use crate::browser::cdp::{Driver, MAIN_TAB};
use crate::browser::page;
use crate::browser::page_messages::{new_messages, read_page_messages, PageMessage, MAX_MESSAGES};
use crate::browser::timing::{wall_ms, Timing, SHOT_TIMEOUT_MS};
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
    routes_from(&nav_file, &home, names)
}

/// `area_routes` from files already read.
fn routes_from(
    nav_file: &Result<nav::NavFile, String>,
    home: &Result<Option<SignInRecipe>, String>,
    names: &[&str],
) -> NamedAreas {
    names
        .iter()
        .map(|name| {
            let route = match (nav_file, home) {
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
    area_route_named(root, organization, project, area)
}

/// Said to a bare `return_to_area` in a discovery that has no area yet:
/// it goes to the discovery's own area, so one must be named.
pub const NEEDS_DISCOVERY_AREA: &str = "return_to_area in a discovery goes back to the discovery's area, and none is named yet - pass `area` with the area's name, or name the area in the action";

/// The route a discovery's bare `return_to_area` takes: the discovery's own
/// area, as `area_route` would take the case's.
pub fn discovery_area_route(root: &Path, organization: &str, project: &str, area: Option<&str>) -> Result<Route, String> {
    let area = area.map(str::trim).filter(|a| !a.is_empty()).ok_or(NEEDS_DISCOVERY_AREA)?;
    area_route_named(root, organization, project, area)
}

/// The route to the recorded area `area`, from the recipe's home.
fn area_route_named(root: &Path, organization: &str, project: &str, area: &str) -> Result<Route, String> {
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
    write_shot(root, bytes).await.ok().flatten()
}

/// Write a picture off the step loop. Its file name once written; `None`
/// when the write failed.
fn write_shot(root: &Path, bytes: Vec<u8>) -> tokio::task::JoinHandle<Option<String>> {
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || store::save_shot(&root, &bytes).ok())
}

/// A step's own picture, of the page as its step left it, on its way to
/// disk without holding up the next step. Asked of the browser at once;
/// where the browser allows it (`Driver::send_deferred`) its answer is read
/// later, and the file is always written off the step loop.
///
/// The next step's first action that changes the page settles it first
/// (`settle`), so the picture never shows that action's work; a check or
/// a read runs while it is taken. `finish` gives the file name once it is
/// on disk, or `None` when the capture failed, timed out, or the browser
/// went away: a record never names a file that is not there.
///
/// A save the guard stopped while the picture was still being taken was
/// sent before anything after its step changed the page: it is its step's
/// (`late_save`), not the next step's.
pub struct Picture {
    root: std::path::PathBuf,
    shot: Shot,
    /// A save stopped while this picture was outstanding, as the case's
    /// sentence: read as its answer was collected, or taken by the next
    /// step before that step changed the page. Charged to this picture's
    /// step by whoever holds it.
    pub late_save: Option<String>,
}

enum Shot {
    /// Asked of the browser; its answer not read yet.
    Asked { id: u64, at: Instant },
    /// Being written.
    Writing(tokio::task::JoinHandle<Option<String>>),
    /// No picture.
    Nothing,
}

impl Picture {
    /// Ask for a picture of the page as it is now. A browser that cannot
    /// answer later is asked and waited for here, as before; only its
    /// write is left to run on.
    pub async fn take<D: Driver>(d: &mut D, root: &Path) -> Picture {
        let limit = Duration::from_millis(SHOT_TIMEOUT_MS);
        let shot = match d.send_deferred(page::SCREENSHOT, page::screenshot_params()).await {
            Some(Ok(id)) => Shot::Asked { id, at: Instant::now() },
            Some(Err(_)) => Shot::Nothing,
            None => match tokio::time::timeout(limit, page::screenshot(d)).await {
                Ok(Ok(bytes)) => Shot::Writing(write_shot(root, bytes)),
                _ => Shot::Nothing,
            },
        };
        Picture { root: root.to_path_buf(), shot, late_save: None }
    }

    /// Still waiting on the browser's answer.
    pub fn is_outstanding(&self) -> bool {
        matches!(self.shot, Shot::Asked { .. })
    }

    /// Read the browser's answer, when it is still to come, within what is
    /// left of `SHOT_TIMEOUT_MS` since it was asked, and start writing it.
    /// A save read on the way is this picture's step's (`late_save`).
    pub async fn settle<D: Driver>(&mut self, d: &mut D) {
        let Shot::Asked { id, at } = self.shot else {
            return;
        };
        let left = Duration::from_millis(SHOT_TIMEOUT_MS).saturating_sub(at.elapsed());
        let bytes = d.collect(id, page::SCREENSHOT, left).await.and_then(|r| page::image_of(&r));
        if let Some(sentence) = d.take_save_blocked() {
            self.late_save.get_or_insert(sentence);
        }
        self.shot = match bytes {
            Ok(bytes) => Shot::Writing(write_shot(&self.root, bytes)),
            Err(_) => Shot::Nothing,
        };
    }

    /// The picture's file name once it is written, or `None`, and a save
    /// stopped while it was taken (`late_save`).
    pub async fn finish<D: Driver>(mut self, d: &mut D) -> (Option<String>, Option<String>) {
        self.settle(d).await;
        let file = match self.shot {
            Shot::Writing(h) => h.await.ok().flatten(),
            _ => None,
        };
        (file, self.late_save)
    }
}

/// The cheap call `step_picture` makes where the step's picture used to be
/// waited for.
pub const READ_WHAT_THE_PAGE_SENT: &str = "Target.getTargetInfo";

/// One cheap round trip to the browser, made where a picture used to be
/// waited for: the browser writes every event it sent before the call
/// ahead of the answer, so a save the page sent after a step's last action
/// is read here, as the picture's answer used to read it. Its answer, or a
/// refusal, does not matter.
async fn read_what_the_page_sent<D: Driver>(d: &mut D) {
    let limit = Duration::from_millis(SHOT_TIMEOUT_MS);
    let _ = d.call_within(READ_WHAT_THE_PAGE_SENT, serde_json::json!({}), limit).await;
}

/// A step's own picture (`Picture::take`), with what the page sent up to
/// that moment read, so `Driver::take_save_blocked` sees a save the page
/// sent after the step's last action.
pub async fn step_picture<D: Driver>(d: &mut D, root: &Path) -> Picture {
    let p = Picture::take(d, root).await;
    if p.is_outstanding() {
        read_what_the_page_sent(d).await;
    }
    p
}

/// What the rest of a step records when a save the guard stopped turned
/// out to be the step before's: the step before failed. The same sentence
/// a later step of a failed case records.
pub const AFTER_STEP_BEFORE_FAILED: &str = "not run: an earlier step of this case failed";

/// The step before's picture, while nothing of this step has changed the
/// page yet (`acted`): a save stopped now is that step's.
fn step_before<'p>(run: &'p mut InRun<'_>, acted: bool) -> Option<&'p mut Picture> {
    if acted {
        return None;
    }
    run.picture.as_deref_mut()
}

/// Whether an action may change the page, so a picture still being taken
/// of the step before is settled first. Checks and reads do not.
pub fn changes_the_page(action: &Action) -> bool {
    !matches!(
        action,
        Action::WaitFor { .. }
            | Action::CheckText { .. }
            | Action::CheckUrl { .. }
            | Action::RememberUrl { .. }
            | Action::ExpectVisible { .. }
            | Action::ExpectHidden { .. }
            | Action::ExpectText { .. }
            | Action::ExpectContainsText { .. }
            | Action::ExpectCount { .. }
            | Action::ExpectAttribute { .. }
            | Action::ExpectStyle { .. }
            | Action::ExpectEnabled { .. }
            | Action::ExpectDisabled { .. }
            | Action::ExpectChecked { .. }
            | Action::ExpectResponse { .. }
            | Action::ExpectFocused { .. }
            | Action::ExpectDownload { .. }
            | Action::ExpectTab { .. }
            | Action::ExpectTabClosed { .. }
            | Action::ExpectDialog { .. }
            | Action::ExpectRow { .. }
            | Action::ExpectNoRow { .. }
            | Action::ExpectSorted { .. }
            | Action::ExpectRowCount { .. }
            | Action::ExpectEach { .. }
            | Action::ExpectOrder { .. }
            | Action::ExpectPosition { .. }
    )
}

/// Run one step's actions in order and report every outcome.
///
/// The first action that fails stops the step: every action after it is
/// not executed and is reported as `AFTER_ACTION_FAILED` (or the sharper
/// reason its failure set - a failed `sign_in`, whose rest would run as the
/// wrong person or as nobody), so none of them waits its full timeout for
/// something the failure means cannot appear. The failure keeps its own
/// detail and picture. The outcomes list still has one entry per action,
/// in order.
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
    /// The Stop is heard before each action too, not only inside the
    /// step's waits: a replay to a step, which the person's Stop and the
    /// assistant's replay `stop` end before the step's next action.
    /// An unattended run hears its Stop between steps and in its waits.
    pub stop_before_each_action: bool,
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
    /// Learned back: each component the step used, in order, with the
    /// version it had, for its record.
    pub components: Vec<ComponentUse>,
    /// A save the guard stopped never fails the step: a discovery's mapping
    /// run counts its stopped saves instead (`Driver::take_saves_stopped`).
    /// The stopped save's sentence is still taken, so it never carries over.
    pub saves_only_counted: bool,
    /// The run's recipe and areas file, read once and again only when they
    /// changed (`RunFiles`). `None` reads them afresh for this step.
    pub files: Option<&'a RunFiles>,
    /// The step before's picture, maybe still being taken: settled before
    /// this step's first action that changes the page.
    pub picture: Option<&'a mut Picture>,
    /// Told each action as it begins and ends, for whoever asks how far
    /// the step has got (a replay's "still running" answer).
    pub progress: Option<&'a StepProgress>,
    /// A request the application refuses fails the action that sent it: a
    /// try's, where the assistant is finding out what the page does. A
    /// replay or a run only reports it - a saved case may provoke the
    /// refusal on purpose and check its message.
    pub refusals_fail: bool,
    /// The case's last action that changed the page, carried from one step
    /// to the next (`Noted`), so a step that fails can name what the page
    /// answered the step before. `None` keeps it within the step.
    pub noted: Option<&'a mut Noted>,
}

/// The case's last action that changed the page, and what the page has
/// answered it so far, carried by the caller from one step to the next:
/// a refusal or a message that comes after the action's own reads is still
/// put on it, and a later failure names it. The caller sets `step` before
/// each step, and puts what `take_late` hands back on its earlier steps'
/// records (`credit_late`).
#[derive(Debug, Default)]
pub struct Noted {
    /// Where the caller's record of the step about to run will be: its
    /// place in the case's steps. Different for every step of a case.
    pub step: usize,
    last: Option<Changed>,
    late: Vec<LateAnswer>,
    /// The case's own area (its script's), where a bare `return_to_area`
    /// takes it back to.
    home: Option<String>,
    /// The area the case is in now, as far as is known: its own, until a
    /// `return_to_area` that worked takes it to another. What a failed
    /// step teaches is kept under it (`remember_failed_step`).
    area: Option<String>,
}

impl Noted {
    /// What the page answered an action of an earlier step, found since
    /// the last take: for that step's record.
    pub fn take_late(&mut self) -> Vec<LateAnswer> {
        std::mem::take(&mut self.late)
    }

    /// `late`, as a step that failed learns from it
    /// (`remember_failed_step`): with the case's last action that changed
    /// the page, when an earlier step ran it, in place of its late answers -
    /// everything the page answered it, what its own reads found while its
    /// step passed included, as that is what the failure line names. Given
    /// once per action, so a step tried again does not count it twice.
    pub fn with_earlier(&mut self, late: &[LateAnswer]) -> Vec<LateAnswer> {
        let step = self.step;
        let Some(last) = self.last.as_mut().filter(|l| l.step != step && !l.kept) else {
            return late.to_vec();
        };
        last.kept = true;
        let mut out: Vec<LateAnswer> =
            late.iter().filter(|l| (l.step, l.action) != (last.step, last.action)).cloned().collect();
        out.push(LateAnswer {
            step: last.step,
            action: last.action,
            page_said: last.said.clone(),
            refused: last.refused.clone(),
            what: last.what.clone(),
            page_step: last.page_step.clone(),
            page_path: last.page_path.clone(),
        });
        out
    }

    /// The last action that changed the page is put nothing more on, nor
    /// named by a failure, once the browser no longer holds its mark
    /// (`Driver::net_since_held`): what came since it can no longer be told
    /// from what came before it.
    fn let_go_if_gone<D: Driver>(&mut self, d: &D) {
        if self.last.as_ref().is_some_and(|l| d.net_since_held(l.mark).is_none()) {
            self.last = None;
        }
    }

    /// A new case: nothing of the last one carries over, and its mark is
    /// let go.
    pub fn forget<D: Driver>(&mut self, d: &D) {
        if let Some(last) = self.last.take() {
            d.net_unmark(last.mark);
        }
        self.late.clear();
        self.home = None;
        self.area = None;
    }

    /// The case runs, or goes on running, in `area` (its script's): the
    /// area it is in starts there, unless a `return_to_area` of this case
    /// has already taken it elsewhere.
    pub fn in_case(&mut self, area: Option<&str>) {
        self.home = area.map(str::trim).filter(|a| !a.is_empty()).map(str::to_string);
        if self.area.is_none() {
            self.area = self.home.clone();
        }
    }

    /// The area the case is in now, when it is known.
    pub fn area(&self) -> Option<&str> {
        self.area.as_deref()
    }

    /// A `return_to_area` that worked: the case is now in the area it
    /// names, or back in its own.
    fn went_to(&mut self, named: Option<&str>) {
        self.area = named.map(str::to_string).or_else(|| self.home.clone());
    }

    /// Read once more what the page answered the last action that changed
    /// it, for a caller with no next action to give it that read (the end
    /// of a discovery call), with no wait. What is new comes back through
    /// `take_late`.
    pub async fn look_again<D: Driver>(&mut self, d: &mut D) {
        self.look(d, true).await;
    }

    /// Look for requests the application refused the last action that
    /// changed the page since its own reads - the network record only, no
    /// read of the page - for a caller deciding whether to go on (a
    /// discovery batch before its next action). What is found comes back
    /// through `take_late`.
    pub async fn look_for_refusals<D: Driver>(&mut self, d: &mut D) {
        self.look(d, false).await;
    }

    async fn look<D: Driver>(&mut self, d: &mut D, messages: bool) {
        self.let_go_if_gone(d);
        let Some(last) = self.last.as_mut() else {
            return;
        };
        let said = if last.replaces_page || !messages {
            Vec::new()
        } else {
            let now = read_page_messages(d).await;
            let said = new_messages(&last.after, &now.messages);
            last.after = now.messages;
            said
        };
        let refused = refused_with_ids(d, last.mark, &last.seen).await;
        // Numbered past its own step, so it is handed back as a late answer.
        credit_answer(last.step.wrapping_add(1), &mut [], &mut self.late, last, said, refused);
    }
}

/// One action that changed the page, as `Noted` keeps it.
#[derive(Debug)]
struct Changed {
    step: usize,
    /// Its place in its step's outcomes.
    action: usize,
    /// It in a few words (`ai_bridge::describe_action`).
    what: String,
    /// What the page showed at the last read after it.
    after: Vec<PageMessage>,
    /// Where the network record stood as it began.
    mark: u64,
    /// Requests that are never its refusals: the run's own, and the ones
    /// already reported.
    seen: Vec<String>,
    /// It ends on another page (`replaces_the_page`), so what that page
    /// shows is not the action's to report.
    replaces_page: bool,
    /// Everything reported on it so far.
    said: Vec<PageMessage>,
    refused: Vec<RefusedRequest>,
    /// The page's step indicator after it, when it has one, and its path.
    page_step: Option<String>,
    page_path: String,
    /// A later step that failed has already learned from it
    /// (`Noted::with_earlier`).
    kept: bool,
}

/// What the page answered an action of an earlier step after that step
/// ended: for the caller to put on the step's record (`credit_late`).
#[derive(Debug, Clone, PartialEq)]
pub struct LateAnswer {
    /// The step's place, as `Noted::step` gave it.
    pub step: usize,
    /// The action's place in the step's outcomes.
    pub action: usize,
    pub page_said: Vec<PageMessage>,
    pub refused: Vec<RefusedRequest>,
    /// The action in a few words (`ai_bridge::describe_action`), and the
    /// page's step indicator and path after it: for a caller that keeps no
    /// record of that step and remembers what it was refused.
    pub what: String,
    pub page_step: Option<String>,
    pub page_path: String,
}

/// Put each late answer on the outcome it belongs to, in the case's step
/// records (indexed as `Noted::step` was set).
pub fn credit_late(steps: &mut [super::StepRecord], late: Vec<LateAnswer>) {
    for l in late {
        if let Some(o) = steps.get_mut(l.step).and_then(|s| s.outcomes.get_mut(l.action)) {
            add_answer(o, l.page_said, l.refused);
        }
    }
}

/// Whether a failure is one TCM made itself rather than one the page gave:
/// a browser that stopped answering, an action not run (a Stop, a step or
/// an action before it that failed), or the no-save guard (a save it
/// stopped, a tab it could not guard). It is no evidence that a message the
/// page showed before it was a refusal.
pub fn tcm_made(o: &ActionOutcome) -> bool {
    use crate::browser::save_guard::{GUARD_START, SETUP_FAILED};
    o.harness
        || o.detail.starts_with("not run: ")
        || o.detail.starts_with(GUARD_START)
        || o.detail.starts_with(SETUP_FAILED)
}

/// What a step that failed in a replay or a run teaches of the
/// application's rules, kept in the discovery map under `area` - the area
/// the case is in (`Noted::area`) - as a run's (`discovery_map::rules_from`):
/// the requests the application refused the step's actions up to the
/// failure and the step before's (`late`: what `Noted::take_late` handed
/// back, passed through `Noted::with_earlier` so the earlier action the
/// failure line names is there whole), and the messages the page showed
/// after the nearest action before the failure that changed the page. A
/// failure TCM made itself (`tcm_made`) is no evidence for a message. Each
/// rule is filed under the page the action's own read after it was on. A
/// step that passed, or no area, keeps nothing; a map that could not be
/// written is only logged.
#[allow(clippy::too_many_arguments)]
pub fn remember_failed_step(
    root: &Path,
    organization: &str,
    project: &str,
    step: &StepScript,
    outcomes: &[ActionOutcome],
    late: &[LateAnswer],
    area: Option<&str>,
    case_id: i32,
) {
    use super::discovery_map::{remember_refusals_logged, rules_from, ActionAnswer, RuleSource};
    let Some(failed) = outcomes.iter().position(|o| !o.ok) else {
        return;
    };
    if area.is_none_or(|a| a.trim().is_empty()) || project.trim().is_empty() {
        return;
    }
    // The outcomes are the expanded actions': a component's own.
    let actions: Vec<Action> = match components::expand_step(root, organization, project, &step.actions) {
        Ok((expanded, _)) => expanded.into_iter().map(|(a, _)| a).collect(),
        Err(_) => step.actions.clone(),
    };
    // One answer per earlier action, however many times it came late.
    let mut answers: Vec<ActionAnswer> = Vec::new();
    let mut whose: Vec<(usize, usize)> = Vec::new();
    for l in late {
        let at = match whose.iter().position(|&w| w == (l.step, l.action)) {
            Some(at) => at,
            None => {
                whose.push((l.step, l.action));
                answers.push(ActionAnswer {
                    page: l.page_path.clone(),
                    step: l.page_step.clone(),
                    after: l.what.clone(),
                    changed: true,
                    ..Default::default()
                });
                answers.len() - 1
            }
        };
        answers[at].said.extend(l.page_said.iter().cloned());
        answers[at].refused.extend(l.refused.iter().cloned());
    }
    answers.extend(outcomes[..=failed].iter().enumerate().map(|(i, o)| ActionAnswer {
        page: o.page_path.clone().unwrap_or_default(),
        step: o.page_step.clone(),
        after: actions.get(i).map(crate::ai_bridge::describe_action).unwrap_or_default(),
        said: o.page_said.clone(),
        refused: o.refused.clone(),
        changed: o.page_path.is_some(),
        failed: i == failed && !tcm_made(o),
    }));
    let rules = rules_from(&answers, RuleSource::Run, Some(case_id), super::sessions::now_ms());
    remember_refusals_logged(root, organization, project, area, rules);
}

/// Add what the page answered an action to its outcome.
fn add_answer(o: &mut ActionOutcome, said: Vec<PageMessage>, refused: Vec<RefusedRequest>) {
    o.page_said.extend(said);
    o.page_said.truncate(MAX_MESSAGES);
    o.refused.extend(refused);
}

/// Put what the page answered `to` after its own reads on it: through
/// `out` when it is of the step running (`step`), through `late` for the
/// caller when it is of an earlier one.
fn credit_answer(
    step: usize,
    out: &mut [ActionOutcome],
    late: &mut Vec<LateAnswer>,
    to: &mut Changed,
    said: Vec<PageMessage>,
    refused: Vec<(String, RefusedRequest)>,
) {
    if said.is_empty() && refused.is_empty() {
        return;
    }
    let (ids, refused): (Vec<String>, Vec<RefusedRequest>) = refused.into_iter().unzip();
    to.seen.extend(ids);
    to.said.extend(said.iter().cloned());
    to.said.truncate(MAX_MESSAGES);
    to.refused.extend(refused.iter().cloned());
    if to.step == step {
        if let Some(o) = out.get_mut(to.action) {
            add_answer(o, said, refused);
        }
    } else {
        late.push(LateAnswer {
            step: to.step,
            action: to.action,
            page_said: said,
            refused,
            what: to.what.clone(),
            page_step: to.page_step.clone(),
            page_path: to.page_path.clone(),
        });
    }
}

/// The longest an action that changed the page waits for the answers to
/// the requests it sent the page's own server, before what the page says
/// is read: a refusal is only one once it is answered.
pub const ANSWER_WAIT_MS: u64 = 3000;

/// How often that wait looks again.
const ANSWER_LOOK_MS: u64 = 50;

/// Wait, at most `ANSWER_WAIT_MS`, until every request the action started
/// (since `mark`) that could be refused (`refusals::awaits_answer`) has its
/// answer. An action that sent none waits nothing; a Stop ends the wait.
async fn await_answers<D: Driver>(d: &mut D, mark: u64, own: &[String], cancel: Option<&AtomicBool>) {
    let mut waited = 0;
    while waited < ANSWER_WAIT_MS
        && !cancel.is_some_and(|c| c.load(Ordering::SeqCst))
        && d.net_since_held(mark).is_some_and(|sent| sent.iter().any(|e| refusals::awaits_answer(e, own)))
    {
        d.idle(Duration::from_millis(ANSWER_LOOK_MS)).await;
        waited += ANSWER_LOOK_MS;
    }
}

/// An action whose page is another once it is done: a navigation, a
/// reload, a trip to an area (`return_to_area`), or one that moves to or
/// opens a tab - itself or one a `when_visible` guards. What that page
/// shows (a banner it always has) is not new because of the action. An
/// `expire_session` only forgets the site's cookies, and its page stays.
fn replaces_the_page(action: &Action) -> bool {
    action.each().into_iter().any(|a| {
        matches!(a, Action::Navigate { .. } | Action::Reload | Action::ReturnToArea { .. }) || a.is_tab_action()
    })
}

/// The note a dialog nobody expected left on a passing action's words
/// (` (a confirm dialog was accepted: "...")`), or nothing.
fn dialog_note(detail: &str) -> &str {
    match detail.find(dialogs::WAS_ACCEPTED).and_then(|w| detail[..w].rfind(" (a")) {
        Some(i) => &detail[i..],
        None => "",
    }
}

/// The action a step is on, as `StepProgress` keeps it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionNow {
    /// The action's place in the step, from 1; 0 before the first. A
    /// component's actions count one by one, as they run.
    pub index: usize,
    /// How many actions the step runs, components expanded.
    pub of: usize,
    pub kind: String,
    /// What it acts on, in short (`Target::describe`), or blank.
    pub target: String,
    /// When it began.
    pub since: Option<Instant>,
    /// 1 once an action of the step has failed (a wait that ran out of
    /// time included), else 0: a failed action stops its step, so no later
    /// action of it begins, the replay ends at that step, and nothing is
    /// tried again. Never more than 1.
    pub failed: u32,
    /// The action begun last was ended by the Stop while it ran (inside its
    /// own wait), not after it finished.
    pub stopped_inside: bool,
}

/// The longest target `ActionNow` keeps.
const SHORT_TARGET: usize = 80;

/// Where a step is, action by action: set by the runner's step loop
/// (`InRun::progress`), read by anyone, never held across an await.
pub struct StepProgress(std::sync::Mutex<ActionNow>);

impl StepProgress {
    pub const fn new() -> Self {
        StepProgress(std::sync::Mutex::new(ActionNow {
            index: 0,
            of: 0,
            kind: String::new(),
            target: String::new(),
            since: None,
            failed: 0,
            stopped_inside: false,
        }))
    }

    fn held(&self) -> std::sync::MutexGuard<'_, ActionNow> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A new step, nothing run of it yet.
    pub fn reset(&self) {
        *self.held() = ActionNow::default();
    }

    /// Action `index` (from 1) of `of` begins now.
    pub fn began(&self, index: usize, of: usize, action: &Action) {
        let target: String = action.targets().first().map(|t| t.describe()).unwrap_or_default();
        let target = if target.chars().count() > SHORT_TARGET {
            format!("{}...", target.chars().take(SHORT_TARGET).collect::<String>())
        } else {
            target
        };
        let mut now = self.held();
        now.index = index;
        now.of = of;
        now.kind = action.kind();
        now.target = target;
        now.since = Some(Instant::now());
    }

    /// The action begun last ended, passing or not.
    /// The action begun last was ended by the Stop while it ran.
    pub fn stopped_inside(&self) {
        self.held().stopped_inside = true;
    }

    pub fn ended(&self, ok: bool) {
        if !ok {
            self.held().failed += 1;
        }
    }

    pub fn now(&self) -> ActionNow {
        self.held().clone()
    }
}

impl Default for StepProgress {
    fn default() -> Self {
        Self::new()
    }
}

/// The longest an `expect_download` waits in a watched run or a try, which
/// has no Stop to end it: the person, or the assistant, is waiting on the
/// answer, and a step must never hold the browser for two minutes unasked.
pub const WATCHED_DOWNLOAD_WAIT_MS: u32 = 30_000;

/// What a line about the page's new messages starts with.
pub const PAGE_SHOWED: &str = "the page showed: ";

/// How a failed action's sentence goes on to name an earlier action of its
/// step and what the page answered it: ` - after <action> <line>`.
pub const AFTER_EARLIER: &str = " - after ";

/// The messages a page newly showed, each quoted: `the page showed: "a"; "b"`.
pub fn page_showed_note(messages: &[PageMessage]) -> String {
    let quoted: Vec<String> = messages.iter().map(|m| format!("\"{}\"", m.text)).collect();
    format!("{PAGE_SHOWED}{}", quoted.join("; "))
}

/// What the page answered an action, as a line: the first request the
/// application refused, with what the page showed after it, or else what
/// the page showed. `None` when it answered neither.
fn answered_line(o: &ActionOutcome) -> Option<String> {
    line_of(&o.page_said, &o.refused)
}

/// `answered_line` of what was said and refused.
fn line_of(said: &[PageMessage], refused: &[RefusedRequest]) -> Option<String> {
    let said = (!said.is_empty()).then(|| page_showed_note(said));
    match (refused.first().map(app_refused_line), said) {
        (Some(refused), Some(said)) => Some(format!("{refused}; {said}")),
        (refused, said) => refused.or(said),
    }
}

/// Every action of a step after one that failed: it is never carried out,
/// so it never waits its full timeout for what cannot appear now.
pub const AFTER_ACTION_FAILED: &str = "not run: an earlier action in this step failed";

/// Why the rest of a step must not run after `outcome`, when it must not:
/// any failed action stops its step, whatever its kind - a `when_visible`
/// whose target never showed passed, so it does not. The one place every
/// run, watched step, try and replay decides it, since they all run their
/// steps through `run_expanded`; a reason an action already set (a failed
/// `sign_in`, a Stop, a refused session) is kept in its place.
fn stops_the_step(outcome: &ActionOutcome) -> Option<&'static str> {
    (!outcome.ok).then_some(AFTER_ACTION_FAILED)
}

/// What an action interrupted by the run's Stop, and every action after it
/// in the step, records: what a stopped case records for what it never
/// ran.
pub const AFTER_STOP: &str = "not run: the run was stopped";

/// Every other action of a step one of whose components could not be used.
pub const AFTER_COMPONENT: &str = "not run: a component in this step could not be run";

/// `run_step_routed` inside an unattended run: the run's Stop reaches the
/// step's waits through `run`, and the step says when it began.
///
/// Every `use_component` is expanded first (`components::expand_step`):
/// its component's actions run in its place, each outcome naming the
/// component, and `run.components` says which versions ran. One that
/// cannot be expanded fails the step with its sentence before any action
/// runs. An expanded action is carried out exactly as a written one, so
/// what a `fill` types is kept out of its outcome the same way.
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
    run.components = Vec::new();
    let (expanded, uses, places) = match components::expand_step_placed(root, organization, project, &step.actions) {
        Ok(x) => x,
        Err((at, why)) => {
            run.began = None;
            run.tab = None;
            run.dialog = None;
            run.page_errors_seen = 0;
            // The sentence on the use that could not be expanded; every
            // other action of the step is not run.
            let out = (0..step.actions.len())
                .map(|i| ActionOutcome::failed(if i == at { why.as_str() } else { AFTER_COMPONENT }))
                .collect();
            return Ok(out);
        }
    };
    if uses.is_empty() {
        let areas = run.areas;
        return run_expanded(d, root, organization, project, step, None, timing, account, lease, route, area, areas, run).await;
    }
    let (actions, from): (Vec<Action>, Vec<Option<String>>) = expanded.into_iter().unzip();
    // An area a component's `return_to_area` names, which the caller did
    // not read with the step's own.
    let missing: Vec<&str> = named_areas(&actions)
        .into_iter()
        .filter(|name| !run.areas.is_some_and(|m| m.contains_key(&nav::module_key(name))))
        .collect();
    let merged: Option<NamedAreas> = (!missing.is_empty()).then(|| {
        let mut all = run.areas.cloned().unwrap_or_default();
        all.extend(match run.files {
            Some(f) => routes_from(&f.nav(root, organization, project), &f.recipe(root, organization, project), &missing),
            None => area_routes(root, organization, project, &missing),
        });
        all
    });
    let areas = merged.as_ref().or(run.areas);
    let expanded_step = StepScript { step_number: step.step_number, actions, unchecked: step.unchecked.clone(), cleanup: step.cleanup, because: Vec::new() };
    run.components = uses;
    // The actions as written, and where each expanded action came from: a sentence
    // that counts the step's actions counts the script's, not the expansion's.
    let written = Some((step.actions.as_slice(), places.as_slice()));
    let mut out = run_expanded(
        d, root, organization, project, &expanded_step, written, timing, account, lease, route, area, areas, run,
    )
    .await?;
    for (o, c) in out.iter_mut().zip(from) {
        o.component = c;
    }
    Ok(out)
}

/// A step whose components are already expanded, run with `areas` for its
/// `return_to_area` actions that name one. `written`, when components were
/// expanded: the step's actions as written, and for each expanded action the
/// index of the written one it came from.
#[allow(clippy::too_many_arguments)]
async fn run_expanded<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    step: &StepScript,
    written: Option<(&[Action], &[usize])>,
    timing: &Timing,
    account: &mut Option<String>,
    lease: &mut Held,
    route: Option<&Route>,
    area: AreaRoute<'_>,
    areas: Option<&NamedAreas>,
    run: &mut InRun<'_>,
) -> Result<Vec<ActionOutcome>, String> {
    // No recipe to run (none saved, no site address): navigation is open,
    // as before the built-in existed; the sign-in itself is what refuses.
    // Looked at per step, the recipe and the areas file alike: a person may
    // flip the switch between two steps of a supervised run. Within a run
    // (`run.files`) a file is read again only when it changed.
    let recipe = match run.files {
        Some(f) => f.recipe(root, organization, project)?,
        None => recipe::load_effective_recipe_if_any(root, organization, project)?,
    };
    let policy = policy_for(recipe.as_ref());
    let nav_file = match run.files {
        Some(f) => f.nav(root, organization, project)?,
        None => nav::load_nav(root, organization, project)?,
    };
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
    let saves_fail = !run.saves_only_counted;
    let mut deferred: Vec<(usize, u32)> = Vec::new();
    let mark = d.net_mark();
    // Every `expect_response` of the step shares one book, armed from this
    // mark: what an earlier check took, and how many requests the step had
    // made as each action began.
    // The kinds are those of the actions as written (`written`), so "after action 3"
    // names the script's third action.
    let mut requests: Option<api_checks::StepRequests> = step.actions.iter().any(holds_expect_response).then(|| {
        let shown = written.map_or(step.actions.as_slice(), |(actions, _)| actions);
        api_checks::StepRequests::new(mark, shown.iter().map(super::patterns::action_kind).collect())
    });
    // A check that asks `same_document` compares the top frame's document with the one it showed now.
    if step.actions.iter().any(|a| matches!(a, Action::ExpectResponse { same_document: true, .. })) {
        if let Some(book) = requests.as_mut() {
            book.document = api_checks::main_document(d).await;
        }
    }
    let began = Instant::now();
    run.began = Some(began);
    run.tab = None;
    run.dialog = None;
    let mut ran_in: Option<String> = None;
    let popups = recipe.as_ref().map(|r| r.close_when_shown.as_slice()).unwrap_or(&[]);
    let loaders = recipe.as_ref().map(|r| r.wait_while_shown.as_slice()).unwrap_or(&[]);
    let here = Here {
        root,
        organization,
        project,
        policy: &policy,
        direct_urls: nav_file.direct_urls,
        step: step.step_number,
        popups,
        loaders,
        cancel: run.cancel,
        stop_each: run.stop_before_each_action,
        stopped_between: AtomicBool::new(false),
    };
    let mut out = Vec::with_capacity(step.actions.len());
    let mut blocked: Option<&'static str> = None;
    // Whether an action of this step that changes the page has begun.
    // Until then a save the guard stopped was sent while the step before's
    // picture was still being taken: it is that step's, not this one's.
    let mut acted = false;
    // The case's last action that changed the page (`Noted`): the caller's,
    // carried across its steps, or this step's own. The next such action's
    // first read is that action's late read: a message the page took a
    // moment to show is that action's, found with no wait of its own.
    let mut carried = run.noted.take();
    let mut own_noted = Noted::default();
    let noted: &mut Noted = match carried.as_deref_mut() {
        Some(n) => n,
        None => &mut own_noted,
    };
    for (index, action) in step.actions.iter().enumerate() {
        if let Some(book) = requests.as_mut() {
            // One note per written action: a component's actions after the first
            // belong to the same one.
            let begins = written.is_none_or(|(_, places)| index == 0 || places.get(index) != places.get(index - 1));
            if begins {
                book.action_began(d.net_since(mark).len());
            }
        }
        if let Some(why) = blocked {
            out.push(ActionOutcome::failed(why));
            continue;
        }
        // A Stop asked while the action before ran: this one and the rest
        // of the step are not carried out.
        if run.stop_before_each_action && run.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            out.push(ActionOutcome::failed(AFTER_STOP));
            blocked = Some(AFTER_STOP);
            continue;
        }
        // A save stopped before this action began - while the page loaded,
        // or between two steps - fails the step here, before it acts.
        if let Some(sentence) = d.take_save_blocked().filter(|_| saves_fail) {
            match step_before(run, acted) {
                Some(p) => {
                    p.late_save.get_or_insert(sentence);
                    out.push(ActionOutcome::failed(AFTER_STEP_BEFORE_FAILED));
                    blocked = Some(AFTER_STEP_BEFORE_FAILED);
                }
                None => {
                    let mut stopped = ActionOutcome::failed(sentence);
                    stopped.screenshot = picture(d, root).await;
                    out.push(stopped);
                    blocked = Some(AFTER_SAVE_BLOCKED);
                }
            }
            continue;
        }
        // The step before's picture shows the page as that step left it. A
        // save read while it was settled fails that step, and this one
        // stops before it acts.
        if changes_the_page(action) && !acted {
            acted = true;
            if let Some(p) = run.picture.as_deref_mut() {
                p.settle(d).await;
                if p.late_save.is_some() {
                    out.push(ActionOutcome::failed(AFTER_STEP_BEFORE_FAILED));
                    blocked = Some(AFTER_STEP_BEFORE_FAILED);
                    continue;
                }
            }
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
        if let Some(p) = run.progress {
            p.began(index + 1, step.actions.len(), action);
        }
        // What the page shows before an action that changes it, and where
        // the network record stands, so what is new after it is the
        // action's. A sign-in is left out: its own trip settles a session
        // the application refused, and its page is the sign-in's.
        let before = if changes_the_page(action) && !matches!(action, Action::SignIn { .. }) {
            let read = read_page_messages(d).await;
            noted.let_go_if_gone(d);
            if let Some(last) = noted.last.as_mut() {
                let said = if last.replaces_page { Vec::new() } else { new_messages(&last.after, &read.messages) };
                let refused = refused_with_ids(d, last.mark, &last.seen).await;
                last.after = read.messages.clone();
                credit_answer(noted.step, &mut out, &mut noted.late, last, said, refused);
            }
            Some((read.messages, d.net_mark()))
        } else {
            None
        };
        // The step's own `api_request` fetches: never the page's doing.
        let mut own: Vec<String> = Vec::new();
        let began_ms = wall_ms();
        let mut outcome = match action {
            Action::SignIn { account: key } => match signin::prepare(root, organization, project, key) {
                Err(why) => {
                    *account = None;
                    lease.forget_sign_in();
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
                        let print = match signed.ok {
                            true => crate::browser::session::cookie_print(d, &r.origins()).await,
                            false => None,
                        };
                        lease.note_sign_in(print);
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
            Action::ReturnToArea { .. } => {
                let (went, stop) = return_to_area(d, action, &area, areas, step.step_number, timing).await;
                blocked = stop;
                went
            }
            // Only the runner knows where the step began.
            Action::ExpectResponse { .. } => {
                let book = requests.get_or_insert_with(|| api_checks::StepRequests::new(mark, Vec::new()));
                api_checks::expect_response(d, action, book, timing).await
            }
            Action::ApiRequest { path, .. } => {
                // Its fetch is the run's own request: a 5xx it gets back is
                // never the page's error.
                let before = d.net_mark();
                let outcome = api_checks::api_request(d, action, timing).await;
                let sent = d.net_since(before);
                d.net_unmark(before);
                own = sent
                    .into_iter()
                    .filter(|e| e.method == "GET" && e.path_query.split(['?', '#']).next() == Some(path.as_str()))
                    .map(|e| e.id)
                    .collect();
                if let Some(book) = d.page_error_book() {
                    book.exclude(own.clone());
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
            // A guard's own look waits for the page to load first: under a
            // spinner its target would read as not there.
            Action::WhenVisible { .. } | Action::RepeatWhileVisible { .. } => {
                match page_loaded(d, &here, action, timing).await {
                    Err((outcome, stop)) => {
                        blocked = stop;
                        outcome
                    }
                    Ok(notes) => {
                        let (mut outcome, stop) = match action {
                            Action::WhenVisible { .. } => when_visible(d, &here, action, &area, areas, timing).await,
                            _ => repeat_while_visible(d, &here, action, &area, areas, timing).await,
                        };
                        popups::add_notes(&mut outcome, &notes);
                        blocked = stop;
                        outcome
                    }
                }
            }
            // The session ended on purpose: nobody is signed in here now,
            // so a replay after it signs in again rather than trusting it.
            Action::ExpireSession => {
                let (outcome, stop) = plain(d, &here, action, timing).await;
                *account = None;
                lease.forget_sign_in();
                blocked = stop;
                outcome
            }
            other => {
                let (outcome, stop) = plain(d, &here, other, timing).await;
                blocked = stop;
                outcome
            }
        };
        // A timed `expect_response` later in the step measures from the action that sent the
        // request: when it began, and when its act was done. An action that sent input is done when the
        // browser answered the last of it; one that sent none (a script-set `fill`, a `navigate`, a guard
        // that did not fire) is done when it finished. Neither moment includes a highlight or a wait for
        // the element, which come before the act.
        if api_checks::times_from(action) {
            if let Some(book) = requests.as_mut() {
                let finished_ms = wall_ms();
                let at_ms = d.last_input_ms().filter(|t| *t >= began_ms).unwrap_or(finished_ms);
                book.note_action_done(began_ms, at_ms);
            }
        }
        // A save the page tried while this action ran is the step's
        // failure, whatever the action itself made of the page - unless
        // nothing of this step has changed the page yet: then it is the
        // step before's, and the rest of this step does not run.
        if let Some(sentence) = d.take_save_blocked().filter(|_| saves_fail) {
            match step_before(run, acted) {
                Some(p) => {
                    p.late_save.get_or_insert(sentence);
                    blocked = Some(AFTER_STEP_BEFORE_FAILED);
                }
                None => {
                    outcome = ActionOutcome::failed(sentence);
                    blocked = Some(AFTER_SAVE_BLOCKED);
                }
            }
        }
        unexpected_dialog(d, &mut dialogs_read, fail_on_unexpected, &mut outcome);
        // The application refused the session the browser holds ("Cookie
        // has been tampered", an empty 400): the account is signed in fresh
        // so its saved session is repaired, and the action fails saying so,
        // with the rest of the step and the case stopped. The action is never
        // tried again: what the earlier actions built is gone. Not looked at
        // once the step is already stopped (a save the guard caught
        // included), nor with a Stop pending.
        let stop_pending = run.cancel.is_some_and(|c| c.load(Ordering::SeqCst));
        if !outcome.ok && !outcome.harness && outcome.detail != AFTER_STOP && blocked.is_none() && !stop_pending {
            if let Some(key) = account.clone() {
                if signin::session_refused(d).await.is_some() {
                    outcome = after_session_refused(d, &here, &key, step.step_number, timing, account, lease).await;
                    blocked = Some(AFTER_SESSION_REFUSED);
                }
            }
        }
        // What the page newly shows, and the requests the application
        // refused, since the action began. Not for a browser that stopped
        // answering, a Stop, or a page signed in afresh: none of those is
        // the action's page.
        let fresh_session = blocked == Some(AFTER_SESSION_REFUSED);
        let readable = !outcome.harness && outcome.detail != AFTER_STOP && !stop_pending && !fresh_session;
        let mut refused_here = false;
        let mut mine: Option<Changed> = None;
        if let Some((before, action_mark)) = before {
            if readable {
                // A refusal is one once it is answered: the requests the
                // action sent are given their answers first.
                await_answers(d, action_mark, &own, run.cancel).await;
                let after = read_page_messages(d).await;
                let replaces = replaces_the_page(action);
                if !replaces {
                    outcome.page_said = new_messages(&before, &after.messages);
                }
                let refused = refused_with_ids(d, action_mark, &own).await;
                outcome.refused = refused.iter().map(|(_, r)| r.clone()).collect();
                outcome.page_step = after.step.clone();
                outcome.page_path = Some(after.path.clone().unwrap_or_default());
                // A try fails on it, like a target that is not there. What a
                // dialog nobody expected left on its words stays.
                if run.refusals_fail && outcome.ok && !outcome.refused.is_empty() {
                    let dialog = dialog_note(&outcome.detail).to_string();
                    outcome.ok = false;
                    outcome.detail = format!("{}{dialog}", answered_line(&outcome).unwrap_or_default());
                    refused_here = true;
                }
                let mut seen = own.clone();
                seen.extend(refused.into_iter().map(|(id, _)| id));
                mine = Some(Changed {
                    step: noted.step,
                    action: out.len(),
                    what: crate::ai_bridge::describe_action(action),
                    after: after.messages,
                    mark: action_mark,
                    seen,
                    replaces_page: replaces,
                    said: outcome.page_said.clone(),
                    refused: outcome.refused.clone(),
                    page_step: after.step,
                    page_path: after.path.unwrap_or_default(),
                    kept: false,
                });
            } else {
                d.net_unmark(action_mark);
            }
        }
        // The step fails here for a reason of its own. The last action
        // before this one that changed the page - in this step or the one
        // before - is looked at once more, as what it caused may have come
        // after its own reads; and what the page answered it is said with
        // the failure, as the likelier cause than what the step ran into.
        if !outcome.ok && !refused_here && readable {
            noted.let_go_if_gone(d);
            if let Some(last) = noted.last.as_mut() {
                // Messages new since this action began are this action's.
                let said = if mine.is_some() || last.replaces_page {
                    Vec::new()
                } else {
                    let now = read_page_messages(d).await;
                    let said = new_messages(&last.after, &now.messages);
                    last.after = now.messages;
                    said
                };
                let mut skip = last.seen.clone();
                if let Some(m) = &mine {
                    skip.extend(m.seen.iter().cloned());
                }
                let refused = refused_with_ids(d, last.mark, &skip).await;
                credit_answer(noted.step, &mut out, &mut noted.late, last, said, refused);
            }
            let in_step = out.iter().enumerate().rev().find_map(|(i, o)| {
                answered_line(o).map(|line| (step.actions.get(i).map(crate::ai_bridge::describe_action).unwrap_or_default(), line))
            });
            let before_step = || {
                let last = noted.last.as_ref().filter(|l| l.step != noted.step)?;
                line_of(&last.said, &last.refused).map(|line| (last.what.clone(), line))
            };
            if let Some((what, line)) = in_step.or_else(before_step) {
                outcome.detail = format!("{}{AFTER_EARLIER}{what} {line}", outcome.detail);
            }
        }
        // This action is now the last that changed the page; the one
        // before it is not asked of again.
        if let Some(m) = mine {
            if let Some(old) = noted.last.replace(m) {
                d.net_unmark(old.mark);
            }
        }
        // A guard that heard the Stop between its own actions was cut
        // there, not inside a wait.
        let between = here.stopped_between.swap(false, Ordering::SeqCst);
        if let Some(p) = run.progress {
            // A Stop heard inside the action is no failure of it.
            if outcome.detail == AFTER_STOP {
                if !between {
                    p.stopped_inside();
                }
            } else {
                p.ended(outcome.ok);
            }
        }
        // A Stop is no failure to picture.
        if !outcome.ok && !outcome.harness && outcome.detail != AFTER_STOP {
            outcome.screenshot = picture(d, root).await;
        }
        blocked = blocked.or(stops_the_step(&outcome));
        // The area the case is in from here, for what a failure teaches.
        if outcome.ok && matches!(action, Action::ReturnToArea { .. }) {
            noted.went_to(action.area_named());
        }
        out.push(outcome);
    }
    // Judging a dialog answers it, which changes the page.
    if !deferred.is_empty() && !acted {
        if let Some(p) = run.picture.as_deref_mut() {
            p.settle(d).await;
            if p.late_save.is_some() {
                blocked = blocked.or(Some(AFTER_STEP_BEFORE_FAILED));
            }
        }
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
    // One that opened after the step's last look: still this step's. What
    // the page answered the action stays on it.
    if let Some(last) = out.last_mut() {
        if last.ok {
            if let Some(s) = seen.iter().skip(dialogs_read).find(|s| s.claimed_by.is_none()) {
                if fail_on_unexpected {
                    let mut failed = ActionOutcome::failed(dialogs::unexpected(&s.kind, &s.message));
                    failed.page_said = std::mem::take(&mut last.page_said);
                    failed.refused = std::mem::take(&mut last.refused);
                    failed.page_step = last.page_step.take();
                    failed.page_path = last.page_path.take();
                    *last = failed;
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
    super::page_errors::read_trace_ids(d, run.page_errors, &run.ignore_page_errors).await;
    let judged = super::page_errors::judge_step(d, run.page_errors, &run.ignore_page_errors, &mut out);
    run.page_errors_seen = judged.seen;
    if let Some(i) = judged.failed {
        out[i].screenshot = picture(d, root).await;
    }
    let ended_in = d.tab_name();
    run.tab = if ended_in != MAIN_TAB { Some(ended_in) } else { ran_in };
    // The step's own mark is done with, and so is the last action's when
    // nothing carries it on to the next step.
    d.net_unmark(mark);
    own_noted.forget(d);
    run.noted = carried;
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
    /// The recipe's `close_when_shown`: closed before an action that
    /// touches the page (`popups::close_shown`).
    popups: &'a [super::recipe::ClosePopup],
    /// The recipe's `wait_while_shown`: waited out before an action that
    /// touches or checks the page (`loading::wait_out`).
    loaders: &'a [super::recipe::LoadingIndicator],
    /// The run's Stop, heard while a loading indicator is waited out.
    cancel: Option<&'a AtomicBool>,
    /// `InRun::stop_before_each_action`: the Stop is heard before each
    /// action a guard runs too (`stop_heard`).
    stop_each: bool,
    /// Set when a guard heard the Stop between its own actions or passes
    /// (`stop_heard`): its outcome is the Stop, but nothing was cut inside
    /// a wait, so the replay says it stopped after that action.
    stopped_between: AtomicBool,
}

impl Here<'_> {
    /// A replay's Stop, asked for: heard between a `when_visible`'s `then`
    /// actions and between a `repeat_while_visible`'s passes, as between a
    /// step's own actions.
    fn stop_heard(&self) -> bool {
        let heard = self.stop_each && self.cancel.is_some_and(|c| c.load(Ordering::SeqCst));
        if heard {
            self.stopped_between.store(true, Ordering::SeqCst);
        }
        heard
    }
}

/// Said by the actions after one the application refused the session for.
pub const AFTER_SESSION_REFUSED: &str = "not run: the site refused the session the browser held";

/// The sentence for an action the application refused the session for:
/// the account (by key, never a login) was signed in fresh, and the case
/// runs again from the start.
pub fn session_refused_mid_case(account_key: &str) -> String {
    format!("the site refused the saved session; {account_key} was signed in fresh - run the case again")
}

/// An action failed on a page that says the application no longer accepts
/// the session (`signin::session_refused`). The account's saved session is
/// dropped and the account signed in fresh, its lease taken first as every
/// sign-in takes it, so the next run finds a good session; the action
/// itself fails with `session_refused_mid_case`. Nothing is tried again.
/// A fresh sign-in the application refuses as well says so
/// (`signin::session_refused_again`).
async fn after_session_refused<D: Driver>(
    d: &mut D,
    here: &Here<'_>,
    key: &str,
    step_number: i32,
    timing: &Timing,
    account: &mut Option<String>,
    lease: &mut Held,
) -> ActionOutcome {
    super::sessions::forget_session(here.root, key);
    crate::applog::info(format!(
        "Auto Run, step {step_number}: the site refused the session for {key}; it is signed in fresh"
    ));
    let (r, who) = match signin::prepare(here.root, here.organization, here.project, key) {
        Ok(found) => found,
        Err(why) => {
            *account = None;
            lease.forget_sign_in();
            return ActionOutcome::failed(why);
        }
    };
    if let Err(why) = lease.hold(here.root, &who.key).await {
        *account = None;
        lease.forget_sign_in();
        return ActionOutcome::failed(why);
    }
    let signed = signin::sign_in_fresh(d, here.root, &r, &who, timing).await;
    if !signed.ok {
        *account = None;
        lease.forget_sign_in();
        return as_action_outcome(&signed);
    }
    lease.note_sign_in(crate::browser::session::cookie_print(d, &r.origins()).await);
    ActionOutcome::failed(session_refused_mid_case(key))
}

/// Before `action`, the project's loading indicators waited out
/// (`loading::wait_out`): the notes to add to its outcome, or the outcome
/// it fails with - and why the rest of the step must not run - when the
/// page was still loading at the indicator's `max_ms` or the run was
/// stopped meanwhile.
async fn page_loaded<D: Driver>(
    d: &mut D,
    here: &Here<'_>,
    action: &Action,
    timing: &Timing,
) -> Result<Vec<String>, (ActionOutcome, Option<&'static str>)> {
    use super::loading::NotReady;
    match super::loading::wait_out(d, here.loaders, action, timing, here.cancel).await {
        Ok(notes) => Ok(notes),
        Err(NotReady::StillLoading(why)) => Err((ActionOutcome::failed(why), None)),
        Err(NotReady::Stopped) => Err((ActionOutcome::failed(AFTER_STOP), Some(AFTER_STOP))),
    }
}

/// Is this an `expect_response`, or does it hold one in its `then`?
fn holds_expect_response(a: &Action) -> bool {
    match a {
        Action::ExpectResponse { .. } => true,
        Action::WhenVisible { then, .. } | Action::RepeatWhileVisible { then, .. } => then.iter().any(holds_expect_response),
        _ => false,
    }
}

/// A plain action - one a `when_visible` may guard - carried out, and why
/// the rest of the step must not run, when it must not. The one place every
/// run, try and replay carries out a plain action, so the project's loading
/// indicators are waited out here, first (`page_loaded`), and then its
/// known pop-ups closed, when one is up (`popups::close_shown`).
async fn plain<D: Driver>(
    d: &mut D,
    here: &Here<'_>,
    action: &Action,
    timing: &Timing,
) -> (ActionOutcome, Option<&'static str>) {
    // A script saved before the switch was turned off. The runner's own
    // trip home never comes through here.
    if matches!(action, Action::Navigate { .. } | Action::OpenTab { .. }) && !here.direct_urls {
        return (ActionOutcome::failed(nav::no_address(here.step)), Some(AFTER_REFUSED_ADDRESS));
    }
    let mut notes = match page_loaded(d, here, action, timing).await {
        Ok(notes) => notes,
        Err(failed) => return failed,
    };
    notes.extend(popups::close_shown(d, here.popups, action, timing, here.policy).await);
    let mut out = match action {
        Action::Upload { selector, file } => {
            upload(d, here.root, here.organization, here.project, action, selector, file, timing).await
        }
        other => execute_in(d, other, timing, here.policy).await,
    };
    popups::add_notes(&mut out, &notes);
    (out, None)
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
    area: &AreaRoute<'_>,
    areas: Option<&NamedAreas>,
    timing: &Timing,
) -> (ActionOutcome, Option<&'static str>) {
    if let Err(why) = action.validate() {
        return (ActionOutcome::failed(format!("{CANNOT_RUN}{why}")), None);
    }
    let Action::WhenVisible { selector, within_ms, then } = action else {
        return (ActionOutcome::failed(format!("{CANNOT_RUN}this is not a when_visible")), None);
    };
    let target = selector.describe();
    match shows_up_to_click(d, selector, within_ms.unwrap_or(WHEN_VISIBLE_MS), timing).await {
        Err(silent) => return (silent, None),
        Ok(false) => return (ActionOutcome::passed(format!("{target} {NOT_SHOWN}")), None),
        Ok(true) => {}
    }
    let mut said: Vec<String> = Vec::with_capacity(then.len());
    for guarded in then {
        if here.stop_heard() {
            return (ActionOutcome::failed(AFTER_STOP), Some(AFTER_STOP));
        }
        // A bounce the guard caught (the page went home) is recovered by
        // walking back to the area, as the step's own `return_to_area`
        // would; not reaching it stops the step the same way.
        let (out, stop) = match guarded {
            Action::ReturnToArea { .. } => return_to_area(d, guarded, area, areas, here.step, timing).await,
            _ => plain(d, here, guarded, timing).await,
        };
        said.push(out.detail);
        if !out.ok {
            let mut failed = ActionOutcome::failed(format!("{target} showed: {}", said.join("; ")));
            failed.harness = out.harness;
            return (failed, stop);
        }
    }
    (ActionOutcome::passed(format!("{target} showed: {}", said.join("; "))), None)
}

/// A `repeat_while_visible`: pass after pass, one look (no waiting) for its
/// selector's matches; the first whose row reads no protected text has its
/// row marked (`repeat::mark`) and the `then` actions run, a row-scoped
/// locator inside that row only. Ends, passed, when no unprotected match is
/// left; fails when `then` fails (said after what that pass did), when a
/// match it must judge sits in no row, or when `max` passes left some. One
/// outcome for the whole loop, saying how many passes ran and what it kept.
async fn repeat_while_visible<D: Driver>(
    d: &mut D,
    here: &Here<'_>,
    action: &Action,
    area: &AreaRoute<'_>,
    areas: Option<&NamedAreas>,
    timing: &Timing,
) -> (ActionOutcome, Option<&'static str>) {
    use crate::browser::repeat::{self, ROW_RULE};
    if let Err(why) = action.validate() {
        return (ActionOutcome::failed(format!("{CANNOT_RUN}{why}")), None);
    }
    let Action::RepeatWhileVisible { selector, max, except, then } = action else {
        return (ActionOutcome::failed(format!("{CANNOT_RUN}this is not a repeat_while_visible")), None);
    };
    let max = max.unwrap_or(1);
    let target = selector.describe();
    let scoped = then.iter().any(|a| a.targets().into_iter().any(repeat::is_row_scoped));
    let mut passes: u32 = 0;
    loop {
        if passes > 0 && here.stop_heard() {
            return (ActionOutcome::failed(AFTER_STOP), Some(AFTER_STOP));
        }
        let found = match repeat::look(d, selector, except).await {
            Ok(found) => found,
            Err(e) if e.is_transient() => Vec::new(),
            Err(e) => return (crate::browser::actions::harness(e), None),
        };
        let mut kept: Vec<String> = Vec::new();
        for k in found.iter().filter_map(|m| m.kept.clone()) {
            if !kept.contains(&k) {
                kept.push(k);
            }
        }
        let open: Vec<&repeat::Match> = found.iter().filter(|m| m.kept.is_none()).collect();
        let Some(next) = open.first() else {
            let said = passes_said(passes);
            let kept_said = kept_said(&kept);
            return (ActionOutcome::passed(format!("{target}: {said}, none left{kept_said}")), None);
        };
        if (!except.is_empty() || scoped) && !next.row {
            let why = format!(
                "{target}: a match is not inside a list row ({ROW_RULE}), so the pass cannot tell which row it works on - point selector at something inside each row"
            );
            return (ActionOutcome::failed(why), None);
        }
        if passes >= max {
            let n = open.len();
            let rows = if n == 1 { "row" } else { "rows" };
            let kept_said = kept_said(&kept);
            return (
                ActionOutcome::failed(format!("{target}: still {n} {rows} after max {max} passes{kept_said}")),
                None,
            );
        }
        let next = (*next).clone();
        let marked = match repeat::mark(d, &next).await {
            Ok(m) => m,
            Err(e) => return (crate::browser::actions::harness(e), None),
        };
        passes += 1;
        let mut said: Vec<String> = Vec::with_capacity(then.len());
        for step in then {
            if here.stop_heard() {
                repeat::unmark(d).await;
                return (ActionOutcome::failed(AFTER_STOP), Some(AFTER_STOP));
            }
            let (out, stop) = match step {
                Action::ReturnToArea { .. } => return_to_area(d, step, area, areas, here.step, timing).await,
                _ => plain(d, here, step, timing).await,
            };
            said.push(out.detail);
            if !out.ok {
                repeat::unmark(d).await;
                let mut failed = ActionOutcome::failed(format!("{target}, pass {passes}: {}", said.join("; ")));
                failed.harness = out.harness;
                return (failed, stop);
            }
        }
        // The page may take a moment to drop the row the pass worked on:
        // the next look waits for that, up to the expect wait, so the same
        // row is not taken twice while it goes.
        if marked {
            let until = Instant::now() + Duration::from_millis(timing.expect_ms.min(ROW_GONE_WAIT_MS));
            while matches!(repeat::row_still_there(d).await, Ok(true)) && Instant::now() < until {
                d.idle(Duration::from_millis(timing.poll_ms)).await;
            }
        }
        repeat::unmark(d).await;
    }
}

/// The longest a `repeat_while_visible` pass waits for its row to go.
pub const ROW_GONE_WAIT_MS: u64 = 5_000;

fn passes_said(n: u32) -> String {
    if n == 1 {
        "ran 1 pass".to_string()
    } else {
        format!("ran {n} passes")
    }
}

fn kept_said(kept: &[String]) -> String {
    if kept.is_empty() {
        String::new()
    } else {
        format!("; kept {} protected ({})", kept.len(), kept.join(", "))
    }
}

/// A `return_to_area`, on its own or inside a `when_visible`'s `then`: a
/// named area by its own route (`areas`), a bare one to the case's area
/// (`area`). Not reaching it stops the rest of the step: what follows would
/// act on the wrong screen.
async fn return_to_area<D: Driver>(
    d: &mut D,
    action: &Action,
    area: &AreaRoute<'_>,
    areas: Option<&NamedAreas>,
    step_number: i32,
    timing: &Timing,
) -> (ActionOutcome, Option<&'static str>) {
    let who = format!("Auto Run, step {step_number}");
    if let Some(name) = action.area_named() {
        return match named_route(areas, name) {
            Ok(rt) => {
                let went = nav::reach_module(d, rt, nav::TripFrom::Elsewhere, timing, &who).await;
                let stop = (!went.ok).then_some(AFTER_NAMED_AREA_UNREACHED);
                (went, stop)
            }
            Err(why) => (ActionOutcome::failed(format!("return_to_area: {why}")), Some(AFTER_NAMED_AREA_UNREACHED)),
        };
    }
    match area {
        AreaRoute::To(rt) => {
            let went = nav::reach_module(d, rt, nav::TripFrom::Elsewhere, timing, &who).await;
            let stop = (!went.ok).then_some(AFTER_AREA_UNREACHED);
            (went, stop)
        }
        AreaRoute::Unknown(why) => (ActionOutcome::failed(*why), Some(AFTER_AREA_UNREACHED)),
    }
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
    use crate::autorun::downloads::{check_file, CellCheck, DownloadCheck, HeaderCheck, OnPage, PageCount, PdfCheck};
    use crate::browser::actions::{CellMatch, HeadersSpec, DOWNLOAD_WAIT_MS};
    if let Err(why) = action.validate() {
        return ActionOutcome::failed(format!("{CANNOT_RUN}{why}"));
    }
    let Action::ExpectDownload { name, within_ms, sheet, headers, cells, contains_text, pdf, .. } = action else {
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
        pdf: pdf.as_ref().map(|p| PdfCheck {
            contains: p.contains.as_ref().map(|c| c.list()).unwrap_or_default(),
            // `validate` above has made sure exactly one is given.
            pages: p.pages.as_ref().and_then(|n| {
                n.equals
                    .map(PageCount::Equals)
                    .or(n.at_least.map(PageCount::AtLeast))
                    .or(n.at_most.map(PageCount::AtMost))
            }),
            on_page: p
                .on_page
                .iter()
                .flatten()
                .map(|o| OnPage { page: o.page, contains: o.contains.list() })
                .collect(),
        }),
    };
    let (path, shown) = (entry.path.clone(), entry.name.clone());
    let job = tokio::task::spawn_blocking(move || check_file(&path, &shown, &check));
    await_file_read(job, &entry.name, cancel, FILE_READ_LIMIT).await
}

/// The longest a downloaded file's content may take to read.
pub const FILE_READ_LIMIT: Duration = Duration::from_secs(60);

/// How often a file read's wait looks at the run's Stop.
const FILE_READ_POLL: Duration = Duration::from_millis(250);

/// Waits for a download's check, read on a blocking thread, as its
/// outcome. The run's Stop (`cancel`) ends the wait as `AFTER_STOP`, and a
/// read that takes longer than `limit` fails naming the file only. Either
/// way the thread is left to finish on its own: a blocking read cannot be
/// interrupted, but it no longer holds up the step.
pub async fn await_file_read(
    mut job: tokio::task::JoinHandle<Result<String, String>>,
    name: &str,
    cancel: Option<&AtomicBool>,
    limit: Duration,
) -> ActionOutcome {
    let deadline = Instant::now() + limit;
    loop {
        match tokio::time::timeout(FILE_READ_POLL.min(limit), &mut job).await {
            Ok(Ok(Ok(sentence))) => return ActionOutcome::passed(sentence),
            Ok(Ok(Err(sentence))) => return ActionOutcome::failed(sentence),
            Ok(Err(_)) => return ActionOutcome::failed(format!("\"{name}\" could not be read")),
            Err(_) => {}
        }
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            return ActionOutcome::failed(AFTER_STOP);
        }
        if Instant::now() >= deadline {
            return ActionOutcome::failed(format!(
                "\"{name}\" took longer than {} seconds to read",
                limit.as_secs()
            ));
        }
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
