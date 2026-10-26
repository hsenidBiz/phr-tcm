//! Which Auto Run browser a bridge call uses.
//!
//! `slot_for` is asked once, at the top of the bridge's router
//! (`ai_bridge::route_as`), and the slot it answers is the one every handler
//! of that call locks and publishes to. Today every caller gets the main
//! browser; the rules that give an assistant a browser of its own come with
//! the extra browsers.
//!
//! Beside that sit the bindings (which assistant a slot's browser is in use
//! by), the views the Assistants card shows and the card's commands.

use crate::autorun::agents::{self, AgentKey, Caller};
use crate::autorun::slots::{SlotNo, SLOT_COUNT};
use crate::commands::autorun::{session_slot, Session};
use std::sync::Mutex;

/// What a route does with a supervised browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// It may open one: a discovery's start, a replay.
    Open,
    /// It acts on the one the caller has, or ends it: a try, a discovery
    /// action, a release, a reset, a header.
    Act,
    /// It only reads the one the caller has: a page, a probe, the captured
    /// requests.
    Read,
}

/// What the route `method path` needs of a supervised browser. `None`: the
/// route uses no supervised browser (a file, Azure DevOps, the database, the
/// process-wide imported capture sets, or `/api-template-run`, which asks
/// `slot_if_any` itself).
pub fn need_of(method: &str, path: &str) -> Option<Need> {
    match (method, path) {
        ("POST", "/autorun-discover-start" | "/autorun-replay") => Some(Need::Open),
        (
            "POST",
            "/autorun-try"
            | "/autorun-discover-action"
            | "/autorun-discover-actions"
            | "/autorun-discover-area"
            | "/autorun-discover-end"
            | "/autorun-replay-stop"
            | "/autorun-release"
            | "/autorun-reset"
            | "/autorun-cookies-clear"
            | "/autorun-headers"
            | "/autorun-cookies"
            | "/autorun-cookie"
            | "/api-template-send"
            | "/api-template-explore-undo",
        ) => Some(Need::Act),
        ("GET", "/autorun-page" | "/autorun-capture") => Some(Need::Read),
        (
            "POST",
            "/autorun-probe"
            | "/autorun-inspect"
            | "/autorun-requests"
            | "/autorun-requests-search"
            | "/autorun-request"
            | "/autorun-response"
            | "/autorun-response-body"
            | "/autorun-capture"
            | "/autorun-requests-export"
            | "/autorun-component-save"
            | "/autorun-validation-rule"
            | "/autorun-explore-change",
        ) => Some(Need::Read),
        _ => None,
    }
}

/// The browser `caller` uses for a route that `need`s one. For now every
/// caller, a default one or an agent, gets the main browser, so nothing
/// changes for anyone; the refusals (no browser to spare, the account is
/// held) come with the rules that give an assistant a browser of its own.
///
/// An agent that opens or acts on the main browser is bound to it
/// (`bind`), so the Assistants card can say whose it is; one that only
/// reads it binds nothing, and the default caller never binds.
pub fn slot_for(caller: &Caller, need: Need) -> Result<SlotNo, (u16, String)> {
    if let Caller::Agent(key) = caller {
        if matches!(need, Need::Open | Need::Act) {
            bind(SlotNo::MAIN, key);
        }
    }
    Ok(SlotNo::MAIN)
}

/// The caller's slot when it has one, else `None`, and never a refusal: for
/// the routes that use a browser only when the caller has one
/// (`/api-template-run`). For now every caller has the main browser.
pub fn slot_if_any(_caller: &Caller) -> Option<SlotNo> {
    Some(SlotNo::MAIN)
}

// ---------------------------------------------------------------------------
// Bindings: which assistant a slot's browser is in use by
// ---------------------------------------------------------------------------

/// The assistant each slot's browser is in use by, if any. Coordination
/// state, not a cache: it says who holds a browser right now, so nothing
/// here may be stored on disk or kept across a restart, and nothing else in
/// the app may keep a second copy of it (the same rule as `IMPORTING` in
/// `commands/autorun.rs`). One `static` mutex, never held across an await.
/// An agent is bound to at most one slot.
static BINDINGS: Mutex<[Option<AgentKey>; SLOT_COUNT]> = Mutex::new([const { None }; SLOT_COUNT]);

fn bindings() -> std::sync::MutexGuard<'static, [Option<AgentKey>; SLOT_COUNT]> {
    BINDINGS.lock().unwrap_or_else(|e| e.into_inner())
}

/// The assistant slot `n`'s browser is in use by.
pub fn bound(n: SlotNo) -> Option<AgentKey> {
    bindings()[n.index()].clone()
}

/// The slot `key` is bound to.
pub fn slot_of(key: &AgentKey) -> Option<SlotNo> {
    bindings().iter().position(|b| b.as_ref() == Some(key)).map(|i| SlotNo(i as u8))
}

/// Slot `n`'s browser is in use by `key` from now on, and `key` is bound
/// to no other slot. Whoever held `n` before no longer does.
pub fn bind(n: SlotNo, key: &AgentKey) {
    let mut b = bindings();
    for held in b.iter_mut() {
        if held.as_ref() == Some(key) {
            *held = None;
        }
    }
    b[n.index()] = Some(key.clone());
}

/// Slot `n`'s browser is in use by nobody.
pub fn unbind(n: SlotNo) {
    bindings()[n.index()] = None;
}

/// `key` holds no browser.
pub fn unbind_agent(key: &AgentKey) {
    for held in bindings().iter_mut() {
        if held.as_ref() == Some(key) {
            *held = None;
        }
    }
}

/// Every binding and every remembered reading of a slot (tests).
pub fn clear() {
    *bindings() = [const { None }; SLOT_COUNT];
    for f in FACTS.iter() {
        *f.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// An agent that has gone past its grace (`agents::Sweep::past_grace`) holds
/// no browser. A quiet one (`agents::Sweep::quiet`: no call for an hour, no
/// session seen to end) no longer holds the main browser; an extra browser it
/// holds is left to the idle release. Called by the sweeper after every sweep.
pub fn on_sweep(s: &agents::Sweep) {
    for key in &s.past_grace {
        unbind_agent(key);
    }
    let mut held = bindings();
    for key in &s.quiet {
        if held[SlotNo::MAIN.index()].as_ref() == Some(key) {
            held[SlotNo::MAIN.index()] = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Views: what the Assistants card shows
// ---------------------------------------------------------------------------

/// What an assistant is doing in its browser.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentActivity {
    Idle,
    Discovering { area: Option<String> },
    Mapping { modules: Vec<String> },
    Exploring,
    Replaying { case_id: i32, step: i32, of: i32 },
    /// Its browser is busy with something not known (a call going).
    Working,
}

/// One connected assistant, as the card shows it. Carries neither the
/// agent's id nor its pid, and no login, host or address: the account is
/// the key the browser's lease holds.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct AgentView {
    pub label: String,
    pub client: Option<String>,
    /// The browser slot it is bound to.
    pub slot: Option<u8>,
    /// Its slot is an extra one, not the main browser.
    pub own_browser: bool,
    /// Which browser it is (`Browser::label`), when its slot has a session.
    pub browser: Option<String>,
    /// The account key its slot's lease holds, never a login.
    pub account: Option<String>,
    pub activity: AgentActivity,
    /// Whole minutes since its last call.
    pub idle_minutes: u32,
    /// Its slot's browser has changes set for testing.
    pub changed_for_testing: bool,
    /// Its slot's browser stops the page's saves.
    pub saves_blocked: bool,
    /// Its slot runs an assistant's replay or a batch, which Stop ends.
    pub can_stop: bool,
}

/// What was last read from a slot's session. A busy slot (a replay, a step,
/// a call holding its lock) is never waited for: it reports what was read
/// last. A reading of what the slot held, kept only so the card never waits
/// on a lock; each slot's is replaced by every successful reading and
/// emptied by `clear`. Not shared data, not saved.
#[derive(Debug, Clone, Default)]
struct SlotFacts {
    /// What the session says (a replay is read from the slot's replay state,
    /// which needs no lock).
    activity: Option<AgentActivity>,
    browser: Option<String>,
    account: Option<String>,
    saves_blocked: bool,
}

static FACTS: [Mutex<Option<SlotFacts>>; SLOT_COUNT] = [const { Mutex::new(None) }; SLOT_COUNT];

fn facts_of(slot: &Option<Session>) -> SlotFacts {
    let Some(s) = slot.as_ref() else {
        return SlotFacts { activity: Some(AgentActivity::Idle), ..SlotFacts::default() };
    };
    let activity = match s.discovery.as_ref() {
        None => AgentActivity::Idle,
        Some(d) => match (&d.mapping, &d.explore) {
            (Some(m), _) => AgentActivity::Mapping { modules: m.modules.clone() },
            (None, Some(_)) => AgentActivity::Exploring,
            (None, None) => AgentActivity::Discovering { area: d.area.clone() },
        },
    };
    SlotFacts {
        activity: Some(activity),
        browser: Some(s.kind.label().to_string()),
        account: s.lease.account().map(str::to_string),
        saves_blocked: s.cdp.is_guarding_saves(),
    }
}

/// Slot `n`'s session read without waiting; a busy slot answers what was
/// read last, `None` when nothing was read yet.
fn slot_facts(n: SlotNo) -> Option<SlotFacts> {
    let mut kept = FACTS[n.index()].lock().unwrap_or_else(|e| e.into_inner());
    match session_slot(n).try_lock() {
        Ok(slot) => {
            let facts = facts_of(&slot);
            *kept = Some(facts.clone());
            Some(facts)
        }
        Err(_) => kept.clone(),
    }
}

/// What the replay going in slot `n` is, when it is an assistant's: the
/// person's replay is never shown as an assistant's.
fn replaying(n: SlotNo) -> Option<AgentActivity> {
    let replay = crate::autorun::replay_to::state(n);
    if !replay.assistants_replay_running() {
        return None;
    }
    let (case_id, _target) = crate::ai_bridge::assistant_replay_of(n)?;
    Some(replaying_activity(case_id, replay.progress()))
}

/// An assistant's replay of `case_id` that is on step `progress.0` of
/// `progress.1`. Before its first step begins that is honestly `0 of 0`:
/// the step it was asked to reach is where it stops, not a step it is on.
pub fn replaying_activity(case_id: i32, progress: (i32, i32)) -> AgentActivity {
    AgentActivity::Replaying { case_id, step: progress.0, of: progress.1 }
}

/// The agents `agents::live()` lists, by number, each with what its slot's
/// browser holds. Slot facts are read with `try_lock` only; a busy slot
/// reports its last known activity, `Working` when none is known.
pub fn agent_views(now_ms: u64) -> Vec<AgentView> {
    agents::live()
        .into_iter()
        .map(|info| {
            let idle_minutes = u32::try_from(now_ms.saturating_sub(info.last_ms) / 60_000).unwrap_or(u32::MAX);
            let base = AgentView {
                label: info.label,
                client: info.client,
                slot: None,
                own_browser: false,
                browser: None,
                account: None,
                activity: AgentActivity::Idle,
                idle_minutes,
                changed_for_testing: false,
                saves_blocked: false,
                can_stop: false,
            };
            let Some(n) = slot_of(&info.key) else { return base };
            let facts = slot_facts(n);
            let activity = replaying(n).or_else(|| facts.as_ref().and_then(|f| f.activity.clone()));
            AgentView {
                slot: Some(n.0),
                own_browser: !n.is_main(),
                browser: facts.as_ref().and_then(|f| f.browser.clone()),
                account: facts.as_ref().and_then(|f| f.account.clone()),
                // The activity is read, not assumed: a busy slot nobody has
                // read yet is working at something.
                activity: activity.unwrap_or(AgentActivity::Working),
                changed_for_testing: !crate::commands::autorun::overrides_now_in(n).is_empty(),
                saves_blocked: facts.as_ref().is_some_and(|f| f.saves_blocked),
                can_stop: crate::autorun::replay_to::state(n).assistants_replay_running()
                    || crate::ai_bridge::batch_going(n),
                ..base
            }
        })
        .collect()
}

/// The views if they differ from `last` (the ones sent before), which then
/// become `last`.
pub fn next_views_to_send(last: &mut Vec<AgentView>, now_ms: u64) -> Option<Vec<AgentView>> {
    let views = agent_views(now_ms);
    if views == *last {
        return None;
    }
    *last = views.clone();
    Some(views)
}

/// Every second, send `AutorunAgentsChanged` when the views differ from the
/// last ones sent: so the event goes at most once a second, and a change
/// site needs to tell no one. Started once, beside `set_discovery_events`;
/// a second call does nothing.
pub fn spawn_agents_watch(app: tauri::AppHandle) {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        tauri::async_runtime::spawn(async move {
            use tauri_specta::Event as _;
            let mut last: Vec<AgentView> = Vec::new();
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if let Some(agents) = next_views_to_send(&mut last, crate::autorun::sessions::now_ms()) {
                    let _ = crate::events::AutorunAgentsChanged { agents }.emit(&app);
                }
            }
        });
    });
}

// ---------------------------------------------------------------------------
// The card's commands
// ---------------------------------------------------------------------------

/// Said for a slot number from `SLOT_COUNT` up.
pub const NO_SUCH_BROWSER: &str = "that Auto Run browser is not open any more";

/// Said by Release when the browser it let go is one the person opened.
pub const PERSONS_BROWSER_KEPT: &str =
    "the main Auto Run browser was opened by you, so it stays open - the assistant no longer uses it";

fn slot_numbered(n: u8) -> Result<SlotNo, String> {
    SlotNo::from_number(n).ok_or_else(|| NO_SUCH_BROWSER.to_string())
}

/// The assistants connected now, and what each is doing.
#[tauri::command]
#[specta::specta]
pub fn auto_run_agents() -> Vec<AgentView> {
    agent_views(crate::autorun::sessions::now_ms())
}

/// Stop what slot `slot`'s assistant is doing: its replay (never the
/// person's), and a batch of discovery actions, at their next look. The
/// assistant's browser stays as it is.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_stop_agent(slot: u8) -> Result<(), String> {
    let n = slot_numbered(slot)?;
    // Only an assistant's replay: the person's own, which a card showing Stop
    // may be a second behind, is never stopped from here.
    let replay = crate::autorun::replay_to::state(n);
    if replay.assistants_replay_running() {
        replay.stop();
    }
    crate::ai_bridge::stop_batch_in(n);
    crate::applog::info(format!("Auto Run: stopped {} (the person stopped it)", whose(n)));
    Ok(())
}

/// End the discovery going in slot `slot`'s browser, as the person's End
/// discovery does (`EndedBy::Person`); with none going this does nothing.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_end_agent_discovery(slot: u8) -> Result<(), String> {
    let n = slot_numbered(slot)?;
    crate::commands::autorun::end_discovery(crate::ai_bridge::EndedBy::Person, n).await;
    Ok(())
}

/// Release slot `slot`'s browser from its assistant (`release_slot`).
#[tauri::command]
#[specta::specta]
pub async fn auto_run_release_agent(slot: u8) -> Result<String, String> {
    release_slot(slot_numbered(slot)?, ReleaseWhy::Person).await
}

/// Who a release or stop is logged for: the assistant's label when slot `n`
/// is bound to one, else the slot. Never an id or a pid.
fn whose(n: SlotNo) -> String {
    match bound(n).and_then(|key| agents::info(&key)) {
        Some(info) => format!("{}'s browser", info.label),
        None if n.is_main() => "the main Auto Run browser".to_string(),
        None => "an Auto Run browser".to_string(),
    }
}

/// Who asked for a release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseWhy {
    Person,
}

/// Stops slot `n`'s assistant's replay (never the person's) and batch, waits
/// for its lock (`RELEASE_WAIT`),
/// then lets the browser go as the assistant's own release does
/// (`release_in_for_assistant`): a discovery is ended, its map kept, and its
/// browser closed; a browser an assistant opened is closed; one the person
/// opened is left open (`PERSONS_BROWSER_KEPT`). The slot is unbound either
/// way. The answer is the card's own sentence for what happened
/// (`release_answer_in`), not the assistant's text. A browser that stays busy past the wait is refused
/// (`BROWSER_BUSY`) and the binding kept.
pub async fn release_slot(n: SlotNo, why: ReleaseWhy) -> Result<String, String> {
    // Read first: the label is kept in the registry until its next sweep.
    let who = whose(n);
    // Only an assistant's replay is stopped; the person's goes on, and the
    // release then waits for its lock and says the browser is busy.
    let replay = crate::autorun::replay_to::state(n);
    if replay.assistants_replay_running() {
        replay.stop();
    }
    crate::ai_bridge::stop_batch_in(n);
    let Ok(mut slot) = tokio::time::timeout(crate::commands::autorun::RELEASE_WAIT, session_slot(n).lock()).await
    else {
        return Err(crate::commands::autorun::BROWSER_BUSY.to_string());
    };
    let answer = release_answer_in(&mut slot, &who).await;
    crate::commands::autorun::publish_discovery_in(n, &slot);
    crate::commands::autorun::publish_overrides_in(n, &slot);
    if slot.is_none() {
        crate::commands::autorun::tell_panes_closed_in(n);
    }
    drop(slot);
    unbind(n);
    let ReleaseWhy::Person = why;
    crate::applog::info(format!("Auto Run: released {who} (the person released it)"));
    Ok(answer)
}

/// Lets go of the browser in `slot` as the assistant's own release does
/// (`release_outcome_in`) and answers the person in sentences of the card's
/// own, by what happened: never the assistant's text, which is JSON for a
/// mapping or explore discovery and names the application's requests. That
/// text goes to the log. `who` is `whose` of the slot.
pub async fn release_answer_in<B: crate::ai_bridge::DiscoveryBrowser>(slot: &mut Option<B>, who: &str) -> String {
    use crate::commands::autorun::ReleaseOutcome;
    match crate::commands::autorun::release_outcome_in(slot).await {
        ReleaseOutcome::AlreadyGone => {
            let mut sentence = format!("{who} had already closed, so there was nothing to close.");
            sentence[..1].make_ascii_uppercase();
            sentence
        }
        ReleaseOutcome::NothingHeld => "No Auto Run browser was held, so there was nothing to release.".to_string(),
        ReleaseOutcome::EndedDiscovery(_, said) => {
            let kept: String = said.chars().take(400).collect();
            crate::applog::info(format!("Auto Run: the discovery ended by the release said: {kept}"));
            format!("Ended the discovery in {who}, kept what it found, and closed the browser.")
        }
        ReleaseOutcome::ClosedAssistants => format!("Released {who} and closed it."),
        ReleaseOutcome::PersonsKept => PERSONS_BROWSER_KEPT.to_string(),
    }
}
