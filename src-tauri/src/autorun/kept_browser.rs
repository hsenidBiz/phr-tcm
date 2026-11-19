//! When an Auto Test case may reuse the signed-in browser the case before
//! it left open. A pure rule: the run loop asks it. Only `close_kept` touches a
//! browser, to close the one it holds. Reuse is refused whenever the case before could have left the
//! browser in a state the next case should not inherit.

use super::lease::Held;
use super::replay::{Browsers, SIGN_IN_STEP, STOPPED_BEFORE_FINISHED};
use super::signin::SIGN_IN_STOPPED;
use super::runner::AFTER_STOP;
use super::{CaseRecord, CaseScript};
use crate::browser::actions::Action;

/// How the case before ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    pub account: Option<String>,
    pub proposed: String,
    pub stopped: bool,
    pub signed_in_ok: bool,
    pub script_signs_in: bool,
    pub no_save: bool,
}

/// What the next case needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Next<'a> {
    pub account: Option<&'a str>,
    pub has_route: bool,
    pub pause_between: bool,
}

/// Why a case starts in a fresh browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fresh {
    NoBrowser,
    AccountChanged,
    NoModuleTrip,
    BeforeNotPassed,
    BeforeStopped,
    BeforeSignInFailed,
    BeforeSignedInByScript,
    BeforeNoSave,
    Paused,
}

const PASSED: &str = "Passed";

/// True when a browser may be kept after this case at all.
pub fn may_keep(ended: &Ended) -> bool {
    ended.account.is_some()
        && ended.proposed == PASSED
        && !ended.stopped
        && ended.signed_in_ok
        && !ended.script_signs_in
        && !ended.no_save
}

/// Whether the next case may use the browser the case before left open.
pub fn may_reuse(before: Option<&Ended>, next: &Next) -> Result<(), Fresh> {
    let Some(b) = before else { return Err(Fresh::NoBrowser) };
    if !may_keep(b) {
        return Err(if b.stopped {
            Fresh::BeforeStopped
        } else if b.proposed != PASSED {
            Fresh::BeforeNotPassed
        } else if !b.signed_in_ok {
            Fresh::BeforeSignInFailed
        } else if b.script_signs_in {
            Fresh::BeforeSignedInByScript
        } else if b.no_save {
            Fresh::BeforeNoSave
        } else {
            Fresh::AccountChanged
        });
    }
    if next.pause_between {
        return Err(Fresh::Paused);
    }
    if !next.has_route {
        return Err(Fresh::NoModuleTrip);
    }
    match (b.account.as_deref(), next.account) {
        (Some(a), Some(n)) if a == n => Ok(()),
        _ => Err(Fresh::AccountChanged),
    }
}

/// Describes how a finished case ended. Where each field comes from:
/// `account` is the account the run used (it wins over the script's);
/// `proposed` is the record's proposal; `stopped` is a record whose reason
/// is `STOPPED_BEFORE_FINISHED`, or any outcome saying `AFTER_STOP`, or a
/// sign-in outcome saying `SIGN_IN_STOPPED`; `signed_in_ok` is step 0's last
/// outcome being ok (true with no step 0); `script_signs_in` is any
/// `sign_in` action in the script; `no_save` is the script's flag.
pub fn ended_from(record: &CaseRecord, script: &CaseScript, account: Option<&str>) -> Ended {
    let sign_in = record.steps.iter().find(|s| s.step_number == SIGN_IN_STEP);
    let signed_in_ok = sign_in.and_then(|s| s.outcomes.last()).is_none_or(|o| o.ok);
    let stopped = record.reason == STOPPED_BEFORE_FINISHED
        || record.steps.iter().any(|s| s.outcomes.iter().any(|o| o.detail == AFTER_STOP))
        || sign_in.is_some_and(|s| s.outcomes.iter().any(|o| o.detail.contains(SIGN_IN_STOPPED)));
    Ended {
        account: account.map(str::to_string),
        proposed: record.proposed.clone(),
        stopped,
        signed_in_ok,
        script_signs_in: script
            .steps
            .iter()
            .any(|s| s.actions.iter().any(|a| matches!(a, Action::SignIn { .. }))),
        no_save: script.no_save,
    }
}

/// The note on a case that used the browser the case before signed in.
pub fn reused_detail(check_ms: u64) -> String {
    format!("used the browser the case before signed in (checked it was still signed in in {check_ms} ms)")
}

/// A signed-in browser left open after a case, for the next case on the same
/// account. It owns the account's lease: the lease is held while the browser
/// is kept and let go when it closes (`close_kept`).
pub struct KeptBrowser<D> {
    pub driver: D,
    pub lease: Held,
    pub ended: Ended,
}

/// Closes the kept browser, then lets its lease go. Nothing for `None`.
pub async fn close_kept<B: Browsers>(browsers: &mut B, kept: &mut Option<KeptBrowser<B::D>>) {
    if let Some(KeptBrowser { driver, lease, .. }) = kept.take() {
        browsers.close(driver).await;
        drop(lease);
    }
}
