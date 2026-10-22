//! `page_until`: click a list's Next control, page by page, until every
//! locator the case wants has been seen, each on any page.
//!
//! The walk is a small state machine (`Walk`) kept apart from the browser so
//! its sentences can be tested without one: it is told what each page
//! showed and answers with what to do next. `page_until` is the loop that
//! asks the page and does it. Nothing here waits a fixed time: a page is
//! given the same settle every click is, and a Next that is gone or disabled
//! ends the walk at once, naming the pages walked, rather than being waited
//! out.

use super::actions::{blocked, failed_by, harness, point_and_pause, settle, shown_now, ActionOutcome};
use super::cdp::{CdpError, Driver};
use super::expect::{self, Check};
use super::input;
use super::locator::Target;
use super::page;
use super::timing::Timing;

/// What the walk does after a page has been looked at.
#[derive(Debug, Clone, PartialEq)]
pub enum Turn {
    /// Every locator has been seen: the sentence that says where.
    Seen(String),
    /// Go to the next page.
    Click,
    /// The walk ends without having seen everything: why.
    Failed(String),
}

/// Where a walk stands: the locators it wants (by name), the page each was
/// first seen on, and the page just looked at.
pub struct Walk {
    names: Vec<String>,
    first_seen: Vec<Option<u32>>,
    page: u32,
    max_pages: u32,
}

fn pages(n: u32) -> &'static str {
    if n == 1 {
        "page"
    } else {
        "pages"
    }
}

impl Walk {
    pub fn new(names: Vec<String>, max_pages: u32) -> Walk {
        let first_seen = vec![None; names.len()];
        Walk { names, first_seen, page: 0, max_pages }
    }

    /// Has the locator at `index` been seen on a page already?
    pub fn seen(&self, index: usize) -> bool {
        self.first_seen.get(index).is_some_and(Option::is_some)
    }

    /// One page looked at; `shown` has one bool per `until`, in order. A
    /// locator seen on an earlier page stays seen whatever `shown` says.
    pub fn look(&mut self, shown: &[bool]) -> Turn {
        self.page += 1;
        for (i, seen) in self.first_seen.iter_mut().enumerate() {
            if seen.is_none() && shown.get(i).copied().unwrap_or(false) {
                *seen = Some(self.page);
            }
        }
        if self.first_seen.iter().all(Option::is_some) {
            let each: Vec<String> = self
                .names
                .iter()
                .zip(&self.first_seen)
                .map(|(name, page)| format!("{name} on page {}", page.unwrap_or(0)))
                .collect();
            return Turn::Seen(format!("saw {}", each.join(" and ")));
        }
        if self.page >= self.max_pages {
            return Turn::Failed(format!("walked {} {} and never saw {}", self.page, pages(self.page), self.missing()));
        }
        Turn::Click
    }

    /// The next control could not be used on the page just looked at; `why`
    /// is "gone" or "disabled".
    pub fn no_next(&self, next: &str, why: &str) -> String {
        format!(
            "walked {} {} and never saw {}: {next} is {why} on page {}",
            self.page,
            pages(self.page),
            self.missing(),
            self.page
        )
    }

    fn missing(&self) -> String {
        self.names
            .iter()
            .zip(&self.first_seen)
            .filter(|(_, seen)| seen.is_none())
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(" and ")
    }
}

/// Click `next` page by page until every `until` locator has been seen.
pub async fn page_until<D: Driver>(
    d: &mut D,
    next: &Target,
    until: &[Target],
    max_pages: u32,
    timing: &Timing,
) -> ActionOutcome {
    let names: Vec<String> = until.iter().map(Target::describe).collect();
    let next_name = next.describe();
    let mut walk = Walk::new(names, max_pages);
    loop {
        settle(d, timing).await;
        let mut shown = Vec::with_capacity(until.len());
        for (i, target) in until.iter().enumerate() {
            if walk.seen(i) {
                shown.push(true);
                continue;
            }
            match shown_now(d, target).await {
                Ok(found) => shown.push(found.unwrap_or(false)),
                Err(out) => return out,
            }
        }
        match walk.look(&shown) {
            Turn::Seen(sentence) => return ActionOutcome::passed(sentence),
            Turn::Failed(why) => return ActionOutcome::failed(why),
            Turn::Click => {}
        }
        match shown_now(d, next).await {
            Ok(Some(true)) => {}
            Ok(_) => return ActionOutcome::failed(walk.no_next(&next_name, "gone")),
            Err(out) => return out,
        }
        page::release(d).await;
        match expect::look(d, next, &Check::Disabled).await {
            Ok(Ok(_)) => return ActionOutcome::failed(walk.no_next(&next_name, "disabled")),
            Ok(Err(_)) => {}
            Err(e) if e.is_transient() => {}
            Err(CdpError::Timeout { .. }) => {}
            Err(e) => return harness(e),
        }
        let ready = match input::wait_ready(d, next, false, timing).await {
            Ok(r) => r,
            Err(b) => return blocked(b),
        };
        if let Err(e) = point_and_pause(d, &ready, timing).await {
            return failed_by(e);
        }
        if let Err(b) = input::click(d, &ready).await {
            return blocked(b);
        }
    }
}
