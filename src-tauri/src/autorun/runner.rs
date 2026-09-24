//! The step loop `auto_run_step` runs: one action at a time, `sign_in`
//! carried out here (it needs the tester's accounts and the project's
//! recipe, which the executor has neither of), and a failure screenshot
//! taken the same way regardless of which of the two produced it.
//!
//! Pulled out of the Tauri command so it can be exercised directly, the
//! same way `autorun::store` and `autorun::signin` already are.

use super::nav::{self, Route};
use super::recipe::{self, SignInRecipe};
use super::signin::{self, SignInOutcome};
use super::{store, StepScript};
use crate::browser::actions::{execute_in, Action, ActionOutcome, Policy};
use crate::browser::cdp::Driver;
use crate::browser::page;
use crate::browser::timing::{Timing, SHOT_TIMEOUT_MS};
use std::path::Path;
use std::time::Duration;

const AFTER_FAILED_SIGN_IN: &str = "not run: the sign-in before this action failed";
const AFTER_UNREACHED: &str = "not run: the module screen was not reached after the sign-in";

/// Where an authored `navigate` may go for this project: everywhere, for a
/// project with no recipe saved yet, or only the recipe's own origins once
/// there is one.
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
pub async fn run_step<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    step: &StepScript,
    timing: &Timing,
    account: &mut Option<String>,
) -> Result<Vec<ActionOutcome>, String> {
    run_step_routed(d, root, organization, project, step, timing, account, None).await
}

/// `run_step`, for an unattended run in a project with module paths. A
/// `sign_in` lands on the home page, so the runner takes the browser back
/// to the case's module screen before the next action; the `sign_in`'s one
/// outcome then says both halves. When the module is not reached, the rest
/// of the step is not run: it would act on the wrong screen.
#[allow(clippy::too_many_arguments)]
pub async fn run_step_routed<D: Driver>(
    d: &mut D,
    root: &Path,
    organization: &str,
    project: &str,
    step: &StepScript,
    timing: &Timing,
    account: &mut Option<String>,
    route: Option<&Route>,
) -> Result<Vec<ActionOutcome>, String> {
    let recipe = recipe::load_recipe(root, organization, project)?;
    let policy = policy_for(recipe.as_ref());
    let mut out = Vec::with_capacity(step.actions.len());
    let mut blocked: Option<&'static str> = None;
    for action in &step.actions {
        if let Some(why) = blocked {
            out.push(ActionOutcome::failed(why));
            continue;
        }
        let mut outcome = match action {
            Action::SignIn { account: key } => match signin::prepare(root, organization, project, key) {
                Err(why) => {
                    *account = None;
                    blocked = Some(AFTER_FAILED_SIGN_IN);
                    ActionOutcome::failed(why)
                }
                Ok((r, who)) => {
                    let signed = signin::sign_in(d, root, &r, &who, timing).await;
                    *account = signed.ok.then(|| who.key.clone());
                    match route {
                        _ if !signed.ok => {
                            blocked = Some(AFTER_FAILED_SIGN_IN);
                            as_action_outcome(&signed)
                        }
                        None => as_action_outcome(&signed),
                        Some(rt) => {
                            let went = nav::reached(&rt.path.module, nav::go_to_module(d, rt, timing).await);
                            if !went.ok {
                                blocked = Some(AFTER_UNREACHED);
                            }
                            signed_then_went(&signed, went)
                        }
                    }
                }
            },
            other => execute_in(d, other, timing, &policy).await,
        };
        if !outcome.ok && !outcome.harness {
            outcome.screenshot = picture(d, root).await;
        }
        out.push(outcome);
    }
    Ok(out)
}

/// A sign-in and the trip back to the module that follows it, as the one
/// outcome the `sign_in` action stands for.
fn signed_then_went(signed: &SignInOutcome, went: ActionOutcome) -> ActionOutcome {
    let detail = format!("{}; then {}", signed.detail, went.detail);
    let mut out = if went.ok { ActionOutcome::passed(detail) } else { ActionOutcome::failed(detail) };
    out.harness = went.harness;
    out
}
