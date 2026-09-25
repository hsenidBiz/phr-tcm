//! From a fresh browser to a signed-in one, for one account.
//!
//! A saved session first, because it costs one navigation; the recipe when
//! there is none or the application no longer accepts it. Whatever
//! happens, no text that leaves here contains the password.

use super::accounts::{find_account, Account};
use super::recipe::{for_account, load_recipe, RecipeStep, SignInRecipe};
use super::sessions::{forget_session, load_fresh_session, now_ms, save_session};
use crate::browser::actions::{execute_in, Action, ActionOutcome, Policy};
use crate::browser::cdp::{CdpError, Driver};
use crate::browser::expect::{expect, Check};
use crate::browser::session;
use crate::browser::timing::Timing;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SignInOutcome {
    pub ok: bool,
    pub detail: String,
    /// True when no form was touched: the saved session was still good.
    pub used_saved_session: bool,
    pub steps: Vec<ActionOutcome>,
    /// True when this failed because the browser connection did, not the
    /// page - `run_step` (via `as_action_outcome`) must not then ask a
    /// browser that is not answering for a failure screenshot. Same
    /// meaning as `ActionOutcome.harness`. Process-internal only: never
    /// crosses the IPC boundary and never lands in a saved run file.
    #[serde(skip)]
    #[specta(skip)]
    pub harness: bool,
}

pub fn redact(text: &str, account: &Account) -> String {
    if account.password.is_empty() {
        text.to_string()
    } else {
        text.replace(&account.password, "(hidden)")
    }
}

/// The recipe and the account, or the sentence that says what to add.
pub fn prepare(root: &Path, org: &str, project: &str, account_key: &str) -> Result<(SignInRecipe, Account), String> {
    let recipe = load_recipe(root, org, project)?.ok_or_else(|| {
        "this project has no sign-in recipe yet - add one in Auto Run, Sign-in recipe".to_string()
    })?;
    let account = find_account(root, account_key)?.ok_or_else(|| {
        format!("there is no account \"{account_key}\" on this machine - add it in Auto Run, Accounts")
    })?;
    Ok((recipe, account))
}

struct Run<'a> {
    account: &'a Account,
    steps: Vec<ActionOutcome>,
}

impl Run<'_> {
    fn keep(&mut self, mut out: ActionOutcome) -> bool {
        out.detail = redact(&out.detail, self.account);
        let ok = out.ok;
        self.steps.push(out);
        ok
    }
    fn done(self, ok: bool, detail: String, used_saved_session: bool, harness: bool) -> SignInOutcome {
        SignInOutcome { ok, detail: redact(&detail, self.account), used_saved_session, steps: self.steps, harness }
    }
}

/// What a failed `session::clear` means, in words - not just "the browser
/// did not answer". `clear` runs `Network.clearBrowserCookies` first, then
/// `Storage.clearDataForOrigin` per origin, so a failure anywhere in it can
/// leave some, or all, of the previous account's state still live: the
/// person has to be told to open a fresh browser rather than trust this one.
fn clear_failed(e: CdpError) -> String {
    format!(
        "the browser did not answer while the last sign-in was being cleared, so someone may still be signed in in that window - open a fresh browser before going on: {e}"
    )
}

pub async fn sign_in<D: Driver>(
    d: &mut D,
    root: &Path,
    recipe: &SignInRecipe,
    account: &Account,
    timing: &Timing,
) -> SignInOutcome {
    let origins = recipe.origins();
    let policy = Policy::only(origins.clone());
    let mut run = Run { account, steps: vec![] };
    let go = Action::Navigate { url: recipe.start_url.clone() };
    let who = if account.label.trim().is_empty() { account.key.clone() } else { account.label.clone() };

    if let Err(e) = session::clear(d, &origins).await {
        return run.done(false, clear_failed(e), false, true);
    }

    if let Some(saved) = load_fresh_session(root, &account.key, recipe.session_minutes, now_ms()) {
        match session::restore(d, &saved).await {
            Ok(ids) => {
                let arrived = execute_in(d, &go, timing, &policy).await;
                // Short-circuit exactly like the plain `&&` this replaces:
                // the marker is never even asked for once the navigate
                // itself has already failed.
                let marker = if arrived.ok {
                    Some(expect(d, &recipe.signed_in, Check::Visible, timing.expect_ms, timing.poll_ms).await)
                } else {
                    None
                };
                let seen = marker.as_ref().is_some_and(|m| m.ok);
                session::unseed(d, &ids).await;
                if seen {
                    // The recipe's own steps are skipped here; these are not.
                    if let Err((n, why, harness_failure)) =
                        run_steps(d, &mut run, &recipe.after_sign_in, timing, &policy).await
                    {
                        return run.done(
                            false,
                            format!("signed in as {who} from a saved session, but after_sign_in step {n} stopped: {why}"),
                            true,
                            harness_failure,
                        );
                    }
                    return run.done(true, format!("signed in as {who} from a saved session"), true, false);
                }
                // The BROWSER, not the saved session, may be what just
                // failed (the navigate or the marker check stopped
                // answering) - that says nothing about whether the saved
                // session is still good, so it must not be thrown away the
                // way an ordinary "this session no longer works" is below.
                let harness_failure = arrived.harness || marker.as_ref().is_some_and(|m| m.harness);
                if harness_failure {
                    let why = if arrived.harness { arrived.detail } else { marker.expect("checked above").detail };
                    // Best effort, same reason as the sibling arm below:
                    // `Network.setCookies` already put the saved session's
                    // cookies live before this failed, and they must not be
                    // left that way just because the browser then stopped
                    // answering.
                    let _ = session::clear(d, &origins).await;
                    return run.done(false, why, false, true);
                }
            }
            Err(e) if !e.is_transient() => {
                // Best effort: `Network.setCookies` may already have put the
                // saved session's cookies live before this failed, and they
                // must not be left that way just because the browser then
                // stopped answering. The session file itself is kept - a
                // browser that stopped answering says nothing about whether
                // the saved session is still good.
                let _ = session::clear(d, &origins).await;
                return run.done(false, format!("the browser did not answer: {e}"), false, true);
            }
            Err(_) => {}
        }
        forget_session(root, &account.key);
        if let Err(e) = session::clear(d, &origins).await {
            return run.done(false, clear_failed(e), false, true);
        }
    }

    if !run.keep(execute_in(d, &go, timing, &policy).await) {
        let last = run.steps.last();
        let why = last.map(|s| s.detail.clone()).unwrap_or_default();
        let harness_failure = last.is_some_and(|s| s.harness);
        return run.done(false, format!("the sign-in page did not open: {why}"), false, harness_failure);
    }
    let steps = for_account(&recipe.steps, account);
    if let Err((n, why, harness_failure)) = run_steps(d, &mut run, &steps, timing, &policy).await {
        return run.done(false, format!("sign-in stopped at step {n}: {why}"), false, harness_failure);
    }

    let marker = expect(d, &recipe.signed_in, Check::Visible, timing.nav_ms, timing.poll_ms).await;
    if !marker.ok {
        let harness_failure = marker.harness;
        return run.done(
            false,
            format!(
                "the recipe ran, but {} never appeared - check the username and password for \"{}\", and the recipe's signed_in locator",
                recipe.signed_in.describe(),
                account.key
            ),
            false,
            harness_failure,
        );
    }

    let after = run_steps(d, &mut run, &recipe.after_sign_in, timing, &policy).await;

    // Saving is a convenience for next time. Failing to save is not a
    // failure to sign in. Captured AFTER after_sign_in, so a saved session
    // keeps what those steps left in the page's storage - and kept even
    // when one of them failed, since the sign-in itself worked.
    if let Ok(captured) = session::capture(d, &origins, now_ms()).await {
        let _ = save_session(root, &account.key, &captured);
    }
    if let Err((n, why, harness_failure)) = after {
        return run.done(
            false,
            format!("signed in as {who}, but after_sign_in step {n} stopped: {why}"),
            false,
            harness_failure,
        );
    }
    run.done(true, format!("signed in as {who}"), false, false)
}

/// Recipe steps in order, each outcome kept. `Err` is the step number,
/// why it stopped, and whether the browser (not the page) was the cause.
/// Shared by the recipe's own steps and `after_sign_in`, so a
/// `when_visible` reads the same wherever it is written.
async fn run_steps<D: Driver>(
    d: &mut D,
    run: &mut Run<'_>,
    steps: &[RecipeStep],
    timing: &Timing,
    policy: &Policy,
) -> Result<(), (usize, String, bool)> {
    for (i, step) in steps.iter().enumerate() {
        let n = i + 1;
        let actions: Vec<&Action> = match step {
            RecipeStep::Do(a) => vec![a],
            RecipeStep::WhenVisible(w) => {
                let shown = expect(d, &w.selector, Check::Visible, u64::from(w.within_ms), timing.poll_ms).await;
                if shown.harness {
                    let why = shown.detail.clone();
                    run.keep(shown);
                    return Err((n, why, true));
                }
                if !shown.ok {
                    run.keep(ActionOutcome::passed(format!(
                        "step {n}: {} did not appear, carried on",
                        w.selector.describe()
                    )));
                    continue;
                }
                w.then.iter().collect()
            }
        };
        for action in actions {
            if !run.keep(execute_in(d, action, timing, policy).await) {
                let last = run.steps.last();
                let why = last.map(|s| s.detail.clone()).unwrap_or_default();
                let harness_failure = last.is_some_and(|s| s.harness);
                return Err((n, why, harness_failure));
            }
        }
    }
    Ok(())
}
