//! Ending a discovery, and what ending one for a replay means.

use super::*;

/// Said by `end_autorun_discovery` once the discovery is over and its
/// browser kept (`end_discovery_in`).
pub const DISCOVERY_ENDED_KEPT: &str =
    "the discovery is over; its browser stays open and signed in, for your next replay or discovery";

/// Said once an assistant's end (without `keep_browser`) has ended the
/// discovery, closed its browser and let its account go.
pub const DISCOVERY_ENDED_RELEASED: &str = "the discovery is over, its browser is closed and its account is free";

/// Said by an assistant's end of a discovery in a browser the person
/// opened: that browser is never closed by an assistant.
pub const DISCOVERY_ENDED_PERSONS: &str =
    "the discovery is over; the browser stays open because the person opened it";

/// Said once a discovery is over and its browser closed: by the assistant's
/// release, and by an end whose guard would not lift.
pub(crate) const DISCOVERY_ENDED_CLOSED: &str = "the discovery is over and its browser is closed";

/// End the discovery in `slot` (`end_discovery_keeping`): its browser is
/// kept, signed in, for the next replay or discovery, and its guard
/// (`GuardOwner::Discovery`) is lifted. Any other guard - the person's, a
/// call's - is not the discovery's to lift. With nothing to end this is
/// still an answer, not an error - ending twice is fine.
///
/// A mapping run's summary is kept first (`finish_mapping`), and the
/// answer is `{detail, summary}`.
pub async fn end_discovery_in<B: DiscoveryBrowser>(slot: &mut Option<B>) -> (u16, String) {
    match end_discovery_keeping(slot, true).await {
        Some(ended) => {
            let detail = if ended.kept { DISCOVERY_ENDED_KEPT } else { DISCOVERY_ENDED_CLOSED };
            ended_answer(detail, ended.summary, ended.explore)
        }
        None => (200, "no discovery is going".to_string()),
    }
}

/// Who ends a discovery (`end_discovery_as`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndedBy {
    /// The person's End discovery on the Setup card: it closes the
    /// assistant's discovery browser so Auto Run can run again.
    Person,
    /// The assistant's `end_autorun_discovery`: the browser is closed and
    /// its account let go (a browser the person opened stays).
    Assistant,
    /// The assistant's `end_autorun_discovery` with `keep_browser`: the
    /// browser stays, signed in, for its next replay or discovery.
    AssistantKeeping,
}

/// End the discovery in `slot` as `by` ends it: the assistant's end closes
/// the browser unless it keeps it (`end_discovery_releasing`,
/// `end_discovery_in`), the person's always closes it
/// (`end_discovery_closing`). A mapping run's summary is kept in every case.
pub async fn end_discovery_as<B: DiscoveryBrowser>(slot: &mut Option<B>, by: EndedBy) -> (u16, String) {
    match by {
        EndedBy::Assistant => end_discovery_releasing(slot).await,
        EndedBy::AssistantKeeping => end_discovery_in(slot).await,
        EndedBy::Person => end_discovery_closing(slot),
    }
}

/// The assistant's end of a discovery, by default: the release path
/// (`end_discovery_closing`) with the same ended answer (a mapping run's
/// summary, an explore session's change list and unrecorded saves) and the
/// words that the browser is closed and its account free. A browser the
/// person opened is never closed by an assistant: that end keeps it, and says so.
pub async fn end_discovery_releasing<B: DiscoveryBrowser>(slot: &mut Option<B>) -> (u16, String) {
    let persons = match slot.as_mut() {
        Some(b) => b.parts().discovery.is_some() && !b.opened_by_assistant(),
        None => false,
    };
    if persons {
        return match end_discovery_keeping(slot, true).await {
            Some(ended) => ended_answer(DISCOVERY_ENDED_PERSONS, ended.summary, ended.explore),
            None => (200, "no discovery is going".to_string()),
        };
    }
    match close_discovery(slot) {
        Some((summary, explore)) => ended_answer(DISCOVERY_ENDED_RELEASED, summary, explore),
        // No discovery, but a replay left the assistant's own browser: this
        // end frees it (and its account) the same way. A person's stays.
        None if slot.as_mut().is_some_and(|b| b.opened_by_assistant()) => {
            close_browser_in(slot);
            crate::applog::info("Auto Run: the assistant's end closed the browser its replay left open");
            (200, DISCOVERY_ENDED_RELEASED.to_string())
        }
        None => (200, "no discovery is going".to_string()),
    }
}

/// End the discovery in `slot` and close its browser, as the person's End
/// discovery and the assistant's release
/// (`commands::autorun::release_in_for_assistant`) do: the person ends it
/// so Auto Run can run again, and release is the repair for a broken
/// browser, so neither keeps one. The answer is `end_discovery_in`'s, with
/// the browser closed.
pub fn end_discovery_closing<B: DiscoveryBrowser>(slot: &mut Option<B>) -> (u16, String) {
    match close_discovery(slot) {
        Some((summary, explore)) => ended_answer(DISCOVERY_ENDED_CLOSED, summary, explore),
        None => (200, "no discovery is going".to_string()),
    }
}

/// A discovery's end, said: `{detail, summary}` for a mapping run,
/// `{detail, explore, note}` for an explore session (`note` only when saves
/// its browser sent are not on its change list), else the words.
pub(crate) fn ended_answer(
    detail: &str,
    summary: Option<crate::autorun::mapping_summary::MappingSummary>,
    explore: Option<crate::autorun::explore::ExploreEnd>,
) -> (u16, String) {
    match (summary, explore) {
        (Some(summary), _) => (200, serde_json::json!({ "detail": detail, "summary": summary }).to_string()),
        (None, Some(explore)) => {
            let mut answer = serde_json::json!({ "detail": detail, "explore": explore });
            if !explore.saves_not_on_the_change_list.is_empty() {
                answer["note"] = serde_json::json!(crate::autorun::explore::UNRECORDED_SAVES);
            }
            (200, answer.to_string())
        }
        (None, None) => (200, detail.to_string()),
    }
}

/// How a discovery ended (`end_discovery_keeping`): the mapping run's
/// summary, if it was one, the explore session's end, if it was one, and
/// whether its browser was kept.
pub(crate) struct Ended {
    pub(crate) summary: Option<crate::autorun::mapping_summary::MappingSummary>,
    pub(crate) kept: bool,
    pub(crate) explore: Option<crate::autorun::explore::ExploreEnd>,
}

/// The way a discovery in `slot` ends and its browser stays: a mapping
/// run's summary is kept (`finish_mapping`), the discovery is taken off the
/// browser, and - when `lift` - the discovery's own guard
/// (`GuardOwner::Discovery`) is lifted. The browser keeps its sign-in, its
/// lease and its page. A guard that will not lift is never left behind with
/// no discovery to end it: a browser the assistant opened is then closed
/// through its normal close; one the person opened is never closed, and its
/// guard stays on until the person's next step decides it. `None` with no
/// discovery going.
pub(crate) async fn end_discovery_keeping<B: DiscoveryBrowser>(slot: &mut Option<B>, lift: bool) -> Option<Ended> {
    use crate::commands::autorun::GuardOwner;
    if !slot.as_mut().is_some_and(|b| b.parts().discovery.is_some()) {
        return None;
    }
    let summary = finish_mapping(slot);
    let explore = finish_explore(slot);
    let (lifted, assistants) = match slot.as_mut() {
        Some(b) => {
            let assistants = b.opened_by_assistant();
            let p = b.parts();
            *p.discovery = None;
            let lifted = if lift && *p.guard_owner == Some(GuardOwner::Discovery) {
                crate::commands::autorun::lift_guard_of(p.driver, p.guarded_case, p.guard_owner).await
            } else {
                true
            };
            (lifted, assistants)
        }
        None => (true, false),
    };
    if !lifted && assistants {
        if let Some(browser) = slot.take() {
            crate::browser::tree::blocking(|| browser.close());
        }
        crate::applog::info("Auto Run discovery ended; its guard would not lift, so its browser is closed");
        return Some(Ended { summary, kept: false, explore });
    }
    crate::applog::info("Auto Run discovery ended; its browser is kept, signed in");
    Some(Ended { summary, kept: true, explore })
}

/// A discovery in `slot` ends and its browser is closed: a mapping run's
/// summary is kept (`finish_mapping`), then the browser goes: the person's
/// End discovery and the assistant's release (`end_discovery_closing`).
/// `None` with no discovery going, else the mapping run's summary, if it
/// was one, and the explore session's end, if it was one.
pub(crate) fn close_discovery<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
) -> Option<(Option<crate::autorun::mapping_summary::MappingSummary>, Option<crate::autorun::explore::ExploreEnd>)> {
    if !slot.as_mut().is_some_and(|b| b.parts().discovery.is_some()) {
        return None;
    }
    let summary = finish_mapping(slot);
    let explore = finish_explore(slot);
    if let Some(browser) = slot.take() {
        browser.close();
    }
    crate::applog::info("Auto Run discovery ended; its browser is closed");
    Some((summary, explore))
}

/// Close out the mapping run in `slot`, if one is going, however it ends:
/// the end route, End discovery, Close browser, or the app exiting with
/// it open (a browser that died ends one of those ways). The saves its
/// guard blocked since the last action are counted first, then its summary
/// (`mapping_summary::summarize`) is kept under its project, replacing the
/// last, and logged as one line of counts and names. The run is taken off
/// the discovery, so closing it twice keeps one summary. `None` with no
/// mapping run going.
pub fn finish_mapping<B: DiscoveryBrowser>(slot: &mut Option<B>) -> Option<crate::autorun::mapping_summary::MappingSummary> {
    use crate::autorun::mapping_summary::{log_line, save_summary, summarize};
    let browser = slot.as_mut()?;
    let p = browser.parts();
    count_blocked_writes(p.driver, p.discovery.as_mut(), &[]);
    let run = p.discovery.as_mut()?.mapping.take()?;
    let summary = summarize(&run);
    if let Some(place) = &run.place {
        if let Err(why) = save_summary(&place.root, &place.organization, &place.project, &summary) {
            crate::applog::warn(format!("Auto Run mapping run: its summary could not be kept: {why}"));
        }
    }
    crate::applog::info(log_line(&summary));
    Some(summary)
}

/// Refused while a discovery holds the browser in `slot`: a replay, or the
/// person's Open browser, would otherwise take the discovery's browser
/// over. The sentence names `end_autorun_discovery`.
pub fn refuse_while_discovering<B: DiscoveryBrowser>(slot: &mut Option<B>) -> Result<(), String> {
    if slot.as_mut().is_some_and(|b| b.parts().discovery.is_some()) {
        return Err(crate::commands::autorun::busy_browser_sentence(true).to_string());
    }
    Ok(())
}

/// Before the assistant's replay to a step takes the browser in `slot`: a
/// discovery holding it is ended as End discovery ends it
/// (`end_discovery_keeping`), so what it mapped is kept, a mapping run's
/// summary is saved, and the browser stays, signed in, for the replay. A
/// no_save discovery's guard stays on - never off between the discovery and
/// the replay that is its last call (`DiscoveryEnded::no_save`); any other
/// discovery's guard (a mapping run's) is lifted. Says whether one was
/// ended, with that summary. Called under the session lock the replay then
/// keeps while it runs, so nothing takes the browser between the end and
/// the replay. The person's own Replay to step still goes through
/// `refuse_while_discovering`.
pub async fn end_discovery_for_replay<B: DiscoveryBrowser>(slot: &mut Option<B>, root: &std::path::Path) -> DiscoveryEnded {
    // Decided as before each of its actions (`discovery_must_not_save`), so a
    // case marked after its last action counts too.
    let no_save = slot
        .as_mut()
        .and_then(|b| b.parts().discovery.as_ref().map(|s| discovery_must_not_save(root, s)))
        .unwrap_or(false);
    match end_discovery_keeping(slot, !no_save).await {
        Some(ended) => {
            crate::applog::info("Auto Run: the assistant's replay to a step ended its discovery first");
            DiscoveryEnded { ended: true, summary: ended.summary, no_save, explore: ended.explore }
        }
        None => DiscoveryEnded::default(),
    }
}

/// Whether the assistant's replay ended a discovery before it took the
/// browser (`end_discovery_for_replay`), and that mapping run's summary
/// when it was one.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DiscoveryEnded {
    pub ended: bool,
    pub summary: Option<crate::autorun::mapping_summary::MappingSummary>,
    /// How an explore session it ended ended: its changes, and the saves its
    /// browser sent that no change accounts for.
    pub explore: Option<crate::autorun::explore::ExploreEnd>,
    /// The discovery it ended must not save (`discovery_must_not_save`): its
    /// guard stays on for that replay, the call that ended it, and for no
    /// call after (`commands::autorun::guard_before_replay`,
    /// `settle_replay_guard`).
    pub no_save: bool,
}

/// Put before the answer to an assistant's replay that ended its discovery
/// first (`end_discovery_for_replay`).
pub const ENDED_DISCOVERY_FIRST: &str =
    "Ended your discovery first (what it mapped is kept); start a new one to explore again.";

/// Said after `ENDED_DISCOVERY_FIRST` when the discovery the replay ended
/// was a no_save one: its guard stays on for this replay
/// (`DiscoveryEnded::no_save`).
pub const DISCOVERY_GUARD_KEPT: &str = "Its save guard stayed on for this replay, because the discovery was started without saving; your next replay of a case that may save is not guarded.";

/// `said`, after `ENDED_DISCOVERY_FIRST` when the replay ended a discovery:
/// its answer, or the browser that would not open after the end.
pub fn after_ended_discovery(ended: bool, said: &str) -> String {
    match ended {
        true => format!("{ENDED_DISCOVERY_FIRST} {said}"),
        false => said.to_string(),
    }
}

/// Close whatever browser `slot` holds - a discovery ends with it. Whether
/// there was one.
pub fn close_browser_in<B: DiscoveryBrowser>(slot: &mut Option<B>) -> bool {
    finish_mapping(slot);
    finish_explore(slot);
    match slot.take() {
        Some(browser) => {
            // Closing waits for the browser's processes to go: off the
            // async worker.
            crate::browser::tree::blocking(|| browser.close());
            true
        }
        None => false,
    }
}

