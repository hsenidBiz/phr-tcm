//! From a fresh browser to a signed-in one, for one account.
//!
//! A saved session first, because it costs one navigation; the recipe when
//! there is none or the application no longer accepts it. Whatever
//! happens, no text that leaves here contains the password.

use super::accounts::{find_account, Account};
use super::recipe::{for_account, load_recipe, RecipeStep, SignInRecipe};
use super::sessions::{forget_session, load_fresh_session, now_ms, save_session};
use crate::browser::actions::{execute_in, Action, ActionOutcome, Policy};
use crate::browser::cdp::Driver;
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
    fn done(self, ok: bool, detail: String, used_saved_session: bool) -> SignInOutcome {
        SignInOutcome { ok, detail: redact(&detail, self.account), used_saved_session, steps: self.steps }
    }
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
        return run.done(false, format!("the browser did not answer: {e}"), false);
    }

    if let Some(saved) = load_fresh_session(root, &account.key, recipe.session_minutes, now_ms()) {
        match session::restore(d, &saved).await {
            Ok(ids) => {
                let arrived = execute_in(d, &go, timing, &policy).await;
                let seen = arrived.ok
                    && expect(d, &recipe.signed_in, Check::Visible, timing.expect_ms, timing.poll_ms).await.ok;
                session::unseed(d, &ids).await;
                if seen {
                    return run.done(true, format!("signed in as {who} from a saved session"), true);
                }
            }
            Err(e) if !e.is_transient() => {
                return run.done(false, format!("the browser did not answer: {e}"), false);
            }
            Err(_) => {}
        }
        forget_session(root, &account.key);
        if let Err(e) = session::clear(d, &origins).await {
            return run.done(false, format!("the browser did not answer: {e}"), false);
        }
    }

    if !run.keep(execute_in(d, &go, timing, &policy).await) {
        let why = run.steps.last().map(|s| s.detail.clone()).unwrap_or_default();
        return run.done(false, format!("the sign-in page did not open: {why}"), false);
    }
    for (i, step) in for_account(&recipe.steps, account).iter().enumerate() {
        let n = i + 1;
        let actions: Vec<&Action> = match step {
            RecipeStep::Do(a) => vec![a],
            RecipeStep::WhenVisible(w) => {
                let shown = expect(d, &w.selector, Check::Visible, u64::from(w.within_ms), timing.poll_ms).await;
                if shown.harness {
                    let why = shown.detail.clone();
                    run.keep(shown);
                    return run.done(false, format!("sign-in stopped at step {n}: {why}"), false);
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
            if !run.keep(execute_in(d, action, timing, &policy).await) {
                let why = run.steps.last().map(|s| s.detail.clone()).unwrap_or_default();
                return run.done(false, format!("sign-in stopped at step {n}: {why}"), false);
            }
        }
    }

    let marker = expect(d, &recipe.signed_in, Check::Visible, timing.nav_ms, timing.poll_ms).await;
    if !marker.ok {
        return run.done(
            false,
            format!(
                "the recipe ran, but {} never appeared - check the username and password for \"{}\", and the recipe's signed_in locator",
                recipe.signed_in.describe(),
                account.key
            ),
            false,
        );
    }

    // Saving is a convenience for next time. Failing to save is not a
    // failure to sign in.
    if let Ok(captured) = session::capture(d, &origins, now_ms()).await {
        let _ = save_session(root, &account.key, &captured);
    }
    run.done(true, format!("signed in as {who}"), false)
}
