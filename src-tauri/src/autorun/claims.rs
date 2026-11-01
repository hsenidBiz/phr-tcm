//! Which assistant is working on which case.
//!
//! An assistant that discovers, tries, replays or saves for a case claims it;
//! another assistant that asks for the same case is told at once who holds it
//! and is not made to wait. A claim lasts `CLAIM_TIMEOUT_MS` (a named subagent's
//! `agents::NAMED_HOLD_MS`, while it is quiet and its browser idle) from the last
//! time its holder claimed the case, and ends sooner when the holder ends its
//! discovery or releases its browser (`ai_bridge::end_claims_of`: the same
//! key, so unnamed subagents of one session share their claims), when the
//! person releases its browser (`commands::autorun_slots::release_slot`), or
//! when its session is past its grace (`on_sweep`). A holder that is no
//! longer live (`agents::is_live`) refuses nobody.
//!
//! Case ids are unique in the organization and Auto Run works in the home
//! project, so a claim is keyed by the case id alone.
//!
//! The claims are coordination state, not a cache: they say who is working on
//! what right now, so nothing here may be stored on disk or kept across a
//! restart, and nothing else in the app may keep a second copy of them (the
//! same rule as `IMPORTING` in `commands/autorun.rs`). They are one `static`
//! mutex that is never held across an await. The person's own commands never
//! claim and are never refused.

use crate::autorun::agents::{self, AgentKey};
use std::sync::Mutex;

/// How long a claim holds a case after its holder last claimed it.
pub const CLAIM_TIMEOUT_MS: u64 = 20 * 60 * 1000;

struct Claim {
    case_id: i32,
    holder: AgentKey,
    /// The first claim of this hold (a take-over starts a new one).
    since_ms: u64,
    /// The last renewal.
    at_ms: u64,
}

static CLAIMS: Mutex<Vec<Claim>> = Mutex::new(Vec::new());

fn claims() -> std::sync::MutexGuard<'static, Vec<Claim>> {
    CLAIMS.lock().unwrap_or_else(|e| e.into_inner())
}

/// How long a hold has lasted, in words: a duration, never a clock time.
pub fn held_words(held_ms: u64) -> String {
    let minutes = held_ms / 60_000;
    match minutes {
        0 => "less than a minute".to_string(),
        1 => "1 minute".to_string(),
        2..=59 => format!("{minutes} minutes"),
        _ => format!("{} h {} min", minutes / 60, minutes % 60),
    }
}

/// What an assistant is told about a case another assistant holds: the label
/// and how long it has been held, never an id or a pid.
pub fn case_claimed(case_id: i32, label: &str, held_ms: u64) -> String {
    let held = held_words(held_ms);
    format!("case {case_id} is being worked on by {label} (held for {held}) - pick another case, or ask the person to release {label} on the Auto Run screen")
}

/// How long `case_id` has been held at `now_ms`, from the first claim.
pub fn held_for(case_id: i32, now_ms: u64) -> u64 {
    claims().iter().find(|c| c.case_id == case_id).map_or(0, |c| now_ms.saturating_sub(c.since_ms))
}

/// A claim has run out `CLAIM_TIMEOUT_MS` after its holder last claimed the
/// case. A named subagent's runs out sooner: `agents::NAMED_HOLD_MS` after
/// it was made, once the holder has made no call for that long either (a
/// holder the registry has forgotten counts as having made none), and never
/// while a browser of the holder's is in use (`autorun_slots::agent_busy`).
fn expired(claim: &Claim, now_ms: u64) -> bool {
    let age = now_ms.saturating_sub(claim.at_ms);
    if age >= CLAIM_TIMEOUT_MS {
        return true;
    }
    claim.holder.name.is_some()
        && age >= agents::NAMED_HOLD_MS
        && !crate::commands::autorun_slots::agent_busy(&claim.holder)
        && agents::info(&claim.holder).map_or(true, |i| now_ms.saturating_sub(i.last_ms) >= agents::NAMED_HOLD_MS)
}

/// `key` works on `case_id` from now: claimed, or renewed when it already
/// held it. Refused with `case_claimed` when another LIVE agent claimed it
/// less than `CLAIM_TIMEOUT_MS` before `now_ms`; a holder that has gone, or
/// whose claim ran out, is taken over. `Ok(true)` when this call made the
/// hold (new, or taken over), `Ok(false)` when it renewed one the key had.
pub fn claim(key: &AgentKey, case_id: i32, now_ms: u64) -> Result<bool, String> {
    let mut all = claims();
    all.retain(|c| !expired(c, now_ms));
    match all.iter_mut().find(|c| c.case_id == case_id) {
        Some(c) if c.holder == *key => {
            c.at_ms = now_ms;
            Ok(false)
        }
        Some(c) => match agents::info(&c.holder).filter(|i| i.gone_since_ms.is_none()) {
            Some(held_by) => Err(case_claimed(case_id, &held_by.label, now_ms.saturating_sub(c.since_ms))),
            None => {
                c.holder = key.clone();
                c.since_ms = now_ms;
                c.at_ms = now_ms;
                Ok(true)
            }
        },
        None => {
            all.push(Claim { case_id, holder: key.clone(), since_ms: now_ms, at_ms: now_ms });
            Ok(true)
        }
    }
}

/// `key`'s hold on `case_id` ends; false when it held none.
pub fn release_case(key: &AgentKey, case_id: i32) -> bool {
    let mut all = claims();
    let before = all.len();
    all.retain(|c| !(c.case_id == case_id && c.holder == *key));
    before != all.len()
}

/// The agent holding `case_id` at `now_ms`, if a claim on it has not run out.
/// It does not say whether that agent is still live (`agents::is_live`).
pub fn holder(case_id: i32, now_ms: u64) -> Option<AgentKey> {
    claims().iter().find(|c| c.case_id == case_id && !expired(c, now_ms)).map(|c| c.holder.clone())
}

/// `key` works on nothing: every claim it holds ends.
pub fn end_for(key: &AgentKey) {
    release_of(key);
}

/// The claims `key` made at or before `last_ms` end (all of them when `None`:
/// the registry had forgotten it); one made by a call after that stays.
pub fn release_before(key: &AgentKey, last_ms: Option<u64>) {
    claims().retain(|c| c.holder != *key || last_ms.is_some_and(|last| c.at_ms > last));
}

/// `end_for`, answering how many claims ended.
pub fn release_of(key: &AgentKey) -> usize {
    let mut all = claims();
    let before = all.len();
    all.retain(|c| c.holder != *key);
    before - all.len()
}

/// The cases `key` holds at `now_ms`, the one it claimed first first.
pub fn cases_of(key: &AgentKey, now_ms: u64) -> Vec<i32> {
    let mut mine: Vec<(u64, i32)> =
        claims().iter().filter(|c| c.holder == *key && !expired(c, now_ms)).map(|c| (c.at_ms, c.case_id)).collect();
    mine.sort();
    mine.into_iter().map(|(_, id)| id).collect()
}

/// Every claim (tests).
pub fn clear() {
    claims().clear();
}
