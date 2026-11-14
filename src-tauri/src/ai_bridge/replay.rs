//! Replaying a case's script for an assistant: starting, waiting for, answering and stopping a replay.

use super::*;

/// A future the replay host hands back: boxed, so the host can be a trait
/// object the app installs once.
pub type HostFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// What the app lends the bridge for an assistant's replay to a step: the
/// bridge has no `AppHandle` of its own (see `INTAKE_SINK`). A test hands
/// `autorun_replay_with` a fake one.
pub trait ReplayHost: Send + Sync {
    /// Run the replay in slot `slot`'s supervised browser, by the same path
    /// as the person's Replay to step button, as the assistant's: it never lifts a
    /// guard held for another case, and a discovery holding the browser is
    /// ended first rather than refusing (`end_discovery_for_replay`). `Err`
    /// is a browser that would not open.
    ///
    /// `holder` is the assistant whose call it is, if it is one: the browser
    /// the replay uses stays bound to it through a browser it replaces. The
    /// default caller passes none and is never bound.
    fn replay(
        &self,
        slot: SlotNo,
        holder: Option<AgentKey>,
        organization: String,
        project: String,
        req: crate::autorun::replay_to::ReplayRequest,
    ) -> HostFuture<'_, Result<AssistantReplay, String>>;
    /// The page the replay left slot `slot`'s browser on, as
    /// `/autorun-page` answers for this organization and project.
    fn page(&self, slot: SlotNo, organization: String, project: String) -> HostFuture<'_, (u16, String)>;
}

/// How an assistant's replay ended, and the discovery it ended before it
/// took the browser, if any.
#[derive(Debug, Clone, PartialEq)]
pub struct AssistantReplay {
    pub end: crate::autorun::replay_to::ReplayEnd,
    pub discovery: DiscoveryEnded,
    /// The supervised browser the replay leaves open, if it does: its
    /// answer says what that blocks (`replay_to::left_open_line`).
    pub left_open: Option<crate::autorun::replay_to::LeftOpen>,
    /// Where the replay started (`replay_to::carry_from`): its answer says
    /// it first.
    pub from: crate::autorun::replay_to::ReplayFrom,
    /// What it made of a browser already signed in as the case's account:
    /// its answer says so next (`replay_to::Reuse::said`).
    pub reuse: crate::autorun::replay_to::Reuse,
    /// What the assistant had changed for testing in the browser when the
    /// replay ended (`autorun::overrides`): its answer ends naming it - a
    /// replay that passed says it passed with these changes in place.
    pub overrides: crate::autorun::overrides::Overrides,
}

pub(crate) static REPLAY_HOST: std::sync::OnceLock<Box<dyn ReplayHost>> = std::sync::OnceLock::new();

/// Called once by the app when the bridge starts. Later calls are ignored.
pub fn set_replay_host(host: Box<dyn ReplayHost>) {
    let _ = REPLAY_HOST.set(host);
}

/// Said when a replay is asked for and the app has lent the bridge no host.
pub(crate) const NO_REPLAY_HOST: &str = "the app could not start replays this session - restart the app";

/// Replay a case to a step in the supervised browser, for the assistant.
pub(crate) async fn autorun_replay(ctx: &BridgeContext, body: &str, n: SlotNo, holder: Option<&AgentKey>) -> (u16, String) {
    let Some(host) = REPLAY_HOST.get() else {
        return (503, NO_REPLAY_HOST.to_string());
    };
    autorun_replay_as(holder, ctx, n, body, host.as_ref(), REPLAY_ANSWER_WAIT).await
}

/// How long `replay_autorun_to_step` waits for its replay to reach its stop
/// point before it answers that the replay is still running. Kept under
/// the wait of an assistant's own tool call.
pub const REPLAY_ANSWER_WAIT: std::time::Duration = std::time::Duration::from_secs(45);

/// An assistant's replay, running apart from the call that started it,
/// and its answer once it ends. Each slot keeps its own (`ASSISTANT_RUNS`). The answer is kept here until a call for
/// the same case and step takes it, so a replay that ends while no call
/// waits never loses it; a replay for another case replaces a finished one.
pub(crate) struct AssistantRun {
    pub(crate) id: u64,
    pub(crate) case_id: i32,
    pub(crate) step: i32,
    /// The script's `saved_at` when the replay started.
    pub(crate) saved_at: Option<String>,
    pub(crate) answer: tokio::sync::watch::Receiver<Option<ReplayDone>>,
    /// Whether the call that started it wanted the page (`page_wanted`): an
    /// answer the replay tool's `stop` collects honours it too.
    pub(crate) page: bool,
}

/// A finished assistant's replay: its answer, when it ended, and how many
/// times its slot's supervised browser had been used then
/// (`commands::autorun::session_uses_in`).
#[derive(Clone)]
pub(crate) struct ReplayDone {
    pub(crate) answer: (u16, String),
    pub(crate) ended: std::time::Instant,
    pub(crate) uses: u64,
}

/// The script's `saved_at` now, for case `case_id`.
pub(crate) fn script_saved_at(root: &std::path::Path, case_id: i32) -> Option<String> {
    crate::autorun::store::load_script(root, case_id).ok().flatten().and_then(|s| s.saved_at)
}

/// Whether `run` may still be handed to a call: running, or ended no more
/// than twice the call's `wait` ago (`REPLAY_ANSWER_WAIT` makes that 90 s)
/// with the script unchanged since it started and the browser not used
/// since it ended. An answer that fails any of these is stale: a new
/// replay starts instead.
pub(crate) fn still_fresh(run: &AssistantRun, n: SlotNo, root: &std::path::Path, wait: std::time::Duration) -> bool {
    let Some(done) = run.answer.borrow().clone() else {
        return true;
    };
    done.ended.elapsed() <= wait * 2
        && script_saved_at(root, run.case_id) == run.saved_at
        && crate::commands::autorun::session_uses_in(n) == done.uses
}

/// Each slot's assistant replay (`AssistantRun`), if any: one per browser.
/// Coordination only, not a cache. Never held across an await.
pub(crate) static ASSISTANT_RUNS: [std::sync::Mutex<Option<AssistantRun>>; SLOT_COUNT] =
    [const { std::sync::Mutex::new(None) }; SLOT_COUNT];

/// Numbers each assistant's replay, so a call takes only its own answer.
pub(crate) static NEXT_ASSISTANT_RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(crate) fn assistant_run(n: SlotNo) -> std::sync::MutexGuard<'static, Option<AssistantRun>> {
    ASSISTANT_RUNS[n.index()].lock().unwrap_or_else(|e| e.into_inner())
}

/// Said when the assistant asks for a replay while its replay of another
/// case or step is still running.
pub fn replay_running_elsewhere(case_id: i32, step: i32) -> String {
    format!(
        "a replay of case {case_id} to step {step} is already running - call replay_autorun_to_step with case {case_id} and step {step} to wait for it, then ask for this one"
    )
}

/// Said, with 202, when the replay has not reached its stop point within
/// the call's wait: where it is, and how to keep waiting.
pub fn replay_still_running(case_id: i32, step: i32, at: i32, of: i32) -> String {
    replay_still_running_on(case_id, step, at, of, None, None)
}

/// Where a replay is, as its 202s say it: step `at` of `of` - the case's
/// clean-up (step `cleanup`) by that name - or, before step 1, signing in
/// and going to the area.
pub(crate) fn replay_now(at: i32, of: i32, cleanup: Option<i32>) -> String {
    match at {
        at if at > 0 && cleanup == Some(at) => "now at the clean-up".to_string(),
        at if at > 0 => format!("now at step {at} of {of}"),
        _ => "now signing in and going to the case's area".to_string(),
    }
}

/// `replay_still_running`, saying too which action of the step it is on
/// (`runner::ActionNow`), for how long, and whether one of the step's
/// actions failed. A replay never tries an action again: a failed one is
/// pictured, the rest of its step is not run, and the replay ends at that
/// step. The case's clean-up (step `cleanup`) is said by that name.
pub fn replay_still_running_on(
    case_id: i32,
    step: i32,
    at: i32,
    of: i32,
    action: Option<&crate::autorun::runner::ActionNow>,
    cleanup: Option<i32>,
) -> String {
    let mut now = replay_now(at, of, cleanup);
    if let Some(a) = action.filter(|a| at > 0 && a.index > 0) {
        let what = if a.target.is_empty() { a.kind.clone() } else { format!("{} {}", a.kind, a.target) };
        let secs = a.since.map_or(0, |s| s.elapsed().as_secs());
        now.push_str(&format!(", on action {} of {} ({what}) for {secs} s", a.index, a.of));
        // A failed action stops its step: no later one of it begins, so
        // the one begun last is the one that failed.
        match a.failed {
            0 => now.push_str(", none of this step's actions has failed yet"),
            _ => now.push_str(&format!(", action {} failed - the replay ends at this step", a.index)),
        }
    }
    format!(
        "still running, {now} - call replay_autorun_to_step again with the same case ({case_id}) and step ({step}) to keep waiting"
    )
}

/// The 202 a replay call answers while its replay still runs: the sentence
/// (`replay_still_running_on`), the step and how many, and once a step's
/// action has begun, `action` (its place, kind, target and seconds on it),
/// `failed_in_step`, and `retries: false` (a replay never retries).
pub fn replay_still_running_answer(
    case_id: i32,
    step: i32,
    at: i32,
    of: i32,
    action: &crate::autorun::runner::ActionNow,
    cleanup: Option<i32>,
) -> serde_json::Value {
    let sentence = replay_still_running_on(case_id, step, at, of, Some(action), cleanup);
    let mut v = serde_json::json!({ "status": "running", "sentence": sentence, "step": at, "of": of });
    if at > 0 && action.index > 0 {
        v["action"] = serde_json::json!({
            "index": action.index,
            "of": action.of,
            "kind": action.kind,
            "target": action.target,
            "seconds": action.since.map_or(0, |s| s.elapsed().as_secs()),
        });
        v["failed_in_step"] = serde_json::json!(action.failed);
        v["retries"] = serde_json::json!(false);
    }
    v
}

/// Said when a replay ended without handing back its answer: its task
/// stopped before it could.
pub(crate) const REPLAY_LOST: &str = "the replay ended without an answer - see Settings, Logs, then replay again";

/// Wait up to `wait` for replay `id`'s answer. Once it is there it is
/// taken: the slot is freed for the next replay. Not there yet, the
/// answer is 202 with where the replay is now.
pub(crate) async fn wait_for_replay(
    n: SlotNo,
    mut answer: tokio::sync::watch::Receiver<Option<ReplayDone>>,
    id: u64,
    case_id: i32,
    step: i32,
    wait: std::time::Duration,
) -> (u16, String) {
    let got = tokio::time::timeout(wait, answer.wait_for(|a| a.is_some()))
        .await
        .map(|r| r.map(|a| a.as_ref().map(|done| done.answer.clone())));
    let taken = |answer: (u16, String)| {
        let mut slot = assistant_run(n);
        if slot.as_ref().is_some_and(|r| r.id == id) {
            *slot = None;
        }
        answer
    };
    match got {
        Ok(Ok(Some(done))) => taken(done),
        Ok(_) => taken((500, REPLAY_LOST.to_string())),
        Err(_) => {
            let replay = crate::autorun::replay_to::state(n);
            let (at, of) = replay.progress();
            let action = replay.action_now();
            let cleanup = replay.cleanup_at();
            (202, replay_still_running_answer(case_id, step, at, of, &action, cleanup).to_string())
        }
    }
}

/// `/autorun-replay`, body `{ case_id, step }`, in slot `slot`'s browser,
/// with its host and how long the call waits for the replay
/// (`REPLAY_ANSWER_WAIT`).
///
/// Refused, with the sentence, before anything opens: a body without the
/// two numbers, an unattended run going, a case with no saved script, a
/// step outside it, the person's replay going, or the assistant's replay of
/// another case or step still running (`replay_running_elsewhere`). Nothing
/// asks the person first: a script marked must not save replays at once,
/// its saves stopped throughout by its no-save guard (`guard_for_case`).
/// Database Access is the module, as for the person's own replay.
///
/// The replay runs apart from the call (`AssistantRun`). One that has not
/// reached its stop point within `wait` answers 202 with where it is
/// (`replay_still_running`); the same case and step asked for again waits
/// on that replay, up to `wait` more, and takes its answer - one that
/// ended in between hands it over at once.
///
/// A discovery holding the browser does not refuse it: the replay ends it
/// first (`end_discovery_for_replay`), and the answer's sentence starts
/// with `ENDED_DISCOVERY_FIRST`.
///
/// The answer is `{ "sentence" }`, and once the browser stands before the
/// step, or where the replay stopped at a failure (not while signing in:
/// the sign-in page shows the account's login), `"page"` beside it: the
/// page as `get_autorun_page` shows it, so the assistant can try the step
/// at once.
pub async fn autorun_replay_with(
    ctx: &BridgeContext,
    slot: SlotNo,
    body: &str,
    host: &'static dyn ReplayHost,
    wait: std::time::Duration,
) -> (u16, String) {
    autorun_replay_as(None, ctx, slot, body, host, wait).await
}

/// `autorun_replay_with` for the assistant `holder`, whose browser the
/// replay uses or opens in slot `slot`; none for the default caller, which
/// holds no browser and is never bound to one by its replay.
pub async fn autorun_replay_as(
    holder: Option<&AgentKey>,
    ctx: &BridgeContext,
    slot: SlotNo,
    body: &str,
    host: &'static dyn ReplayHost,
    wait: std::time::Duration,
) -> (u16, String) {
    let answer = replay_answer_with(holder, ctx, slot, body, host, wait).await;
    with_changes_on_a_refusal(answer, slot)
}

/// A replay's refusal (any 4xx or 5xx) that does not name what the
/// assistant changed for testing yet - one given before the replay took the
/// browser - names it, as last published for slot `slot`
/// (`commands::autorun::overrides_now_in`): a replay ended by the host names
/// it already, read under the lock.
pub fn with_changes_on_a_refusal(answer: (u16, String), slot: SlotNo) -> (u16, String) {
    use crate::autorun::overrides::{CHANGED_FOR_TESTING, PASSED_WITH_CHANGES};
    if answer.0 < 400 || answer.1.contains(CHANGED_FOR_TESTING) || answer.1.contains(PASSED_WITH_CHANGES) {
        return answer;
    }
    crate::autorun::overrides::with_note(answer, &crate::commands::autorun::overrides_now_in(slot))
}

/// `autorun_replay_with`, before a refusal names the test changes.
pub(crate) async fn replay_answer_with(
    holder: Option<&AgentKey>,
    ctx: &BridgeContext,
    n: SlotNo,
    body: &str,
    host: &'static dyn ReplayHost,
    wait: std::time::Duration,
) -> (u16, String) {
    use crate::autorun::replay_to::{self, ReplayRequest};
    const SHAPE: &str = "{ \"case_id\": <number>, \"step\": <number> }";
    let number = |key: &str| -> Result<i32, (u16, String)> {
        body_field(body, key, SHAPE)?
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .ok_or((400, format!("{key} must be a number")))
    };
    let (case_id, step) = match (number("case_id"), number("step")) {
        (Ok(c), Ok(s)) => (c, s),
        (Err(refused), _) | (_, Err(refused)) => return refused,
    };
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    // The same replay still running, or ended with a fresh answer not yet
    // taken (`still_fresh`): this call waits on it, and nothing new
    // starts. A stale answer is dropped, and a new replay starts below.
    let same = {
        let mut slot = assistant_run(n);
        match slot.as_ref().filter(|r| r.case_id == case_id && r.step == step) {
            Some(r) if still_fresh(r, n, &root, wait) => Some((r.answer.clone(), r.id)),
            Some(_) => {
                *slot = None;
                None
            }
            None => None,
        }
    };
    if let Some((answer, id)) = same {
        return wait_for_replay(n, answer, id, case_id, step, wait).await;
    }
    let db_read_access =
        crate::autorun::preconditions::read_access(!ctx.disabled_tools.iter().any(|t| t == "db_query"));
    let req = ReplayRequest { case_id, step, db_read_access };
    if let Err(why) = replay_to::check(&root, &ctx.org, &ctx.project, &req) {
        return (409, why);
    }
    // Looked at and claimed under one lock, so two calls cannot both start.
    let (tx, answer) = tokio::sync::watch::channel(None);
    let id = NEXT_ASSISTANT_RUN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let attach = {
        let mut slot = assistant_run(n);
        let fresh = slot.as_ref().is_some_and(|r| still_fresh(r, n, &root, wait));
        let running = slot.as_ref().map(|r| (r.case_id, r.step, r.id, r.answer.clone()));
        match running {
            Some((c, s, other, theirs)) if c == case_id && s == step && fresh => Some((theirs, other)),
            Some((c, s, _, theirs)) if theirs.borrow().is_none() => return (409, replay_running_elsewhere(c, s)),
            _ if replay_to::state(n).is_running() => return (409, replay_to::ALREADY_RUNNING.to_string()),
            _ => {
                let saved_at = script_saved_at(&root, case_id);
                *slot = Some(AssistantRun { id, case_id, step, saved_at, answer: answer.clone(), page: page_wanted(body) });
                replay_to::state(n).forget_stop_request();
                None
            }
        }
    };
    if let Some((theirs, other)) = attach {
        return wait_for_replay(n, theirs, other, case_id, step, wait).await;
    }
    let (org, project) = (ctx.org.clone(), ctx.project.clone());
    let holder = holder.cloned();
    let reader = match &holder {
        Some(key) => format!("{}:{}", key.id, key.name.as_deref().unwrap_or("")),
        None => String::new(),
    };
    let full = page_full(body);
    tokio::spawn(async move {
        let answer = run_assistant_replay(host, n, holder, org, project, req, reader, full).await;
        // Kept in the slot for the next call when none is waiting, while
        // it stays fresh (`still_fresh`).
        let done = ReplayDone {
            answer,
            ended: std::time::Instant::now(),
            uses: crate::commands::autorun::session_uses_in(n),
        };
        let _ = tx.send(Some(done));
    });
    wait_for_replay(n, answer, id, case_id, step, wait).await
}

/// Said by the replay tool's `stop` when no replay is going.
pub const NO_REPLAY_RUNNING: &str = "no replay is running - there is nothing to stop";

/// Said by the replay tool's `stop` while the replay going is the person's own.
pub const PERSONS_REPLAY_GOING: &str =
    "the replay going is the person's own - only they can stop it, with Stop in the Auto Run pane";

/// Said by the replay tool's `stop` when the assistant's last replay has
/// already ended, its answer not yet collected.
pub fn replay_already_ended(case_id: i32, step: i32) -> String {
    format!(
        "{NO_REPLAY_RUNNING}: the replay of case {case_id} to step {step} has ended - call replay_autorun_to_step with case {case_id} and step {step} for its answer"
    )
}

/// The 202 the replay tool's `stop` answers when the replay has not ended
/// within the call's wait: it was asked to stop, and hears it once the
/// action it is on finishes (an action's own wait for the page runs out
/// first).
pub fn replay_stopping_answer(n: SlotNo, case_id: i32, step: i32) -> serde_json::Value {
    let replay = crate::autorun::replay_to::state(n);
    let (at, of) = replay.progress();
    let action = replay.action_now();
    let mut now = replay_now(at, of, replay.cleanup_at());
    if at > 0 && action.index > 0 {
        now.push_str(&format!(", on action {} of {}", action.index, action.of));
    }
    let sentence = format!(
        "asked to stop: the replay of case {case_id} to step {step} ends once the action it is on finishes ({now}) - send replay_autorun_to_step stop: true again to wait for it"
    );
    serde_json::json!({ "status": "stopping", "sentence": sentence, "step": at, "of": of })
}

/// The 202 the replay tool's `stop` answers when the assistant's replay was
/// running no step to stop - it had not yet started, or had ended and its
/// answer was still being made ready - and no answer came within the wait.
/// Nothing was asked to stop.
pub fn replay_not_asked_answer(case_id: i32, step: i32) -> serde_json::Value {
    let sentence = format!(
        "not asked to stop: the replay of case {case_id} to step {step} was not running a step (it was starting, or had ended and its answer was being made ready) - send replay_autorun_to_step stop: true again to stop it, or replay_autorun_to_step with case {case_id} and step {step} for its answer"
    );
    serde_json::json!({ "status": "not_stopped", "sentence": sentence })
}

/// `/autorun-replay-stop`, the assistant's `replay_autorun_to_step` with `stop`, waiting
/// up to `wait` for the replay in slot `slot` to end. Its replay still going
/// is asked to stop through the replay's own stop
/// (`ReplayState::stop_on_request`, the one the person's Stop sets), which every replay hears before each
/// action, before each step and inside its waits; the answer is then the
/// replay's own, taken as `replay_autorun_to_step` takes it: stopped on
/// request, after which action of which step, with no page - or, had it
/// already stopped at a failing step, that failure with its page, or had
/// it already reached its step, that finished answer with its page. Not
/// ended within `wait`: 202 (`replay_stopping_answer`). Only the
/// assistant's replay running is asked to stop: between its end and its
/// answer the call only waits (202 `replay_not_asked_answer` past the
/// wait). No replay going: 404; the person's own: 409, never stopped.
pub async fn autorun_replay_stop_with(slot: SlotNo, wait: std::time::Duration) -> (u16, String) {
    let replay = crate::autorun::replay_to::state(slot);
    let mine = assistant_run(slot).as_ref().map(|r| (r.answer.clone(), r.id, r.case_id, r.step, r.page));
    match mine {
        Some((answer, id, case_id, step, page)) if answer.borrow().is_none() => {
            // Only the assistant's own replay going is asked to stop. Its
            // slot is freed before its answer is sent (the page is read in
            // between), and a replay the person starts in that gap clears
            // the stop and must never hear this one: with nothing of the
            // assistant's running, the call only waits for its answer.
            let asked = replay.assistants_replay_running();
            if asked {
                replay.stop_on_request();
                crate::applog::info(format!("Auto Run replay of case {case_id} to step {step}: the assistant asked it to stop"));
            }
            match wait_for_replay(slot, answer, id, case_id, step, wait).await {
                (202, _) if asked => (202, replay_stopping_answer(slot, case_id, step).to_string()),
                (202, _) => (202, replay_not_asked_answer(case_id, step).to_string()),
                // As the replay call that started it asked: `page: false`
                // leaves the page out here too.
                done if !page => without_page(done, r#"{"page":false}"#),
                done => done,
            }
        }
        _ if replay.is_running() => (409, PERSONS_REPLAY_GOING.to_string()),
        Some((_, _, case_id, step, _)) => (404, replay_already_ended(case_id, step)),
        None => (404, NO_REPLAY_RUNNING.to_string()),
    }
}

/// `s` with its first letter a capital, as a sentence after another one.
pub(crate) fn capitalised(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The assistant's replay through `host`, and its answer.
pub(crate) async fn run_assistant_replay(
    host: &'static dyn ReplayHost,
    n: SlotNo,
    holder: Option<AgentKey>,
    org: String,
    project: String,
    req: crate::autorun::replay_to::ReplayRequest,
    reader: String,
    full: bool,
) -> (u16, String) {
    use crate::autorun::replay_to::{ReplayEnd, ReplayFrom};
    let (case_id, step) = (req.case_id, req.step);
    let replay = crate::autorun::replay_to::state(n);
    let (end, discovery, left_open, from, reuse, overrides) = match host.replay(n, holder, org.clone(), project.clone(), req).await {
        Ok(AssistantReplay { end, discovery, left_open, from, reuse, overrides }) => {
            (end, discovery, left_open, from, reuse, overrides)
        }
        Err(why) => return (503, why),
    };
    // Where it started, said first: carried on, from the beginning, or
    // already there (then nothing ran, and that is all there is to say).
    // A browser signed in already says so next, where a sign-in would be.
    let reused = reuse.said(reuse.account.as_deref()).map(|line| format!("{line} ")).unwrap_or_default();
    // Stopped by the replay tool's `stop`: said as stopped on request, after the
    // last action to begin of the step it was on.
    let on_request = matches!(end, ReplayEnd::Stopped { .. }) && replay.stop_was_requested();
    // The case's clean-up is said as such, never as a step number.
    let cleanup = replay.cleanup_at();
    let ended = if on_request {
        let (at, _) = replay.progress();
        let action = replay.action_now();
        crate::autorun::replay_to::stopped_on_request(case_id, at, action.index, action.stopped_inside, cleanup)
    } else {
        end.sentence_in(cleanup)
    };
    // From the beginning for a reason another case's stand gave, it says so.
    let started = match (&reuse.not_carried, from) {
        (Some(why), ReplayFrom::Beginning) => format!("Started from the beginning: {why}"),
        _ => from.said(case_id, step),
    };
    let said = match (&end, from) {
        (ReplayEnd::Refused(_) | ReplayEnd::Blocked(_), _) => end.sentence(),
        (_, ReplayFrom::AlreadyThere) => from.said(case_id, step),
        _ => format!("{started} {reused}{}", capitalised(&ended)),
    };
    // Said before that when the replay ended the assistant's own discovery,
    // with the guard a no_save one leaves on.
    let said = match discovery.no_save {
        true => format!("{DISCOVERY_GUARD_KEPT} {said}"),
        false => said,
    };
    let mut sentence = after_ended_discovery(discovery.ended, &said);
    // A browser the replay leaves open, on a line of its own after it.
    // Not after the person's stop: Stop, Close or Release stopped it, and
    // the browser may be closing right behind the answer. A stop the
    // assistant asked for leaves it open, holding its account, so it is said.
    let refused_or_stopped = matches!(end, ReplayEnd::Refused(_) | ReplayEnd::Blocked(_))
        || (matches!(end, ReplayEnd::Stopped { .. }) && !on_request);
    if let (Some(left), false) = (&left_open, refused_or_stopped) {
        sentence.push('\n');
        sentence.push_str(&crate::autorun::replay_to::left_open_line(left));
    }
    // What the assistant changed for testing, last, so the result is never
    // taken for a normal run's: a replay that reached its step passed with
    // these changes in place.
    let changed = match end {
        ReplayEnd::Ready { .. } => overrides.passed_line(),
        _ => overrides.changed_line(),
    };
    if let Some(line) = changed {
        sentence.push('\n');
        sentence.push_str(&line);
    }
    // An ended explore session's unrecorded saves are said in the sentence,
    // so no arm of the answer drops them.
    if let Some(end) = discovery.explore.as_ref().filter(|e| !e.saves_not_on_the_change_list.is_empty()) {
        sentence.push_str(&format!(
            "\nThe explore session this replay ended sent saves that are not on its change list: {}. {}",
            end.saves_not_on_the_change_list.join(", "),
            crate::autorun::explore::UNRECORDED_SAVES
        ));
    }
    // An ended mapping run's summary goes beside the sentence, as
    // `end_autorun_discovery` hands it back; an ended explore session's end
    // beside it too.
    let with_summary = |mut answer: serde_json::Value| {
        if let Some(summary) = &discovery.summary {
            answer["summary"] = serde_json::json!(summary);
        }
        if let Some(end) = &discovery.explore {
            answer["explore"] = serde_json::json!(end);
        }
        answer.to_string()
    };
    match end {
        ReplayEnd::Refused(_) | ReplayEnd::Blocked(_) => (409, sentence),
        // A sign-in that stopped stands on the sign-in page, whose username
        // field shows the account's login: accounts are named by key, never
        // by login, so its page is not answered.
        ReplayEnd::StoppedAt { phase: crate::autorun::replay_to::ReplayPhase::SignIn, .. } => {
            (200, with_summary(serde_json::json!({ "sentence": sentence })))
        }
        // Where the browser stands: before the step, or at the failure it
        // stopped on (a browser that went is never `StoppedAt`).
        ReplayEnd::Ready { .. } | ReplayEnd::StoppedAt { .. } => {
            let answer = match compacted(n, &reader, full, host.page(n, org, project)).await {
                (200, page) => serde_json::json!({ "sentence": sentence, "page": page }),
                (_, why) => serde_json::json!({ "sentence": sentence, "page_unavailable": why }),
            };
            (200, with_summary(answer))
        }
        // Stop, Close or Release ended it, or the browser went: there is
        // no page to read, and the browser may be closing behind the answer.
        ReplayEnd::Stopped { .. } | ReplayEnd::BrowserGone { .. } => {
            (200, with_summary(serde_json::json!({ "sentence": sentence })))
        }
    }
}

