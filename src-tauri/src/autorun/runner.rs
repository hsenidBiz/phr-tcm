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
use super::{store, StepScript};
use crate::browser::actions::{
    execute_in, shows_up, upload_in, Action, ActionOutcome, Policy, CANNOT_RUN, NOT_SHOWN, WHEN_VISIBLE_MS,
};
use crate::browser::cdp::Driver;
use crate::browser::page;
use crate::browser::timing::{Timing, SHOT_TIMEOUT_MS};
use std::path::Path;
use std::time::Duration;

const AFTER_FAILED_SIGN_IN: &str = "not run: the sign-in before this action failed";
const AFTER_UNREACHED: &str = "not run: the module screen was not reached after the sign-in";
const AFTER_REFUSED_ADDRESS: &str = "not run: this step opened a page by address, which this project does not allow";
/// The rest of a step after the page tried to save in a no-save script.
pub const AFTER_SAVE_BLOCKED: &str = "not run: the page tried to save, and this script must not";

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
    run_step_routed(d, root, organization, project, step, timing, account, &mut lease, None).await
}

/// `run_step` with the browser's own account lease: a `sign_in` is first
/// cleared with `lease` (`Held::ready`), and one that is refused signs
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
    if step.actions.iter().any(|a| matches!(a, Action::ExpectResponse { .. })) {
        api_checks::settle(d, timing).await;
    }
    let mark = d.net_mark();
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
        let mut outcome = match action {
            Action::SignIn { account: key } => match signin::prepare(root, organization, project, key) {
                Err(why) => {
                    *account = None;
                    blocked = Some(AFTER_FAILED_SIGN_IN);
                    ActionOutcome::failed(why)
                }
                Ok((r, who)) => match lease.ready(root, &who.key).await {
                    // Someone else is signed in as this account: nobody is
                    // signed in here, and the browser stays as it was.
                    Err(why) => {
                        blocked = Some(AFTER_FAILED_SIGN_IN);
                        ActionOutcome::failed(why)
                    }
                    Ok(ready) => {
                        let signed = signin::sign_in(d, root, &r, &who, timing).await;
                        lease.signed_in(ready, signed.ok);
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
            // Only the runner knows where the step began.
            Action::ExpectResponse { .. } => api_checks::expect_response(d, action, mark, timing).await,
            Action::ApiRequest { .. } => api_checks::api_request(d, action, timing).await,
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
        if !outcome.ok && !outcome.harness {
            outcome.screenshot = picture(d, root).await;
        }
        out.push(outcome);
    }
    Ok(out)
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
        Action::Navigate { .. } if !here.direct_urls => {
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
