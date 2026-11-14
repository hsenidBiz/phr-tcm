//! The browser a discovery drives (`DiscoveryBrowser`) and the rules for taking it over, keeping it and letting it go.

use super::*;

/// What a discovery works with in the Auto Run browser: its driver, its
/// account lease, the account KEY it is signed in as, and the discovery
/// under way in it (`None` in a browser the person opened).
pub struct DiscoveryParts<'a, D> {
    pub driver: &'a mut D,
    pub lease: &'a mut crate::autorun::lease::Held,
    pub signed_in: &'a mut Option<String>,
    pub discovery: &'a mut Option<crate::commands::autorun::DiscoveryState>,
    /// The case whose no-save guard the browser holds, and who the guard
    /// belongs to (`commands::autorun::GuardOwner`).
    pub guarded_case: &'a mut Option<i32>,
    pub guard_owner: &'a mut Option<crate::commands::autorun::GuardOwner>,
    /// What the assistant changed for testing in this browser
    /// (`autorun::overrides`). `None` for a browser that keeps no record
    /// (a test's fake): nothing may be changed in it.
    pub overrides: Option<&'a mut crate::autorun::overrides::Overrides>,
}

/// The Auto Run browser as a discovery sees it: the app's session
/// (`commands::autorun::Session`), or a test's fake.
pub trait DiscoveryBrowser {
    type D: crate::browser::cdp::Driver;
    fn parts(&mut self) -> DiscoveryParts<'_, Self::D>;
    /// Close the browser, and with it the discovery and its lease.
    fn close(self);
    /// Is the browser still there to drive: its processes run and its own
    /// connection still answers (`commands::autorun::held_browser_alive`)?
    /// A browser the app holds that has gone is let go
    /// (`forget_gone_browser`) rather than kept as busy. A test's fake is
    /// alive unless it says otherwise.
    fn alive(&mut self) -> impl std::future::Future<Output = bool> + Send {
        async { true }
    }
    /// Whether the assistant opened this browser (its replay, or a
    /// discovery): its release may close it, and its discovery may take it
    /// over. A test's fake is the person's unless it says otherwise.
    fn opened_by_assistant(&mut self) -> bool {
        false
    }
    /// A discovery took this browser over from the replay that left it
    /// (`make_way_for_discovery`), or the assistant's reset cleaned it
    /// (`commands::autorun::reset_in_for_assistant`): no replay carries on
    /// from where it stood, and no watched step names what happened before.
    /// Nothing to forget in a test's fake unless it counts it.
    fn taken_over(&mut self) {}
    /// Which browser this is (Edge or Chrome), when known: a discovery that
    /// names the other one does not take it over. A test's fake says none.
    fn browser(&mut self) -> Option<crate::browser::launch::Browser> {
        None
    }
}

/// Said in place of an answer from a held browser that had gone: the app
/// has closed it the normal way and let it go, and none is open now.
pub const BROWSER_GONE: &str = "the Auto Run browser had closed, so the app let it go: no Auto Run browser is open now - start a discovery with start_autorun_discovery, or the person opens one with Open browser on the Auto Run tab";

/// Let go of the browser `slot` holds when it has gone (`alive` says no):
/// a mapping run's summary is kept (`finish_mapping`), the browser is
/// closed through its normal close (which ends what is left of its job and
/// removes its profile) and the slot is emptied. True when it did; a live
/// browser, or none, is left as it is. Called before anything is refused
/// only because a browser is held, and when a held browser fails to
/// answer.
pub async fn forget_gone_browser<B: DiscoveryBrowser>(slot: &mut Option<B>) -> bool {
    match slot.as_mut() {
        None => return false,
        Some(b) => {
            if b.alive().await {
                return false;
            }
        }
    }
    let discovering = slot.as_mut().is_some_and(|b| b.parts().discovery.is_some());
    finish_mapping(slot);
    finish_explore(slot);
    if let Some(b) = slot.take() {
        crate::browser::tree::blocking(|| b.close());
    }
    let what = if discovering { "discovery browser" } else { "browser" };
    crate::applog::info(format!("Auto Run: the held {what} had closed or stopped answering; it is let go"));
    true
}

/// Refused only while `slot` holds a browser that is really there, with
/// the busy sentence (`busy_browser_sentence`): one that has gone is let go
/// first (`forget_gone_browser`), and nothing is refused for it. Before a
/// discovery opens.
pub async fn refuse_while_held<B: DiscoveryBrowser>(slot: &mut Option<B>) -> Result<(), String> {
    forget_gone_browser(slot).await;
    match slot.as_mut() {
        Some(b) => Err(crate::commands::autorun::busy_browser_sentence(b.parts().discovery.is_some()).to_string()),
        None => Ok(()),
    }
}

/// What `make_way_for_discovery` did with the browser `slot` held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MadeWay {
    /// None was held (or one that had gone was let go): the discovery opens
    /// a new one.
    Free,
    /// The assistant's replay had left one open: the discovery took it over,
    /// signed in as it was.
    TookOver,
    /// The replay's browser kept a guard it would not lift, so it was closed
    /// through its normal close: the discovery opens a new one.
    Closed,
}

/// Where a guarded discovery reads its save words from (`nav::load_nav`):
/// the project it explores. `make_way_for_discovery` arms the discovery's
/// guard with them when it takes a browser over.
#[derive(Debug, Clone, Copy)]
pub struct GuardFrom<'a> {
    pub root: &'a std::path::Path,
    pub organization: &'a str,
    pub project: &'a str,
}

/// Before a discovery opens in `slot`: a held browser that has gone is let
/// go first (`forget_gone_browser`). A live one the assistant's replay left
/// open (opened by the assistant, no discovery in it, no guard of the
/// person's) is taken over, sign-in and all: its tabs other than `main` are
/// closed, the dialogs the replay expected or saw are forgotten, and the
/// replay's stand and last page change too (`DiscoveryBrowser::taken_over`).
/// Its guard is never lifted here. A discovery that will be guarded
/// (`guarded`: it must not save, or is a mapping run, with where its save
/// words are) has its guard on before this returns, still under the lock
/// the caller took: the replay's guard taken over as it is, or - with none
/// on - the discovery's own armed now (`guard_a_no_save_discovery`), its
/// owner the discovery's either way. So no call that takes the lock between
/// the takeover and the start (`discover_start_in`) finds the browser
/// unguarded with a no_save discovery on record. A guard that will not go
/// on refuses the discovery with its sentence, the browser left as the
/// replay left it. For any other discovery, the replay's guard stays until
/// the start has decided the sign-in. One that is another browser than the
/// one `wanted` (Edge or Chrome) is closed through its normal close
/// instead, and a new one opened. Any other live browser - the person's, or
/// a discovery's - refuses it with the busy sentence (`refuse_while_held`),
/// untouched. A replay going never reaches here
/// (`refuse_discovery_while_busy`).
pub async fn make_way_for_discovery<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    wanted: Option<crate::browser::launch::Browser>,
    guarded: Option<GuardFrom<'_>>,
) -> Result<MadeWay, String> {
    use crate::commands::autorun::GuardOwner;
    forget_gone_browser(slot).await;
    let left_open = slot.as_mut().is_some_and(|b| {
        b.opened_by_assistant() && {
            let p = b.parts();
            p.discovery.is_none() && *p.guard_owner != Some(GuardOwner::Person)
        }
    });
    if !left_open {
        return refuse_while_held(slot).await.map(|()| MadeWay::Free);
    }
    let other_browser = match (wanted, slot.as_mut().and_then(|b| b.browser())) {
        (Some(wanted), Some(open)) => wanted != open,
        _ => false,
    };
    if other_browser {
        close_browser_in(slot);
        crate::applog::info("Auto Run: a discovery asked for another browser than the one the assistant's replay left open, so it closed that one");
        return Ok(MadeWay::Closed);
    }
    if let Some(b) = slot.as_mut() {
        let p = b.parts();
        // First the guard, before the browser is touched: one that will not
        // go on leaves it as the replay left it.
        if let Some(from) = guarded {
            guard_a_no_save_discovery(p.driver, true, from.root, from.organization, from.project).await?;
            *p.guarded_case = None;
            crate::commands::autorun::claim_guard(p.guard_owner, GuardOwner::Discovery);
        }
        crate::browser::cdp::Driver::close_other_tabs(p.driver).await;
        // Nothing the replay expected of a dialog, or saw, is the discovery's.
        if let Some(book) = crate::browser::cdp::Driver::dialog_book(p.driver) {
            book.disarm();
            book.take_seen();
        }
        b.taken_over();
    }
    crate::applog::info("Auto Run: a discovery took over the browser the assistant's replay left open");
    Ok(MadeWay::TookOver)
}

/// The guard a call of the assistant's left on a browser a discovery took
/// over (`GuardOwner::Call`), lifted once the discovery's start has decided
/// its sign-in: after going home in a browser already signed in, right
/// before the sign-in otherwise. A discovery's own guard, or the person's,
/// is not this to lift. One the browser will not lift stays on, still the
/// call's: no discovery call lifts it as it ends (each lifts only the guard
/// it put on itself), so it stays until a later replay lifts it (or a reset
/// or release does). Saves stay stopped meanwhile, never let through.
pub(crate) async fn lift_a_calls_guard<D: crate::browser::cdp::Driver>(
    d: &mut D,
    guarded_case: &mut Option<i32>,
    owner: &mut Option<crate::commands::autorun::GuardOwner>,
) {
    if !d.is_guarding_saves() || !matches!(owner, None | Some(crate::commands::autorun::GuardOwner::Call { .. })) {
        return;
    }
    if !crate::commands::autorun::lift_guard_of(d, guarded_case, owner).await {
        crate::applog::warn("Auto Run discovery: the replay's guard would not lift yet; it stays on until a later replay lifts it");
    }
}

/// Go home in a browser a discovery took over already signed in
/// (`nav::load_home`, which keeps the cookies and waits for the signed-in
/// marker), so the replay's page is never filed under the discovery's area.
/// False when it did not arrive: the discovery then signs in.
pub(crate) async fn went_home<D: crate::browser::cdp::Driver>(
    d: &mut D,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    timing: &crate::browser::timing::Timing,
) -> bool {
    let recipe = match crate::autorun::recipe::load_effective_recipe(root, organization, project) {
        Ok(recipe) => recipe,
        Err(_) => return false,
    };
    let out = crate::autorun::nav::load_home(d, &crate::autorun::nav::Home::of(&recipe), timing).await;
    if !out.ok {
        crate::applog::info(format!("Auto Run discovery: going home in the kept browser failed ({}); it signs in", out.detail));
    }
    out.ok
}

/// Put before what a discovery start answers when it first closed the
/// browser the assistant's replay left open (`MadeWay::Closed`).
pub const CLOSED_LEFT_OPEN: &str = "Closed the browser your replay left open.";

/// Put before what a discovery start answers when it took over the browser
/// the assistant's replay left open (`MadeWay::TookOver`).
pub const TOOK_OVER_LEFT_OPEN: &str = "Took over the browser your replay left open (its other tabs closed).";

/// Does this answer from a held browser say the browser failed, rather
/// than the page: a 503, or the words of a closed browser or a failed
/// DevTools socket?
pub(crate) fn said_browser_failed(answer: &(u16, String)) -> bool {
    answer.0 == 503
        || answer.1.contains(&crate::browser::cdp::CdpError::Closed.to_string())
        || answer.1.contains("DevTools socket failed")
}

/// `answer`, from the browser `slot` holds - unless it says the browser
/// failed and the browser has indeed gone: then it is let go right here
/// (`forget_gone_browser`) and the answer is `BROWSER_GONE`, so the next
/// call does not meet the same dead browser.
pub async fn let_go_if_silent<B: DiscoveryBrowser>(slot: &mut Option<B>, answer: (u16, String)) -> (u16, String) {
    if said_browser_failed(&answer) && forget_gone_browser(slot).await {
        return (409, BROWSER_GONE.to_string());
    }
    answer
}

