//! The supervised runner's IPC surface.
//!
//! NOTHING here calls Azure DevOps - sending a reviewed run there is
//! `commands::autorun_publish`, a separate command reached only by the
//! Send button. The browser session lives for as long as the screen keeps
//! it open; steps run against it one at a time, driven by the human
//! clicking through.

use crate::autorun::replay_to::NotSignedIn;
use crate::autorun::slots::{SlotNo, EXTRA_CAP, SLOT_COUNT};
use crate::autorun::store;
use crate::autorun::{CaseScript, LocalRun, StepScript};
use crate::browser::actions::ActionOutcome;
use crate::browser::cdp::{Cdp, Driver};
use crate::browser::launch::{launch_in_at, Browser, LaunchedBrowser};
use base64::Engine;
use std::path::PathBuf;
use tauri::Manager;

/// The one live session. A second Open replaces the first, so a stray
/// browser can never leave the screen permanently stuck.
static SESSION: tokio::sync::Mutex<Option<Session>> = tokio::sync::Mutex::const_new(None);

/// The browsers beside the main one, slot 1 first. Each is its own lock:
/// no path holds two slots' locks at once. The rule for the few that must
/// (Release all, the app's exit and the environment save and switch) is to
/// lock them in slot order, 0, 1, 2, once extra browsers can open.
static EXTRA_SESSIONS: [tokio::sync::Mutex<Option<Session>>; EXTRA_CAP] =
    [const { tokio::sync::Mutex::const_new(None) }; EXTRA_CAP];

/// Slot `n`'s session lock: the main slot is `SESSION`, the person's.
pub(crate) fn session_slot(n: SlotNo) -> &'static tokio::sync::Mutex<Option<Session>> {
    if n.is_main() {
        &SESSION
    } else {
        &EXTRA_SESSIONS[n.index() - 1]
    }
}

/// Serialises a change to the imported capture sets (an import), so two
/// imports at once cannot both pass the five-set cap or lose each other's
/// set. Coordination only: the sets themselves live in the Rust cache.
static IMPORTING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The capture sets the assistant imported from HAR files
/// (`autorun_har`'s import), by name: read-only, never replayed into a
/// browser. Kept in the Rust cache's memory-only tier
/// (`cache::keys::imported_captures`), not in the session, so a set is read
/// with no browser open; the cache drops them when a different account
/// signs in, and `forget_imported_captures` when the person signs in or out
/// of TCM, so another account never reads them. A copy: each set is shared,
/// so this copies names and pointers, not requests.
pub fn imported_captures() -> crate::autorun::capture_tools::ImportedSets {
    crate::cache::session_fresh(&crate::cache::keys::imported_captures(), crate::cache::keys::IMPORTED_CAPTURES_TTL)
        .unwrap_or_default()
}

/// Change the imported capture sets with `f` (`capture_tools::store_import`),
/// one change at a time (`IMPORTING`); kept, and its time renewed, only
/// when `f` answers 200.
pub fn change_imported_captures(
    f: impl FnOnce(&mut crate::autorun::capture_tools::ImportedSets) -> (u16, String),
) -> (u16, String) {
    let _one_at_a_time = IMPORTING.lock().unwrap_or_else(|e| e.into_inner());
    let mut sets = imported_captures();
    let answer = f(&mut sets);
    if answer.0 == 200 {
        crate::cache::session_put(&crate::cache::keys::imported_captures(), sets);
    }
    answer
}

/// Forget every imported capture set: the TCM account signed in or out.
pub fn forget_imported_captures() {
    let _one_at_a_time = IMPORTING.lock().unwrap_or_else(|e| e.into_inner());
    crate::cache::forget(&crate::cache::keys::imported_captures());
}

pub(crate) struct Session {
    pub(crate) browser: LaunchedBrowser,
    /// Which browser it is, Edge or Chrome: a discovery that names the other
    /// one opens that one rather than take this over.
    pub(crate) kind: Browser,
    pub(crate) cdp: Cdp,
    /// The account last signed in as, in THIS browser. `None` until a
    /// sign-in succeeds. Written on every sign-in; nothing reads it yet -
    /// unattended replay (a later phase) is what will.
    pub(crate) account: Option<String>,
    /// The account this browser holds (`autorun::lease`), from a sign-in
    /// (one that failed too) until it signs in as another account. It goes with the
    /// session: closing the browser, a second Open replacing it, or the
    /// app exiting all drop it.
    pub(crate) lease: crate::autorun::lease::Held,
    /// The case whose no-save guard this browser holds, if any
    /// (`guard_for_case`).
    pub(crate) guarded_case: Option<i32>,
    /// Who the save guard on this browser belongs to (`GuardOwner`), and so
    /// how long it lasts; `None` while nothing guards it. Cleared with the
    /// guard (`lift_guard_of`), and gone with the browser.
    pub(crate) guard_owner: Option<GuardOwner>,
    /// The Auto Run folder whose `downloads/supervised` this browser saves
    /// into, emptied once it closes. `None` when downloads could not be
    /// switched on.
    pub(crate) downloads_root: Option<PathBuf>,
    /// The case this browser's tabs belong to (`runner::tabs_for_case`):
    /// a step of another case closes every tab but `main` first.
    pub(crate) tabs_case: Option<i32>,
    /// The discovery under way in this browser, if any: what the page
    /// routes record is filed under its area and stamps that area explored.
    pub(crate) discovery: Option<DiscoveryState>,
    /// Who opened this browser (`OpenedBy`): the assistant's release and
    /// its discovery close only one the assistant opened.
    pub(crate) opened_by: OpenedBy,
    /// Where the assistant's last replay in this browser left it, while
    /// nothing has used the browser since (`replay_to::ReplayStand`): its
    /// next replay of that case carries on from there. Any other use - a
    /// step, a try, a sign-in, a start or another replay - clears it.
    pub(crate) replay_stand: Option<crate::autorun::replay_to::ReplayStand>,
    /// The case a watched step-by-step run is on in this browser
    /// (`auto_run_step`): its script is not deleted until the run ends
    /// (`release_stepped_run`), another case is stepped, or the browser
    /// closes.
    pub(crate) stepping: Option<store::Replaying>,
    /// A watched run's last action that changed the page, carried from one
    /// step to the next (`runner::Noted`), so a failing step can name what
    /// the page answered the step before. Begun afresh at a case's first
    /// step, or a step of another case.
    pub(crate) noted: crate::autorun::runner::Noted,
    /// The test headers and cookies the assistant set in this browser
    /// (`autorun::overrides`): named in every try, discovery and replay
    /// answer while any are in place, taken away by the assistant's reset
    /// (`reset_in_for_assistant`), its clear of the cookies, or the person
    /// taking the browser (`Session::make_persons`), and gone with the
    /// browser.
    pub(crate) overrides: crate::autorun::overrides::Overrides,
    /// Which slot holds this browser (`session_slot`): the downloads folder
    /// it saves into, and the use count, overrides and discovery flag its
    /// changes are published to, are this slot's.
    pub(crate) slot: SlotNo,
}

impl Session {
    /// The browser is used by something other than a watched step or the
    /// replay that left it: no replay carries on from where it stood, and
    /// the watched run's last page change (`noted`) is no longer the page's
    /// - a later step names nothing that happened before this use.
    pub(crate) fn used_otherwise(&mut self) {
        self.replay_stand = None;
        self.noted.forget(&self.cdp);
    }

    /// The person takes this browser (their step, start, sign-in or
    /// replay): whatever the assistant changed for testing is taken away
    /// first (`overrides::take_for_person`), and then it is theirs from now
    /// on. When the changes would not go, `Err(TEST_CHANGES_STUCK)`: the
    /// person's action is refused, never run with them in place, and the
    /// browser keeps its mark.
    pub(crate) async fn make_persons(&mut self) -> Result<(), String> {
        let taken = crate::autorun::overrides::take_for_person(&mut self.cdp, &mut self.overrides).await;
        show_overrides_in(self.slot, &self.overrides);
        taken?;
        self.opened_by = OpenedBy::Person;
        // No assistant has this browser any more.
        crate::commands::autorun_slots::unbind(self.slot);
        Ok(())
    }
}

/// What each slot's browser's overrides are now, as last published
/// (`publish_overrides_in`): what the assistant's replay answer names, read
/// once the replay has let the browser go. Cleared when the browser closes.
static OVERRIDES_SHOWN: [std::sync::Mutex<crate::autorun::overrides::Overrides>; SLOT_COUNT] =
    [const { std::sync::Mutex::new(crate::autorun::overrides::Overrides::new()) }; SLOT_COUNT];

/// Publish `o` as the main browser's overrides (`overrides_now`).
/// Called under the session lock, after anything that changed them.
pub fn show_overrides(o: &crate::autorun::overrides::Overrides) {
    show_overrides_in(SlotNo::MAIN, o);
}

/// Publish `o` as slot `n`'s browser's overrides (`overrides_now_in`).
/// Called under that slot's session lock, after anything that changed them.
pub fn show_overrides_in(n: SlotNo, o: &crate::autorun::overrides::Overrides) {
    *OVERRIDES_SHOWN[n.index()].lock().unwrap_or_else(|e| e.into_inner()) = o.clone();
}

/// Publish the overrides of the browser `slot` holds (none without one) as
/// slot `n`'s, after anything that may have changed them.
pub(crate) fn publish_overrides_in(n: SlotNo, slot: &Option<Session>) {
    match slot.as_ref() {
        Some(s) => show_overrides_in(n, &s.overrides),
        None => show_overrides_in(n, &crate::autorun::overrides::Overrides::new()),
    }
}

/// The overrides of the browser `slot` holds, read under its lock: what a
/// try, discovery or replay answer names the moment it finishes.
pub(crate) fn overrides_of(slot: &Option<Session>) -> crate::autorun::overrides::Overrides {
    slot.as_ref().map(|s| s.overrides.clone()).unwrap_or_default()
}

/// The main browser's overrides as last published
/// (`publish_overrides_in`).
pub fn overrides_now() -> crate::autorun::overrides::Overrides {
    overrides_now_in(SlotNo::MAIN)
}

/// Slot `n`'s browser's overrides as last published
/// (`publish_overrides_in`).
pub fn overrides_now_in(n: SlotNo) -> crate::autorun::overrides::Overrides {
    OVERRIDES_SHOWN[n.index()].lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// How many times each slot's browser has been used for anything but
/// a page read since the app started: a step, a try, a sign-in, a watched
/// start, a setup making way, a replay, or a browser opened or closed. An
/// assistant's replay answer nobody took is handed over only while its
/// slot's count has not moved since it ended
/// (`ai_bridge::autorun_replay_with`).
static SESSION_USES: [std::sync::atomic::AtomicU64; SLOT_COUNT] =
    [const { std::sync::atomic::AtomicU64::new(0) }; SLOT_COUNT];

/// The main browser was used (`SESSION_USES`).
pub fn note_session_use() {
    note_session_use_in(SlotNo::MAIN);
}

/// Slot `n`'s browser was used (`SESSION_USES`).
pub fn note_session_use_in(n: SlotNo) {
    SESSION_USES[n.index()].fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

/// How many times the main browser has been used (`SESSION_USES`).
pub fn session_uses() -> u64 {
    session_uses_in(SlotNo::MAIN)
}

/// How many times slot `n`'s browser has been used (`SESSION_USES`).
pub fn session_uses_in(n: SlotNo) -> u64 {
    SESSION_USES[n.index()].load(std::sync::atomic::Ordering::SeqCst)
}

/// Who opened the supervised browser. The assistant: its replay or its
/// discovery opened it. The person: Open browser, or their own replay.
/// A browser already open keeps its mark when an assistant's replay or try
/// uses it; the person's own step or replay in it makes it the person's,
/// so the assistant never closes a browser the person is working in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenedBy {
    Person,
    Assistant,
}

/// Who the supervised browser's save guard belongs to, which decides how
/// long it lasts and who may lift it.
///
/// - `Person`: the person's own step or replay of a case marked `no_save`.
///   Never lifted by the assistant; the person's next step or replay of a
///   case that may save lifts it.
/// - `Discovery`: a discovery started with `no_save` (or for a case marked
///   so), or a mapping run, while it runs. It lasts until that discovery
///   ends; the assistant's replay that ends it to take the browser is the
///   last call it guards (`guard_before_replay`), and the guard is then that
///   replay's call's (`settle_replay_guard`).
/// - `Call`: one call of the assistant's - a discovery action or batch that
///   said `no_save` itself (lifted when that call ends), or its try or
///   replay of a case marked `no_save` (`case_id`, guarded for every call on
///   that case). The assistant's replay of another case that may save lifts
///   it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardOwner {
    Person,
    Discovery,
    Call { case_id: Option<i32> },
}

impl GuardOwner {
    /// Which of two owners a guard keeps: the person's above a discovery's
    /// above a call's, so a later call never takes over a longer guard.
    fn rank(&self) -> u8 {
        match self {
            GuardOwner::Call { .. } => 0,
            GuardOwner::Discovery => 1,
            GuardOwner::Person => 2,
        }
    }
}

/// The guard went on, or stayed on, for `wanted`: it is `wanted`'s unless a
/// longer owner already holds it (`GuardOwner::rank`).
pub fn claim_guard(owner: &mut Option<GuardOwner>, wanted: GuardOwner) {
    if owner.as_ref().is_none_or(|held| held.rank() <= wanted.rank()) {
        *owner = Some(wanted);
    }
}

/// After the guard was put where case `case_id` needs it (`guard_for_case`)
/// by `who`: no guard on, no owner; this case's guard, `who`'s
/// (`claim_guard`); a guard kept for another case keeps its owner.
pub fn settle_guard_owner<D: Driver>(
    d: &D,
    guarded_case: Option<i32>,
    owner: &mut Option<GuardOwner>,
    case_id: i32,
    who: GuardOwner,
) {
    if !d.is_guarding_saves() {
        *owner = None;
    } else if guarded_case == Some(case_id) {
        claim_guard(owner, who);
    }
}

/// Lift the save guard on `d`, whoever owns it: the browser stops guarding
/// and the guard's case and owner are forgotten. Only once the browser says
/// it stopped: a lift it refuses keeps both, and the browser still answers
/// every paused request as a guarded one does, so it is asked again. True
/// when no guard is left on.
pub async fn lift_guard_of<D: Driver>(d: &mut D, guarded_case: &mut Option<i32>, owner: &mut Option<GuardOwner>) -> bool {
    if d.is_guarding_saves() {
        if let Err(e) = d.stop_guarding_saves().await {
            crate::applog::warn(format!("Auto Run: the no-save guard could not be lifted yet: {e}"));
            return false;
        }
    }
    *guarded_case = None;
    *owner = None;
    true
}

/// A discovery under way in the supervised browser: the area it explores
/// and the account KEY it explores as (never a login). Both are `None` from
/// the moment the browser opens for a discovery until its sign-in arrives.
pub struct DiscoveryState {
    pub area: Option<String>,
    pub account: Option<String>,
    /// When the discovery started (milliseconds since the epoch): a page it
    /// reads again keeps the locators matched since then.
    pub started_at: u64,
    /// The components tried in this discovery that worked, by
    /// `components::draft_fingerprint`. It goes with the discovery.
    pub tried: Vec<String>,
    /// Set when this discovery is a mapping run: read only, every save the
    /// page tries is stopped (`browser::save_guard`).
    pub mapping: Option<MappingRun>,
    /// The case this discovery explores for, when the assistant named one.
    /// Its saved script is read again before every action, so a case marked
    /// `no_save` keeps its discovery guarded (`ai_bridge::guard_a_no_save_discovery`).
    pub case_id: Option<i32>,
    /// Set for a discovery that must not save: its start said `no_save`, or
    /// the case it explores for is marked `no_save`. Every save the page
    /// tries is then blocked, never only reported, and it is never cleared
    /// while the discovery lasts (`GuardOwner::Discovery`). An action's own
    /// `no_save` never sets it: that is `call_no_save`.
    pub no_save: bool,
    /// The call under way said `no_save` itself (`ai_bridge::mark_discovery_for`):
    /// its actions are guarded, and the guard that call put on is lifted
    /// when it ends unless the discovery itself must not save
    /// (`GuardOwner::Call`). Set by every discovery call, for that call only.
    pub call_no_save: bool,
    /// The case the call under way named (`case_id`), which may not be the
    /// one the discovery is for: the guard that call puts on is that case's
    /// (`GuardOwner::Call`). Set by every discovery call, for that call only.
    pub call_case_id: Option<i32>,
    /// Set when this discovery is an explore session: the assistant does a
    /// plain task with no test case, nothing is filed in the discovery map,
    /// and its change list is its own file (`autorun::explore`).
    pub explore: Option<crate::autorun::explore::ExploreRun>,
    /// What this discovery saw in the areas it left, filed under the area it
    /// moved to (`discovery_map::carry_seen`): read by the save's seen check
    /// while this discovery goes on, never written to the map, gone with the
    /// discovery.
    pub carried: Vec<crate::autorun::discovery_map::AreaMap>,
}

impl DiscoveryState {
    /// The id of the explore session this discovery is, once its change
    /// list file exists.
    pub fn explore_session(&self) -> Option<&str> {
        self.explore.as_ref().and_then(|e| e.session.as_deref())
    }
}

/// The case a discovery is for, as its start named it, whether it must not
/// save (`DiscoveryState::no_save`), and whether it is an explore session
/// (`DiscoveryState::explore`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiscoveryFor {
    pub case_id: Option<i32>,
    pub no_save: bool,
    pub explore: bool,
}

/// A mapping run: a discovery that walks the menus of `modules` and saves
/// nothing. Its browser's save guard is on from the sign-in to the end.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MappingRun {
    /// The test-case Modules the run maps, as named at its start.
    pub modules: Vec<String>,
    /// When the run started (milliseconds since the epoch).
    pub started_at: u64,
    /// The areas the run added.
    pub added: Vec<String>,
    /// The areas whose path the run changed: (name, old path, new path).
    pub updated: Vec<(String, String, String)>,
    /// The areas the run found where they were.
    pub unchanged: Vec<String>,
    /// The areas the run could not reach: (name, reason).
    pub unreached: Vec<(String, String)>,
    /// How many saves the page tried that the guard stopped.
    pub blocked_writes: u32,
    /// Every outcome, in the order it came: a screen met more than once is
    /// summarized by its last (`mapping_summary::summarize`). The third
    /// part is where that screen is, when the save said (`locate_last`).
    pub outcomes: Vec<(String, MappingOutcome, Option<MappingScreen>)>,
    /// Where the run's summary is kept, set once it has signed in. A run
    /// that never signed in mapped nothing, and keeps no summary.
    pub place: Option<MappingPlace>,
}

/// What happened to one screen in a mapping run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingOutcome {
    Added,
    Updated,
    Unchanged,
    Unreached,
}

/// Where a screen a mapping run met is: the address path its clicks
/// arrive on (or had to, for one not reached), when it is known, and its
/// menu path in words (`nav::menu_path`). Two outcomes with the same
/// `arrived` are the same screen whatever they were named.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MappingScreen {
    pub arrived: Option<String>,
    pub menu: String,
}

/// The Auto Run folder and project a mapping run's summary is saved under.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MappingPlace {
    pub root: PathBuf,
    pub organization: String,
    pub project: String,
}

impl MappingRun {
    /// A screen the run added.
    pub fn record_added(&mut self, name: String) {
        self.outcomes.push((name.clone(), MappingOutcome::Added, None));
        self.added.push(name);
    }

    /// A screen whose menu path the run changed from `old` to `new`.
    pub fn record_updated(&mut self, name: String, old: String, new: String) {
        self.outcomes.push((name.clone(), MappingOutcome::Updated, None));
        self.updated.push((name, old, new));
    }

    /// A screen the run found where it was, or a person's it left alone.
    pub fn record_unchanged(&mut self, name: String) {
        self.outcomes.push((name.clone(), MappingOutcome::Unchanged, None));
        self.unchanged.push(name);
    }

    /// A screen the run could not reach. The reason is kept with its
    /// addresses taken out (`mapping_summary::without_addresses`).
    pub fn record_unreached(&mut self, name: String, reason: &str) {
        self.outcomes.push((name.clone(), MappingOutcome::Unreached, None));
        self.unreached.push((name, crate::autorun::mapping_summary::without_addresses(reason)));
    }

    /// Where the screen the last outcome named is. With no outcome yet
    /// there is nothing to place, and nothing happens.
    pub fn locate_last(&mut self, screen: MappingScreen) {
        if let Some(last) = self.outcomes.last_mut() {
            last.2 = Some(screen);
        }
    }

    /// A run over `modules` (trimmed, blanks left out) starting now.
    pub fn new(modules: &[String]) -> Self {
        MappingRun {
            modules: modules.iter().map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).collect(),
            started_at: crate::autorun::sessions::now_ms(),
            ..Default::default()
        }
    }
}

/// Said when a step arrives with no browser behind it. Pulled out so a
/// test can hold the wording to account - "no session" would tell the
/// person nothing about what to do.
pub fn describe_session_error() -> String {
    "no browser is open - press Open browser first".to_string()
}

/// Where scripts and runs live: beside the app's other data.
///
/// The value app setup published process-wide: the active environment's
/// folder. The AI bridge writes through that same value and has no
/// `AppHandle` to derive one from, and two independent derivations of "the
/// autorun directory" is exactly how an assistant's saved script ends up
/// somewhere this screen never looks. There is no fallback, so the handle
/// is unused; it stays so every caller keeps its shape.
pub fn root(_app: &tauri::AppHandle) -> Result<PathBuf, String> {
    store::resolve_root()
}

/// Guards a run id from escaping the runs directory. The rule itself lives
/// in `store::safe_run_id` - `load_run` needs it too, and there is exactly
/// one copy - re-exported here because this command module is the id's
/// first IPC-facing entry point, and existing callers still import it from
/// here.
pub use store::safe_run_id;

/// Whether the one supervised session is currently open. Peeked by
/// `auto_run_replay` (F7): an unattended run and a supervised session
/// must never be open at once, and this is the mutex side of that
/// exclusion - the other direction (`auto_run_open_browser` refusing
/// while an unattended run is going) uses the pure, AppHandle-free
/// `autorun_replay::replay_is_running` instead.
pub async fn supervised_session_is_open() -> bool {
    SESSION.lock().await.is_some()
}

/// `supervised_session_is_open` without waiting: a session lock that is
/// taken counts as open (something is using the browser). A lock held for a
/// moment (the Assistants card reading the slot) is waited out first
/// (`try_lock_briefly`), so it is never mistaken for a browser in use.
pub fn supervised_session_is_open_now() -> bool {
    try_lock_briefly(&SESSION).map_or(true, |slot| slot.is_some())
}

/// How many times `try_lock_briefly` tries.
const BRIEF_TRIES: usize = 5;

/// `Mutex::try_lock` that waits out a hold of a moment: up to
/// `BRIEF_TRIES` tries with a millisecond between, then `None` - a replay,
/// a step or a call holding the lock still reads as busy. For the readers
/// that answer the person from a lock they must not wait on.
pub fn try_lock_briefly<T>(m: &tokio::sync::Mutex<T>) -> Option<tokio::sync::MutexGuard<'_, T>> {
    try_lock_with_retries(m, BRIEF_TRIES, &|| std::thread::sleep(std::time::Duration::from_millis(1)))
}

/// `try_lock_briefly` with its tries and its pause given.
pub fn try_lock_with_retries<'a, T>(
    m: &'a tokio::sync::Mutex<T>,
    tries: usize,
    pause: &dyn Fn(),
) -> Option<tokio::sync::MutexGuard<'a, T>> {
    for attempt in 0..tries {
        if let Ok(guard) = m.try_lock() {
            return Some(guard);
        }
        if attempt + 1 < tries {
            pause();
        }
    }
    None
}

#[tauri::command]
#[specta::specta]
pub async fn auto_run_open_browser(app: tauri::AppHandle, browser_name: String) -> Result<(), String> {
    if crate::commands::autorun_replay::replay_is_running() {
        return Err(UNATTENDED_GOING.to_string());
    }
    // A discovery's browser is never replaced: heard before a replay is
    // stopped, and again under the lock. No replay goes beside a discovery
    // (`replay_supervised` refuses the person's, and the assistant's ends
    // the discovery under the lock first), so the lock is free to take.
    // A discovery browser that has gone is let go first, never refused for.
    if auto_run_discovery_active() {
        let mut slot = SESSION.lock().await;
        let_go_if_gone(&mut slot).await;
        if slot.as_ref().is_some_and(|s| s.discovery.is_some()) {
            return Err(busy_browser_sentence(true).to_string());
        }
    }
    // A replay going in the browser this replaces ends first: it is heard
    // without the session's lock, which the replay holds.
    crate::autorun::replay_to::stop();
    let mut slot = SESSION.lock().await;
    let_go_if_gone(&mut slot).await;
    crate::ai_bridge::refuse_while_discovering(&mut slot)?;
    let opened = open_into(root(&app), &mut slot, SlotNo::MAIN, Browser::from_name(&browser_name), OpenedBy::Person).await;
    // A discovery this replaced is over, whether or not the new one opened.
    publish_discovery(&slot);
    opened?;
    // The person's choice, once it opened, for a replay that finds no
    // browser. Best effort: the browser is open either way.
    match root(&app) {
        Ok(root) => store::remember_browser(&root, &browser_name),
        Err(e) => crate::applog::warn(format!("auto-run: the browser choice could not be remembered: {e}")),
    }
    Ok(())
}

/// Said when a supervised browser is asked for while an unattended run is
/// going.
const UNATTENDED_GOING: &str = "an unattended run is going - wait for it, or stop it first";

/// The supervised session, opened with the browser last chosen
/// (`store::last_browser`) when none is open - the same opening as Open
/// browser, marked as opened `by`. Called with the session lock held.
async fn open_if_none(app: &tauri::AppHandle, slot: &mut Option<Session>, n: SlotNo, by: OpenedBy) -> Result<(), String> {
    // A held browser that has gone is let go, and a new one opened. Whose the
    // new one is stays with the caller (`replay_supervised` holds the
    // assistant's binding through all of this).
    let_go_if_gone_in(n, slot).await;
    if slot.is_some() {
        return Ok(());
    }
    if crate::commands::autorun_replay::replay_is_running() {
        return Err(UNATTENDED_GOING.to_string());
    }
    let which = Browser::from_name(&store::last_browser(&root(app)?));
    open_into(root(app), slot, n, which, by).await
}

/// Said by `open_for_discovery` when the app set up no data directory.
pub const NO_DATA_DIRECTORY: &str = "the app could not set up its data directory this session - restart the app";

/// What a discovery is refused for before anything is looked at under the
/// session's lock: an unattended run, a recording or check, a replay to a
/// step in slot `n`. A replay holds the lock while it goes, so it is heard
/// here, not waited for.
pub(crate) fn refuse_discovery_while_busy(n: SlotNo) -> Result<(), String> {
    if crate::commands::autorun_replay::replay_is_running() {
        return Err(UNATTENDED_GOING.to_string());
    }
    crate::commands::autorun_record::refuse_while_recording()?;
    if crate::autorun::replay_to::state(n).is_running() {
        return Err(crate::autorun::replay_to::ALREADY_RUNNING.to_string());
    }
    Ok(())
}

/// Open the Auto Run browser for the assistant's discovery, in the browser
/// named (`edge` or `chrome`). Refused while anything else holds the
/// browser - the person's browser included, which a discovery never
/// replaces or takes over. The session is marked a discovery from the
/// moment it opens; its area and account are set once the sign-in arrives.
/// `mapping` makes it a mapping run, whose save guard goes on once it has
/// signed in (`ai_bridge::discover_start_in`); so does the guard of a
/// discovery for a case that must not save (`no_save`). A browser the
/// assistant's own replay left open is taken over, signed in as it was
/// (`ai_bridge::make_way_for_discovery`); a guarded discovery's guard is on
/// in it before this lets the lock go - the replay's handed on without a
/// gap, or the discovery's own armed with `from`'s save words - so no call
/// between here and the start finds it unguarded. Unless `browser_name`
/// names the other browser: that one is closed, and the one named opened.
/// With none named a new browser is the one last chosen. Says which
/// (`MadeWay`). It all happens in slot `n`'s browser, under that slot's lock.
/// `holder` is the assistant whose call it is, if it is one: its binding of
/// slot `n` is kept through closing a browser that has gone, or the other
/// kind, and through opening the new one, so no other assistant's call can
/// take the slot in between; the default caller passes none and is never bound.
pub(crate) async fn open_for_discovery(
    browser_name: Option<&str>,
    mapping: Option<MappingRun>,
    no_save: DiscoveryFor,
    from: crate::ai_bridge::GuardFrom<'_>,
    n: SlotNo,
    holder: Option<&crate::autorun::agents::AgentKey>,
) -> Result<crate::ai_bridge::MadeWay, String> {
    use crate::ai_bridge::MadeWay;
    refuse_discovery_while_busy(n)?;
    let wanted = browser_name.map(Browser::from_name);
    let guarded = (mapping.is_some() || no_save.no_save).then_some(from);
    let mut slot = session_slot(n).lock().await;
    let _hold = holder.map(|key| crate::commands::autorun_slots::hold_binding(n, key));
    // Refused only for a browser that is really there: one that has gone
    // is let go, and the discovery opens.
    let made_way = crate::ai_bridge::make_way_for_discovery(&mut slot, wanted, guarded).await;
    publish_discovery_in(n, &slot);
    let made_way = made_way?;
    if made_way == MadeWay::Closed {
        tell_panes_closed_in(n);
    }
    if made_way != MadeWay::TookOver {
        let root = store::configured_root().ok_or_else(|| NO_DATA_DIRECTORY.to_string())?;
        let which = wanted.unwrap_or_else(|| Browser::from_name(&store::last_browser(&root)));
        open_into(Ok(root), &mut slot, n, which, OpenedBy::Assistant).await?;
    }
    if let Some(key) = holder {
        crate::commands::autorun_slots::bind_if_free(n, key);
    }
    if let Some(session) = slot.as_mut() {
        session.discovery = Some(DiscoveryState {
            area: None,
            account: None,
            started_at: crate::autorun::sessions::now_ms(),
            tried: Vec::new(),
            mapping,
            case_id: no_save.case_id,
            no_save: no_save.no_save,
            call_no_save: false,
            call_case_id: None,
            explore: no_save.explore.then(crate::autorun::explore::ExploreRun::default),
            carried: Vec::new(),
        });
    }
    publish_discovery_in(n, &slot);
    Ok(made_way)
}

/// End the discovery under way in slot `n`'s browser as `by` ends it
/// (`ai_bridge::end_discovery_as`):
/// the assistant's `end_autorun_discovery` keeps its browser signed in for
/// its next replay or discovery, its guard lifted; the person's End
/// discovery closes it so Auto Run can run again. With no discovery going
/// this does nothing.
pub(crate) async fn end_discovery(by: crate::ai_bridge::EndedBy, n: SlotNo) -> (u16, String) {
    // A batch of actions holding the browser gives way before its next
    // action, so this is not kept waiting behind it.
    crate::ai_bridge::stop_batch_in(n);
    let mut slot = session_slot(n).lock().await;
    // A discovery browser that has gone ends with it, as any other does.
    let gone = slot.as_ref().is_some_and(|s| s.discovery.is_some()) && let_go_if_gone_in(n, &mut slot).await;
    if gone {
        return (200, DISCOVERY_BROWSER_HAD_GONE.to_string());
    }
    let held = slot.is_some();
    let answer = crate::ai_bridge::end_discovery_as(&mut slot, by).await;
    publish_discovery_in(n, &slot);
    // The assistant's end names what it changed for testing, read here
    // under the lock: the browser it keeps still has it.
    let answer = match by {
        crate::ai_bridge::EndedBy::Assistant => crate::autorun::overrides::with_note(answer, &overrides_of(&slot)),
        _ => answer,
    };
    if held && slot.is_none() {
        tell_panes_closed_in(n);
    }
    answer
}

/// End Discovery on Auto Run's Setup card: the person closes the
/// assistant's discovery browser so Auto Run can run again
/// (`ai_bridge::EndedBy::Person`), and the window is told. With no
/// discovery going this does nothing. Nothing is lost: the map keeps what
/// was seen, and a mapping run's summary is kept.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_end_discovery() -> Result<(), String> {
    end_discovery(crate::ai_bridge::EndedBy::Person, SlotNo::MAIN).await;
    Ok(())
}

/// Whether a discovery holds each slot's Auto Run browser, as last
/// published. Kept beside the sessions rather than read from them, so asking
/// never waits on the session lock a replay holds while it goes.
static DISCOVERY_ACTIVE: [std::sync::atomic::AtomicBool; SLOT_COUNT] =
    [const { std::sync::atomic::AtomicBool::new(false) }; SLOT_COUNT];

/// Whether the discovery in each slot is a mapping run, as last published
/// with it (`publish_discovery_in`). Only one mapping run goes at a time,
/// whoever runs it, so the start of a second reads this
/// (`ai_bridge::mapping_refusal`). Like the flag above it is coordination
/// state, not a cache: memory only, never kept or read from anywhere else.
static MAPPING_ACTIVE: [std::sync::atomic::AtomicBool; SLOT_COUNT] =
    [const { std::sync::atomic::AtomicBool::new(false) }; SLOT_COUNT];

/// The window `autorun_discovery_changed` goes to. Set once during app
/// setup: the bridge, which starts and ends discoveries, has no
/// `AppHandle` of its own. Unset in the test binaries, where publishing
/// only moves the flag.
static DISCOVERY_EVENTS: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

/// Called once during app setup.
pub fn set_discovery_events(app: tauri::AppHandle) {
    let _ = DISCOVERY_EVENTS.set(app);
}

/// Say whether `slot`, the main session, now holds a discovery, to the
/// window too when that changed. Called, with the session lock held,
/// wherever a discovery can start or end: its open, its end, a failed
/// sign-in closing it, Close browser, and Open browser replacing the session.
pub(crate) fn publish_discovery(slot: &Option<Session>) {
    publish_discovery_in(SlotNo::MAIN, slot);
}

/// Say whether `slot`, slot `n`'s session, now holds a discovery. The window
/// is told (`AutorunDiscoveryChanged`) for the main slot only, with the
/// shape it has always had.
pub(crate) fn publish_discovery_in(n: SlotNo, slot: &Option<Session>) {
    use std::sync::atomic::Ordering;
    use tauri_specta::Event as _;
    let active = slot.as_ref().is_some_and(|s| s.discovery.is_some());
    let mapping = slot.as_ref().and_then(|s| s.discovery.as_ref()).is_some_and(|d| d.mapping.is_some());
    MAPPING_ACTIVE[n.index()].store(mapping, Ordering::SeqCst);
    if DISCOVERY_ACTIVE[n.index()].swap(active, Ordering::SeqCst) != active && n.is_main() {
        if let Some(app) = DISCOVERY_EVENTS.get() {
            let _ = crate::events::AutorunDiscoveryChanged { active }.emit(app);
        }
    }
}

/// Whether the assistant is exploring the app in the Auto Run browser:
/// Auto Run's runs and Open browser wait while it is.
#[tauri::command]
#[specta::specta]
pub fn auto_run_discovery_active() -> bool {
    discovery_active_in(SlotNo::MAIN)
}

/// Whether a discovery holds slot `n`'s browser, as last published.
pub fn discovery_active_in(n: SlotNo) -> bool {
    DISCOVERY_ACTIVE[n.index()].load(std::sync::atomic::Ordering::SeqCst)
}

/// Whether a mapping run holds slot `n`'s browser, as last published.
pub fn mapping_in(n: SlotNo) -> bool {
    MAPPING_ACTIVE[n.index()].load(std::sync::atomic::Ordering::SeqCst)
}

/// The slot whose mapping run is starting right now: taken by a mapping
/// start before anything opens (`claim_mapping_start`) and given back when
/// the start returns, by which time `MAPPING_ACTIVE` says whether the run is
/// going. Coordination state, not a cache: memory only. One std mutex, never
/// held across an await.
static MAPPING_STARTING: std::sync::Mutex<Option<SlotNo>> = std::sync::Mutex::new(None);

/// A mapping start under way in a slot; dropping it gives the start back.
pub struct MappingStart(SlotNo);

impl Drop for MappingStart {
    fn drop(&mut self) {
        let mut s = MAPPING_STARTING.lock().unwrap_or_else(|e| e.into_inner());
        if *s == Some(self.0) {
            *s = None;
        }
    }
}

/// Slot `n` starts a mapping run, or `Err(going)`: per slot, whether a
/// mapping run is going or starting there. Looked at and taken in one step,
/// so two starts at the same moment cannot both pass.
pub fn claim_mapping_start(n: SlotNo) -> Result<MappingStart, [bool; SLOT_COUNT]> {
    let mut s = MAPPING_STARTING.lock().unwrap_or_else(|e| e.into_inner());
    let mut going = SlotNo::all().map(mapping_in);
    if let Some(m) = *s {
        going[m.index()] = true;
    }
    if SlotNo::all().into_iter().any(|m| m != n && going[m.index()]) {
        return Err(going);
    }
    *s = Some(n);
    Ok(MappingStart(n))
}

/// Whether a discovery holds any slot's browser.
pub fn any_discovery_active() -> bool {
    SlotNo::all().into_iter().any(discovery_active_in)
}

/// One area of the discovery map, as the Discovery card shows it.
#[derive(serde::Serialize, specta::Type, Clone, Debug, PartialEq)]
pub struct AreaView {
    /// `""` is the bucket for what belongs to no area.
    pub area: String,
    /// Milliseconds since the epoch; a JavaScript number holds it exactly.
    #[specta(type = Option<f64>)]
    pub explored_at: Option<u64>,
    pub account: Option<String>,
    pub stale: bool,
    /// Why it is stale, as a sentence; `None` when it is not.
    pub stale_reason: Option<String>,
    pub pages: u32,
    pub elements: u32,
    pub writes: Vec<crate::autorun::discovery_map::WriteEntry>,
}

#[derive(serde::Serialize, specta::Type, Clone, Debug, PartialEq)]
pub struct MapView {
    pub areas: Vec<AreaView>,
}

/// The project's discovery map at `now`, area by area. The bucket for what
/// belongs to no area is never stale: there is nothing to explore again.
pub fn map_view(root: &std::path::Path, org: &str, project: &str, now: u64) -> Result<MapView, String> {
    use crate::autorun::discovery_map::{load_map, stale_reason};
    let map = load_map(root, org, project)?;
    let areas = map
        .areas
        .into_iter()
        .map(|a| {
            let why = if a.area.is_empty() { None } else { stale_reason(&a, now) };
            AreaView {
                stale: why.is_some(),
                stale_reason: why.map(sentence_case),
                pages: a.pages.len() as u32,
                elements: a.pages.iter().map(|p| p.elements.len() as u32).sum(),
                explored_at: a.explored_at,
                account: a.account,
                writes: a.writes,
                area: a.area,
            }
        })
        .collect();
    Ok(MapView { areas })
}

/// `s` with its first letter capitalised.
fn sentence_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// What discovery has mapped of this project, for the Discovery card.
#[tauri::command(async)]
#[specta::specta]
pub fn auto_run_load_map(app: tauri::AppHandle, organization: String, project: String) -> Result<MapView, String> {
    map_view(&root(&app)?, &organization, &project, crate::autorun::sessions::now_ms())
}

/// Forget what discovery mapped of `area`. Saved scripts keep running;
/// new saves there need the area explored again.
#[tauri::command]
#[specta::specta]
pub fn auto_run_forget_map_area(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    area: String,
) -> Result<(), String> {
    crate::autorun::discovery_map::forget_area(&root(&app)?, &organization, &project, &area)
}

/// Reset map in the Discovery window, offered when the map cannot be read:
/// the damaged file is moved aside (never deleted) and discovery starts an
/// empty map. Hands back where it was moved, project-relative.
#[tauri::command]
#[specta::specta]
pub fn auto_run_reset_map(app: tauri::AppHandle, organization: String, project: String) -> Result<Option<String>, String> {
    crate::autorun::discovery_map::reset_map(&root(&app)?, &organization, &project, crate::autorun::sessions::now_ms())
}

/// The project's last mapping run's summary, for the Discovery dialog:
/// `None` until a mapping run has ended.
#[tauri::command(async)]
#[specta::specta]
pub fn auto_run_load_mapping_summary(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Option<crate::autorun::mapping_summary::MappingSummary>, String> {
    crate::autorun::mapping_summary::load_summary(&root(&app)?, &organization, &project)
}

/// One saved component, as the Components dialog shows it.
#[derive(serde::Serialize, specta::Type, Clone, Debug, PartialEq)]
pub struct ComponentView {
    pub name: String,
    pub description: String,
    pub inputs: Vec<crate::autorun::components::ComponentInput>,
    pub tried_area: String,
    /// When it last ran on the live app, milliseconds since the epoch;
    /// `None` when never.
    #[specta(type = Option<f64>)]
    pub tried_at: Option<u64>,
    pub version: u32,
    /// How many times it has been changed so far.
    pub changes: u32,
    /// The saved scripts that use it, by case id. While any do, it stays.
    pub used_by_cases: Vec<i32>,
}

#[derive(serde::Serialize, specta::Type, Clone, Debug, PartialEq)]
pub struct ComponentsView {
    pub components: Vec<ComponentView>,
}

/// The project's components, each with the scripts that use it.
pub fn components_view(root: &std::path::Path, org: &str, project: &str) -> Result<ComponentsView, String> {
    use crate::autorun::components::{load_components, users_of};
    let file = load_components(root, org, project)?;
    let components = file
        .components
        .into_iter()
        .map(|c| ComponentView {
            used_by_cases: users_of(root, &c.name).cases,
            tried_at: (c.tried_at > 0).then_some(c.tried_at),
            name: c.name,
            description: c.description,
            inputs: c.inputs,
            tried_area: c.tried_area,
            version: c.version,
            changes: c.changes,
        })
        .collect();
    Ok(ComponentsView { components })
}

/// The project's components, for the Components dialog and its Setup row.
#[tauri::command(async)]
#[specta::specta]
pub fn auto_run_load_components(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<ComponentsView, String> {
    components_view(&root(&app)?, &organization, &project)
}

/// Remove a component no saved script uses (`components::remove_unused`,
/// as the assistant's retire): one in use stays, and the refusal names the
/// cases that use it. Hands back its saved name.
#[tauri::command]
#[specta::specta]
pub fn auto_run_remove_component(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    name: String,
) -> Result<String, String> {
    let removed = crate::autorun::components::remove_unused(&root(&app)?, &organization, &project, &name)?;
    crate::applog::info(format!("Auto Run component {removed} removed"));
    Ok(removed)
}

/// Reset in the Components dialog, offered when the file cannot be read:
/// the damaged file is moved aside (never deleted). Hands back where it was
/// moved, project-relative.
#[tauri::command]
#[specta::specta]
pub fn auto_run_reset_components(app: tauri::AppHandle, organization: String, project: String) -> Result<String, String> {
    crate::autorun::components::reset_components(&root(&app)?, &organization, &project)
}

/// Said when something wants the browser a session holds. A discovery's
/// names the assistant's own way out.
pub fn busy_browser_sentence(discovering: bool) -> &'static str {
    if discovering {
        "the assistant is exploring the app in the Auto Run browser - end it with end_autorun_discovery, or press End discovery in Auto Run, Setup, first"
    } else {
        "close the supervised browser first"
    }
}

/// The sentence for a browser a session holds, or `None` when none is
/// open.
pub async fn open_session_refusal() -> Option<String> {
    let mut slot = SESSION.lock().await;
    // A browser that has gone refuses nothing: it is let go.
    let_go_if_gone(&mut slot).await;
    slot.as_ref().map(|s| busy_browser_sentence(s.discovery.is_some()).to_string())
}

/// Said when an assistant's own browser (a slot beside the main one) is open
/// where a global check - an unattended run, a recording, the environment
/// switch - looks. Every caller is the person's own command, so the sentence
/// is for the person: it says what to do on the Auto Run screen.
pub const EXTRA_BROWSER_OPEN: &str = "an assistant's Auto Run browser is open - wait for it to finish, or release it on the Assistants card at the top of the Auto Run screen";

/// The lowest slot whose browser is open, if any: the one a global check
/// names when it refuses.
pub fn first_open(open: [bool; SLOT_COUNT]) -> Option<SlotNo> {
    SlotNo::all().into_iter().find(|n| open[n.index()])
}

/// Which slots hold a browser, each looked at on its own (never two locks at
/// once), a browser that has gone let go first. Stops at the first one open,
/// so the slots after it are `false` whether or not they hold one. The
/// second is whether main's holds a discovery, for its sentence.
async fn slots_open() -> ([bool; SLOT_COUNT], bool) {
    let mut open = [false; SLOT_COUNT];
    let mut main_discovering = false;
    for n in SlotNo::all() {
        let mut slot = session_slot(n).lock().await;
        let_go_if_gone_in(n, &mut slot).await;
        if let Some(s) = slot.as_ref() {
            open[n.index()] = true;
            main_discovering = n.is_main() && s.discovery.is_some();
            break;
        }
    }
    (open, main_discovering)
}

/// `open_session_refusal` for every slot: main's sentence when main holds a
/// browser (the person's, or a discovery), `EXTRA_BROWSER_OPEN` when the
/// first one open is an assistant's beside it, `None` when none is.
pub async fn open_session_refusal_any() -> Option<String> {
    let (open, main_discovering) = slots_open().await;
    first_open(open).map(|n| {
        if n.is_main() {
            busy_browser_sentence(main_discovering).to_string()
        } else {
            EXTRA_BROWSER_OPEN.to_string()
        }
    })
}

/// Whether any slot's session is open, as `supervised_session_is_open` says
/// of main's alone.
pub async fn any_session_open() -> bool {
    first_open(slots_open().await.0).is_some()
}

/// `any_session_open` without waiting, as `supervised_session_is_open_now`
/// does it: a lock that stays taken counts as open (something is using that
/// browser), one held for a moment is waited out (`try_lock_briefly`).
pub fn any_session_open_now() -> bool {
    SlotNo::all().into_iter().any(|n| try_lock_briefly(session_slot(n)).map_or(true, |slot| slot.is_some()))
}

/// The slots' session locks as the environment save and switch hold them
/// (`lock_every_slot`).
pub(crate) struct EverySlot {
    held: Vec<tokio::sync::MutexGuard<'static, Option<Session>>>,
    /// An assistant's slot (1 or 2) stayed taken through the brief tries: a
    /// replay, a batch or an open is using that browser. Its lock is not held.
    pub busy: bool,
}

impl EverySlot {
    /// Whether any slot whose lock is held has a browser open.
    pub fn any_open(&self) -> bool {
        self.held.iter().any(|s| s.is_some())
    }
}

/// The slots' session locks, taken in slot order 0, 1, 2 and held together:
/// the environment save and switch, which refuse while any holds a browser.
/// Callers hold `opening().write()` first, so no browser opens between the
/// first lock and the last. Main's lock is waited for, as ever; an
/// assistant's slot is only tried briefly (`try_lock_briefly`), because what
/// holds it can be a whole replay: a slot that stays taken sets `busy`
/// (the caller refuses with `SWITCH_BUSY`) and the tries stop there.
pub(crate) async fn lock_every_slot() -> EverySlot {
    let mut every = EverySlot { held: Vec::with_capacity(SLOT_COUNT), busy: false };
    for n in SlotNo::all() {
        if n.is_main() {
            every.held.push(session_slot(n).lock().await);
        } else if let Some(slot) = try_lock_briefly(session_slot(n)) {
            every.held.push(slot);
        } else {
            every.busy = true;
            break;
        }
    }
    every
}

/// A slot's session lock held, as an open or a replay holds it: for a test
/// of what waits on a browser in use. Nothing else holds a lock this way.
pub struct HeldSlot(#[allow(dead_code)] tokio::sync::MutexGuard<'static, Option<Session>>);

/// Wait for slot `n`'s session lock and hold it until the result drops.
pub async fn hold_slot(n: SlotNo) -> HeldSlot {
    HeldSlot(session_slot(n).lock().await)
}

/// `hold_slot` without waiting: `None` when something holds the lock.
pub fn try_hold_slot(n: SlotNo) -> Option<HeldSlot> {
    session_slot(n).try_lock().ok().map(HeldSlot)
}

/// Said by `end_autorun_discovery` when the discovery's browser had
/// already gone: it is let go, and the discovery is over.
pub const DISCOVERY_BROWSER_HAD_GONE: &str =
    "the discovery is over: its browser had already closed, and the app let it go";

/// Is the browser a session holds still there to drive? Its processes run
/// (its job still holds some; off Windows, its first process) and its own
/// DevTools connection answers a trivial call: one closed (the window or
/// its last tab was closed, or the browser crashed) or whose socket failed
/// is gone, even while some of its processes linger. One that is slow to
/// answer is busy, not gone. The yes or no is `launch::still_alive`'s.
pub async fn held_browser_alive<T: crate::browser::cdp::Transport>(
    browser: &mut LaunchedBrowser,
    cdp: &mut Cdp<T>,
) -> bool {
    use crate::browser::cdp::CdpError;
    let processes = crate::browser::tree::blocking(|| browser.processes());
    // On Windows the job is the word on whether it runs; when the job
    // cannot be listed (`None`) the first process says nothing - Edge may
    // have handed it on - so the socket is asked instead.
    let pid_ended = if cfg!(windows) && processes.is_none() { false } else { browser.pid_ended() };
    // Nothing of it runs: gone, without asking.
    if !crate::browser::launch::still_alive(processes, pid_ended, true) {
        return false;
    }
    let ask = serde_json::json!({ "expression": "1", "returnByValue": true });
    let mut asked = cdp.call_within("Runtime.evaluate", ask.clone(), ALIVE_WITHIN).await;
    // A refusal can be a page between two documents, or the answer to a
    // page that has just closed read before the browser's word that it
    // closed: asked once more a moment later, by when that word is read.
    if matches!(asked, Err(CdpError::Protocol { .. })) {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        asked = cdp.call_within("Runtime.evaluate", ask, ALIVE_WITHIN).await;
    }
    let answered = !matches!(asked, Err(CdpError::Closed) | Err(CdpError::Transport(_)));
    crate::browser::launch::still_alive(processes, pid_ended, answered)
}

/// How long a liveness check waits for the browser's connection to answer.
/// No answer in that time is a busy page, not a dead browser.
const ALIVE_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

/// Let go of the session in `slot` when its browser has gone
/// (`ai_bridge::forget_gone_browser`), saying so to the window when a
/// discovery went with it. True when it did. Called with the lock held.
pub(crate) async fn let_go_if_gone(slot: &mut Option<Session>) -> bool {
    let_go_if_gone_in(SlotNo::MAIN, slot).await
}

/// `let_go_if_gone` for slot `n`'s session: the discovery flag is that
/// slot's, and the panes are told only when it is the main browser that went.
pub(crate) async fn let_go_if_gone_in(n: SlotNo, slot: &mut Option<Session>) -> bool {
    let gone = crate::ai_bridge::forget_gone_browser(slot).await;
    if gone {
        publish_discovery_in(n, slot);
        tell_panes_closed_in(n);
    }
    gone
}

impl crate::ai_bridge::DiscoveryBrowser for Session {
    type D = Cdp;
    fn parts(&mut self) -> crate::ai_bridge::DiscoveryParts<'_, Cdp> {
        crate::ai_bridge::DiscoveryParts {
            driver: &mut self.cdp,
            lease: &mut self.lease,
            signed_in: &mut self.account,
            discovery: &mut self.discovery,
            guarded_case: &mut self.guarded_case,
            guard_owner: &mut self.guard_owner,
            overrides: Some(&mut self.overrides),
        }
    }
    fn close(self) {
        close_session(self);
    }
    async fn alive(&mut self) -> bool {
        held_browser_alive(&mut self.browser, &mut self.cdp).await
    }
    fn opened_by_assistant(&mut self) -> bool {
        self.opened_by == OpenedBy::Assistant
    }
    fn taken_over(&mut self) {
        // Its tabs other than `main` were just closed: they are no case's.
        self.tabs_case = None;
        self.used_otherwise();
        note_session_use_in(self.slot);
    }
    fn browser(&mut self) -> Option<Browser> {
        Some(self.kind)
    }
}

/// Open the supervised browser into `slot`, which is slot number `n`'s
/// session, closing one already there, marked as opened `by`. `root` is the
/// Auto Run folder its downloads go under (in slot `n`'s own folder). Called
/// with slot `n`'s session lock held.
async fn open_into(
    root: Result<PathBuf, String>,
    slot: &mut Option<Session>,
    n: SlotNo,
    which: Browser,
    by: OpenedBy,
) -> Result<(), String> {
    // Read for as long as this goes, never waited for: this holds its slot's
    // lock, which a switch holding the write waits for.
    let Ok(_opening) = crate::commands::autorun_slots::opening().try_read() else {
        return Err(crate::commands::autorun_slots::OPEN_WHILE_SWITCHING.to_string());
    };
    // Looked at with the session lock held: a recording claims its slot and
    // then waits on this lock to look for a session, so the two can never
    // both miss each other.
    crate::commands::autorun_record::refuse_while_recording()?;
    // A browser the person opens is theirs: no assistant holds it, even one
    // that was bound to this slot while it had no browser.
    if by == OpenedBy::Person {
        crate::commands::autorun_slots::unbind(n);
    }
    if let Some(old) = slot.take() {
        close_session(old);
    }
    let browser = launch_in_at(which, crate::commands::autorun_slots::window_position(n))?;
    // The browser binds its port when it is ready, which varies with what
    // else the machine is doing: asked until it answers, as an unattended
    // run's launch asks, rather than after one fixed sleep that a busy
    // machine outlasts.
    let port = browser.port;
    if let Err(last) = until_answers(|| Cdp::answers(port), ANSWER_TRIES, ANSWER_PAUSE).await {
        close_browser(browser);
        crate::applog::warn(format!("Auto-run browser never answered: {last}"));
        return Err(crate::commands::autorun_replay::NEVER_ANSWERED.to_string());
    }
    let mut cdp = match Cdp::connect(browser.port).await {
        Ok(cdp) => cdp,
        Err(e) => {
            // Connect failed after the process was already spawned and its
            // temp profile created. Without this, `browser` is dropped here
            // (Child has no kill-on-drop) and `*slot` is never assigned, so
            // `auto_run_close_browser` has nothing to take - the msedge.exe
            // and its %TEMP% profile dir leak for the rest of the app's
            // lifetime, and every retry leaks another pair.
            close_browser(browser);
            return Err(e);
        }
    };
    // Network and console events on from the start, as an unattended run's
    // browser has them: a step that checks a request reads the network
    // record, and a failure can say what the page was doing. Losing them is
    // no reason not to open the browser.
    if let Err(e) = crate::browser::page_log::watch(&mut cdp).await {
        crate::applog::warn(format!("auto-run: the page log could not be switched on: {e}"));
    }
    // The page's own requests and responses, for the assistant to read
    // while this browser is open (`net_capture`); gone when it closes.
    capture_supervised(&mut cdp, root.as_deref().ok());
    // Downloads are kept while this browser is open, in a folder emptied
    // as it opens and as it closes: they belong to no run. Losing them is
    // no reason not to open the browser either.
    let downloads_root = match keep_supervised_downloads(root, n, &mut cdp).await {
        Ok(root) => Some(root),
        Err(e) => {
            crate::applog::warn(format!("auto-run: downloads could not be switched on: {e}"));
            None
        }
    };
    *slot = Some(Session {
        browser,
        kind: which,
        cdp,
        account: None,
        lease: crate::autorun::lease::Held::supervised_in(n),
        guarded_case: None,
        guard_owner: None,
        downloads_root,
        tabs_case: None,
        discovery: None,
        opened_by: by,
        replay_stand: None,
        stepping: None,
        noted: Default::default(),
        overrides: Default::default(),
        slot: n,
    });
    note_session_use_in(n);
    // A tab the page opens waits, paused, until this connection reads that
    // it opened and sets it up: read between commands too, or a popup a
    // person opens by hand would sit blank until the next step.
    answer_between_commands_in(n);
    crate::applog::info(format!("Auto-run opened {}", which.label()));
    Ok(())
}

/// Switch a supervised browser's capture on, with the application's site
/// from the active environment's address when it has one - so a browser a
/// person signs in to by hand captures from the start. A sign-in, or a trip
/// to a module, names the site again (`Driver::set_capture_site`).
pub fn capture_supervised<T: crate::browser::cdp::Transport>(cdp: &mut Cdp<T>, root: Option<&std::path::Path>) {
    cdp.capture_requests();
    let site = root
        .and_then(|r| crate::environments::read_active(r).ok().flatten())
        .map(|e| e.start_url.trim().to_string())
        .filter(|u| !u.is_empty());
    if let Some(site) = site {
        if let Some(c) = cdp.capture_mut() {
            c.set_app_site(Some(&site));
        }
    }
}

/// How many times, and how far apart, a browser just started is asked
/// whether its DevTools port answers: about 15 seconds in all.
pub const ANSWER_TRIES: u32 = 60;
pub const ANSWER_PAUSE: std::time::Duration = std::time::Duration::from_millis(250);

/// Ask `ask` up to `tries` times, `every` apart (the first after one
/// pause), until it answers. `Err` is the last refusal, for the log: it
/// names a local port, nothing a person can act on.
pub async fn until_answers<F, Fut>(mut ask: F, tries: u32, every: std::time::Duration) -> Result<(), String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let mut last = String::from("it was never asked");
    for _ in 0..tries {
        tokio::time::sleep(every).await;
        match ask().await {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// End the browser's whole process tree and drop its throwaway profile
/// (`LaunchedBrowser::close`). Shared with `autorun_replay`, whose
/// `RealBrowsers` closes one of these after every `open`, and with the
/// recorder and the held template browsers. It waits for every process of
/// the browser to be gone first: a browser still shutting down holds files
/// in its profile, and removing the folder under it fails. True when the
/// tree is gone. One that will not go is dropped here, which closes its
/// job, and Windows kills what is left. The unattended run, which must not
/// start another browser beside one that will not go, uses
/// `LaunchedBrowser::close` itself.
pub(crate) fn close_browser(browser: LaunchedBrowser) -> bool {
    browser.close().is_ok()
}

/// Point slot `n`'s supervised browser at its own downloads folder
/// (`downloads/supervised` for main), emptied first of whatever an earlier
/// session left there. Returns the Auto Run folder it lives in, for the
/// close to empty it again.
async fn keep_supervised_downloads(root: Result<PathBuf, String>, n: SlotNo, cdp: &mut Cdp) -> Result<PathBuf, String> {
    let root = root?;
    if let Err(e) = store::empty_supervised_downloads_for(&root, n) {
        crate::applog::warn(format!("auto-run: an earlier browser's downloads could not all be removed: {e}"));
    }
    cdp.enable_downloads(&store::supervised_downloads_dir_for(&root, n)).await.map_err(|e| e.to_string())?;
    Ok(root)
}

/// The browser goes first: one still running holds the files it saved.
fn close_session(s: Session) {
    note_session_use_in(s.slot);
    // No assistant has a browser that is gone. A call that closes this
    // browser to open another for the same assistant binds it again.
    crate::commands::autorun_slots::unbind(s.slot);
    // What the assistant changed for testing went with the browser.
    show_overrides_in(s.slot, &crate::autorun::overrides::Overrides::new());
    close_browser(s.browser);
    if let Some(root) = s.downloads_root {
        if let Err(e) = store::empty_supervised_downloads_for(&root, s.slot) {
            crate::applog::warn(format!("auto-run: the closed browser's downloads could not all be removed: {e}"));
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn auto_run_close_browser() -> Result<(), String> {
    // A replay going in this browser is stopped first, without the lock it
    // holds: it ends at its next look, and the browser closes after it. A
    // case's setup still running is stopped between its template steps.
    crate::autorun::replay_to::stop();
    crate::autorun::setup::stop();
    crate::ai_bridge::stop_batch();
    // A discovery going in it ends with it: its state lives in the session.
    let mut slot = SESSION.lock().await;
    if crate::ai_bridge::close_browser_in(&mut slot) {
        crate::applog::info("Auto-run browser closed");
    }
    publish_discovery(&slot);
    Ok(())
}

/// Release Auto Run browser, on Auto Run's Setup card: for a browser the
/// app still holds after it is gone, so a restart is never needed. A replay
/// going is stopped (in every slot) and a case's setup between its template
/// steps; then every Auto Run browser record is cleared - every slot's
/// supervised browser, the discovery in it (a mapping run keeps its summary)
/// and the replay's, which is that same browser - and the browser closed
/// through its normal close, with each assistant's hold on it; then the
/// signed-in template browsers held for API templates; then
/// every browser still in the app's registry (`browser::tree::end_all`).
/// Each is ended through the job the app put it in, so nothing outside the
/// app's own jobs - the person's own Edge or Chrome, or any other program -
/// is ever touched.
///
/// It never races a run: the unattended run's, the recorder's and the API
/// template run's claims are TAKEN, not only looked at, and held with the
/// session's lock until `end_all` has returned, so nothing can start a
/// browser between the checks and the end. A claim another holds refuses
/// it with that run's sentence. The answer says what was released.
pub async fn release_autorun_browsers() -> Result<String, String> {
    let _unattended = crate::commands::autorun_replay::OneAtATime::claim().ok_or_else(|| UNATTENDED_GOING.to_string())?;
    let _recorder = crate::commands::autorun_record::RecorderClaim::claim()
        .ok_or_else(|| crate::commands::autorun_record::RECORDING_BUSY.to_string())?;
    let _template = crate::api_templates::runner::claim().ok_or_else(|| TEMPLATE_RUN_GOING.to_string())?;
    // A replay holds the session's lock while it goes: it is stopped first,
    // without the lock, and ends at its next look. Every slot's is.
    for n in SlotNo::all() {
        crate::autorun::replay_to::state(n).stop();
    }
    crate::autorun::setup::stop();
    crate::ai_bridge::stop_all_batches();
    // No browser opens while this goes (an open reads the opening lock and is
    // refused when it is written), and then every slot's session is locked in
    // slot order, each within `RELEASE_WAIT`.
    let Ok(_opening) = tokio::time::timeout(RELEASE_WAIT, crate::commands::autorun_slots::opening().write()).await
    else {
        return Err(BROWSER_BUSY.to_string());
    };
    let mut slots = Vec::with_capacity(SLOT_COUNT);
    for n in SlotNo::all() {
        let Ok(slot) = tokio::time::timeout(RELEASE_WAIT, session_slot(n).lock()).await else {
            return Err(BROWSER_BUSY.to_string());
        };
        slots.push(slot);
    }
    let mut held = false;
    for (n, slot) in SlotNo::all().into_iter().zip(slots.iter_mut()) {
        held |= crate::ai_bridge::close_browser_in(slot);
        publish_discovery_in(n, slot);
        // Nobody holds a browser that is gone, one bound while it had none
        // included.
        crate::commands::autorun_slots::unbind(n);
    }
    crate::browser::tree::blocking(crate::api_templates::held::close_all);
    let others = crate::browser::tree::held_profiles().len();
    let left = crate::browser::tree::blocking(crate::browser::tree::end_all);
    // The locks and the claims go only now: nothing could start a browser
    // that `end_all` would then have ended.
    drop(slots);
    tell_panes_closed();
    let answer = released_sentence(held, others, left);
    crate::applog::info(format!("Auto Run: {answer}"));
    Ok(answer)
}

/// How long Release waits for the browser's lock: a replay to stop, or a
/// supervised step or an assistant's call to finish.
pub(crate) const RELEASE_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// Said when Release is pressed while the template runner's one slot is
/// held: by an API template run, a fixture run (Run or Rebuild, the
/// person's or the assistant's), a cleanup run, or a case's setup (a
/// watched start's, a replay's or an unattended case's). Each holds that
/// claim for as long as its browser is open.
pub const TEMPLATE_RUN_GOING: &str =
    "an API template, fixture, cleanup or case setup run is going - wait for it, or stop it first";

/// Said when the browser's lock did not come free within `RELEASE_WAIT`: a
/// replay still stopping, or a long step or call still going in it.
pub const BROWSER_BUSY: &str = "the Auto Run browser is busy - press Release Auto Run browser again in a moment";

/// Whether the supervised browser open now is one the assistant opened,
/// with no discovery in it, holding the account `key`: what a template
/// refused that account names the release (`reset_autorun_browser`) for. Asked without
/// waiting on a replay or a step holding the lock: a busy browser answers
/// no.
pub async fn assistant_browser_holds(n: SlotNo, key: &str) -> bool {
    match tokio::time::timeout(std::time::Duration::from_millis(500), session_slot(n).lock()).await {
        Ok(slot) => slot.as_ref().is_some_and(|s| {
            s.opened_by == OpenedBy::Assistant && s.discovery.is_none() && s.lease.account() == Some(key)
        }),
        Err(_) => false,
    }
}

/// Said to the assistant's release while the person's own Auto Run browser
/// is open and answering: only the person closes it.
pub const PERSONS_BROWSER_OPEN: &str =
    "the person's Auto Run browser is open - ask them to close it, or to press Release Auto Run browser";

/// Tell the panes the supervised browser is gone (`AutorunSessionChanged`
/// with `opened: false`), so a Run Tests pane stops calling it its own.
/// Nothing to tell in the test binaries, which set up no window.
pub(crate) fn tell_panes_closed() {
    use tauri_specta::Event as _;
    if let Some(app) = DISCOVERY_EVENTS.get() {
        let _ = crate::events::AutorunSessionChanged { opened: false, account: None }.emit(app);
    }
}

/// `tell_panes_closed` for the browser of slot `n`: the panes show the main
/// browser only, so another slot's closing tells them nothing.
pub(crate) fn tell_panes_closed_in(n: SlotNo) {
    if n.is_main() {
        tell_panes_closed();
    }
}

/// Tell the panes of a change to the supervised browser (`changed`, when
/// there is one), as `tell_panes_closed` tells them it went. Nothing to tell
/// in the test binaries, which set up no window.
pub(crate) fn tell_panes(changed: Option<crate::events::AutorunSessionChanged>) {
    use tauri_specta::Event as _;
    if let (Some(app), Some(changed)) = (DISCOVERY_EVENTS.get(), changed) {
        let _ = changed.emit(app);
    }
}

/// `tell_panes` for the browser of slot `n`: only the main browser's changes
/// reach the panes.
pub(crate) fn tell_panes_in(n: SlotNo, changed: Option<crate::events::AutorunSessionChanged>) {
    if n.is_main() {
        tell_panes(changed);
    }
}

/// Tell the panes `event` of the browser of slot `n`, when it is the main
/// browser. The panes show the person's browser only, so an extra browser's
/// replay (it opening, signing in, stepping, closing) tells them nothing.
/// Every pane event of `replay_supervised` goes through this.
fn emit_if_main<E: tauri_specta::Event + serde::Serialize + Clone>(n: SlotNo, app: &tauri::AppHandle, event: E) {
    if n.is_main() {
        let _ = event.emit(app);
    }
}

/// What Release says it did: whether it let a held Auto Run browser go
/// (`held`), how many other browsers of the app's own it found running
/// (`others`), and how many of those would not end (`left`).
pub fn released_sentence(held: bool, others: usize, left: usize) -> String {
    let closed = others.saturating_sub(left);
    let mut said = match (held, closed) {
        (false, 0) => "no Auto Run browser was held, and none of the app's own browsers was open - nothing to release".to_string(),
        (true, 0) => "the Auto Run browser is released and closed".to_string(),
        (false, n) => format!("no Auto Run browser was held; {n} of the app's own browsers {} closed", if n == 1 { "was" } else { "were" }),
        (true, n) => format!(
            "the Auto Run browser is released and closed, and {n} more of the app's own browsers {} closed",
            if n == 1 { "was" } else { "were" }
        ),
    };
    if left > 0 {
        said.push_str(&format!(
            "; {left} would not close yet, and Windows ends {} when the app exits",
            if left == 1 { "it" } else { "them" }
        ));
    }
    said
}

/// Release Auto Run browser on Auto Run's Setup card
/// (`release_autorun_browsers`). Asks no confirm, as Close browser and End
/// discovery ask none: it closes only browsers the app opened, and the map
/// keeps what a discovery saw.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_release_browser() -> Result<String, String> {
    release_autorun_browsers().await
}

/// `/autorun-release`, the assistant's reset with `release`: the
/// repair for a broken browser, and the only call of the assistant's that
/// closes one. Only what is truly gone, or the assistant's own: a held
/// browser that has gone is let go (`forget_gone_browser`); a live
/// discovery is ended (the map and a mapping run's summary kept) and its
/// browser closed (`ai_bridge::end_discovery_closing`); a live browser the
/// assistant's replay opened is closed through its normal close; a live
/// browser the person opened is never closed - the answer asks for the
/// person. A replay going is never stopped: one going holds the lock, and
/// the answer says to wait.
pub async fn release_for_assistant(n: SlotNo) -> (u16, String) {
    if crate::autorun::replay_to::state(n).is_running() {
        return (409, crate::ai_bridge::replay_going_for_a_release(n));
    }
    crate::ai_bridge::stop_batch_in(n);
    let Ok(mut slot) = tokio::time::timeout(RELEASE_WAIT, session_slot(n).lock()).await else {
        return (409, BROWSER_BUSY.to_string());
    };
    let answer = release_in_for_assistant(&mut slot).await;
    publish_discovery_in(n, &slot);
    publish_overrides_in(n, &slot);
    if slot.is_none() {
        tell_panes_closed_in(n);
    }
    // Released: the assistant no longer has this browser, a person's left
    // open included.
    crate::commands::autorun_slots::unbind(n);
    crate::applog::info(format!("Auto Run: the assistant's release: {}", answer.1));
    answer
}

/// The assistant's release over `slot` (`release_for_assistant`), for any
/// held browser, a test's fake included.
pub async fn release_in_for_assistant<B: crate::ai_bridge::DiscoveryBrowser>(slot: &mut Option<B>) -> (u16, String) {
    match release_outcome_in(slot).await {
        ReleaseOutcome::AlreadyGone => (200, "the Auto Run browser had already closed; the app let it go, and none is held now".to_string()),
        ReleaseOutcome::NothingHeld => (200, "no Auto Run browser was held - nothing to release".to_string()),
        ReleaseOutcome::EndedDiscovery(status, said) => (status, said),
        ReleaseOutcome::ClosedAssistants => (200, RELEASED_YOURS.to_string()),
        ReleaseOutcome::PersonsKept => (409, PERSONS_BROWSER_OPEN.to_string()),
    }
}

/// What a release did to the browser in a slot. The assistant's release
/// says it in words written for the assistant (`release_in_for_assistant`);
/// the card's Release says it in its own (`release_slot`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseOutcome {
    /// The browser had gone; the app let it go.
    AlreadyGone,
    /// None was held.
    NothingHeld,
    /// A discovery was ended, its map kept, and its browser closed: the
    /// answer `end_discovery_closing` gave, for the assistant.
    EndedDiscovery(u16, String),
    /// A browser the assistant opened was closed.
    ClosedAssistants,
    /// The browser is the person's: it stays open.
    PersonsKept,
}

/// Lets go of the browser in `slot` as a release does, and says what it did.
pub async fn release_outcome_in<B: crate::ai_bridge::DiscoveryBrowser>(slot: &mut Option<B>) -> ReleaseOutcome {
    if crate::ai_bridge::forget_gone_browser(slot).await {
        return ReleaseOutcome::AlreadyGone;
    }
    let discovering = match slot.as_mut() {
        None => return ReleaseOutcome::NothingHeld,
        Some(b) => b.parts().discovery.is_some(),
    };
    if discovering {
        let (status, said) = crate::ai_bridge::end_discovery_closing(slot);
        return ReleaseOutcome::EndedDiscovery(status, said);
    }
    if slot.as_mut().is_some_and(|b| b.opened_by_assistant()) {
        crate::ai_bridge::close_browser_in(slot);
        return ReleaseOutcome::ClosedAssistants;
    }
    ReleaseOutcome::PersonsKept
}

/// Said by the assistant's release once it closed the browser its own
/// replay opened.
pub const RELEASED_YOURS: &str =
    "closed the Auto Run browser your replay left open; its account is free, and none is held now";

/// Said by the assistant's reset with no Auto Run browser held (or one that
/// had gone, now let go).
pub const NO_BROWSER_OPEN: &str = "No browser is open.";

/// Said by the assistant's reset while a replay holds the browser: a reset
/// never stops one.
pub const REPLAY_RUNNING_AT_RESET: &str = "A replay is running: try again when it has finished.";

/// Said by the assistant's reset when the home page no longer holds the
/// app's sign-in: the account and its cookies' print are forgotten.
pub const SIGN_IN_GONE_AT_RESET: &str = "Reset, but the sign-in has gone: the next replay signs in again.";

/// Put before the reset's answer when it first ended the assistant's
/// discovery in that browser.
pub const DISCOVERY_ENDED_AT_RESET: &str = "Ended your discovery (what it mapped is kept).";

/// Said when the browser's lock did not come free within `RELEASE_WAIT`
/// for the assistant's reset.
const BUSY_AT_RESET: &str = "the Auto Run browser is busy - call reset_autorun_browser again in a moment";

/// What the assistant's reset says once the browser is clean and still
/// signed in as `account`.
pub fn reset_sentence(account: &str) -> String {
    format!("Reset: other tabs closed, dialogs cleared, guard lifted, at the home page, still signed in as {account}.")
}

/// `/autorun-reset`, the assistant's `reset_autorun_browser`: a clean page
/// in the browser its replay or discovery left, without signing out, so it
/// never has to release and sign in again only to start clean
/// (`reset_in_for_assistant`). Refused while a replay runs (one going holds
/// the lock, and a reset never stops it); a batch of discovery actions is
/// asked to give way first, as release asks.
pub async fn reset_for_assistant(n: SlotNo, organization: &str, project: &str) -> (u16, String) {
    if crate::autorun::replay_to::state(n).is_running() {
        return (409, crate::ai_bridge::replay_running_refusal(n));
    }
    crate::ai_bridge::stop_batch_in(n);
    let Ok(mut slot) = tokio::time::timeout(RELEASE_WAIT, session_slot(n).lock()).await else {
        return (409, BUSY_AT_RESET.to_string());
    };
    // With no browser there is nothing to reset, data directory or not.
    let Some(root) = crate::autorun::store::configured_root() else {
        if slot.is_none() {
            return (404, NO_BROWSER_OPEN.to_string());
        }
        return (503, NO_DATA_DIRECTORY.to_string());
    };
    let held = slot.is_some();
    let account_before = slot.as_ref().and_then(|s| s.account.clone());
    let timing = crate::browser::timing::Timing::supervised();
    let answer = reset_in_for_assistant(&mut slot, &root, organization, project, &timing).await;
    let answer = crate::ai_bridge::let_go_if_silent(&mut slot, answer).await;
    publish_discovery_in(n, &slot);
    publish_overrides_in(n, &slot);
    if held && slot.is_none() {
        tell_panes_closed_in(n);
    } else if let Some(s) = slot.as_ref() {
        // A sign-in the reset found gone is forgotten: the panes hear that
        // nobody is signed in now, as they hear a replay's sign-in.
        tell_panes_in(n, crate::autorun::replay_to::sign_in_forgotten_event(&account_before, &s.account));
    }
    // A guard the browser would not lift pauses every request: the browser
    // is answered between the assistant's calls too.
    if slot.as_ref().is_some_and(|s| s.cdp.is_guarding_saves()) {
        answer_between_commands_in(n);
    }
    crate::applog::info(format!("Auto Run: the assistant's reset: {}", answer.1));
    answer
}

/// The assistant's reset over `slot` (`reset_for_assistant`), for any held
/// browser, a test's fake included. In order: a held browser that has gone
/// is let go and none is open; the person's browser - one they opened, or
/// one holding a guard they set - is never touched; the assistant's
/// discovery is ended as `end_autorun_discovery` ends it (what it mapped
/// kept, its guard lifted, the browser kept); the other tabs are closed;
/// the dialogs a replay expected or saw are forgotten (one opening later is
/// answered as nobody's); the assistant's own guard is lifted - no case is
/// in play to keep one; the replay's stand and last page change are
/// forgotten (`DiscoveryBrowser::taken_over`); and the home page is loaded
/// (`nav::load_home`), cookies kept. The sign-in still stands only when a
/// replay would have reused it before that load (`replay_to::sign_in_stands`:
/// the account, the lease, the sign-in's cookies) and the home page then
/// shows it (the signed-in marker, a session not refused);
/// otherwise - a home page whose marker never shows included - the account
/// and its cookies' print are forgotten, so the next replay signs in again,
/// and the app log says which condition failed (`NotSignedIn`).
///
/// The order matters: the sign-in's cookies are judged BEFORE the home load
/// (`replay_to::sign_in_stands`, on the page the replay left, as a replay's
/// own reuse check judges them), because the home page - the login page on
/// PeoplesHR - may re-issue a session cookie of the very same sign-in. Only
/// the app's own home load runs between that check and the cookie print
/// taken again after it, so a sign-in that stood still stands; the marker
/// and the refusal are looked at after the load.
/// A home page that does not open answers 502 (503 when the browser did not
/// answer), still saying what was cleaned first.
pub async fn reset_in_for_assistant<B: crate::ai_bridge::DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    if crate::ai_bridge::forget_gone_browser(slot).await {
        return (404, NO_BROWSER_OPEN.to_string());
    }
    let persons = match slot.as_mut() {
        None => return (404, NO_BROWSER_OPEN.to_string()),
        Some(b) => !b.opened_by_assistant() || *b.parts().guard_owner == Some(GuardOwner::Person),
    };
    if persons {
        return (409, PERSONS_BROWSER_OPEN.to_string());
    }
    // Read before anything changes: with no recipe, nothing is touched.
    let recipe = match crate::autorun::recipe::load_effective_recipe(root, organization, project) {
        Ok(recipe) => recipe,
        Err(why) => return (409, why),
    };
    let ended = crate::ai_bridge::end_discovery_keeping(slot, true).await;
    let summary = match ended {
        // Its guard would not lift, so its browser was closed.
        Some(ended) if !ended.kept => {
            return crate::ai_bridge::ended_answer(crate::ai_bridge::DISCOVERY_ENDED_CLOSED, ended.summary, ended.explore);
        }
        Some(ended) => Some(ended.summary),
        None => None,
    };
    let Some(b) = slot.as_mut() else {
        return (404, NO_BROWSER_OPEN.to_string());
    };
    let (lifted, left) = {
        let p = b.parts();
        Driver::close_other_tabs(p.driver).await;
        if let Some(book) = Driver::dialog_book(p.driver) {
            book.disarm();
            book.take_seen();
        }
        // What the assistant changed for testing is taken away: its test
        // headers, and the cookies it set - only those.
        let left = match p.overrides {
            Some(o) => crate::autorun::overrides::undo(p.driver, o).await,
            None => Vec::new(),
        };
        (lift_guard_of(p.driver, p.guarded_case, p.guard_owner).await, left)
    };
    b.taken_over();
    let guard = if lifted { "guard lifted" } else { GUARD_STILL_ON_AT_RESET };
    let p = b.parts();
    // The sign-in's cookies, judged before the home load re-issues any.
    let account = p.signed_in.clone();
    let stands = match account.as_deref() {
        Some(key) => {
            crate::autorun::replay_to::sign_in_stands(p.driver, root, organization, project, key, &account, &*p.lease).await
        }
        None => Err(NotSignedIn::NoAccount),
    };
    let home = crate::autorun::nav::load_home(p.driver, &crate::autorun::nav::Home::of(&recipe), timing).await;
    // The home page loaded, but its signed-in marker never showed: the
    // sign-in has gone. Neither the page failing to load nor the browser
    // not answering says that.
    let marker_gone = !home.ok && !home.harness && home.detail == crate::autorun::nav::HOME_NEVER_SIGNED_IN;
    // An after_sign_in step that stopped came after the marker showed: the
    // browser is home, signed in, and the sign-in is judged below.
    let loaded = home.ok || (!home.harness && home.detail.starts_with(crate::autorun::nav::AFTER_SIGN_IN_STOPPED_AT_HOME));
    if !loaded && !marker_gone {
        // The sign-in is left as it was (the next replay's own check judges
        // it), but a reason found before the load is not lost.
        if let Err(why) = &stands {
            crate::applog::info(format!(
                "Auto Run: the assistant's reset found the sign-in in doubt before the home page failed to open: {}",
                why.reason()
            ));
        }
        let status = if home.harness { 503 } else { 502 };
        return said_at_reset(status, summary, with_changes_left(reset_failed_sentence(guard, &home.detail), &left));
    }
    let verdict: Result<(), NotSignedIn> = match (stands, account.as_deref()) {
        (Err(why), _) => Err(why),
        _ if marker_gone => Err(NotSignedIn::HomeNeverSignedIn),
        (Ok(_), None) => Err(NotSignedIn::NoAccount),
        (Ok(recipe), Some(key)) => {
            // `load_home` waits for the marker itself only when the recipe
            // has `after_sign_in` steps; otherwise it is looked for here.
            let looked = if recipe.after_sign_in.is_empty() {
                crate::autorun::signin::already_signed_in_why(p.driver, &recipe, Some(key), key, timing, timing.nav_ms).await
            } else if crate::autorun::signin::session_refused(p.driver).await.is_some() {
                Err(NotSignedIn::SessionRefused)
            } else {
                Ok(())
            };
            match looked {
                // Only the app's own home load ran since the cookies were
                // judged: what it re-issued belongs to the same sign-in.
                Ok(()) => {
                    crate::autorun::replay_to::retake_sign_in(p.driver, &recipe.origins(), &mut *p.lease, "the home page").await
                }
                Err(why) => Err(why),
            }
        }
    };
    let said = match (verdict, account) {
        (Ok(()), Some(key)) => reset_sentence(&key).replace("guard lifted", guard),
        (verdict, _) => {
            if let Err(why) = verdict {
                crate::applog::info(format!("Auto Run: the assistant's reset found the sign-in gone: {}", why.reason()));
            }
            *p.signed_in = None;
            p.lease.forget_sign_in();
            if lifted {
                SIGN_IN_GONE_AT_RESET.to_string()
            } else {
                format!("{SIGN_IN_GONE_AT_RESET} {GUARD_STILL_ON_SENTENCE}")
            }
        }
    };
    said_at_reset(200, summary, with_changes_left(said, &left))
}

/// The reset's sentence `said`, saying after it which test changes it
/// could not take away (`overrides::undo`), when any.
fn with_changes_left(said: String, left: &[String]) -> String {
    if left.is_empty() {
        return said;
    }
    format!(
        "{said} These test changes could not be removed yet: {}. Call reset_autorun_browser again, or with release: true.",
        left.join("; ")
    )
}

/// `/autorun-cookies-clear`, the assistant's `autorun_cookies` clear: a
/// real sign-out in its own browser (`clear_cookies_in_for_assistant`).
/// Refused while a replay runs; the browser's lock is waited for 5 s at
/// most. The panes hear that nobody is signed in now, as they hear the
/// reset's sign-in gone (`sign_in_forgotten_event`).
pub async fn clear_cookies_for_assistant(n: SlotNo) -> (u16, String) {
    if crate::autorun::replay_to::state(n).is_running() {
        return (409, crate::ai_bridge::replay_running_refusal(n));
    }
    let Ok(mut slot) = tokio::time::timeout(crate::ai_bridge::CAPTURE_LOCK_WAIT, session_slot(n).lock()).await else {
        return (409, crate::ai_bridge::OVERRIDES_BUSY.to_string());
    };
    let (answer, event) = clear_cookies_in_for_assistant(n, &mut slot).await;
    let answer = crate::ai_bridge::let_go_if_silent(&mut slot, answer).await;
    publish_overrides_in(n, &slot);
    tell_panes_in(n, event);
    crate::applog::info(format!("Auto Run: the assistant's clear of the cookies answered {}", answer.0));
    answer
}

/// `autorun_cookies`' clear over `slot`: a real sign-out. Every cookie of
/// the app's confirmed site (`overrides::confirmed_origin`, never whatever
/// page shows - an identity provider's cookies are never touched) is
/// deleted, its host's and its parent domains' alike, the sign-in's too;
/// with no confirmed site, nothing is (409). Then the browser is
/// signed in as nobody: the account is forgotten, its cookie fingerprint
/// and lease let go, and the panes hear it (`sign_in_forgotten_event`, as
/// the reset tells them). The test headers go too (spec decision 1). A
/// discovery going keeps going, signed out; no replay carries on from where
/// one stood (`DiscoveryBrowser::taken_over`).
pub async fn clear_cookies_in_for_assistant<B: crate::ai_bridge::DiscoveryBrowser>(
    n: SlotNo,
    slot: &mut Option<B>,
) -> ((u16, String), Option<crate::events::AutorunSessionChanged>) {
    use crate::autorun::overrides as ov;
    if let Err(refused) = crate::ai_bridge::refuse_acting(n, slot).await {
        return (refused, None);
    }
    let Some(b) = slot.as_mut() else {
        return ((404, NO_BROWSER_OPEN.to_string()), None);
    };
    let (host, deleted, failed, before) = {
        let p = b.parts();
        let origin = match ov::confirmed_origin(&*p.driver) {
            Ok(origin) => origin,
            Err(refused) => return (refused, None),
        };
        let host = ov::url_host(&origin).unwrap_or_default();
        let cookies = match ov::cookies_for_host(p.driver, &host).await {
            Ok(c) => c,
            Err(e) => return (ov::browser_failed(e), None),
        };
        let (mut deleted, mut failed) = (0usize, 0usize);
        for c in &cookies {
            let params = serde_json::json!({
                "name": c["name"].as_str().unwrap_or(""),
                "domain": c["domain"].as_str().unwrap_or(""),
                "path": c["path"].as_str().unwrap_or("/"),
            });
            match crate::browser::cdp::Driver::call(p.driver, "Network.deleteCookies", params).await {
                Ok(_) => deleted += 1,
                Err(e @ (crate::browser::cdp::CdpError::Closed | crate::browser::cdp::CdpError::Transport(_))) => {
                    return (ov::browser_failed(e), None)
                }
                Err(_) => failed += 1,
            }
        }
        // Signed out for real: nobody is signed in here now, and the
        // account is free for another browser.
        let before = p.signed_in.take();
        p.lease.let_go();
        if let Some(o) = p.overrides {
            o.cookies.retain(|_, set| !crate::browser::session::cookie_belongs(&set.domain, &host));
            if !o.headers.is_empty() {
                if let Err(e) = crate::browser::cdp::Driver::set_extra_headers(p.driver, &Default::default()).await {
                    crate::applog::warn(format!("Auto Run: the test headers could not be removed at the sign-out: {e}"));
                } else {
                    o.headers.clear();
                }
            }
        }
        (host, deleted, failed, before)
    };
    b.taken_over();
    let event = crate::autorun::replay_to::sign_in_forgotten_event(&before, &None);
    let mut said = format!(
        "Signed out: deleted {deleted} cookie{} of {host}; nobody is signed in here now, the account is let go, and the next replay signs in again. A discovery going keeps going, signed out.",
        if deleted == 1 { "" } else { "s" }
    );
    if failed > 0 {
        said.push_str(&format!(" {failed} cookie{} would not delete.", if failed == 1 { "" } else { "s" }));
    }
    let o = b.parts().overrides.as_deref().cloned().unwrap_or_default();
    (crate::ai_bridge::overrides_answer(said, &o), event)
}

/// Said in place of "guard lifted" when the browser would not lift it: saves
/// stay stopped, and a later call lifts it.
pub const GUARD_STILL_ON_AT_RESET: &str = "guard still on (it would not lift yet)";

/// `GUARD_STILL_ON_AT_RESET` as a sentence of its own, after the reset's
/// sign-in-gone sentence.
pub const GUARD_STILL_ON_SENTENCE: &str = "The guard is still on (it would not lift yet).";

/// What the reset says when it cleaned the browser but the home page did not
/// open (`detail`, from `nav::load_home`): what it did first, then why not.
pub fn reset_failed_sentence(guard: &str, detail: &str) -> String {
    format!(
        "Reset: other tabs closed, dialogs cleared, {guard}, but the home page did not open: {detail}. Try reset_autorun_browser again, or with release: true if the browser is stuck."
    )
}

/// The reset's answer `said` with `status`, after the discovery-ended line
/// when it ended one (`summary` is `Some`), as `{detail, summary}` when that
/// was a mapping run.
fn said_at_reset(
    status: u16,
    summary: Option<Option<crate::autorun::mapping_summary::MappingSummary>>,
    said: String,
) -> (u16, String) {
    match summary {
        Some(Some(summary)) => {
            let detail = format!("{DISCOVERY_ENDED_AT_RESET} {said}");
            (status, serde_json::json!({ "detail": detail, "summary": summary }).to_string())
        }
        Some(None) => (status, format!("{DISCOVERY_ENDED_AT_RESET} {said}")),
        None => (status, said),
    }
}

/// Auto Run's browsers go with the app. A recording - or a Start, a check or
/// a Try still going - is ended the way Cancel ends it, which closes the
/// recording browser; then the supervised browser is closed. Each takes its
/// throwaway profile with it (`close_browser`). Called from the app's exit
/// hook in lib.rs and from an update restart, both of which bound it.
///
/// A Try or a check after Stop only sees the cancel at its next 250 ms poll
/// (`CANCEL_POLL`), and its claim is dropped only once its own browser is
/// already closed - so waiting here for the recorder to be let go is what
/// makes "the browser is gone" true for the caller, not just "a cancel was
/// asked for".
///
/// The browsers API template runs kept signed in (`api_templates::held`)
/// go too, closed off the async threads alongside the rest, so the
/// caller's bound covers them. Then every browser still in the registry
/// (`browser::tree`) is ended, an unattended run's included. Windows kills
/// any that are left when the app's job handles close.
///
/// A Playwright tab repo command still running (`pw_repo::process`) is
/// ended first, its whole job with it.
pub async fn close_autorun_browsers() {
    // The Playwright tab's repo command goes first: ending its job is
    // immediate, and nothing below can then hold it past the caller's bound.
    crate::pw_repo::process::stop_all();
    let held = tauri::async_runtime::spawn_blocking(crate::api_templates::held::close_all);
    let _ = crate::commands::autorun_record::auto_run_record_cancel().await;
    while crate::commands::autorun_record::recording_is_going() {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    for n in SlotNo::all() {
        crate::autorun::replay_to::state(n).stop();
    }
    crate::autorun::setup::stop();
    crate::ai_bridge::stop_all_batches();
    // Every slot's browser, taken in slot order, one lock at a time. Each
    // closes on a blocking thread of its own, so the caller's bound is not
    // spent three times over.
    let mut closing = Vec::new();
    for n in SlotNo::all() {
        let mut slot = session_slot(n).lock().await;
        // A mapping run going keeps its summary, as any other ending does.
        crate::ai_bridge::finish_mapping(&mut slot);
        // An explore session going has its change list closed the same way.
        crate::ai_bridge::finish_explore(&mut slot);
        let session = slot.take();
        drop(slot);
        // Closing waits for the browser's processes to go: on a blocking
        // thread, so the caller's bound around this future still holds.
        if let Some(s) = session {
            closing.push(tauri::async_runtime::spawn_blocking(move || close_session(s)));
        }
    }
    for done in closing {
        let _ = done.await;
        crate::applog::info("Auto-run browser closed as the app exits");
    }
    let _ = held.await;
    // Last, every browser the app started that is still running: an
    // unattended run's, which nothing above closes, and any other one.
    // Each is ended as a whole tree, and its profile removed.
    let _ = tauri::async_runtime::spawn_blocking(crate::browser::tree::end_all).await;
}

/// What a supervised step answers: one outcome per action, the tab the
/// step ran in when that was not `main` (`runner::InRun::tab`), and the
/// browser dialog it met, if any (`runner::InRun::dialog`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct StepRun {
    pub outcomes: Vec<ActionOutcome>,
    pub tab: Option<String>,
    pub dialog: Option<crate::autorun::StepDialog>,
    /// How many page errors a `"page_errors": "flag"` script counted in the
    /// step (`runner::InRun::page_errors_seen`).
    pub page_errors_seen: u32,
}

/// Run one step's actions in order and report every outcome. Actions after
/// an ordinary failure still run: the watcher learns more from "the click
/// worked, the check did not" than from a run that stops at the first red.
/// A failed `sign_in` is the exception - see `autorun::runner::run_step`,
/// which does the actual work; this command is just the IPC-facing shell
/// around it.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_step(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    case_id: i32,
    step: StepScript,
) -> Result<StepRun, String> {
    let root = root(&app)?;
    // The case's script is not deleted while it is run step by step: noted
    // before anything reads it, and kept by the browser below.
    let replaying = store::replaying(case_id);
    // The step's fixture values: shared drafts' current outputs, and what
    // the case's setup gave at its start. The step the pane holds is never
    // changed; a value still missing refuses the step with its sentence.
    let step = crate::autorun::setup::resolve_step(&root, &organization, &project, case_id, &step)?;
    let mut slot = SESSION.lock().await;
    // A discovery's browser is the assistant's: a step would act behind its
    // back, and could lift a mapping run's save guard (`guard_supervised`).
    // One that has gone is let go rather than refused for.
    if slot.as_ref().is_some_and(|s| s.discovery.is_some()) {
        let_go_if_gone(&mut slot).await;
    }
    crate::ai_bridge::refuse_while_discovering(&mut slot)?;
    let session = slot.as_mut().ok_or_else(describe_session_error)?;
    // A browser the person runs a step in is theirs from now on, and no
    // replay carries on from where it stood.
    session.make_persons().await?;
    session.replay_stand = None;
    hold_stepped_run(&mut session.stepping, replaying);
    note_session_use();
    // Another case's tabs do not carry over into this one.
    // The saved script's own choices about dialogs nobody expected and the
    // page's own errors, and which of its steps comes first.
    let saved = crate::autorun::store::load_script(&root, case_id).ok().flatten();
    let first_step = saved.as_ref().and_then(|s| s.steps.iter().map(|s| s.step_number).min()) == Some(step.step_number);
    if first_step || session.tabs_case != Some(case_id) {
        session.noted.forget(&session.cdp);
    }
    // Every step a place of its own. The pane holds the records, so what an
    // earlier step's action caused late is said only in a failure's words.
    session.noted.step = session.noted.step.wrapping_add(1);
    session.noted.take_late();
    // The area the case is in: its script's, unless a `return_to_area` of
    // an earlier step took it elsewhere.
    session.noted.in_case(saved.as_ref().and_then(|s| s.area.as_deref()));
    crate::autorun::runner::supervised_step_begins(&mut session.cdp, &mut session.tabs_case, case_id, first_step).await;
    guard_supervised(session, &root, &organization, &project, case_id, true).await?;
    // A watched run makes no trip of its own: a bare `return_to_area` builds
    // the case's route from its saved script, and one that names an area
    // that area's route - each only when the step has one.
    use crate::autorun::runner::{area_route, area_routes, named_areas, AreaRoute};
    let wants_area = step.actions.iter().flat_map(|a| a.each()).any(|a| {
        matches!(a, crate::browser::actions::Action::ReturnToArea { .. }) && a.area_named().is_none()
    });
    let resolved = if wants_area { Some(area_route(&root, &organization, &project, case_id)) } else { None };
    let area = match &resolved {
        Some(Ok(r)) => AreaRoute::To(r),
        Some(Err(why)) => AreaRoute::Unknown(why),
        None => AreaRoute::Unknown(crate::autorun::runner::NEEDS_SCRIPT_AREA),
    };
    let areas = area_routes(&root, &organization, &project, &named_areas(&step.actions));
    let mut run = crate::autorun::runner::InRun {
        areas: Some(&areas),
        fail_on_unexpected_dialog: saved.as_ref().is_some_and(|s| s.fail_on_unexpected_dialog),
        page_errors: saved.as_ref().and_then(|s| s.page_errors),
        ignore_page_errors: saved.map(|s| s.ignore_page_errors).unwrap_or_default(),
        noted: Some(&mut session.noted),
        ..Default::default()
    };
    let outcomes = crate::autorun::runner::run_step_in_run(
        &mut session.cdp,
        &root,
        &organization,
        &project,
        &step,
        &crate::browser::timing::Timing::supervised(),
        &mut session.account,
        &mut session.lease,
        None,
        area,
        &mut run,
    )
    .await?;
    let (tab, dialog, page_errors_seen) = (run.tab.take(), run.dialog.take(), run.page_errors_seen);
    drop(run);
    // A step that failed teaches the application's rules, with what came
    // late for the step before and the earlier action its failure line
    // names (`runner::remember_failed_step`).
    if outcomes.iter().any(|o| !o.ok) {
        let late = session.noted.take_late();
        let answers = session.noted.with_earlier(&late);
        crate::autorun::runner::remember_failed_step(
            &root,
            &organization,
            &project,
            &step,
            &outcomes,
            &answers,
            session.noted.area(),
            case_id,
        );
    }
    Ok(StepRun { outcomes, tab, dialog, page_errors_seen })
}

/// Replay case `case_id`'s saved steps 1 to `step` - 1 in the supervised
/// browser and stop before `step` (`autorun::replay_to`), for the person's
/// Replay to step button. The browser open is used; with none, the one last
/// chosen is opened first, as Open browser opens it. One replay at a time.
/// The pane hears `AutorunReplayProgress` before each step. Refusals and
/// the end come back as the `ReplayEnd` with its sentence, which the pane
/// shows as it is; `Err` is a browser that would not open.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_replay_to_step(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    case_id: i32,
    step: i32,
    db_read_access: bool,
) -> Result<crate::autorun::replay_to::ReplayAnswer, String> {
    let db_read_access = crate::autorun::preconditions::read_access(db_read_access);
    let req = crate::autorun::replay_to::ReplayRequest { case_id, step, db_read_access };
    // The person's own replay, as their own step: it may lift an earlier
    // case's guard. The tabs its steps ran in go to the pane beside them.
    let (end, tabs, _, _, _, _, _) = replay_supervised(&app, SlotNo::MAIN, &organization, &project, req, ReplayBy::Person, None).await?;
    // The clean-up named as such: noted by this replay's steps.
    let cleanup = crate::autorun::replay_to::cleanup_at();
    Ok(crate::autorun::replay_to::ReplayAnswer::with_tabs(end, tabs, cleanup))
}

/// Who asked for a replay to a step, which decides two rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayBy {
    /// The person's Replay to step button.
    Person,
    /// The assistant's `replay_autorun_to_step` (`ai_bridge`'s
    /// `/autorun-replay`).
    Assistant,
}

impl ReplayBy {
    /// `guard_for_case`'s `may_lift`, for a guard `owner` holds: the
    /// person's replay may lift a no-save guard held for another case; the
    /// assistant's may switch one on, and lifts only one a call of its own
    /// left (`GuardOwner::Call`) - never the person's, nor a no_save
    /// discovery's.
    pub fn may_lift(self, owner: Option<&GuardOwner>) -> bool {
        match self {
            ReplayBy::Person => true,
            ReplayBy::Assistant => matches!(owner, None | Some(GuardOwner::Call { .. })),
        }
    }

    /// Who a guard this replay puts on for case `case_id` belongs to.
    pub fn owns_as(self, case_id: i32) -> GuardOwner {
        match self {
            ReplayBy::Person => GuardOwner::Person,
            ReplayBy::Assistant => GuardOwner::Call { case_id: Some(case_id) },
        }
    }

    /// Whether a discovery holding the browser is ended first rather than
    /// refusing: only for the assistant, whose own discovery it is. The
    /// person's button is greyed while one runs, and refused if pressed.
    pub fn ends_discovery(self) -> bool {
        self == ReplayBy::Assistant
    }

    /// Who a browser this replay opens is marked as opened by.
    pub fn opens_as(self) -> OpenedBy {
        match self {
            ReplayBy::Person => OpenedBy::Person,
            ReplayBy::Assistant => OpenedBy::Assistant,
        }
    }
}

/// A replay to a step in slot `n`'s supervised browser (the person's
/// callers pass `SlotNo::MAIN`), for the person's button and the assistant's
/// tool alike (`ai_bridge`'s `/autorun-replay`). One at a time in a slot;
/// refused before anything opens when the checks fail; the browser open is
/// used, else the one last chosen is opened. `by` decides
/// whether another case's guard may be lifted and whether a discovery
/// holding the browser is ended (under the same lock the replay then runs
/// under) or refused (`ReplayBy`); the third value is the discovery it
/// ended, if any. With the end come the tabs its steps ran in outside
/// `main` (`replay_to_traced`), and, last, what it made of a browser
/// already signed in as the case's account (`replay_to_traced`'s `reuse`).
/// `Err` is a browser that would not open. `holder` is the assistant whose
/// call it is, if it is one: its binding of slot `n` is kept through ending a
/// discovery, letting a gone browser go and opening the browser the replay
/// uses, so no other assistant's call can take the slot in between. The
/// person's replay and the default caller pass none.
pub(crate) async fn replay_supervised(
    app: &tauri::AppHandle,
    n: SlotNo,
    organization: &str,
    project: &str,
    req: crate::autorun::replay_to::ReplayRequest,
    by: ReplayBy,
    holder: Option<&crate::autorun::agents::AgentKey>,
) -> Result<
    (
        crate::autorun::replay_to::ReplayEnd,
        Vec<crate::autorun::replay_to::ReplayedTab>,
        crate::ai_bridge::DiscoveryEnded,
        Option<crate::autorun::replay_to::LeftOpen>,
        crate::autorun::replay_to::ReplayFrom,
        crate::autorun::replay_to::Reuse,
        crate::autorun::overrides::Overrides,
    ),
    String,
> {
    use crate::ai_bridge::DiscoveryEnded;
    use crate::autorun::replay_to::{self, OneReplay, ReplayEnd, ReplayFrom};
    const BEGINNING: ReplayFrom = ReplayFrom::Beginning;
    // Whose it is: the replay tool's `stop` stops the assistant's only.
    let claimed = match by {
        ReplayBy::Assistant => OneReplay::claim_for_assistant_in(n),
        ReplayBy::Person => OneReplay::claim_in(n),
    };
    let _one = match claimed {
        Ok(one) => one,
        Err(why) => return Ok((ReplayEnd::Refused(why), Vec::new(), DiscoveryEnded::default(), None, BEGINNING, Default::default(), overrides_now_in(n))),
    };
    let root = root(app)?;
    // Its script is not deleted while the replay goes: noted before the
    // check reads the script, so a delete is either done already (the
    // check finds no script) or refused.
    let _replaying = store::replaying(req.case_id);
    // Refused before anything opens.
    if let Err(why) = replay_to::check(&root, organization, project, &req) {
        return Ok((ReplayEnd::Refused(why), Vec::new(), DiscoveryEnded::default(), None, BEGINNING, Default::default(), overrides_now_in(n)));
    }
    let (case_id, step, db_read_access) = (req.case_id, req.step, req.db_read_access);
    let secrets = std::sync::Arc::clone(&app.state::<crate::db::DbSecrets>().0);
    let mut slot = session_slot(n).lock().await;
    let hold = holder.map(|key| crate::commands::autorun_slots::hold_binding(n, key));
    // A Close that took the lock first is respected: no browser is opened
    // again for a replay the person has already stopped.
    if let Some(stopped) = replay_to::stopped_before_opening(&replay_to::state(n).cancel) {
        return Ok((stopped, Vec::new(), DiscoveryEnded::default(), None, BEGINNING, Default::default(), overrides_now_in(n)));
    }
    // A replay never runs beside a discovery. The person's is refused; the
    // assistant's ends its own discovery first, the way End discovery does,
    // still holding the lock it replays under, and replays in that same
    // browser, signed in as it was.
    // A held browser that has gone - a discovery's, or one an earlier replay
    // or the person left - is let go before anything is refused for it or
    // replayed in it (`open_if_none` then opens a new one).
    let_go_if_gone_in(n, &mut slot).await;
    let mut discovery = DiscoveryEnded::default();
    if by.ends_discovery() {
        discovery = crate::ai_bridge::end_discovery_for_replay(&mut slot, &root).await;
        if discovery.ended {
            publish_discovery_in(n, &slot);
        }
    } else if let Err(why) = crate::ai_bridge::refuse_while_discovering(&mut slot) {
        return Ok((ReplayEnd::Refused(why), Vec::new(), DiscoveryEnded::default(), None, BEGINNING, Default::default(), overrides_now_in(n)));
    }
    let had_browser = slot.is_some();
    if let Err(why) = open_if_none(app, &mut slot, n, by.opens_as()).await {
        return Err(crate::ai_bridge::after_ended_discovery(discovery.ended, &why));
    }
    if let Some(key) = holder {
        crate::commands::autorun_slots::bind_if_free(n, key);
    }
    // The hold covered the open only: a browser that closes during the
    // replay, or at its end, leaves the slot empty and unbound
    // (`close_session`), as it does for anyone else.
    drop(hold);
    // The panes hear of a browser this replay opened, and of the account
    // it leaves the browser signed in as, before the lock lets them in. Only
    // the person's browser is theirs to hear of (`emit_if_main`).
    if let Some(changed) = replay_to::opened_event(had_browser) {
        emit_if_main(n, app, changed);
    }
    let session = slot.as_mut().ok_or_else(describe_session_error)?;
    // The person's replay makes a browser already open theirs; the
    // assistant's leaves its mark as it was.
    if by == ReplayBy::Person {
        if let Err(why) = session.make_persons().await {
            let changed = session.overrides.clone();
            return Ok((ReplayEnd::Refused(why), Vec::new(), discovery, None, BEGINNING, Default::default(), changed));
        }
    }
    // Where it starts: the assistant's own browser, open and alive, that
    // its last replay of this case left before an earlier step, with
    // nothing used in it since, carries on from there; anything else
    // starts from the beginning. The stand is used up either way.
    note_session_use_in(n);
    let stand = session.replay_stand.take();
    // A watched run's last page change is not carried past a replay.
    session.noted.forget(&session.cdp);
    // The script as it is now, loaded once: what this replay runs, what
    // decides whether it carries on, and what its stand is stamped with.
    let saved = store::load_script(&root, case_id).ok().flatten();
    let mut not_carried: Option<String> = None;
    let from = match (by, had_browser, session.opened_by, &stand, &saved) {
        (ReplayBy::Assistant, true, OpenedBy::Assistant, Some(stand), Some(saved)) => {
            let (here, _) = crate::ai_bridge::current_page(&mut session.cdp).await;
            let now_ms = crate::autorun::sessions::now_ms();
            match replay_to::carry_from(Some(stand), saved, step, &here, now_ms) {
                // Not the same case's carry: another case's first steps,
                // when they are the same and left the page as it is.
                ReplayFrom::Beginning => {
                    let now = replay_to::page_state(&mut session.cdp).await;
                    let (from, why) = replay_to::carry_across_why(Some(stand), saved, step, &now, now_ms);
                    not_carried = why;
                    from
                }
                same_case => same_case,
            }
        }
        _ => BEGINNING,
    };
    let account_before = session.account.clone();
    // The replay starts the case afresh, closing every tab but `main`
    // first (`replay_to_checked`): the tabs left are this case's.
    session.tabs_case = Some(case_id);
    // A case's setup runs in a browser of its own, the one last chosen.
    let mut setup_browsers = crate::commands::autorun_replay::RealBrowsers::new(
        crate::browser::launch::Browser::from_name(&store::last_browser(&root)),
        false,
    );
    let mut tabs = Vec::new();
    let mut reuse = replay_to::Reuse::default();
    // Where the page stood after each case step this replay ran.
    let mut marks = replay_to::Marks::default();
    // The guard as its owner needs it, before anything runs: a no_save
    // discovery this replay ended keeps its guard on (`guard_before_replay`),
    // and whether this replay may lift a guard is decided by whose it is.
    let guard = guard_before_replay(
        &mut session.cdp,
        &mut session.guard_owner,
        discovery.no_save,
        by,
        &root,
        organization,
        project,
    )
    .await;
    let mut end = match (guard, from) {
        (Err(why), _) => ReplayEnd::Refused(why),
        (Ok(_), ReplayFrom::AlreadyThere) => ReplayEnd::Ready {
            case_id,
            step,
            notice: None,
            whole: saved.as_ref().and_then(|s| replay_to::whole_case(s, step)),
            cleanup: saved.as_ref().and_then(|s| replay_to::cleanup_ran(s, step)),
        },
        (Ok(_), ReplayFrom::CarriedOn(k)) => {
            replay_to::carry_on(
                &mut session.cdp,
                &root,
                organization,
                project,
                &req,
                k,
                &mut session.account,
                &mut session.lease,
                &mut session.guarded_case,
                &crate::browser::timing::Timing::supervised(),
                replay_to::state(n),
                |k, of| {
                    emit_if_main(n, app, crate::events::AutorunReplayProgress { case_id, step: k, of });
                },
                &mut tabs,
                replay_to::wrote_before(stand.as_ref(), k),
                &mut marks,
            )
            .await
        }
        (Ok(_), ReplayFrom::CarriedOnFrom { k, .. }) => {
            // The checkpoint it carries on from saved nothing.
            replay_to::carry_on(
                &mut session.cdp,
                &root,
                organization,
                project,
                &req,
                k + 1,
                &mut session.account,
                &mut session.lease,
                &mut session.guarded_case,
                &crate::browser::timing::Timing::supervised(),
                replay_to::state(n),
                |k, of| {
                    emit_if_main(n, app, crate::events::AutorunReplayProgress { case_id, step: k, of });
                },
                &mut tabs,
                false,
                &mut marks,
            )
            .await
        }
        (Ok(may_lift), ReplayFrom::Beginning) => {
            replay_to::replay_to_traced(
                &mut session.cdp,
                &mut setup_browsers,
                &root,
                organization,
                project,
                &req,
                &mut session.account,
                &mut session.lease,
                &mut session.guarded_case,
                may_lift,
                &crate::browser::timing::Timing::supervised(),
                replay_to::state(n),
                || crate::autorun::preconditions::for_run(&root, Some(secrets.as_ref()), db_read_access),
                |k, of| {
                    emit_if_main(n, app, crate::events::AutorunReplayProgress { case_id, step: k, of });
                },
                &mut tabs,
                &mut reuse,
                &mut marks,
            )
            .await
        }
    };
    // An assistant's replay that reached its step leaves its stand, for its
    // next replay of this case to carry on from; any other end leaves none.
    session.replay_stand = match (&end, &saved) {
        (ReplayEnd::Ready { .. }, Some(saved)) if by == ReplayBy::Assistant => {
            let (here, _) = crate::ai_bridge::current_page(&mut session.cdp).await;
            // What the old stand's checkpoints still hold, then this run's own.
            let mut checkpoints = replay_to::checkpoints_kept(from, stand.as_ref());
            checkpoints.append(&mut marks.checkpoints);
            // What an earlier segment of this replay did stays with it.
            let unclean = replay_to::unclean_kept(from, stand.as_ref()).or(marks.unclean);
            Some(replay_to::ReplayStand::of(saved, step, &here, crate::autorun::sessions::now_ms(), checkpoints).with_unclean(unclean))
        }
        _ => None,
    };
    if from == ReplayFrom::Beginning {
        reuse.not_carried = not_carried;
    }
    if let Some(changed) = replay_to::signed_in_event(&account_before, &session.account) {
        emit_if_main(n, app, changed);
    }
    // A guard this replay put on for its case is the replay's; one it
    // lifted has no owner left; one it kept keeps its own - but a no_save
    // discovery's guard, once the discovery is gone, was this replay's only.
    let discovering = session.discovery.is_some();
    settle_replay_guard(&session.cdp, session.guarded_case, &mut session.guard_owner, case_id, by, discovering);
    if session.cdp.is_guarding_saves() {
        answer_between_commands_in(n);
    }
    // The browser is kept or let go the same way on every path: kept while
    // it is there, so the person can heal the step; let go, through its
    // normal close, once it has gone - when the replay met it closed, or
    // stopped early and the browser is then found gone.
    let gone = match &end {
        ReplayEnd::BrowserGone { .. } => {
            crate::ai_bridge::close_browser_in(&mut slot);
            publish_discovery_in(n, &slot);
            true
        }
        ReplayEnd::Ready { .. } => false,
        _ => let_go_if_gone_in(n, &mut slot).await,
    };
    if gone {
        // Said as the browser closing, keeping a failure's reason and rows.
        end = match end {
            ReplayEnd::StoppedAt { step, why, outcomes, .. } => ReplayEnd::BrowserGone { step, why: Some(why), outcomes },
            ReplayEnd::Stopped { step } => ReplayEnd::BrowserGone { step, why: None, outcomes: Vec::new() },
            other => other,
        };
        emit_if_main(n, app, crate::events::AutorunSessionChanged { opened: false, account: None });
    }
    let how = match &end {
        ReplayEnd::Ready { .. } => match from {
            ReplayFrom::Beginning if reuse.went.is_some() => "ready, signed in already".to_string(),
            ReplayFrom::Beginning => "ready".to_string(),
            ReplayFrom::CarriedOn(k) => format!("ready, carried on from step {k}"),
            ReplayFrom::CarriedOnFrom { case_id: other, k } => {
                format!("ready, carried on from step {k} of case {other}")
            }
            ReplayFrom::AlreadyThere => "already there, nothing ran".to_string(),
        },
        ReplayEnd::StoppedAt { phase: replay_to::ReplayPhase::SignIn, .. } => "stopped while signing in".to_string(),
        ReplayEnd::StoppedAt { phase: replay_to::ReplayPhase::Area, .. } => "stopped going to the area".to_string(),
        ReplayEnd::StoppedAt { step, .. } => format!("stopped at step {step}"),
        ReplayEnd::Blocked(_) => "blocked before signing in".to_string(),
        ReplayEnd::Stopped { step } => format!("stopped before step {step} finished"),
        ReplayEnd::BrowserGone { step, .. } => format!("stopped at step {step}: the browser had closed, and it is let go"),
        ReplayEnd::Refused(_) => "refused".to_string(),
    };
    let who = match by {
        ReplayBy::Person => "",
        ReplayBy::Assistant => " for the assistant",
    };
    crate::applog::info(format!("Auto Run replay of case {case_id} to step {step}{who}: {how}"));
    // A replay that stopped on a failure is kept as the case's newest word,
    // for get_autorun_failures and a suspected defect; one that reached its
    // step lets an earlier one go.
    let title = store::load_script(&root, case_id).ok().flatten().map(|s| s.title).unwrap_or_default();
    let ran_as = slot.as_ref().and_then(|s| s.lease.account().map(str::to_string));
    crate::autorun::replay_record::note(&root, case_id, &title, step, &end, ran_as, crate::autorun::sessions::now_ms());
    if let Some(o) = crate::autorun::proven::outcome_of_replay(&end) {
        crate::autorun::proven::note_outcome(&root, case_id, o);
    }
    // The browser the replay leaves open, if it does, and whose it is.
    let left_open = slot.as_ref().map(|s| replay_to::LeftOpen {
        account: s.lease.account().map(str::to_string),
        by_assistant: s.opened_by == OpenedBy::Assistant,
    });
    // What the assistant changed for testing, as it stands the moment the
    // replay ends, read under the lock the replay ran under.
    let changed = slot.as_ref().map(|s| s.overrides.clone()).unwrap_or_default();
    Ok((end, tabs, discovery, left_open, from, reuse, changed))
}

/// The replay's stop control: the replay going, if any, ends at its next
/// look and says where it stopped.
#[tauri::command]
#[specta::specta]
pub fn auto_run_stop_replay() {
    crate::autorun::replay_to::stop();
}

/// A supervised case's preconditions, checked where the case starts and
/// before its sign-in. `blocked`: the case is Blocked, with the sentence
/// as its reason, and the pane never signs it in. `notice`: said before
/// step 1, and the case goes on (the checks were skipped while
/// `db_read_access`, the Database Access module being on, is off). Neither: the case goes on. The active environment's database is
/// looked up only when the case's script has preconditions. Once they are
/// met, the case's fixtures are prepared as an unattended case's are
/// (`autorun::setup::check_supervised`): a setup fixture that is not
/// saved, a shared fixture never built or a failed setup run is `blocked` with its
/// sentence, and what the setup gave is kept for the case's steps.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_check_preconditions(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    case_id: i32,
    db_read_access: bool,
) -> Result<crate::autorun::preconditions::PreconditionCheck, String> {
    let db_read_access = crate::autorun::preconditions::read_access(db_read_access);
    let root = root(&app)?;
    // A watched case starts here: the last case's tabs are closed, even
    // when it is the same case started again.
    if let Some(session) = SESSION.lock().await.as_mut() {
        session.cdp.close_other_tabs().await;
        session.tabs_case = Some(case_id);
        // The person starting a case in this browser makes it theirs.
        session.make_persons().await?;
        session.used_otherwise();
        note_session_use();
    }
    let secrets = std::sync::Arc::clone(&app.state::<crate::db::DbSecrets>().0);
    let mut checked = crate::autorun::preconditions::check_script(&root, &organization, &project, case_id, || {
        crate::autorun::preconditions::for_run(&root, Some(secrets.as_ref()), db_read_access)
    })
    .await
    .inspect_err(|_| crate::autorun::setup::forget(case_id))?;
    if checked.blocked.is_some() {
        // A start that ends Blocked leaves no setup values behind.
        crate::autorun::setup::forget(case_id);
        return Ok(checked);
    }
    let mut browsers = crate::commands::autorun_replay::RealBrowsers::new(
        crate::browser::launch::Browser::from_name(&store::last_browser(&root)),
        false,
    );
    let timing = crate::commands::autorun_replay::replay_timing(false);
    let blocked = supervised_setup(
        &mut browsers,
        &root,
        &organization,
        &project,
        case_id,
        &timing,
        crate::api_templates::runner::RUN_LIMIT,
        &crate::api_templates::runner::RETRY_PAUSES,
    )
    .await;
    if let Some(why) = blocked {
        // A precondition "checks skipped" notice is still said.
        checked.blocked = Some(why);
    }
    Ok(checked)
}

/// The supervised browser making way for a setup that signs in as `key`
/// (`setup::make_way`). The main browser always makes way, as it always did
/// (it only lets go of an account it holds); an extra browser does only when
/// it holds `key` - the account is held by one browser at a time, so that is
/// where the setup's sign-in would meet it. Each slot's lock is held for that
/// alone, one after the other, never while the setup runs.
pub async fn make_way_for_setup(key: String, timing: &crate::browser::timing::Timing) {
    for n in SlotNo::all() {
        let mut slot = session_slot(n).lock().await;
        let Some(s) = slot.as_mut() else { continue };
        let holds = s.lease.account() == Some(key.as_str()) || s.account.as_deref() == Some(key.as_str());
        if !n.is_main() && !holds {
            continue;
        }
        s.used_otherwise();
        note_session_use_in(n);
        crate::autorun::setup::make_way(&mut s.cdp, &mut s.account, &mut s.lease, &key, timing).await;
        if holds {
            break;
        }
    }
}

/// A supervised start's setup (`setup::check_supervised`), with the
/// supervised browser making way only once the setup will run. Nothing
/// here holds `SESSION` while the setup runs, so Close, a step, Open
/// browser and an unattended Start all still answer; Close stops the
/// setup between its template steps. `Some` is the Blocked sentence.
#[allow(clippy::too_many_arguments)]
pub async fn supervised_setup<B: crate::autorun::replay::Browsers>(
    browsers: &mut B,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    case_id: i32,
    timing: &crate::browser::timing::Timing,
    limit: std::time::Duration,
    retry_pauses: &[std::time::Duration],
) -> Option<String> {
    crate::autorun::setup::check_supervised(
        browsers,
        root,
        organization,
        project,
        case_id,
        timing,
        |key| make_way_for_setup(key, timing),
        limit,
        retry_pauses,
    )
    .await
}

/// Put the supervised browser's no-save guard where this case needs it
/// (`guard_for_case`), for a step or an assistant's try alike. `may_lift`
/// is the person's step; an assistant's try never lifts a guard. A guard
/// this puts on for the case is the person's for a step, the try's call's
/// for a try (`settle_guard_owner`).
pub(crate) async fn guard_supervised(
    session: &mut Session,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    case_id: i32,
    may_lift: bool,
) -> Result<(), String> {
    // A discovery that must not save keeps its guard: no step run in its
    // browser lifts it.
    let who = if may_lift { GuardOwner::Person } else { GuardOwner::Call { case_id: Some(case_id) } };
    let may_lift = may_lift && !session.discovery.as_ref().is_some_and(|s| s.no_save);
    let out =
        guard_for_case(&mut session.cdp, &mut session.guarded_case, root, organization, project, case_id, may_lift).await;
    settle_guard_owner(&session.cdp, session.guarded_case, &mut session.guard_owner, case_id, who);
    if session.cdp.is_guarding_saves() {
        answer_between_commands_in(session.slot);
    }
    out
}

/// Before a replay of `by`'s runs in the browser `d`: the guard as its
/// owner needs it. `ended_no_save` is a no_save discovery the replay ended
/// to take the browser: the replay is the call that ended it, so it is
/// guarded as the discovery's last call (`GuardOwner::Discovery`) - put
/// back on here when the browser does not have it - and the guard is that
/// replay's alone once it settles (`settle_replay_guard`). Answers the
/// `may_lift` the replay runs with (`ReplayBy::may_lift`). `Err` is a guard
/// that would not go on: the replay is refused.
pub async fn guard_before_replay<D: Driver>(
    d: &mut D,
    owner: &mut Option<GuardOwner>,
    ended_no_save: bool,
    by: ReplayBy,
    root: &std::path::Path,
    organization: &str,
    project: &str,
) -> Result<bool, String> {
    if ended_no_save {
        claim_guard(owner, GuardOwner::Discovery);
    }
    if *owner == Some(GuardOwner::Discovery) && !d.is_guarding_saves() {
        let nav = crate::autorun::nav::load_nav(root, organization, project)
            .map_err(|why| crate::browser::save_guard::setup_failed(&why))?;
        d.guard_saves(&nav.save_words).await.map_err(|e| crate::browser::save_guard::setup_failed(&e.to_string()))?;
    }
    Ok(by.may_lift(owner.as_ref()))
}

/// After a replay of `by`'s of case `case_id`: the guard's owner settled as
/// after any guard (`settle_guard_owner`). A no_save discovery's guard the
/// replay kept, with no discovery running any more (`discovering` false),
/// lasted for that replay only: it becomes that call's
/// (`GuardOwner::Call`), so the next call that does not need it lifts it -
/// otherwise it would outlive its discovery with nothing left to end it. A
/// replay while such a discovery still runs stays under its guard.
pub fn settle_replay_guard<D: Driver>(
    d: &D,
    guarded_case: Option<i32>,
    owner: &mut Option<GuardOwner>,
    case_id: i32,
    by: ReplayBy,
    discovering: bool,
) {
    settle_guard_owner(d, guarded_case, owner, case_id, by.owns_as(case_id));
    if !discovering && *owner == Some(GuardOwner::Discovery) {
        *owner = Some(GuardOwner::Call { case_id: guarded_case });
    }
}

/// Put a browser's no-save guard where this case needs it. A case whose
/// script on this machine is marked `no_save` is guarded, with the
/// project's save words read afresh, so an edit on the Setup tab counts
/// from the next step. Any other case has an earlier case's guard lifted -
/// but only when `may_lift`: a person's own step. An assistant's try may add
/// a guard and never take one away, so naming the wrong case (or one with
/// no script) can never let a supervised no-save case's draft be saved.
/// A save stopped for the case that held the guard before, and not yet
/// reported, is that case's: it is written to the application log under
/// that case and never carried into this one. `Err` is the step or try
/// refused: a no-save case never runs unguarded.
pub async fn guard_for_case<D: Driver>(
    d: &mut D,
    guarded_case: &mut Option<i32>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    case_id: i32,
    may_lift: bool,
) -> Result<(), String> {
    let no_save = store::load_script(root, case_id)?.is_some_and(|s| s.no_save);
    if !no_save && !may_lift {
        return Ok(());
    }
    if let Some(before) = guarded_case.filter(|c| *c != case_id) {
        if let Some(sentence) = d.take_save_blocked() {
            crate::applog::warn(format!("Auto Run, case {before}: {sentence}"));
        }
    }
    if no_save {
        let guarded = match crate::autorun::nav::load_nav(root, organization, project) {
            Err(why) => Err(why),
            Ok(nav) => d.guard_saves(&nav.save_words).await.map_err(|e| e.to_string()),
        };
        guarded.map_err(|why| crate::browser::save_guard::setup_failed(&why))?;
        *guarded_case = Some(case_id);
    } else if d.is_guarding_saves() {
        // Lifted only once the browser says so; until then it keeps
        // answering every paused request as a guarded browser does.
        match d.stop_guarding_saves().await {
            Ok(()) => *guarded_case = None,
            Err(e) => crate::applog::warn(format!("Auto Run: the no-save guard could not be lifted yet: {e}")),
        }
    } else {
        *guarded_case = None;
    }
    Ok(())
}

/// How often the supervised browser is read between commands while
/// something waits on it (`Cdp::wants_reading`), and for how long each
/// time.
const ANSWER_EVERY: std::time::Duration = std::time::Duration::from_millis(50);
const ANSWER_FOR: std::time::Duration = std::time::Duration::from_millis(10);

/// Whether the task that answers each slot's supervised browser between
/// commands is running. A slot's is only ever changed with that slot's
/// session lock held, so starting one and the last one ending can never miss
/// each other.
static ANSWERING: [std::sync::atomic::AtomicBool; SLOT_COUNT] =
    [const { std::sync::atomic::AtomicBool::new(false) }; SLOT_COUNT];

/// A guarded browser pauses every request until it is answered, and the
/// client only reads the socket while something calls it. Between two
/// commands - a person reading the page before pressing the next step -
/// a task reads it (`keep_answering`), so the page is never held up
/// waiting. This does it for slot `n`'s browser, called with that slot's
/// session lock held.
pub(crate) fn answer_between_commands_in(n: SlotNo) {
    if ANSWERING[n.index()].swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(keep_answering(
        session_slot(n),
        &ANSWERING[n.index()],
        |s: &mut Session| &mut s.cdp,
        ANSWER_EVERY,
        ANSWER_FOR,
    ));
}

/// Every `every`, read the browser in `slot` for `read_for` while it wants
/// reading (`Cdp::wants_reading`: it is guarded, or it drives the whole
/// browser, where a tab the page opens waits to be set up); a command
/// holding the slot reads it itself and is never waited on. Ends, clearing
/// `running` with the slot still locked, once the slot is empty or its
/// browser no longer wants reading.
pub async fn keep_answering<S, T: crate::browser::cdp::Transport>(
    slot: &tokio::sync::Mutex<Option<S>>,
    running: &std::sync::atomic::AtomicBool,
    cdp_of: fn(&mut S) -> &mut Cdp<T>,
    every: std::time::Duration,
    read_for: std::time::Duration,
) {
    loop {
        tokio::time::sleep(every).await;
        let Ok(mut held) = slot.try_lock() else { continue };
        match held.as_mut().map(cdp_of) {
            Some(cdp) if cdp.wants_reading() => cdp.pump(read_for).await,
            _ => {
                running.store(false, std::sync::atomic::Ordering::SeqCst);
                return;
            }
        }
    }
}

/// One failure screenshot as a data URL the webview can show. The name is
/// checked in `store::load_shot`; nothing outside the shots folder can be
/// read through here.
#[tauri::command(async)]
#[specta::specta]
pub fn auto_run_shot(app: tauri::AppHandle, name: String) -> Result<String, String> {
    let bytes = store::load_shot(&root(&app)?, &name)?;
    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[tauri::command(async)]
#[specta::specta]
pub fn auto_run_load_script(
    app: tauri::AppHandle,
    case_id: i32,
) -> Result<Option<CaseScript>, String> {
    store::load_script(&root(&app)?, case_id)
}

/// Every listed case's script in one call - what the Auto Run screen asks
/// for when it opens. Off the main thread, as every read here is: the
/// window stays responsive while hundreds of files are read.
#[tauri::command(async)]
#[specta::specta]
pub fn auto_run_load_scripts(app: tauri::AppHandle, case_ids: Vec<i32>) -> Result<Vec<store::ScriptLookup>, String> {
    Ok(store::load_scripts(&root(&app)?, &case_ids))
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_save_script(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    script: CaseScript,
) -> Result<(), String> {
    save_script_from_editor(&root(&app)?, &organization, &project, script)
}

/// Said by a save or an import that names no organization or project.
pub const NO_PROJECT: &str = "choose an organization and a project first - scripts follow that project's rules";

/// Whether a script may open pages by address is the project's own rule,
/// read from a file found by organization and project. With either blank
/// that file is one no project has, which reads as "allowed" - so a save
/// with no project would skip the rule instead of applying it.
fn require_project(organization: &str, project: &str) -> Result<(), String> {
    if organization.trim().is_empty() || project.trim().is_empty() {
        return Err(NO_PROJECT.to_string());
    }
    Ok(())
}

/// The pure half of [`auto_run_save_script`], so a test can reach it
/// without an `AppHandle`.
pub fn save_script_from_editor(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    mut script: CaseScript,
) -> Result<(), String> {
    require_project(organization, project)?;
    // A person saving from the editor is a fresh start for the assistant's
    // repair count, whatever the editor happened to send - and the reason
    // for the last one is no longer relevant once a person has looked.
    script.repairs = 0;
    script.last_repair = None;
    script.precondition_repairs = Vec::new();
    // The editor owns what it shows: the steps, title, account, area, Must
    // not save, preconditions and the shared-state marks. Everything else
    // is the stored script's, whatever the editor sent - so a setting the
    // editor has no control for survives a save from it, and a stale
    // editor can never write an old value back. Spelt out field by field,
    // so a new field cannot be added without saying whose it is.
    let stored = store::load_script(root, script.case_id)?.unwrap_or_else(|| CaseScript {
        case_id: script.case_id,
        title: String::new(),
        account: None,
        area: None,
        steps: Vec::new(),
        repairs: 0,
        last_repair: None,
        suspected_defect: None,
        no_save: false,
        preconditions: Vec::new(),
        precondition_repairs: Vec::new(),
        setup: None,
        changes: Vec::new(),
        needs_unchanged: Vec::new(),
        needs_state: Default::default(),
        saved_at: None,
        fail_on_unexpected_dialog: false,
        page_errors: None,
        ignore_page_errors: Vec::new(),
        organization: None,
        project: None,
        checked: false,
        checks: 0,
        given_up: Vec::new(),
        cleared_defect: None,
    });
    let CaseScript {
        // The editor's own.
        case_id: _,
        title: _,
        account: _,
        area: _,
        steps: _,
        no_save: _,
        preconditions: _,
        changes: _,
        needs_unchanged: _,
        needs_state: _,
        // Reset above: a person's save starts the repair count afresh.
        repairs: _,
        last_repair: _,
        precondition_repairs: _,
        // The store keeps the mark on disk whatever a save sends
        // (`store::save_scripts_atomically`).
        suspected_defect: _,
        // Every save stamps its own time, and its project and whether it
        // was checked, below.
        saved_at: _,
        organization: _,
        project: _,
        checked: _,
        // Stamped below: the person's save is the acceptance of what was given up.
        checks: _,
        given_up: _,
        // Kept by the store like the mark.
        cleared_defect: _,
        // The stored script's. Only the assistant writes a setup, so the
        // webview can never change a fixture or write an old one back.
        setup,
        fail_on_unexpected_dialog,
        page_errors,
        ignore_page_errors,
    } = stored;
    script.setup = setup;
    script.fail_on_unexpected_dialog = fail_on_unexpected_dialog;
    script.page_errors = page_errors;
    script.ignore_page_errors = ignore_page_errors;
    // The editor's save runs no seen check, so its locators vouch for
    // nothing (`seen_check::vouches`), whatever an earlier save did.
    script.organization = Some(organization.trim().to_string());
    script.project = Some(project.trim().to_string());
    script.checked = false;
    // The person's save is the accepted path: it stamps the checks it leaves
    // and accepts whatever an assistant gave up.
    script.checks = crate::autorun::case_values::check_count(&script) as u32;
    script.given_up = Vec::new();
    // The project's rules - no address while that is switched off, only
    // recorded areas - the same ones the import and the assistant's save
    // apply.
    crate::autorun::nav::check_project_rules(root, organization, project, std::slice::from_ref(&script))?;
    // Through the same helper the bundle paths use, as a bundle of one:
    // the script editor is a THIRD way in, and a case id of 0 or an empty
    // step list refused from a file but accepted from the editor would be
    // a rule that depends on which door you came through.
    let case_id = script.case_id;
    store::save_scripts_atomically(root, std::slice::from_ref(&script)).map_err(|e| e.to_string())?;
    // The person's save is the acceptance of a script that waited for a
    // rewritten case: the case runs again. Said in the log when the file
    // cannot be written; the save itself is down.
    let waiting = crate::autorun::divergences::load(root, organization, project)
        .map(|l| l.iter().any(|d| d.case_id == case_id && d.status == crate::autorun::divergences::DivergenceStatus::AwaitingRepair))
        .unwrap_or(false);
    if waiting {
        let now = crate::autorun::sessions::now_ms();
        match crate::autorun::divergences::update(root, organization, project, |list| {
            Ok(crate::autorun::divergences::close_for_editor_save(list, case_id, now))
        }) {
            Ok(steps) => {
                for step in steps {
                    crate::applog::info(format!(
                        "Auto Run: case {case_id} saved in the script editor; its divergence at step {step} is closed"
                    ));
                }
            }
            Err(e) => crate::applog::warn(format!(
                "Auto Run: case {case_id} saved in the script editor, but its divergence could not be closed: {e}"
            )),
        }
    }
    Ok(())
}

/// A person's Clear on a case's suspected-defect mark: they have looked and
/// decided it is not, or no longer, the application's fault. Removes the
/// mark and nothing else - the script's actions and repair count stay.
#[tauri::command]
#[specta::specta]
pub fn auto_run_clear_suspected_defect(app: tauri::AppHandle, case_id: i32) -> Result<(), String> {
    clear_suspected_defect(&root(&app)?, case_id)
}

/// The pure half of [`auto_run_clear_suspected_defect`], so a test can
/// reach it without an `AppHandle`. A case with no mark is left as it is.
pub fn clear_suspected_defect(root: &std::path::Path, case_id: i32) -> Result<(), String> {
    if store::clear_suspected_defect_any(root, case_id)? {
        crate::applog::info(format!("Auto Run: the suspected defect on case {case_id} was cleared from the app"));
    }
    Ok(())
}

/// The plan for a selection, as the screen shows it: the order, the order
/// split into batches (`phases`) at each pause, and the pauses: one before
/// each batch after the first (settings to set, state to put back), and one
/// before Batch 1 when it needs settings. `counts` is `(this order's
/// pauses, the suggested order's pauses)`, only when Auto Run's own order
/// for the PBI needs more pauses than the suggestion. `saved` says whether
/// the PBI has an order of its own on this machine. Titles are the
/// screen's: it has the case list.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct PlanView {
    pub order: Vec<i32>,
    pub phases: Vec<Vec<i32>>,
    pub pauses: Vec<crate::autorun::plan::Pause>,
    pub counts: Option<(u32, u32)>,
    pub saved: bool,
}

/// The plan for the cases selected on the Auto Run tab, in list order.
/// Each case's marks come from its saved script; a case with no script
/// has none. Scripts and orders are kept by work item id, so the
/// organization and project name the selection's source and nothing more.
/// `preview_order` stands in for the saved order for this one call (the
/// Execution order dialog asks about a list the person has moved but not
/// saved); nothing is written.
#[tauri::command(async)]
#[specta::specta]
pub fn auto_run_plan(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    pbi_id: i32,
    case_ids: Vec<i32>,
    preview_order: Option<Vec<i32>>,
) -> Result<PlanView, String> {
    let _ = (organization, project);
    Ok(plan_at(&root(&app)?, pbi_id, &case_ids, preview_order.as_deref()))
}

/// The pure half of [`auto_run_plan`], so a test can reach it without an
/// `AppHandle`. A script that cannot be read counts as no marks (and is
/// logged), so one bad file never stops the plan.
pub fn plan_at(root: &std::path::Path, pbi_id: i32, case_ids: &[i32], preview_order: Option<&[i32]>) -> PlanView {
    use crate::autorun::plan::{batches_for, needs_of, Marks, Needs};
    let selected: Vec<(i32, Marks, Needs)> = case_ids
        .iter()
        .map(|&id| match store::load_script(root, id) {
            Ok(Some(script)) => (id, Marks::of(&script), needs_of(&script)),
            Ok(None) => (id, Marks::default(), Needs::new()),
            Err(e) => {
                crate::applog::warn(format!("auto-run: case {id}'s script could not be read for the plan: {e}"));
                (id, Marks::default(), Needs::new())
            }
        })
        .collect();
    let saved = match preview_order {
        Some(p) => Some(p.to_vec()),
        None => store::load_order(root, pbi_id),
    };
    let (plan, counts) = batches_for(&selected, saved.as_deref());
    PlanView {
        order: plan.order,
        phases: plan.batches,
        pauses: plan.pauses,
        counts: counts.map(|(a, b)| (a as u32, b as u32)),
        saved: preview_order.is_none() && saved.is_some(),
    }
}

/// Save Auto Run's own execution order for a PBI on this machine. Only
/// the cases given move: the rest of an order already saved keeps its
/// places (`store::merge_order`). Run Tests' order is separate and is not
/// changed.
#[tauri::command]
#[specta::specta]
pub fn auto_run_save_order(app: tauri::AppHandle, pbi_id: i32, case_ids: Vec<i32>) -> Result<(), String> {
    // The dialog may order only the cases ticked now: the rest of the saved
    // order is kept.
    store::save_order_merged(&root(&app)?, pbi_id, &case_ids)
}

/// "Use suggested order": forget the PBI's own order, so the suggested
/// order is used again.
#[tauri::command]
#[specta::specta]
pub fn auto_run_clear_order(app: tauri::AppHandle, pbi_id: i32) -> Result<(), String> {
    store::clear_order(&root(&app)?, pbi_id)
}

/// Import a BUNDLE of scripts from one file - the shape an assistant
/// writes for a whole PBI, and the shape the Auto Run screen's Import
/// button reads back.
///
/// Takes a PATH, not the file's contents: the frontend used to read the
/// file itself and hand over base64 text, but `atob` decodes base64 to a
/// latin-1 binary string, so any non-ASCII byte (an accent, a curly
/// quote, an em dash) came out mojibake, and a UTF-8 BOM made the JSON
/// look corrupt before it ever reached the parser. Reading here, in Rust,
/// with the same BOM handling `import_parser` already uses, sidesteps
/// both.
///
/// All or nothing, via `store::save_scripts_atomically`: every entry is
/// validated and serialised before a single file is written, so a bad
/// entry - or a filesystem error partway through a big bundle - never
/// leaves the tester unable to tell which cases are current. Returns the
/// case ids that landed, so the screen can say what changed rather than
/// just "done".
///
/// Every script is checked against the live app as an assistant's save is
/// (`seen_check`), in full, before anything is written, so the test cases
/// are read from Azure DevOps first: a case's own words may name what a
/// check looks for. With no way to read them the import is refused, never
/// let through unchecked.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_import_scripts(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    path: String,
) -> Result<Vec<i32>, String> {
    let root = root(&app)?;
    require_project(&organization, &project)?;
    let scripts = read_import_file(&path)?;
    let cases = import_case_texts(&app, &organization, &scripts).await?;
    import_scripts(&root, &organization, &project, scripts, cases.as_ref())
}

/// The test cases' own text by case id: each step's action and expected
/// result, which the seen check reads.
pub type CaseTexts = std::collections::HashMap<i32, Vec<String>>;

/// Said when an import cannot read its scripts' test cases.
pub const IMPORT_NEEDS_CASES: &str =
    "Imported scripts are checked against the live app and their test cases; sign in and try again.";

/// Closes the list of what an import named that was never seen.
pub const IMPORT_UNSEEN_THEN: &str =
    "Explore the area with discovery so its map holds what these scripts use, then import again.";

/// The text of every case `scripts` are for, read from Azure DevOps (read
/// only). `None` when they cannot be read: signed out or offline.
async fn import_case_texts(
    app: &tauri::AppHandle,
    organization: &str,
    scripts: &[CaseScript],
) -> Result<Option<CaseTexts>, String> {
    let Ok(token) = crate::state::get_fresh_token(app).await else { return Ok(None) };
    let mut ids: Vec<i32> = scripts.iter().map(|s| s.case_id).collect();
    ids.sort_unstable();
    ids.dedup();
    match crate::ado::AdoClient::new(token).get_test_cases_by_ids(organization, &ids, None, None).await {
        Ok(cases) => Ok(Some(
            cases
                .into_iter()
                .map(|c| (c.id, crate::steps_xml::plain_texts(&c.steps)))
                .collect(),
        )),
        // Azure DevOps refuses the whole batch when any id is not a work
        // item, so it cannot say which: the ids are named back instead.
        Err(crate::ado::AdoError::NotFound) => Err(format!(
            "Azure DevOps has no work item for at least one of {}. An imported script is checked against its own test case, so nothing was imported.",
            ids.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(", ")
        )),
        Err(e) => {
            crate::applog::warn(format!("Import scripts: the test cases could not be read: {e:?}"));
            Ok(None)
        }
    }
}

/// The pure half of [`auto_run_import_scripts`]: everything that does not
/// need an `AppHandle`, so it can be exercised directly in tests the same
/// way `autorun::store`'s functions are. `cases` is the scripts' test
/// cases' text, `None` when it could not be read.
pub fn import_scripts_from_path(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    path: &str,
    cases: Option<&CaseTexts>,
) -> Result<Vec<i32>, String> {
    require_project(organization, project)?;
    let scripts = read_import_file(path)?;
    import_scripts(root, organization, project, scripts, cases)
}

/// The scripts an import file holds; refused when it holds none.
fn read_import_file(path: &str) -> Result<Vec<CaseScript>, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Could not read {path}: {e}"))?;
    let content = content.strip_prefix('\u{feff}').unwrap_or(&content);
    let scripts: Vec<CaseScript> = serde_json::from_str(content).map_err(|e| {
        format!(
            "that file is not a list of action scripts: {e}. Expected an array of {{ case_id, title, steps }}."
        )
    })?;
    if scripts.is_empty() {
        return Err("that file has no scripts in it".to_string());
    }
    Ok(scripts)
}

/// Checks and writes the scripts an import file held: all or nothing.
fn import_scripts(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    mut scripts: Vec<CaseScript>,
    cases: Option<&CaseTexts>,
) -> Result<Vec<i32>, String> {
    crate::autorun::nav::check_project_rules(root, organization, project, &scripts)?;
    // An import can mark a script Must not save, never unmark one: only a
    // person saving from the editor turns the flag off.
    for sc in scripts.iter_mut() {
        if !sc.no_save && store::load_script(root, sc.case_id)?.is_some_and(|old| old.no_save) {
            sc.no_save = true;
        }
        // Every step of every imported script is checked below, so each
        // vouches for its locators once it lands.
        sc.organization = Some(organization.trim().to_string());
        sc.project = Some(project.trim().to_string());
        sc.checked = crate::autorun::proven::may_be_checked(root, organization, project, sc.case_id);
        sc.checks = crate::autorun::case_values::check_count(sc) as u32;
    }
    // With a component in use, the check and the write hold the components
    // lock, so a component save cannot slip between them.
    let check_and_save = || -> Result<(), String> {
        check_imported_seen(root, organization, project, &scripts, cases)?;
        store::save_scripts_atomically(root, &scripts).map_err(|e| e.to_string())
    };
    if scripts.iter().any(crate::autorun::seen_check::uses_components) {
        crate::autorun::components::with_components_locked(check_and_save)?;
    } else {
        check_and_save()?;
    }
    let ids: Vec<i32> = scripts.iter().map(|sc| sc.case_id).collect();
    crate::applog::info(format!("Imported {} auto-run script(s)", ids.len()));
    Ok(ids)
}

/// The save route's seen check, over every step of every imported script.
/// Every failure is listed, script by script, then what to do.
fn check_imported_seen(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    scripts: &[CaseScript],
    cases: Option<&CaseTexts>,
) -> Result<(), String> {
    let cases = cases.ok_or_else(|| IMPORT_NEEDS_CASES.to_string())?;
    let map = crate::autorun::seen_check::load_checked_map(root, organization, project)?;
    // Read only when a script uses a component.
    let components = if scripts.iter().any(crate::autorun::seen_check::uses_components) {
        crate::autorun::components::load_components(root, organization, project)?
    } else {
        crate::autorun::components::ComponentFile::default()
    };
    // The Test files, so the size the app shows for one a script uploads
    // passes as the script's own data.
    let files = crate::test_files::list(&crate::test_files::folder(root, organization, project)).unwrap_or_else(|e| {
        crate::applog::warn(format!("Import scripts: the Test files could not be listed: {e}"));
        Vec::new()
    });
    let mut lines: Vec<String> = Vec::new();
    for sc in scripts {
        let Some(text) = cases.get(&sc.case_id) else {
            lines.push(format!("Case {}: Azure DevOps has no test case with this id.", sc.case_id));
            continue;
        };
        for (u, hint) in crate::autorun::seen_check::check_seen_all_hinted(&map, &components, sc, text, None, &files) {
            lines.push(match (&u.refused, hint) {
                (Some(why), _) => format!("Case {}, step {}: {why}.", sc.case_id, u.step),
                (None, hint) => format!(
                    "Case {}, step {}: {}",
                    sc.case_id,
                    u.step,
                    crate::autorun::seen_check::never_seen(&u.locator, hint.as_deref())
                ),
            });
        }
    }
    if lines.is_empty() {
        return Ok(());
    }
    crate::applog::info(format!("Import scripts refused: {} unseen locator(s) or missing case(s)", lines.len()));
    lines.push(IMPORT_UNSEEN_THEN.to_string());
    Err(lines.join("\n"))
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_save_run(app: tauri::AppHandle, run: LocalRun) -> Result<(), String> {
    let root = root(&app)?;
    let ids: Vec<i32> = run.cases.iter().map(|c| c.case_id).collect();
    save_run_at(&root, run)?;
    release_stepped_run(&ids);
    Ok(())
}

/// A watched run's result was saved: the run has ended, and the case it
/// was stepping (one of `case_ids`) may have its script deleted again. A
/// browser busy with something else right now keeps its note until it
/// closes.
pub fn release_stepped_run(case_ids: &[i32]) {
    if let Some(mut slot) = try_lock_briefly(&SESSION) {
        if let Some(s) = slot.as_mut() {
            release_saved(&mut s.stepping, case_ids);
        }
    }
}

/// A step of `guard`'s case runs: the browser keeps the note for that
/// case, and one for another case it held is let go.
pub fn hold_stepped_run(held: &mut Option<store::Replaying>, guard: store::Replaying) {
    if held.as_ref().map(store::Replaying::case_id) != Some(guard.case_id()) {
        *held = Some(guard);
    }
}

/// A run naming `case_ids` was saved: the note is let go when its case is
/// one of them.
pub fn release_saved(held: &mut Option<store::Replaying>, case_ids: &[i32]) {
    if held.as_ref().is_some_and(|g| case_ids.contains(&g.case_id())) {
        *held = None;
    }
}

/// `auto_run_save_run` for a given data root. The first save of a run - a
/// supervised one, which the screen builds - records the active
/// environment's name; a run already on disk keeps whatever it has, so an
/// old run reviewed later is never stamped with today's environment.
pub fn save_run_at(root: &std::path::Path, mut run: LocalRun) -> Result<(), String> {
    if !safe_run_id(&run.id) {
        return Err(format!("run id {:?} is not a safe filename", run.id));
    }
    let first = matches!(store::load_run(root, &run.id), Ok(None));
    if run.environment.is_none() && first {
        run.environment = crate::environments::active(root).ok().map(|e| e.name);
    }
    store::save_run_guarded(root, &run)?;
    // A supervised run's first save comes once, after its pictures are
    // taken: the shots folder is pruned then. A later save (the Review
    // screen's) adds no pictures, so it does not read every run to prune.
    if first {
        for record in &run.cases {
            if let Some(o) = crate::autorun::proven::outcome_of_record(record) {
                crate::autorun::proven::note_outcome(root, record.case_id, o);
            }
        }
        store::prune_old_shots(root);
    }
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn auto_run_list_runs(app: tauri::AppHandle) -> Vec<LocalRun> {
    match root(&app) {
        Ok(r) => store::list_runs(&r),
        Err(_) => vec![],
    }
}

/// `Ok(None)` for a run id nobody has saved - the review screen uses this
/// to load a run for the Send-to-Azure-DevOps screen without pulling every
/// run in the list.
#[tauri::command(async)]
#[specta::specta]
pub fn auto_run_load_run(app: tauri::AppHandle, run_id: String) -> Result<Option<LocalRun>, String> {
    store::load_run(&root(&app)?, &run_id)
}

/// Said when a report is asked for a run this machine no longer has.
pub const REPORT_RUN_GONE: &str = "this run is no longer on this machine";

/// The pure half of [`auto_run_open_report`]: one run as a single HTML page
/// (`autorun::report`), written atomically to `<root>/reports/<run id>.html`
/// - the same file, overwritten, on every open. The reports folder sits
/// beside the shots folder, which is how the page links its pictures
/// (`../shots/<name>`). "Is Auto Run on" is passed in, the way the
/// Test files commands take it, so a locked build's refusal is testable.
/// The run id is checked with `store::safe_run_id` before anything touches
/// the disk, since it becomes a file name. The scripts on this machine
/// supply a failed action's words and a case's area. Returns the file's
/// path, for the opener.
///
/// `ran_at` is the run's start as the webview shows it (the person's own
/// locale); the page escapes it and prints it as given, and blank falls
/// back to the run's start in UTC.
pub fn write_report_at(
    offered: bool,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    run_id: &str,
    ran_at: &str,
) -> Result<std::path::PathBuf, String> {
    crate::modules::require(crate::modules::Module::AutoRun, offered)?;
    if !safe_run_id(run_id) {
        return Err(format!("run id {run_id:?} is not a safe filename"));
    }
    let run = store::load_run(root, run_id)?.ok_or_else(|| REPORT_RUN_GONE.to_string())?;
    let scripts: Vec<CaseScript> = run
        .cases
        .iter()
        .filter_map(|c| store::load_script(root, c.case_id).ok().flatten())
        .collect();
    // The project's components, so an action one ran is named as it
    // expands. A file that does not read leaves those actions named by
    // their component only.
    let components = crate::autorun::components::load_components(root, organization, project).unwrap_or_default();
    let divergences = crate::autorun::divergences::load(root, organization, project).unwrap_or_default();
    let html = crate::autorun::report::build_with_extras(
        &run,
        &scripts,
        &components,
        ran_at,
        &|name| store::shot_exists(root, name),
        &|name| store::download_size(root, run_id, name),
        &crate::autorun::report::ReportExtras { divergences: &divergences },
    );
    let dir = store::reports_dir(root);
    let path = dir.join(format!("{run_id}.html"));
    std::fs::create_dir_all(&dir).map_err(|e| {
        crate::applog::warn(format!("auto run report: the reports folder could not be made: {e}"));
        REPORT_NOT_WRITTEN.to_string()
    })?;
    crate::ai_tools::atomic_write(&path, &html).map_err(|e| {
        crate::applog::warn(format!("auto run report for {run_id} could not be written: {e}"));
        REPORT_NOT_WRITTEN.to_string()
    })?;
    crate::applog::info(format!("auto run report for {run_id} written"));
    Ok(path)
}

/// Said when the report page could not be written to disk.
pub const REPORT_NOT_WRITTEN: &str = "the report could not be written - see Settings, Logs";

/// Said when the browser could not be asked to open the report.
pub const REPORT_NOT_OPENED: &str = "the report could not be opened in your browser - see Settings, Logs";

/// Writes one run's report and opens it in the default browser. `ran_at` is
/// the run's start time as the webview shows it. Async, with
/// the write on a blocking thread: reading a run's scripts and writing the
/// page must not hold the main thread, which would freeze the window.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_open_report(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    run_id: String,
    ran_at: String,
) -> Result<(), String> {
    let offered = crate::modules::on(crate::modules::Module::AutoRun);
    let root = root(&app)?;
    let path = tauri::async_runtime::spawn_blocking(move || write_report_at(offered, &root, &organization, &project, &run_id, &ran_at))
        .await
        .map_err(|e| {
            crate::applog::warn(format!("auto run report: the writer stopped: {e}"));
            REPORT_NOT_WRITTEN.to_string()
        })??;
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| {
        crate::applog::warn(format!("the auto run report could not be opened: {e}"));
        REPORT_NOT_OPENED.to_string()
    })
}

/// The pure half of [`auto_run_open_download`]: the one path Open may hand
/// to the default app, a plain name in the run's own download folder
/// (`store::download_file`). "Is Auto Run on" is passed in, as the
/// report takes it.
pub fn download_path_at(
    offered: bool,
    root: &std::path::Path,
    run_id: &str,
    name: &str,
) -> Result<std::path::PathBuf, String> {
    crate::modules::require(crate::modules::Module::AutoRun, offered)?;
    store::download_file(root, run_id, name)
}

/// The pure half of [`auto_run_download_sizes`].
pub fn download_sizes_at(
    offered: bool,
    root: &std::path::Path,
    run_id: &str,
) -> Result<Vec<crate::autorun::DownloadFile>, String> {
    crate::modules::require(crate::modules::Module::AutoRun, offered)?;
    Ok(store::download_files(root, run_id))
}

/// Said when the default app could not be asked to open a download.
pub const DOWNLOAD_NOT_OPENED: &str = "the file could not be opened - see Settings, Logs";

/// Opens one of a run's downloads with the system's default app. Only a
/// plain name in that run's own download folder is opened; anything else
/// is refused with `that file is not one of this run's downloads`.
#[tauri::command]
#[specta::specta]
pub fn auto_run_open_download(app: tauri::AppHandle, run_id: String, name: String) -> Result<(), String> {
    let path = download_path_at(crate::modules::on(crate::modules::Module::AutoRun), &root(&app)?, &run_id, &name)?;
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| {
        crate::applog::warn(format!("a run's download could not be opened: {e}"));
        DOWNLOAD_NOT_OPENED.to_string()
    })
}

/// The files in a run's download folder with their sizes, read from disk
/// now: a name the run recorded that is missing here is a file that is no
/// longer on this machine.
#[tauri::command(async)]
#[specta::specta]
pub fn auto_run_download_sizes(
    app: tauri::AppHandle,
    run_id: String,
) -> Result<Vec<crate::autorun::DownloadFile>, String> {
    download_sizes_at(crate::modules::on(crate::modules::Module::AutoRun), &root(&app)?, &run_id)
}

/// A run id the frontend can stamp on a new session.
#[tauri::command]
#[specta::specta]
pub fn auto_run_new_id() -> String {
    store::new_run_id()
}

/// The tester's accounts, passwords included: the Accounts dialog edits
/// them in place. This is the one IPC call that carries a password, by the
/// owner's decision (see `autorun::accounts`). It is never logged.
#[tauri::command(async)]
#[specta::specta]
pub fn auto_run_list_accounts(
    app: tauri::AppHandle,
) -> Result<Vec<crate::autorun::accounts::Account>, String> {
    crate::autorun::accounts::load_accounts(&root(&app)?)
}

/// Replace the accounts list. Returns the keys whose saved session was
/// dropped, so the screen can say so.
#[tauri::command]
#[specta::specta]
pub fn auto_run_save_accounts(
    app: tauri::AppHandle,
    accounts: Vec<crate::autorun::accounts::Account>,
) -> Result<Vec<String>, String> {
    let dropped = crate::autorun::accounts::save_accounts(&root(&app)?, &accounts)?;
    crate::applog::info(format!("Auto-run accounts saved ({} account(s))", accounts.len()));
    Ok(dropped)
}

#[tauri::command(async)]
#[specta::specta]
pub fn auto_run_load_recipe(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Option<crate::autorun::recipe::SignInRecipe>, String> {
    crate::autorun::recipe::load_recipe(&root(&app)?, &organization, &project)
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_save_recipe(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    recipe: crate::autorun::recipe::SignInRecipe,
) -> Result<(), String> {
    crate::autorun::recipe::save_recipe(&root(&app)?, &organization, &project, &recipe)?;
    crate::applog::info("Auto-run sign-in recipe saved");
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn auto_run_load_quirks(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<crate::autorun::quirks::Quirk>, String> {
    crate::autorun::quirks::load_quirks(&root(&app)?, &organization, &project)
}

/// What a supervised run just saved says about the project's quirks: the
/// run pane calls this once, after its save; the review screen never does.
#[tauri::command(async)]
#[specta::specta]
pub fn auto_run_count_evidence(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    run_id: String,
) -> Result<bool, String> {
    crate::autorun::quirks::count_saved_run(
        &root(&app)?,
        &organization,
        &project,
        &run_id,
        crate::autorun::sessions::now_ms(),
    )
}

// The Known quirks list's own changes, one per button. Each is saved the
// moment it is made and answers with the whole list as saved, so the
// dialog never shows a list the file does not hold.

/// A note the person types in, added as theirs. Past the active cap it is
/// refused with the notes worth retiring.
#[tauri::command]
#[specta::specta]
pub fn auto_run_add_quirk(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    text: String,
) -> Result<Vec<crate::autorun::quirks::Quirk>, String> {
    use crate::autorun::quirks::{record_in, update_quirks, FROM_AUTORUN};
    let now = crate::autorun::sessions::now_ms();
    let (_, list) = update_quirks(&root(&app)?, &organization, &project, |l| {
        record_in(l, &text, "person", FROM_AUTORUN, Vec::new(), now)
    })?;
    crate::applog::info("Auto Run: a project quirk was added");
    Ok(list)
}

/// A note's text, changed - whoever wrote it.
#[tauri::command]
#[specta::specta]
pub fn auto_run_edit_quirk(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: String,
    text: String,
) -> Result<Vec<crate::autorun::quirks::Quirk>, String> {
    let (_, list) = crate::autorun::quirks::update_quirks(&root(&app)?, &organization, &project, |l| {
        crate::autorun::quirks::edit_in(l, &id, &text)
    })?;
    crate::applog::info("Auto Run: a project quirk was edited");
    Ok(list)
}

/// Retired by the person - any note, theirs or an assistant's, with an
/// optional reason. It stays in the file and can be restored.
#[tauri::command]
#[specta::specta]
pub fn auto_run_retire_quirk(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: String,
    reason: Option<String>,
) -> Result<Vec<crate::autorun::quirks::Quirk>, String> {
    let now = crate::autorun::sessions::now_ms();
    let (_, list) = crate::autorun::quirks::update_quirks(&root(&app)?, &organization, &project, |l| {
        crate::autorun::quirks::retire_in(l, &id, reason.as_deref(), None, false, now)
    })?;
    crate::applog::info("Auto Run: a project quirk was retired");
    Ok(list)
}

/// A retired note, back on the active list - refused while that is full.
#[tauri::command]
#[specta::specta]
pub fn auto_run_restore_quirk(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: String,
) -> Result<Vec<crate::autorun::quirks::Quirk>, String> {
    let (_, list) = crate::autorun::quirks::update_quirks(&root(&app)?, &organization, &project, |l| {
        crate::autorun::quirks::restore_in(l, &id)
    })?;
    crate::applog::info("Auto Run: a project quirk was restored");
    Ok(list)
}

/// A note removed from the file altogether.
#[tauri::command]
#[specta::specta]
pub fn auto_run_delete_quirk(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: String,
) -> Result<Vec<crate::autorun::quirks::Quirk>, String> {
    let (_, list) = crate::autorun::quirks::update_quirks(&root(&app)?, &organization, &project, |l| {
        crate::autorun::quirks::delete_in(l, &id)
    })?;
    crate::applog::info("Auto Run: a project quirk was deleted");
    Ok(list)
}

/// The project's areas and its address switch, as the Areas dialog shows
/// them. A project with no file reads as no areas, switch on.
#[tauri::command(async)]
#[specta::specta]
pub fn auto_run_load_nav(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<crate::autorun::nav::NavView, String> {
    let nav = crate::autorun::nav::load_nav(&root(&app)?, &organization, &project)?;
    Ok(crate::autorun::nav::view(&nav))
}

/// The project's own save words (Setup, Save words), replaced as a whole
/// list. The built-in words are not in it and cannot be removed.
#[tauri::command]
#[specta::specta]
pub fn auto_run_set_save_words(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    words: Vec<String>,
) -> Result<crate::autorun::nav::NavView, String> {
    let nav = crate::autorun::nav::set_save_words(&root(&app)?, &organization, &project, &words)?;
    crate::applog::info(format!("Auto-run: the project has {} save words of its own", nav.save_words.len()));
    Ok(crate::autorun::nav::view(&nav))
}

/// The project's protected baseline (Setup, Protected records), replaced as
/// a whole list: names trimmed, blank and repeated ones dropped.
#[tauri::command]
#[specta::specta]
pub fn auto_run_set_protected(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    records: Vec<crate::autorun::nav::ProtectedRecord>,
) -> Result<crate::autorun::nav::NavView, String> {
    let nav = crate::autorun::nav::set_protected(&root(&app)?, &organization, &project, &records)?;
    crate::applog::info(format!("Auto-run: the project has {} protected records", nav.protected.len()));
    Ok(crate::autorun::nav::view(&nav))
}

/// "Scripts may open pages by address", saved the moment it is flipped.
#[tauri::command]
#[specta::specta]
pub fn auto_run_set_direct_urls(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    allowed: bool,
) -> Result<crate::autorun::nav::NavView, String> {
    let nav = crate::autorun::nav::set_direct_urls(&root(&app)?, &organization, &project, allowed)?;
    crate::applog::info(format!(
        "Auto-run: scripts may open pages by address: {}",
        if allowed { "on" } else { "off" }
    ));
    Ok(crate::autorun::nav::view(&nav))
}

/// Forget one area, by name. The module's other areas stay. The dialog
/// asks first.
#[tauri::command]
#[specta::specta]
pub fn auto_run_remove_module_path(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    area: String,
) -> Result<crate::autorun::nav::NavView, String> {
    let nav = crate::autorun::nav::remove_path(&root(&app)?, &organization, &project, &area)?;
    crate::applog::info("Auto-run area removed");
    Ok(crate::autorun::nav::view(&nav))
}

/// Put back the path an assistant's repair replaced, and clear its note
/// (`area_repair::put_old_path_back`). Nothing is replayed: Try checks it.
#[tauri::command]
#[specta::specta]
pub fn auto_run_put_old_area_path_back(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    area: String,
) -> Result<crate::autorun::nav::NavView, String> {
    let nav = crate::autorun::area_repair::put_old_path_back(&root(&app)?, &organization, &project, &area)?;
    crate::applog::info("Auto-run: an area's path from before an assistant's repair was put back");
    Ok(crate::autorun::nav::view(&nav))
}

/// Sign the named account in, in the open browser. Used before a case's
/// first step, and by the `sign_in` action in the middle of one. An account
/// anything else holds (`autorun::lease`) is refused at once, with the
/// sentence that says who has it, and the browser is left as it was.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_sign_in(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    account_key: String,
) -> Result<crate::autorun::signin::SignInOutcome, String> {
    let root = root(&app)?;
    // Refused before the browser is waited for: no account, no recipe.
    crate::autorun::signin::prepare(&root, &organization, &project, &account_key)?;
    let mut slot = SESSION.lock().await;
    let session = slot.as_mut().ok_or_else(describe_session_error)?;
    // The person signing in here makes the browser theirs.
    session.make_persons().await?;
    session.used_otherwise();
    note_session_use();
    crate::autorun::signin::sign_in_leased(
        &mut session.cdp,
        &root,
        &organization,
        &project,
        &account_key,
        &mut session.lease,
        &mut session.account,
        &crate::browser::timing::Timing::supervised(),
    )
    .await
}

/// Throw a saved session away, so the next sign-in goes through the form.
#[tauri::command]
#[specta::specta]
pub fn auto_run_forget_session(app: tauri::AppHandle, account_key: String) -> Result<(), String> {
    crate::autorun::sessions::forget_session(&root(&app)?, &account_key);
    Ok(())
}

/// Both `auto_run_clear_scripts` and `auto_run_clear_runs` refuse while a
/// run is going - an unattended run reads scripts as it goes, and a
/// supervised session writes them (Script) and reads them back (Run) - so
/// "clear" during either would race a job already using the very files it
/// is about to remove. The sentences match the ones `auto_run_open_browser`
/// and `auto_run_replay` already use for the same two exclusions, so a
/// person who has seen one has seen both.
///
/// `pub` rather than crate-private so a test can exercise the guard
/// directly - it takes no `AppHandle`, and building a real supervised
/// browser session just to see this message would be much more test than
/// the message deserves.
pub async fn refuse_while_a_run_is_going() -> Result<(), String> {
    if crate::commands::autorun_replay::replay_is_running() {
        return Err("an unattended run is going - wait for it, or stop it first".to_string());
    }
    if let Some(busy) = open_session_refusal_any().await {
        return Err(busy);
    }
    Ok(())
}

/// Remove the named cases' scripts from this machine, for the "Clear
/// scripts" button on the (development-only) Auto Run toolbar. Nothing in
/// Azure DevOps is touched - scripts never lived there.
///
/// Returns `u32`, not the `usize` `store::clear_scripts` itself returns -
/// specta refuses to export `usize` to TypeScript at all ("BigInt-style
/// types... to avoid precision loss"), and a count of files removed from a
/// handful of cases never comes close to needing 64 bits.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_clear_scripts(app: tauri::AppHandle, case_ids: Vec<i32>) -> Result<u32, String> {
    refuse_while_a_run_is_going().await?;
    let removed = store::clear_scripts(&root(&app)?, &case_ids)?;
    Ok(removed as u32)
}

/// Remove every saved run and screenshot from this machine, for the
/// "Clear results" button on the (development-only) Auto Run toolbar.
/// Runs already sent to Azure DevOps are removed too - the record there
/// is the durable one, and the confirm the screen shows says so.
///
/// Returns `u32` for the same reason `auto_run_clear_scripts` does.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_clear_runs(app: tauri::AppHandle) -> Result<u32, String> {
    refuse_while_a_run_is_going().await?;
    let removed = store::clear_runs(&root(&app)?)?;
    Ok(removed as u32)
}
