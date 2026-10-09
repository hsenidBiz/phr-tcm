//! From a fresh browser to a signed-in one, for one account.
//!
//! A saved session first, because it costs one navigation; the recipe when
//! there is none or the application no longer accepts it. Whatever
//! happens, no text that leaves here contains the password.

use super::accounts::{find_account, Account};
use super::recipe::{for_account, load_effective_recipe, RecipeStep, SignInRecipe, WhenVisible};
use super::sessions::{forget_session, load_fresh_session, now_ms, save_session};
use super::timing::{FRESH_LOGIN_WINDOW_MS, PROMPT_WINDOW_MS};
use crate::browser::actions::{
    execute_in, harness_timeout, shown_now, shows_up, Action, ActionOutcome, Policy, WHEN_VISIBLE_FLOOR_MS,
};
use crate::browser::cdp::{CdpError, Driver};
use crate::browser::expect::{expect, Check};
use crate::browser::input::{COVERED_BY, MOVED_BEFORE_CLICK, STILL_MOVING};
use crate::browser::locator::Target;
use crate::browser::session;
use crate::browser::timing::Timing;
use std::path::Path;
use std::time::{Duration, Instant};

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
    /// The optional (`when_visible`) steps whose element DID show up, by
    /// description - `button "Continue here"` is PeoplesHR taking the
    /// account's session over from wherever else it was signed in.
    /// Process-internal like `harness`.
    #[serde(skip)]
    #[specta(skip)]
    pub appeared: Vec<String>,
}

pub fn redact(text: &str, account: &Account) -> String {
    if account.password.is_empty() {
        text.to_string()
    } else {
        text.replace(&account.password, "(hidden)")
    }
}

/// The recipe as it runs in the active environment, and the account, or
/// the sentence that says what to add.
pub fn prepare(root: &Path, org: &str, project: &str, account_key: &str) -> Result<(SignInRecipe, Account), String> {
    let recipe = load_effective_recipe(root, org, project)?;
    let account = find_account(root, account_key)?.ok_or_else(|| {
        format!("there is no account \"{account_key}\" on this machine - add it in Auto Run, Accounts")
    })?;
    Ok((recipe, account))
}

struct Run<'a> {
    /// `None` only for `run_after_sign_in`: those steps hold no login.
    account: Option<&'a Account>,
    steps: Vec<ActionOutcome>,
    appeared: Vec<String>,
}

impl Run<'_> {
    fn hide(&self, text: &str) -> String {
        match self.account {
            Some(a) => redact(text, a),
            None => text.to_string(),
        }
    }
    fn keep(&mut self, mut out: ActionOutcome) -> bool {
        out.detail = self.hide(&out.detail);
        let ok = out.ok;
        self.steps.push(out);
        ok
    }
    fn done(self, ok: bool, detail: String, used_saved_session: bool, harness: bool) -> SignInOutcome {
        SignInOutcome {
            ok,
            detail: self.hide(&detail),
            used_saved_session,
            steps: self.steps,
            harness,
            appeared: self.appeared,
        }
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

/// How `sign_in`'s words begin when the start address would not open.
pub const PAGE_DID_NOT_OPEN: &str = "the sign-in page did not open: ";
/// What `sign_in`'s words hold when the sign-in worked and `after_sign_in`
/// did not.
pub const AFTER_SIGN_IN_STOPPED: &str = ", but after_sign_in step ";
/// How `sign_in`'s words begin when the steps ran and the marker never
/// showed.
pub const MARKER_NEVER_APPEARED: &str = "the recipe ran, but ";

/// Sign in, trying the account's saved session first.
pub async fn sign_in<D: Driver>(
    d: &mut D,
    root: &Path,
    recipe: &SignInRecipe,
    account: &Account,
    timing: &Timing,
) -> SignInOutcome {
    sign_in_with(d, root, recipe, account, timing, true).await
}

/// The supervised browser's sign-in as `account_key`: its account lease
/// first (`lease`), then `sign_in`, and the browser's signed-in account
/// (`signed_in`) set from how it went. `Err` is the sentence for an account
/// that cannot be signed in at all - none on this machine, no recipe, or
/// one something else holds - and then the browser is left as it was. The
/// account is this browser's from the lease on, whichever way the sign-in
/// goes: one that fails partway may still have signed it in. Shared by the
/// pane's own sign-in and a replay to a step.
#[allow(clippy::too_many_arguments)]
pub async fn sign_in_leased<D: Driver>(
    d: &mut D,
    root: &Path,
    org: &str,
    project: &str,
    account_key: &str,
    lease: &mut super::lease::Held,
    signed_in: &mut Option<String>,
    timing: &Timing,
) -> Result<SignInOutcome, String> {
    let (recipe, account) = prepare(root, org, project, account_key)?;
    lease.hold(root, &account.key).await?;
    let out = sign_in(d, root, &recipe, &account, timing).await;
    *signed_in = out.ok.then(|| account.key.clone());
    crate::applog::info(format!("Auto-run sign-in as {}: {}", account.key, if out.ok { "ok" } else { "failed" }));
    Ok(out)
}

/// Sign in through the recipe's own steps, never a saved session: the
/// check a recorded recipe must pass has to prove the steps themselves
/// work. A session that works is still saved afterwards, as any sign-in's
/// is.
pub async fn sign_in_fresh<D: Driver>(
    d: &mut D,
    root: &Path,
    recipe: &SignInRecipe,
    account: &Account,
    timing: &Timing,
) -> SignInOutcome {
    sign_in_with(d, root, recipe, account, timing, false).await
}

/// A no-save case's guard is held from the moment the sign-in has arrived
/// on its start page (`open_start`) until it ends, on every path out: from
/// there on the sign-in is the runner's own, and what it sends (a login
/// form, the home page's own requests) is not the script's shared draft.
/// Leaving the page the case was on is NOT held: a save that page sends as
/// it goes (a beacon or a keepalive request on pagehide) is still stopped.
async fn sign_in_with<D: Driver>(
    d: &mut D,
    root: &Path,
    recipe: &SignInRecipe,
    account: &Account,
    timing: &Timing,
    try_saved_session: bool,
) -> SignInOutcome {
    // What the page met while the run signed in is the run's doing, not a
    // step's: let go of it once the sign-in is over.
    let errors_before = d.page_error_book().map(|b| b.len());
    let out = sign_in_held(d, root, recipe, account, timing, try_saved_session).await;
    if let (Some(n), Some(book)) = (errors_before, d.page_error_book()) {
        book.drop_after(n);
    }
    d.hold_saves(false);
    out
}

/// The sign-in's way to its start page. Only once it has arrived there -
/// the page the case was on is gone, with whatever it sent on the way out -
/// is a no-save case's guard held.
async fn open_start<D: Driver>(d: &mut D, go: &Action, timing: &Timing, policy: &Policy) -> ActionOutcome {
    let arrived = execute_in(d, go, timing, policy).await;
    if arrived.ok {
        d.hold_saves(true);
    }
    arrived
}

async fn sign_in_held<D: Driver>(
    d: &mut D,
    root: &Path,
    recipe: &SignInRecipe,
    account: &Account,
    timing: &Timing,
    try_saved_session: bool,
) -> SignInOutcome {
    let origins = recipe.origins();
    let policy = Policy::only(origins.clone());
    let mut run = Run { account: Some(account), steps: vec![], appeared: vec![] };
    let go = Action::Navigate { url: recipe.start_url.clone() };
    let who = if account.label.trim().is_empty() { account.key.clone() } else { account.label.clone() };

    if let Err(e) = session::clear(d, &origins).await {
        return run.done(false, clear_failed(e), false, true);
    }

    let saved = if try_saved_session {
        load_fresh_session(root, &account.key, recipe.session_minutes, now_ms())
    } else {
        None
    };
    if let Some(saved) = saved {
        match session::restore(d, &saved).await {
            Ok(ids) => {
                let arrived = open_start(d, &go, timing, &policy).await;
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
                    // The recipe's own steps are skipped here; these are
                    // not. The page was signed in before it loaded, so its
                    // prompts come with the marker: the short window.
                    let prompts = Prompts::Together { window_ms: PROMPT_WINDOW_MS };
                    if let Err((n, why, harness_failure)) =
                        run_steps(d, &mut run, &recipe.after_sign_in, prompts, timing, &policy).await
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

    if !run.keep(open_start(d, &go, timing, &policy).await) {
        let last = run.steps.last();
        let why = last.map(|s| s.detail.clone()).unwrap_or_default();
        let harness_failure = last.is_some_and(|s| s.harness);
        return run.done(false, format!("{PAGE_DID_NOT_OPEN}{why}"), false, harness_failure);
    }
    let steps = for_account(&recipe.steps, account);
    let prompts = Prompts::SignIn { login_field: login_field(&steps) };
    if let Err((n, why, harness_failure)) = run_steps(d, &mut run, &steps, prompts, timing, &policy).await {
        return run.done(false, format!("sign-in stopped at step {n}: {why}"), false, harness_failure);
    }

    let marker = expect(d, &recipe.signed_in, Check::Visible, timing.nav_ms, timing.poll_ms).await;
    if !marker.ok {
        let harness_failure = marker.harness;
        return run.done(
            false,
            format!(
                "{MARKER_NEVER_APPEARED}{} never appeared - check the username and password for \"{}\", and the recipe's signed_in locator",
                recipe.signed_in.describe(),
                account.key
            ),
            false,
            harness_failure,
        );
    }

    // A fresh login: PeoplesHR's "another active session" modal can come a
    // moment after the shell does, so the longer window.
    let prompts = Prompts::Together { window_ms: FRESH_LOGIN_WINDOW_MS };
    let after = run_steps(d, &mut run, &recipe.after_sign_in, prompts, timing, &policy).await;

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
            format!("signed in as {who}{AFTER_SIGN_IN_STOPPED}{n} stopped: {why}"),
            false,
            harness_failure,
        );
    }
    run.done(true, format!("signed in as {who}"), false, false)
}

/// The recipe's `after_sign_in` again, on a page that is already signed
/// in but was just loaded afresh. Going home by address reloads the
/// application (`nav::go_home`), and a reload can undo what these steps
/// did: PeoplesHR draws its menu closed on every load (2026-09-30). No
/// account, because `after_sign_in` holds no login - a placeholder there
/// is refused - so there is no password to hide. Its prompts get the short
/// window: the page was signed in before it loaded. `Err` is the step
/// number, why it stopped, and whether the browser was the cause.
pub async fn run_after_sign_in<D: Driver>(
    d: &mut D,
    steps: &[RecipeStep],
    timing: &Timing,
    policy: &Policy,
) -> Result<(), (usize, String, bool)> {
    let mut run = Run { account: None, steps: vec![], appeared: vec![] };
    let prompts = Prompts::Together { window_ms: PROMPT_WINDOW_MS };
    run_steps(d, &mut run, steps, prompts, timing, policy).await
}

/// How a run of recipe steps treats its optional (`when_visible`) steps.
#[derive(Clone, Copy)]
enum Prompts<'a> {
    /// The sign-in's own steps. While the login field is already showing
    /// the page has loaded, so a prompt is looked for once, not waited for.
    /// Otherwise (no login field, or the form is gone) it is waited for up
    /// to its own `within_ms`, as written.
    SignIn { login_field: Option<&'a Target> },
    /// `after_sign_in`: each run of consecutive prompts is watched together
    /// in one window of this many ms (`watch_together`).
    Together { window_ms: u64 },
}

/// The sign-in's login field: the first thing its steps fill in.
fn login_field(steps: &[RecipeStep]) -> Option<&Target> {
    steps.iter().find_map(|s| match s {
        RecipeStep::Do(Action::Fill { selector, .. }) => Some(selector),
        _ => None,
    })
}

/// Recipe steps in order, each outcome kept. `Err` is the step number,
/// why it stopped, and whether the browser (not the page) was the cause.
/// Shared by the recipe's own steps and `after_sign_in`, so a
/// `when_visible` reads the same wherever it is written; `prompts` says
/// how long one is watched for.
async fn run_steps<D: Driver>(
    d: &mut D,
    run: &mut Run<'_>,
    steps: &[RecipeStep],
    prompts: Prompts<'_>,
    timing: &Timing,
    policy: &Policy,
) -> Result<(), (usize, String, bool)> {
    let mut i = 0;
    while i < steps.len() {
        let n = i + 1;
        match (&steps[i], prompts) {
            (RecipeStep::Do(a), _) => {
                run_action(d, run, n, a, timing, policy).await?;
                i += 1;
            }
            (RecipeStep::WhenVisible(_), Prompts::Together { window_ms }) => {
                let end = steps[i..]
                    .iter()
                    .position(|s| !matches!(s, RecipeStep::WhenVisible(_)))
                    .map_or(steps.len(), |k| i + k);
                let group: Vec<(usize, &WhenVisible)> = steps[i..end]
                    .iter()
                    .enumerate()
                    .filter_map(|(k, s)| match s {
                        RecipeStep::WhenVisible(w) => Some((i + k + 1, w)),
                        RecipeStep::Do(_) => None,
                    })
                    .collect();
                watch_together(d, run, &group, window_ms, timing, policy).await?;
                i = end;
            }
            (RecipeStep::WhenVisible(w), Prompts::SignIn { login_field }) => {
                let shown = match sign_in_prompt_shown(d, w, login_field, timing).await {
                    Ok(shown) => shown,
                    Err(silent) => return Err(silent_at(run, n, silent)),
                };
                if shown {
                    let seen = run.hide(&w.selector.describe());
                    run.appeared.push(seen);
                    for action in &w.then {
                        run_action(d, run, n, action, timing, policy).await?;
                    }
                } else {
                    carried_past(run, n, w);
                }
                i += 1;
            }
        }
    }
    Ok(())
}

/// One action of step `n`, its outcome kept.
async fn run_action<D: Driver>(
    d: &mut D,
    run: &mut Run<'_>,
    n: usize,
    action: &Action,
    timing: &Timing,
    policy: &Policy,
) -> Result<(), (usize, String, bool)> {
    if run.keep(execute_in(d, action, timing, policy).await) {
        return Ok(());
    }
    let last = run.steps.last();
    let why = last.map(|s| s.detail.clone()).unwrap_or_default();
    let harness_failure = last.is_some_and(|s| s.harness);
    Err((n, why, harness_failure))
}

/// Step `n` stopped because the browser did not answer a look for its
/// prompt: `silent` is kept, and the browser is the cause.
fn silent_at(run: &mut Run<'_>, n: usize, silent: ActionOutcome) -> (usize, String, bool) {
    let why = silent.detail.clone();
    run.keep(silent);
    (n, why, true)
}

fn carried_past(run: &mut Run<'_>, n: usize, w: &WhenVisible) {
    run.keep(ActionOutcome::passed(format!("step {n}: {} did not appear, carried on", w.selector.describe())));
}

/// Is a sign-in step's prompt there? Looked for once when the login field
/// is already showing - the page has loaded - and waited for up to its own
/// `within_ms` otherwise.
async fn sign_in_prompt_shown<D: Driver>(
    d: &mut D,
    w: &WhenVisible,
    login_field: Option<&Target>,
    timing: &Timing,
) -> Result<bool, ActionOutcome> {
    if let Some(field) = login_field {
        if shown_now(d, field).await? == Some(true) {
            if let Some(shown) = shown_now(d, &w.selector).await? {
                return Ok(shown);
            }
        }
    }
    shows_up(d, &w.selector, w.within_ms, timing).await
}

/// The window a run of consecutive prompts is watched for: `window_ms`,
/// unless the prompts' own waits added up are shorter. One by one, each
/// prompt was watched for its own `within_ms` (never less than
/// `WHEN_VISIBLE_FLOOR_MS`) after the ones before it, so that sum is the
/// latest any of them was ever looked for: a shared window that long
/// never misses what the old waits caught, and a recipe with short waits
/// stays short.
fn shared_window(group: &[(usize, &WhenVisible)], window_ms: u64) -> u64 {
    let one_by_one: u64 = group.iter().map(|(_, w)| u64::from(w.within_ms.max(WHEN_VISIBLE_FLOOR_MS))).sum();
    window_ms.min(one_by_one)
}

/// How long one attempt at a prompt's action may take inside the window:
/// a few polls (120 ms at the default 40), and no more, so a prompt that
/// something else is sitting on does not hold up the others. It is never
/// too short to see the element: the wait always finishes the look it has
/// started, a look sees whether the element holds still by itself (at
/// least 2 frames and 50 ms inside one probe), and no call in it is given less
/// than `cdp::MIN_CALL_TIMEOUT`.
fn attempt_timing(timing: &Timing) -> Timing {
    Timing { action_ms: timing.poll_ms * 3, ..timing.clone() }
}

/// Was the action refused only because something sat over its element or
/// moved it - another prompt arriving, which this same watch will handle?
fn in_the_way(out: &ActionOutcome) -> bool {
    !out.ok && !out.harness && [COVERED_BY, MOVED_BEFORE_CLICK, STILL_MOVING].iter().any(|s| out.detail.contains(s))
}

/// Where one prompt of a watched group stands.
#[derive(Default)]
struct Watched {
    /// Every `then` action has been carried out.
    handled: bool,
    /// It has shown at least once (and is named in `appeared`).
    seen: bool,
    /// How many of its `then` actions are done.
    done: usize,
    /// The last attempt something else was in the way of, while it waits
    /// for another look.
    blocked: Option<ActionOutcome>,
}

/// One go at a showing prompt's remaining `then` actions, each with the
/// short attempt budget. `Ok(true)` when all are done; `Ok(false)` when one
/// waits for the next look (kept in `blocked`, not as a step): something
/// was in the way of it; or the browser did not answer within the short
/// budget, which a page busy for longer than one short attempt does and
/// which must not hold up the other prompts for a full action's wait (a
/// browser truly gone fails the next look, and one still blocked when the
/// window ends gets one full try there); or the prompt no longer matches
/// after its first action failed. Any other failure is given the full
/// action budget, as an action outside the watch would be. So is a later
/// action's failure after an earlier one of the same prompt put it away:
/// that is the action failing, not the prompt going away by itself.
async fn attempt_prompt<D: Driver>(
    d: &mut D,
    run: &mut Run<'_>,
    n: usize,
    w: &WhenVisible,
    state: &mut Watched,
    timing: &Timing,
    policy: &Policy,
) -> Result<bool, (usize, String, bool)> {
    let short = attempt_timing(timing);
    while let Some(action) = w.then.get(state.done) {
        let out = execute_in(d, action, &short, policy).await;
        if in_the_way(&out) || (!out.ok && out.harness) {
            state.blocked = Some(out);
            return Ok(false);
        }
        if out.ok {
            run.keep(out);
        } else if state.done == 0
            && shown_now(d, &w.selector).await.map_err(|silent| silent_at(run, n, silent))? == Some(false)
        {
            // The prompt went away between the look and the attempt: it
            // waits for the next look like a covered one, and the window's
            // end carries it past if it stays gone.
            state.blocked = Some(out);
            return Ok(false);
        } else {
            run_action(d, run, n, action, timing, policy).await?;
        }
        state.done += 1;
    }
    state.blocked = None;
    Ok(true)
}

/// Watch a run of consecutive prompts together, for one window
/// (`shared_window`). Each look takes in every prompt not yet handled, and
/// a prompt that shows has its `then` tried there and then, in recipe
/// order among those showing at the same look. A prompt counts as handled
/// only once its actions have succeeded: one that another prompt is
/// covering (the session modal arriving over the menu toggle) is tried
/// again on a later look, after the one on top has been dealt with. The
/// watch ends as soon as every prompt has been handled, or when the
/// window does. A prompt still in the way at the end gets one last try
/// with the full action budget; one that never showed is carried past, as
/// a single `when_visible` is. A page so busy that no look finishes within
/// the window is looked at for up to an ordinary action's wait before it
/// is called the browser gone silent.
async fn watch_together<D: Driver>(
    d: &mut D,
    run: &mut Run<'_>,
    group: &[(usize, &WhenVisible)],
    window_ms: u64,
    timing: &Timing,
    policy: &Policy,
) -> Result<(), (usize, String, bool)> {
    let window = shared_window(group, window_ms);
    let deadline = Instant::now() + Duration::from_millis(window);
    // How long a window in which no look finishes is kept looking at: a
    // busy page, not a silent browser, until an ordinary action's wait.
    let silence = window.max(timing.action_ms);
    let silent_end = Instant::now() + Duration::from_millis(silence);
    let mut idled = 0u64;
    let mut states: Vec<Watched> = group.iter().map(|_| Watched::default()).collect();
    // Whether any look completed: a window that ends without one is the
    // browser not answering, not a page with nothing to show.
    let mut looked = false;
    loop {
        let mut showing = vec![];
        d.set_deadline(Some(if looked { deadline } else { silent_end }));
        for (k, (n, w)) in group.iter().enumerate() {
            if states[k].handled {
                continue;
            }
            match shown_now(d, &w.selector).await {
                Ok(Some(shown)) => {
                    looked = true;
                    if shown {
                        showing.push(k);
                    }
                }
                Ok(None) => {}
                Err(silent) => {
                    d.set_deadline(None);
                    return Err(silent_at(run, *n, silent));
                }
            }
        }
        d.set_deadline(None);
        for k in showing {
            let (n, w) = group[k];
            if !states[k].seen {
                states[k].seen = true;
                let seen = run.hide(&w.selector.describe());
                run.appeared.push(seen);
            }
            states[k].handled = attempt_prompt(d, run, n, w, &mut states[k], timing, policy).await?;
        }
        if states.iter().all(|s| s.handled) {
            return Ok(());
        }
        // The clock, or the idles alone, whichever says so first. In a
        // real browser an idle takes the time it says, so the clock always
        // gets there first; a test's fake clock idles without sleeping.
        let window_over = Instant::now() >= deadline || idled >= window;
        let silence_over = Instant::now() >= silent_end || idled >= silence;
        if window_over && (looked || silence_over) {
            break;
        }
        d.idle(Duration::from_millis(timing.poll_ms)).await;
        idled += timing.poll_ms;
    }
    if !looked {
        let (n, w) = group[0];
        return Err(silent_at(run, n, harness_timeout(silence, &w.selector.describe())));
    }
    for (k, (n, w)) in group.iter().enumerate() {
        let state = &mut states[k];
        if state.handled {
            continue;
        }
        if state.blocked.take().is_some() {
            // Still in the way when the window closed. If none of its
            // actions ran and it no longer matches, the page put it away
            // itself: nothing left to do. Once one has run, the rest are
            // still owed.
            if state.done == 0 {
                match shown_now(d, &w.selector).await {
                    Ok(Some(false)) => {
                        run.keep(ActionOutcome::passed(format!(
                            "step {n}: {} went away before it could be used, carried on",
                            w.selector.describe()
                        )));
                        continue;
                    }
                    Ok(_) => {}
                    Err(silent) => return Err(silent_at(run, *n, silent)),
                }
            }
            // Otherwise the last try is an ordinary action's, with its
            // full wait and its own failure.
            for action in &w.then[state.done..] {
                run_action(d, run, *n, action, timing, policy).await?;
            }
        } else {
            carried_past(run, *n, w);
        }
    }
    Ok(())
}
