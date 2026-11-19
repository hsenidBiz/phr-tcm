//! When an Auto Test case may reuse the signed-in browser the case before
//! it left open. A pure rule: the run loop asks it, nothing here touches a
//! browser. Reuse is refused whenever the case before could have left the
//! browser in a state the next case should not inherit.

use super::replay::SIGN_IN_STEP;
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

/// True when a browser may be kept after this case at all.
pub fn may_keep(ended: &Ended) -> bool {
    ended.account.is_some()
        && ended.proposed == "Passed"
        && !ended.stopped
        && ended.signed_in_ok
        && !ended.script_signs_in
        && !ended.no_save
}

/// Whether the next case may use the browser the case before left open.
pub fn may_reuse(before: Option<&Ended>, next: &Next) -> Result<(), Fresh> {
    let Some(b) = before else { return Err(Fresh::NoBrowser) };
    if !may_keep(b) {
        return Err(if b.account.is_none() {
            Fresh::AccountChanged
        } else if b.stopped {
            Fresh::BeforeStopped
        } else if b.proposed != "Passed" {
            Fresh::BeforeNotPassed
        } else if !b.signed_in_ok {
            Fresh::BeforeSignInFailed
        } else if b.script_signs_in {
            Fresh::BeforeSignedInByScript
        } else {
            Fresh::BeforeNoSave
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

/// Describes how a finished case ended. `account` is the account the run
/// used, which wins over whatever the script names.
pub fn ended_from(record: &CaseRecord, script: &CaseScript, account: Option<&str>) -> Ended {
    let signed_in_ok = record
        .steps
        .iter()
        .find(|s| s.step_number == SIGN_IN_STEP)
        .and_then(|s| s.outcomes.last())
        .is_none_or(|o| o.ok);
    Ended {
        account: account.map(str::to_string),
        proposed: record.proposed.clone(),
        stopped: record.reason.starts_with(AFTER_STOP),
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
