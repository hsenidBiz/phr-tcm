//! The application's menu preferences, set by the app itself after a
//! sign-in. An area is recorded with its menu drawn one way (grouped, say,
//! or flat); an account signed in with the other draws a different menu, and
//! the area's clicks find nothing. The project's recipe names the dialog
//! (`MenuPreferences`) and the area keeps the values in force when it was
//! saved (`ModulePath::preferences`); here the dialog is read and, only
//! where an account differs, changed.

use super::nav::Home;
use super::recipe::MenuPreferences;
use super::signin;
use crate::browser::actions::{execute_in, Action, ActionOutcome, Policy};
use crate::browser::cdp::Driver;
use crate::browser::expect::{expect, Check};
use crate::browser::locator::{resolve, Target};
use crate::browser::page;
use crate::browser::timing::Timing;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// `this` is a setting's control, a checkbox or a switch: on when it is
/// checked, or says `aria-checked="true"`.
pub const CONTROL_STATE_JS: &str =
    "function() { return this.checked === true || this.getAttribute('aria-checked') === 'true'; }";

/// Set on the page before the dialog is closed: a page that reloads after a
/// change loses it, which is how the wait for the reload knows.
const RELOAD_SENTINEL_SET_JS: &str = "window.__tcmMenuSetup = true";
const RELOAD_SENTINEL_KEPT_JS: &str = "window.__tcmMenuSetup === true";

/// How long the page is given to reload after a change before the sign-in
/// marker is waited for anyway: a page that updates its menu without a
/// reload never lets go of the sentinel.
const RELOAD_WAIT_MS: u64 = 5_000;

async fn click(d: &mut impl Driver, target: &Target, timing: &Timing) -> ActionOutcome {
    execute_in(d, &Action::Click { selector: target.clone() }, timing, &Policy::open()).await
}

/// Click each of `open`, in order.
async fn open_dialog(d: &mut impl Driver, prefs: &MenuPreferences, timing: &Timing) -> Result<(), String> {
    for (i, target) in prefs.open.iter().enumerate() {
        let out = click(d, target, timing).await;
        if !out.ok {
            return Err(format!("the preferences dialog would not open (click {}: {})", i + 1, out.detail));
        }
    }
    Ok(())
}

async fn close_dialog(d: &mut impl Driver, prefs: &MenuPreferences, timing: &Timing) -> Result<(), String> {
    let out = click(d, &prefs.close, timing).await;
    if out.ok {
        Ok(())
    } else {
        Err(format!("the preferences dialog would not close ({})", out.detail))
    }
}

/// One control's state, while the dialog is open. A control the page does
/// not show once, or twice, is an error: which one is meant is not known.
async fn control_is_on(d: &mut impl Driver, name: &str, control: &Target) -> Result<bool, String> {
    let found = resolve(d, &control.including_hidden()).await.map_err(|e| format!("{name}: {e}"))?;
    let [one] = found.as_slice() else {
        return Err(format!("the \"{name}\" control matches {} elements in the dialog, not one", found.len()));
    };
    let value = page::call_value(d, one, CONTROL_STATE_JS, &[]).await.map_err(|e| format!("{name}: {e}"))?;
    value.as_bool().ok_or_else(|| format!("the \"{name}\" control did not say whether it is on"))
}

/// Each setting's value in the dialog right now, by name. The dialog is
/// opened and closed again; when reading fails after it opened, it is still
/// closed.
pub async fn read_preferences<D: Driver>(
    d: &mut D,
    prefs: &MenuPreferences,
    timing: &Timing,
) -> Result<BTreeMap<String, bool>, String> {
    open_dialog(d, prefs, timing).await?;
    let mut got = BTreeMap::new();
    let mut failed = None;
    for s in &prefs.settings {
        match control_is_on(d, &s.name, &s.control).await {
            Ok(on) => {
                got.insert(s.name.clone(), on);
            }
            Err(e) => {
                failed = Some(e);
                break;
            }
        }
    }
    let closed = close_dialog(d, prefs, timing).await;
    if let Some(e) = failed {
        return Err(e);
    }
    closed?;
    Ok(got)
}

/// Make the account's preferences what the area needs (`wanted`, by name;
/// a setting it has no value for gets the recipe's `on`), changing only
/// those that differ. Answers a note for each one changed.
///
/// The change is a save (the page posts the new preferences), so a case
/// that must save nothing would have it stopped by the guard. The sign-in
/// is let through the guard for its own requests (`signin::sign_in_held`);
/// this takes the same allowance for the stretch that changes a setting and
/// lets the page reload, and gives it back after. A trip with nothing to
/// change never lets anything through.
///
/// A change reloads the page: the sign-in marker is waited for and
/// `after_sign_in` run again, so the side menu is opened the way the
/// scripts expect.
pub async fn match_preferences<D: Driver>(
    d: &mut D,
    home: &Home,
    wanted: &BTreeMap<String, bool>,
    timing: &Timing,
) -> Result<Vec<String>, String> {
    let Some(prefs) = &home.menu_preferences else { return Ok(Vec::new()) };
    open_dialog(d, prefs, timing).await?;

    // What differs, read before anything is clicked.
    let mut differ = Vec::new();
    let mut failed = None;
    for s in &prefs.settings {
        let want = wanted.get(&s.name).copied().unwrap_or(s.on);
        match control_is_on(d, &s.name, &s.control).await {
            Ok(now) if now != want => differ.push((s, want)),
            Ok(_) => {}
            Err(e) => {
                failed = Some(e);
                break;
            }
        }
    }
    if let Some(e) = failed {
        let _ = close_dialog(d, prefs, timing).await;
        return Err(e);
    }
    if differ.is_empty() {
        close_dialog(d, prefs, timing).await?;
        return Ok(Vec::new());
    }

    // The change, the close and the reload are the app's own requests.
    d.hold_saves(true);
    let out = change_and_close(d, home, prefs, &differ, timing).await;
    d.hold_saves(false);
    out
}

async fn change_and_close<D: Driver>(
    d: &mut D,
    home: &Home,
    prefs: &MenuPreferences,
    differ: &[(&super::recipe::MenuPreference, bool)],
    timing: &Timing,
) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    let mut failed = None;
    for (s, want) in differ {
        let out = click(d, &s.control, timing).await;
        if !out.ok {
            failed = Some(format!("the \"{}\" setting could not be changed ({})", s.name, out.detail));
            break;
        }
        notes.push(format!("{} turned {}", s.name, if *want { "on" } else { "off" }));
    }
    let _ = page::eval_value(d, RELOAD_SENTINEL_SET_JS).await;
    let closed = close_dialog(d, prefs, timing).await;
    if let Some(e) = failed {
        return Err(e);
    }

    // The page reloads: wait for it to let go of the sentinel, then for the
    // signed-in page and the recipe's own steps.
    let limit = Instant::now() + Duration::from_millis(timing.nav_ms.min(RELOAD_WAIT_MS));
    while Instant::now() < limit {
        match page::eval_value(d, RELOAD_SENTINEL_KEPT_JS).await {
            Ok(v) if v.as_bool() == Some(true) => d.idle(Duration::from_millis(timing.poll_ms)).await,
            _ => break,
        }
    }
    let marker = expect(d, &home.signed_in, Check::Visible, timing.nav_ms, timing.poll_ms).await;
    if !marker.ok {
        return Err("the page did not show it was signed in after the preferences were changed".to_string());
    }
    d.confirm_capture_site();
    if !home.after_sign_in.is_empty() {
        let policy = Policy::only(home.origins.clone());
        if let Err((n, why, _)) = signin::run_after_sign_in(d, &home.after_sign_in, timing, &policy).await {
            crate::applog::warn(format!("menu preferences: after_sign_in step {n} stopped: {why}"));
            return Err(format!("after_sign_in step {n} stopped after the preferences were changed"));
        }
    }
    closed?;
    Ok(notes)
}
