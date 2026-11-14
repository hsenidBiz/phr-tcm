//! Accounts: proposing them, checking a sign-in, and reading them.

use super::*;

/// Said for a proposal whose accounts do not have the expected shape -
/// in place of the parser's own words, which can quote a password.
pub const PROPOSAL_SHAPE_REFUSED: &str = "each account needs a key, a label and a username as text, and a password as text when one is given - and no other field";

/// `POST /accounts-propose`: the assistant's proposed logins for the active
/// environment, REPLACING whatever it proposed before, passwords included.
/// Each may carry the password the assistant read in the same database
/// lookup - accepted only for an environment marked as a test environment,
/// and otherwise the whole call is refused. A password is never logged or
/// repeated in an answer, and nothing reaches the accounts until a person
/// adds it in the app.
pub(crate) async fn accounts_propose(ctx: &BridgeContext, body: &str) -> (u16, String) {
    use crate::environments::{
        active, proposed_password_missing, save_proposals, StoredProposal, PROPOSED_PASSWORD_NOT_TEST,
    };
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Proposal {
        key: String,
        #[serde(default)]
        label: String,
        username: String,
        #[serde(default)]
        role: Option<String>,
        #[serde(default)]
        password: Option<String>,
        #[serde(default)]
        no_password: Option<String>,
    }
    let shape = "{ \"accounts\": [{ \"key\": \"hr.supervisor\", \"label\": \"HR supervisor\", \"username\": \"sup1\", \"role\"?: \"Supervisor\", \"password\"?: \"(test environments only)\", \"no_password\"?: \"why there is none\" }] }";
    let raw = match body_field(body, "accounts", shape) {
        Ok(v) => v,
        Err(refused) => return refused,
    };
    let raw = json_arg(Some(&raw)).unwrap_or(raw);
    // No parser text in the answer: serde's would quote a value it could
    // not read, and the value may be a password.
    let list: Vec<Proposal> = match serde_json::from_value(raw) {
        Ok(l) => l,
        Err(_) => return (400, format!("{PROPOSAL_SHAPE_REFUSED}. Expected {shape}.")),
    };
    if list.is_empty() {
        return (400, format!("\"accounts\" needs at least one account. Expected {shape}."));
    }
    // A password and the reason for having none say opposite things.
    if let Some(p) = list.iter().find(|p| p.password.is_some() && p.no_password.is_some()) {
        return (
            400,
            format!("the account \"{}\" has both a password and no_password - give one or the other", p.key.trim()),
        );
    }
    let mut proposals: Vec<StoredProposal> = list
        .into_iter()
        .map(|p| {
            let key = p.key.trim().to_string();
            let label = match p.label.trim() {
                "" => key.clone(),
                l => l.to_string(),
            };
            let role = p.role.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
            // Kept exactly as read: a password's spaces are its own.
            let no_password = p.no_password.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
            StoredProposal {
                key,
                label,
                username: p.username.trim().to_string(),
                role,
                password: p.password,
                no_password,
                marks: vec![],
                proposed_ms: Some(crate::autorun::sessions::now_ms()),
            }
        })
        .collect();
    let with_password = proposals.iter().filter(|p| p.password.is_some()).count();
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let env = match active(&root) {
        Ok(e) => e,
        Err(e) => return (500, e),
    };
    if env.test_environment {
        // Every account says what its password is, or why it has none.
        let missing: Vec<&str> = proposals
            .iter()
            .filter(|p| p.password.is_none() && p.no_password.is_none())
            .map(|p| p.key.as_str())
            .collect();
        if !missing.is_empty() {
            return (400, proposed_password_missing(&missing));
        }
    } else {
        if with_password > 0 {
            return (400, PROPOSED_PASSWORD_NOT_TEST.to_string());
        }
        // Only a test environment keeps passwords, so only there is a
        // reason for a missing one worth keeping.
        for p in &mut proposals {
            p.no_password = None;
        }
    }
    // Which logins can sign in at all: expired ones and ones someone is
    // signed in as are marked, and the usable ones come first. A mark never
    // carries a password.
    let marks = propose_marks(ctx, &root, &env, &proposals).await;
    if let Some(m) = &marks {
        // Kept with the proposal, for the person ticking logins in the app.
        for p in &mut proposals {
            p.marks = m.marks_of(&p.key);
        }
        let order = m.usable_first(&proposals.iter().map(|p| p.key.clone()).collect::<Vec<_>>());
        proposals.sort_by_key(|p| order.iter().position(|k| *k == p.key).unwrap_or(usize::MAX));
    }
    // Within the same marks, the logins an earlier Check sign-in found
    // reaching their area come first, the never-checked next.
    let checked = crate::autorun::sign_in_check::load_marks(&root, &env.id);
    let wanted_area = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("area").and_then(|a| a.as_str().map(str::to_string)))
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty());
    let now = crate::autorun::sessions::now_ms();
    if !checked.is_empty() {
        proposals.sort_by_key(|p| {
            crate::autorun::sign_in_check::proposal_rank(&p.marks, &checked, &p.key, wanted_area.as_deref(), now)
        });
    }
    if let Err(why) = save_proposals(&root, &env.id, &proposals) {
        return (400, why);
    }
    crate::applog::info(format!(
        "Environments: the assistant proposed {} account(s) for {}, {with_password} with a password",
        proposals.len(),
        env.name
    ));
    let mut said = format!(
        "proposed {} account(s) for the environment {}, {with_password} with a password - a person picks which to add in the app, and a proposed password is used unless they type another. Another call replaces this proposal.",
        proposals.len(),
        env.name
    );
    if let Some(m) = &marks {
        let marked: Vec<String> = m
            .marks
            .iter()
            .filter(|(_, marks)| !marks.is_empty())
            .map(|(key, marks)| format!("{key} ({})", marks.join(", ")))
            .collect();
        if !marked.is_empty() {
            said.push_str(&format!(
                " Marked: {}. A login that is expired cannot sign in until its password is reset; one signed in elsewhere is logged out by a sign-in. The unmarked logins are listed first - prefer them.",
                marked.join("; ")
            ));
        }
        if let Some(note) = &m.note {
            said.push_str(&format!(" Note: {note}."));
        }
    }
    let earlier: Vec<String> = proposals
        .iter()
        .filter_map(|p| checked.get(&p.key).map(|m| format!("{} ({})", p.key, crate::autorun::sign_in_check::mark_phrase(m, now))))
        .collect();
    if !earlier.is_empty() {
        said.push_str(&format!(
            " Checked earlier with check_autorun_sign_in: {}. The ones that reached the area asked for (`area`) are listed first; a check for another area, or over a week old, does not rank.",
            earlier.join("; ")
        ));
    }
    (200, said)
}

/// What `POST /autorun-check-sign-in` reads: `accounts` (account keys, one
/// to three) and `area` (an area name; the sign-in alone when absent).
pub(crate) const CHECK_SIGN_IN_SHAPE: &str = "{ \"accounts\": [<an account key>, at most 3], \"area\": <an area name>, optional }";

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CheckSignInBody {
    #[serde(default)]
    pub(crate) accounts: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) area: Option<String>,
}

/// `check_autorun_sign_in`, with the browsers, the database and the project
/// given so a test can drive it: each named account (every saved one when
/// none is) is signed in once, in a browser of its own, and walked to the
/// area if one is named. Answers `{ checked: [{ account, summary, signed_in,
/// landed, stopped, password, area }] }`. An account that is not saved is
/// refused first, naming it, before any browser opens.
pub async fn check_sign_in_in<B: crate::autorun::replay::Browsers, S: crate::autorun::setup_find::RowSource>(
    browsers: &mut B,
    rows: Option<&S>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    body: &str,
) -> (u16, String) {
    let asked: CheckSignInBody = match serde_json::from_str(if body.trim().is_empty() { "{}" } else { body }) {
        Ok(a) => a,
        Err(_) => return (400, format!("that is not what check_autorun_sign_in reads. Expected {CHECK_SIGN_IN_SHAPE}.")),
    };
    let saved = match crate::autorun::accounts::load_accounts(root) {
        Ok(a) => a,
        Err(e) => return (500, e),
    };
    let Some(named_keys) = asked.accounts else {
        return (
            400,
            format!(
                "check_autorun_sign_in needs `accounts`, the keys of the accounts to check (at most {} per call; get_accounts lists them). Expected {CHECK_SIGN_IN_SHAPE}.",
                crate::autorun::sign_in_check::MOST_ACCOUNTS
            ),
        );
    };
    let mut keys: Vec<String> = Vec::new();
    for k in named_keys.iter().map(|k| k.trim()).filter(|k| !k.is_empty()) {
        // An account named twice is checked, and counted, once.
        if !keys.iter().any(|seen| seen == k) {
            keys.push(k.to_string());
        }
    }
    if keys.is_empty() {
        return (400, format!("`accounts` names no account. Expected {CHECK_SIGN_IN_SHAPE}."));
    }
    if keys.len() > crate::autorun::sign_in_check::MOST_ACCOUNTS {
        return (
            400,
            format!(
                "{} accounts in one call is too many - at most {} at a time, as each sign-in takes as long as one does. Split them across calls.",
                keys.len(),
                crate::autorun::sign_in_check::MOST_ACCOUNTS
            ),
        );
    }
    let strangers: Vec<&str> = keys.iter().filter(|k| !saved.iter().any(|a| &a.key == *k)).map(String::as_str).collect();
    if !strangers.is_empty() {
        return (
            400,
            format!(
                "no account named {} is saved on this machine - get_accounts lists the keys",
                strangers.iter().map(|k| format!("\"{k}\"")).collect::<Vec<_>>().join(", ")
            ),
        );
    }
    let area = named(asked.area.as_deref());
    let env = match crate::environments::active(root) {
        Ok(e) => e,
        Err(e) => return (500, e),
    };
    let queries = (env.password_expiry_query.as_str(), env.active_session_query.as_str());
    // A check that never starts spends nothing: busy is said first, then
    // every account's cap before any is counted. Counting is per call, not
    // per account's browser: an account the precheck then skips (expired
    // password, session held elsewhere) still spends the attempt.
    if let Some(busy) = crate::commands::autorun_check_signin::refusal() {
        return (409, busy);
    }
    for key in &keys {
        if let Some(why) = crate::autorun::signin_attempts::would_refuse(root, key) {
            return (409, why);
        }
    }
    for key in &keys {
        if let Err(why) = crate::autorun::signin_attempts::take_if_open(root, key) {
            return (409, why);
        }
    }
    let done = crate::commands::autorun_check_signin::check_with(
        browsers, rows, queries, root, organization, project, &keys, area.as_deref(),
    )
    .await;
    match done {
        Err(why) => (409, why),
        Ok(list) => {
            let checked: Vec<serde_json::Value> = list
                .iter()
                .map(|c| {
                    let mut v = serde_json::to_value(c).unwrap_or_default();
                    v["summary"] = serde_json::json!(crate::autorun::sign_in_check::summary(c));
                    v
                })
                .collect();
            // An account that signed in ends its sign-in problem.
            for c in list.iter().filter(|c| c.signed_in) {
                crate::autorun::signin_attempts::close(root, &c.account);
            }
            (200, serde_json::json!({ "checked": checked }).to_string())
        }
    }
}

/// `POST /autorun-check-sign-in` in the app: real browsers (headless, the
/// one last chosen), and the environment's database when reading it is on.
pub(crate) async fn autorun_check_sign_in(ctx: &BridgeContext, body: &str) -> (u16, String) {
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    // Said before anything is read or started.
    if let Some(busy) = crate::commands::autorun_check_signin::refusal() {
        return (409, busy);
    }
    let db = stage_db(ctx).ok();
    let rows = db.as_ref().map(crate::autorun::active_login::StageRows);
    let mut browsers = crate::commands::autorun_replay::RealBrowsers::new(
        crate::browser::launch::Browser::from_name(&crate::autorun::store::last_browser(&root)),
        false,
    );
    check_sign_in_in(&mut browsers, rows.as_ref(), &root, &ctx.org, &ctx.project, body).await
}

/// `POST /autorun-inspect-sign-in` and `POST /autorun-propose-sign-in` in the
/// app: real browsers (headless, the one last chosen).
pub(crate) async fn autorun_sign_in_proposal(ctx: &BridgeContext, body: &str, propose: bool) -> (u16, String) {
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let mut browsers = crate::commands::autorun_replay::RealBrowsers::new(
        crate::browser::launch::Browser::from_name(&crate::autorun::store::last_browser(&root)),
        false,
    );
    let timing = crate::browser::timing::Timing::supervised();
    if propose {
        crate::commands::autorun_signin_proposal::propose_in(&mut browsers, &root, &ctx.org, &ctx.project, body, &timing).await
    } else {
        crate::commands::autorun_signin_proposal::inspect_in(&mut browsers, &root, &ctx.org, &ctx.project, body, &timing).await
    }
}

/// The marks of the logins an assistant proposes, from the environment's
/// password expiry and active session queries. `None` when neither is set
/// (nothing asked, nothing said). A database that is off or does not answer
/// marks nothing and says so once.
pub(crate) async fn propose_marks(
    ctx: &BridgeContext,
    root: &std::path::Path,
    env: &crate::environments::Environment,
    proposals: &[crate::environments::StoredProposal],
) -> Option<crate::autorun::active_login::Marks> {
    use crate::autorun::active_login::{mark_logins, Marks, StageRows, NOT_CHECKED_NOTE};
    if env.password_expiry_query.trim().is_empty() && env.active_session_query.trim().is_empty() {
        return None;
    }
    let Ok(db) = stage_db(ctx) else {
        return Some(Marks { marks: vec![], note: Some(NOT_CHECKED_NOTE.to_string()) });
    };
    let logins: Vec<(String, String)> = proposals.iter().map(|p| (p.key.clone(), p.username.clone())).collect();
    let env_id = env.id.clone();
    let minutes = crate::autorun::recipe::load_effective_recipe_if_any(root, &ctx.org, &ctx.project)
        .ok()
        .flatten()
        .map_or(480, |r| r.session_minutes);
    let root = root.to_path_buf();
    // This app's own earlier sign-ins are not someone else's.
    let ours = move |key: &str| {
        crate::autorun::active_login::session_ownership(&root, &env_id, key, minutes, crate::autorun::sessions::now_ms())
    };
    Some(
        mark_logins(
            &StageRows(&db),
            &env.password_expiry_query,
            &env.active_session_query,
            &logins,
            &ours,
            &crate::autorun::password_expiry::today(),
        )
        .await,
    )
}

/// Said when `GET /accounts` arrives while the person has switched the
/// `get_accounts` tool off.
pub const GET_ACCOUNTS_OFF: &str = "reading the accounts is switched off in Test Case Manager";

/// `GET /accounts`: the active environment's accounts - key, label,
/// username and whether a password is set (`has_password`). Never the
/// password itself, in any environment: Auto Run signs in by itself, so a
/// script names an account by its key. The log line says only how many were
/// read. Beside them, the
/// project's Test files (`test_files_answer`), or `test_files_note` saying
/// why there are none to show. An account that is held right now carries
/// `in_use_by`, who holds it as the caller reads it (`in_use_by_words`); a
/// free one has no such key.
///
/// Refused while the person has switched `get_accounts` off on the AI
/// Bridge tab. The MCP proxy already hides a switched-off tool; this route
/// checks again itself, as the database routes do (`stage_db`).
pub(crate) fn accounts_read(ctx: &BridgeContext, caller_slot: Option<SlotNo>) -> (u16, String) {
    if ctx.disabled_tools.iter().any(|t| t == "get_accounts") {
        return (409, GET_ACCOUNTS_OFF.to_string());
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let files = test_files_answer(&root, ctx);
    // The Test files do not hang on the environment: when its accounts
    // cannot be read they are still answered, with why the accounts are
    // not. Only when neither can be read is the call a failure.
    let accounts_failed = |e: String| -> (u16, String) {
        match &files {
            Ok(rows) => (200, serde_json::json!({ "accounts_note": e, "test_files": rows }).to_string()),
            Err(_) => (500, e),
        }
    };
    let env = match crate::environments::active(&root) {
        Ok(e) => e,
        Err(e) => return accounts_failed(e),
    };
    // Read by the id just read, so the accounts and the test-environment
    // mark are the same environment's even if a switch lands in between.
    let accounts = match crate::autorun::accounts::load_accounts_for(&root, &env.id) {
        Ok(a) => a,
        Err(e) => return accounts_failed(e),
    };
    let shown = env.test_environment;
    // Who holds each account right now, as this caller reads it: a free
    // account has no `in_use_by`. Main's session is looked at only when it
    // holds one, and never waited for.
    let held = crate::autorun::lease::holders_in(&env.id);
    let main_is_persons = held.iter().any(|(_, h)| matches!(h, crate::autorun::lease::Holder::Browser { slot: 0 }))
        && main_is_opened_by_the_person();
    let list: Vec<serde_json::Value> = accounts
        .iter()
        .map(|a| {
            let mut one = serde_json::json!({
                "key": a.key,
                "label": a.label,
                "username": a.username,
                "has_password": !a.password.trim().is_empty(),
            });
            if let Some((_, holder)) = held.iter().find(|(key, _)| *key == a.key) {
                one["in_use_by"] = serde_json::Value::String(in_use_by_words(holder, caller_slot, main_is_persons));
            }
            one
        })
        .collect();
    let mut out = serde_json::json!({
        "environment": env.name,
        "test_environment": shown,
        "accounts": list,
    });
    match files {
        Ok(files) => out["test_files"] = serde_json::Value::Array(files),
        Err(why) => out["test_files_note"] = serde_json::Value::String(why),
    }
    crate::applog::info(format!("AI bridge: the assistant read the {} account(s) of {}", accounts.len(), env.name));
    (200, out.to_string())
}

/// Whether main's session is open and the person opened it (`OpenedBy`),
/// read without waiting: a session busy with a step or a replay reads as not.
pub(crate) fn main_is_opened_by_the_person() -> bool {
    use crate::commands::autorun::{session_slot, try_lock_briefly, OpenedBy};
    try_lock_briefly(session_slot(SlotNo::MAIN))
        .is_some_and(|session| session.as_ref().is_some_and(|s| s.opened_by == OpenedBy::Person))
}

/// The live assistant a slot's browser is in use by, by label.
pub(crate) fn label_of_slot(slot: SlotNo) -> Option<String> {
    let key = crate::commands::autorun_slots::bound(slot)?;
    crate::autorun::agents::info(&key).filter(|i| i.gone_since_ms.is_none()).map(|i| i.label)
}

/// `get_accounts`' `in_use_by` for one held account, as `caller_slot` (the
/// asker's own browser, if it has one) reads it:
///
/// - the asker's own browser: "your browser";
/// - an assistant's: "{label}'s browser";
/// - main's, bound to nobody: "the person's browser" when the person opened
///   it (`main_is_persons`), else "the main Auto Run browser";
/// - an extra browser nobody is bound to now (its assistant is gone and the
///   browser not yet released): "another Auto Run browser";
/// - an unattended run, an API template run, Auto Run setup: as the
///   refusals word them.
///
/// Labels only: never a login, an id or a host.
pub fn in_use_by_words(holder: &crate::autorun::lease::Holder, caller_slot: Option<SlotNo>, main_is_persons: bool) -> String {
    use crate::autorun::lease::Holder;
    match holder {
        Holder::Browser { slot } => {
            let Some(slot) = SlotNo::from_number(*slot) else {
                return "another Auto Run browser".to_string();
            };
            if caller_slot == Some(slot) {
                "your browser".to_string()
            } else if let Some(label) = label_of_slot(slot) {
                format!("{label}'s browser")
            } else if slot.is_main() {
                (if main_is_persons { "the person's browser" } else { "the main Auto Run browser" }).to_string()
            } else {
                "another Auto Run browser".to_string()
            }
        }
        Holder::Case { .. } => "an unattended run".to_string(),
        Holder::Template => "an API template run".to_string(),
        Holder::Setup => "Auto Run setup".to_string(),
    }
}

/// An answer holding a lease refusal for an account whose holder is another
/// slot's browser, with that browser's live assistant named right after the
/// refusal's sentence: ". {label} is signed in as it: work on cases that
/// sign in as another account, or wait until {label} is done." The refusal
/// itself stays word for word (`lease::is_in_use` still matches it). An
/// answer with no refusal, an account held by the asker's own browser, by
/// anything else, or by a browser no live assistant is bound to comes back
/// unchanged. `env` is the active environment's id, the key the lease table
/// uses. Labels only: never a login or an id.
pub fn with_holder_named(answer: (u16, String), caller_slot: impl Into<Option<SlotNo>>, env: &str) -> (u16, String) {
    with_holder_named_at(answer, caller_slot, env, crate::autorun::sessions::now_ms())
}

/// `with_holder_named` at `now_ms`: the sentence also says how long it has
/// been since the holder's last call, worded as the reset's holds note words
/// it ("no call for <1 min" / "no call for N min").
pub fn with_holder_named_at(
    answer: (u16, String),
    caller_slot: impl Into<Option<SlotNo>>,
    env: &str,
    now_ms: u64,
) -> (u16, String) {
    use crate::autorun::lease::{holder_of, locate_in_use, Holder};
    let (status, text) = answer;
    let Some((key, end)) = locate_in_use(&text).map(|(key, _, end)| (key.to_string(), end)) else {
        return (status, text);
    };
    let caller_slot: Option<SlotNo> = caller_slot.into();
    let named = (|| {
        let Holder::Browser { slot } = holder_of(env, &key)? else { return None };
        let slot = SlotNo::from_number(slot)?;
        if caller_slot == Some(slot) {
            return None;
        }
        let label = label_of_slot(slot)?;
        let quiet = crate::commands::autorun_slots::bound(slot)
            .and_then(|key| crate::autorun::agents::info(&key))
            .map(|info| crate::commands::autorun_slots::quiet_words(now_ms.saturating_sub(info.last_ms) / 1000));
        Some((label, quiet))
    })();
    let Some((label, quiet)) = named else { return (status, text) };
    let quiet = quiet.map(|q| format!(" (no call for {q})")).unwrap_or_default();
    // The sentence's own full stop, if the answer has one, is the one this ends with.
    let rest = text[end..].strip_prefix('.').unwrap_or(&text[end..]);
    let said = format!(
        ". {label} is signed in as it{quiet}: work on cases that sign in as another account, or wait until {label} is done."
    );
    (status, format!("{}{said}{rest}", &text[..end]))
}

/// `with_holder_named` for the answer to one request, in the active
/// environment, for the routes that sign an account in: those that open or
/// act on the caller's browser, and the API template runs. Any other route's
/// answer is returned as it is - a failure listing may quote an old refusal.
pub(crate) fn name_the_holder(
    answer: (u16, String),
    caller: &crate::autorun::agents::Caller,
    method: &str,
    path: &str,
) -> (u16, String) {
    use crate::commands::autorun_slots::{need_of, slot_if_any, Need};
    let signs_in = matches!(need_of(method, path), Some(Need::Open | Need::Act))
        || (method == "POST" && matches!(path, "/api-template-run" | "/api-template-prove"));
    if !signs_in || crate::autorun::lease::locate_in_use(&answer.1).is_none() {
        return answer;
    }
    let env = autorun_root().ok().and_then(|root| crate::environments::active_id(&root).ok());
    match env {
        Some(env) => with_holder_named(answer, slot_if_any(caller), &env),
        None => answer,
    }
}

// ------------------------------------------------- case claims, one mapping run

