//! Reading the Auto Run browser's page: the page dump, what is seen and recorded from it, locator probes and inspection.

use super::*;

/// The page in the browser the person opened, as text: Chrome's own
/// accessibility tree with a locator on every line.
/// `within` (a locator, as a script writes one: a string, an object or a
/// chain), its `nth` (`within_target_nth`) and `limit`
/// (`PAGE_LINES_DEFAULT` when left out, at most `PAGE_LINES_MAX`) are read
/// from the query.
pub(crate) async fn autorun_page(ctx: &BridgeContext, target: &str, n: SlotNo, owner: &str) -> (u16, String) {
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    let raw = q(target, "within").filter(|w| !w.trim().is_empty());
    let within = match within_target_nth(raw.as_deref(), q(target, "nth").as_deref()) {
        Ok(t) => t,
        Err(why) => return (400, why),
    };
    let query = q(target, "query").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let limit = page_limit(q(target, "limit").as_deref());
    let read = supervised_page_read_in(n, &ctx.org, &ctx.project, limit, within.as_ref(), query.as_deref());
    // The whole page: a landmark this reader already has is one line, before
    // the limit cuts the page (`page_compact`), unless `page=full`. A part of
    // the page, or the lines of a query, are answered as they are.
    if within.is_none() && query.is_none() {
        let full = q(target, "page").is_some_and(|p| p.trim().eq_ignore_ascii_case("full"));
        return Box::pin(compacted(n, owner, full, read)).await;
    }
    read.await
}

/// The line limit a page read was asked for: `PAGE_LINES_DEFAULT` when
/// none (or none that is a positive number) is given, and never more than
/// `PAGE_LINES_MAX`. Capped as well as defaulted: a snapshot is text an
/// assistant has to read, and a limit of a million turns "see the page"
/// into a reply nothing can use.
pub fn page_limit(asked: Option<&str>) -> usize {
    asked
        .and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(crate::browser::snapshot::PAGE_LINES_DEFAULT)
        .min(crate::browser::snapshot::PAGE_LINES_MAX)
}

/// A `within` locator from its text: JSON (an object, a chain, or a quoted
/// string), or a bare css selector. Checked as a script's locator is. A
/// css string, bare or quoted, becomes a structured css link, never a
/// legacy string target: a legacy target resolves to its first match,
/// hidden or not, so "within matched 3" could never be said of it, and a
/// hidden template could be listed in place of the dialog a person sees.
/// A structured css link counts every visible match. A `text=` string
/// becomes a text link the same way.
pub fn within_target(raw: &str) -> Result<crate::browser::locator::Target, String> {
    use crate::browser::locator::{LocatorStep, Target};
    let t = raw.trim();
    let target = if t.starts_with('{') || t.starts_with('[') || t.starts_with('"') {
        serde_json::from_str::<Target>(t).map_err(|e| format!("within is not a locator: {e}"))?
    } else {
        Target::Legacy(t.to_string())
    };
    // A `text=` string is the legacy text form: a text link, which counts
    // its visible matches too.
    let target = match target {
        Target::Legacy(s) => match s.strip_prefix("text=") {
            Some(words) => Target::One(LocatorStep { text: Some(words.trim().to_string()), ..LocatorStep::default() }),
            None => Target::One(LocatorStep { css: Some(s), ..LocatorStep::default() }),
        },
        structured => structured,
    };
    target.validate().map_err(|e| format!("within is not a locator: {e}"))?;
    Ok(target)
}

/// Said to an `nth` sent with no `within` for it to pick from.
pub const NTH_NEEDS_WITHIN: &str = "nth picks one of the elements within matches: send within with it";

/// A `within` locator from its text (`within_target`), with `nth`, when
/// given, picking one of its matches: counted from 0 as a locator's `nth`
/// is, and put on its last link, so a chain picks among its final matches.
/// `None` with no `within`. Refused: an `nth` with no `within`, one that is
/// not a count from 0, and one the `within` already gives.
pub fn within_target_nth(
    raw: Option<&str>,
    nth: Option<&str>,
) -> Result<Option<crate::browser::locator::Target>, String> {
    use crate::browser::locator::Target;
    let nth = match nth.map(str::trim).filter(|n| !n.is_empty()) {
        None => None,
        Some(n) => Some(
            n.parse::<i32>()
                .ok()
                .filter(|n| *n >= 0)
                .ok_or_else(|| format!("nth is a whole number counting from 0, not {n}"))?,
        ),
    };
    let Some(raw) = raw else {
        return match nth {
            Some(_) => Err(NTH_NEEDS_WITHIN.to_string()),
            None => Ok(None),
        };
    };
    let target = within_target(raw)?;
    let Some(n) = nth else {
        return Ok(Some(target));
    };
    let mut links = match target {
        Target::One(step) => vec![step],
        Target::Chain(steps) => steps,
        // `within_target` makes a css or text link of every string.
        Target::Legacy(_) => target.links(),
    };
    let last = links.last_mut().ok_or_else(|| "within is not a locator: a locator list is empty".to_string())?;
    if last.nth.is_some() {
        return Err("within gives an nth already: send nth once, in within or beside it".to_string());
    }
    last.nth = Some(n);
    Ok(Some(if links.len() == 1 { Target::One(links.remove(0)) } else { Target::Chain(links) }))
}

/// The supervised browser's page as text, `limit` lines at most: what
/// `/autorun-page` answers, and what a replay that reached its step hands
/// back beside its sentence. What it printed is filed in the project's
/// discovery map.
pub async fn supervised_page(organization: &str, project: &str, limit: usize) -> (u16, String) {
    supervised_page_in(SlotNo::MAIN, organization, project, limit).await
}

/// [`supervised_page`] in slot `slot`'s browser.
pub async fn supervised_page_in(slot: SlotNo, organization: &str, project: &str, limit: usize) -> (u16, String) {
    supervised_page_within_in(slot, organization, project, limit, None).await
}

/// [`supervised_page`], listing only what is inside the one element
/// `within` matches when it is given (`page_read_in_within`).
pub async fn supervised_page_within(
    organization: &str,
    project: &str,
    limit: usize,
    within: Option<&crate::browser::locator::Target>,
) -> (u16, String) {
    supervised_page_within_in(SlotNo::MAIN, organization, project, limit, within).await
}

/// [`supervised_page_within`] in slot `n`'s browser.
pub async fn supervised_page_within_in(
    n: SlotNo,
    organization: &str,
    project: &str,
    limit: usize,
    within: Option<&crate::browser::locator::Target>,
) -> (u16, String) {
    supervised_page_read_in(n, organization, project, limit, within, None).await
}

/// `get_autorun_page`'s read in slot `n`'s browser. A `within` that matches
/// several elements lists the first and says how many there were
/// (`first_of_many`). With a `query` (the lines of the whole page, or of the
/// `within` element, that contain it, case ignored, each with the lines above
/// it that it sits under) the page is read whole, nothing of it is recorded
/// as seen (the answer says so), and `limit` cuts the matching lines.
pub(crate) async fn supervised_page_read_in(
    n: SlotNo,
    organization: &str,
    project: &str,
    limit: usize,
    within: Option<&crate::browser::locator::Target>,
    query: Option<&str>,
) -> (u16, String) {
    if crate::autorun::replay_to::state(n).is_running() {
        return (409, replay_going_for_a_page_read(n));
    }
    // The lock is held for exactly one protocol job - whoever holds it
    // holds the browser, and the person may be using it.
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    let Some(session) = slot.as_mut() else {
        return (409, NO_SUPERVISED_BROWSER.to_string());
    };
    let (within, many) = first_of_many(&mut session.cdp, within).await;
    let root = if project.trim().is_empty() { None } else { crate::autorun::store::configured_root() };
    let tabs_case = session.tabs_case;
    let answer = match query {
        Some(query) => {
            let (status, text, _) =
                read_and_file_within(&mut session.cdp, crate::browser::snapshot::PAGE_LINES_MAX, within.as_ref(), None).await;
            if status == 200 { (status, query_lines(&text, query, limit)) } else { (status, text) }
        }
        None => {
            page_read_in_within(session, root.as_deref(), organization, project, tabs_case, limit, within.as_ref()).await
        }
    };
    let answer = match many {
        Some(said) if answer.0 == 200 => (answer.0, format!("{said}\n{}", answer.1)),
        _ => answer,
    };
    // A browser that has gone is let go here, not kept to answer 503 again.
    let answer = let_go_if_silent(&mut slot, answer).await;
    crate::commands::autorun::publish_discovery_in(n, &slot);
    answer
}

/// A `within` that matches several elements, as the first of them (`nth` 0)
/// and the sentence that says how many matched. A `within` that already has
/// an `nth`, matches one or none, or cannot be counted is returned as it is.
pub async fn first_of_many<D: crate::browser::cdp::Driver>(
    d: &mut D,
    within: Option<&crate::browser::locator::Target>,
) -> (Option<crate::browser::locator::Target>, Option<String>) {
    use crate::browser::locator::Target;
    let Some(target) = within else { return (None, None) };
    let mut links = target.links();
    if links.is_empty() || links.last().is_some_and(|l| l.nth.is_some()) {
        return (Some(target.clone()), None);
    }
    let count = match crate::browser::locator::resolve_explained(d, target).await {
        Ok(found) => found.handles.len(),
        Err(_) => return (Some(target.clone()), None),
    };
    if count < 2 {
        return (Some(target.clone()), None);
    }
    if let Some(last) = links.last_mut() {
        last.nth = Some(0);
    }
    let first = if links.len() == 1 { Target::One(links.remove(0)) } else { Target::Chain(links) };
    (
        Some(first),
        Some(format!("within matched {count} elements: this lists the first (nth 0) - send nth to list another.")),
    )
}

/// The lines of a page read (`text`, one line each, the indent showing the
/// depth) that contain `query` (case ignored), each with the lines above it
/// that it sits under, at most `limit` of them in all. Said when nothing
/// matches, and that nothing was recorded as seen.
pub fn query_lines(text: &str, query: &str, limit: usize) -> String {
    let want = query.trim().to_lowercase();
    let lines: Vec<&str> = text.lines().collect();
    let depth = |l: &str| l.len() - l.trim_start().len();
    let mut keep = vec![false; lines.len()];
    let mut matched = 0usize;
    for (i, line) in lines.iter().enumerate() {
        if !line.to_lowercase().contains(&want) {
            continue;
        }
        matched += 1;
        keep[i] = true;
        let mut need = depth(line);
        for j in (0..i).rev() {
            if need == 0 {
                break;
            }
            if depth(lines[j]) < need {
                keep[j] = true;
                need = depth(lines[j]);
            }
        }
    }
    let said = "Recorded nothing as seen: a read with a query lists only the matching lines.";
    if matched == 0 {
        return format!("no line of the page contains \"{}\"\n{said}", query.trim());
    }
    let shown: Vec<&str> = lines.iter().zip(&keep).filter(|(_, k)| **k).map(|(l, _)| *l).collect();
    let total = shown.len();
    let mut out: Vec<String> = shown.iter().take(limit).map(|l| (*l).to_string()).collect();
    if total > limit {
        out.push(format!("{} more lines - call get_autorun_page with a larger limit or a narrower query", total - limit));
    }
    out.push(format!("{matched} lines contain \"{}\". {said}", query.trim()));
    out.join("\n")
}

/// Said at the end of a page read during a discovery that has no area yet:
/// nothing was recorded.
pub const READ_WITH_NO_AREA: &str = "Recorded nothing as seen: this discovery has no area yet. Save the area with save_autorun_area, or name it with \"area\" on discover_autorun_actions, then read the page again.";

/// Said at the end of a page read during a discovery when the page is not
/// on the application's own origins: nothing was recorded.
pub const READ_OFF_THE_APP: &str =
    "Recorded nothing as seen: this page is not on one of this project's allowed origins.";

/// Said at the end of a page read during a discovery when what it showed
/// could not be filed (the reason is in the log).
pub const READ_NOT_FILED: &str =
    "Recorded nothing as seen: the discovery map could not be written (see Settings -> Logs).";

/// The line a page read during a discovery ends with when it recorded.
pub fn recorded_on(n: usize, area: &str) -> String {
    let what = if n == 1 { "element" } else { "elements" };
    format!("Recorded {n} {what} as seen on {}.", area.trim())
}

/// The page in `browser` as text, `limit` lines at most: what
/// `get_autorun_page` answers. `root` is where the discovery map lives,
/// `None` when there is nowhere to file (no project, no data folder).
///
/// During a discovery a read of the page counts exactly as an action's
/// read of it does (the same page text, filed by the same `read_and_file`),
/// so a read of the whole page explores it. It is filed under the
/// discovery's current area, and the answer ends with one line saying how
/// many elements were recorded as seen there. A discovery with no area yet
/// records nothing and says so (`READ_WITH_NO_AREA`): the case's area or
/// the unattributed bucket would file it where the discovery is not.
///
/// Outside a discovery the read is filed as it always was
/// (`discovery_sighting`: the area of the case the browser last ran, else
/// the unattributed bucket), never explores anything, and the answer is the
/// page text alone.
pub async fn page_read_in<B: DiscoveryBrowser>(
    browser: &mut B,
    root: Option<&std::path::Path>,
    organization: &str,
    project: &str,
    tabs_case: Option<i32>,
    limit: usize,
) -> (u16, String) {
    page_read_in_within(browser, root, organization, project, tabs_case, limit, None).await
}

/// [`page_read_in`], listing only the subtree of the one element `within`
/// matches when it is given, refused (400, naming the count) when it
/// matches none or several. Such a read is filed as one the limit cut
/// short (`read_and_file_within`): it never explores the page.
#[allow(clippy::too_many_arguments)]
pub async fn page_read_in_within<B: DiscoveryBrowser>(
    browser: &mut B,
    root: Option<&std::path::Path>,
    organization: &str,
    project: &str,
    tabs_case: Option<i32>,
    limit: usize,
    within: Option<&crate::browser::locator::Target>,
) -> (u16, String) {
    let p = browser.parts();
    // An explore session's read is the page text alone: nothing is filed
    // and no line about the map is added.
    if p.discovery.as_ref().is_some_and(|s| s.explore.is_some()) {
        let (status, text, _) = read_and_file_within(p.driver, limit, within, None).await;
        return (status, text);
    }
    let discovery_area = p.discovery.as_ref().map(|s| named(s.area.as_deref()));
    let at = root.and_then(|root| {
        discovery_sighting(root, organization, project, p.discovery.as_ref(), tabs_case, p.signed_in.as_deref())
    });
    let Some(area) = discovery_area else {
        let (status, text, _) = read_and_file_within(p.driver, limit, within, at.as_ref()).await;
        return (status, text);
    };
    let Some(area) = area else {
        let (status, text, _) = read_and_file_within(p.driver, limit, within, None).await;
        return if status == 200 { (status, format!("{text}\n\n{READ_WITH_NO_AREA}")) } else { (status, text) };
    };
    let (status, text, filed) = read_and_file_within(p.driver, limit, within, at.as_ref()).await;
    if status != 200 {
        return (status, text);
    }
    // Named as the map files it.
    let area = match root {
        Some(root) => crate::autorun::discovery_map::canonical_area(root, organization, project, &area),
        None => area,
    };
    let line = match filed {
        Filed::Recorded(n) => recorded_on(n, &area),
        Filed::OffOrigin => READ_OFF_THE_APP.to_string(),
        Filed::Not | Filed::Unrecorded => READ_NOT_FILED.to_string(),
    };
    (status, format!("{text}\n\n{line}"))
}

/// Where what the live page shows is filed in the discovery map
/// (`autorun::discovery_map`): the project, the area it belongs to (`None`
/// is the unattributed bucket), and whether a discovery is under way - in
/// which case a page read stamps the area explored by `account`, an account
/// KEY, never a login, and replaces what the map held for that page.
pub struct Sighting {
    pub root: std::path::PathBuf,
    pub org: String,
    pub project: String,
    pub area: Option<String>,
    pub account: Option<String>,
    /// When the discovery under way started, or `None` outside one.
    pub discovering: Option<u64>,
    /// The application's own origins, as a discovery's `navigate` is held
    /// to them (`runner::policy_for` the project's recipe): a page read
    /// files nothing from a page this does not allow.
    pub policy: crate::browser::actions::Policy,
}

/// The origins a page read may file from: those of the project's recipe in
/// the active environment, as an action's address is checked; any, with no
/// recipe (`runner::policy_for(None)`, as for actions); none, when the
/// recipe cannot be read, so an unreadable recipe never widens what counts.
pub fn recording_policy(root: &std::path::Path, organization: &str, project: &str) -> crate::browser::actions::Policy {
    match crate::autorun::recipe::load_effective_recipe_if_any(root, organization, project) {
        Ok(recipe) => crate::autorun::runner::policy_for(recipe.as_ref()),
        Err(why) => {
            // The error itself can quote the file (an address in it), so
            // only its kind is logged.
            let kind = if why.starts_with("the sign-in recipe is not readable") { "not a valid recipe" } else { "a file error" };
            unrecorded(&format!("the sign-in recipe could not be read ({kind}), so no page read is filed"));
            crate::browser::actions::Policy::only(vec![])
        }
    }
}

/// The area a recording belongs to: the discovery's own area, else the area
/// the saved script of `case_id` names, else none (the unattributed bucket).
/// A blank name counts as none.
pub fn recording_area(root: &std::path::Path, discovery_area: Option<&str>, case_id: Option<i32>) -> Option<String> {
    let named = |a: &str| {
        let a = a.trim();
        (!a.is_empty()).then(|| a.to_string())
    };
    discovery_area.and_then(named).or_else(|| {
        let script = crate::autorun::store::load_script(root, case_id?).ok().flatten()?;
        script.area.as_deref().and_then(named)
    })
}

/// Where the supervised browser's sightings go, or `None` when there is
/// nowhere to file them (no project chosen, no data directory).
pub(crate) fn supervised_sighting(
    session: &crate::commands::autorun::Session,
    organization: &str,
    project: &str,
) -> Option<Sighting> {
    if project.trim().is_empty() {
        return None;
    }
    let root = crate::autorun::store::configured_root()?;
    discovery_sighting(
        &root,
        organization,
        project,
        session.discovery.as_ref(),
        session.tabs_case,
        session.account.as_deref(),
    )
}

/// Where a browser's sightings go under `root`: the discovery's own area
/// and account KEY while one is going (`discovery`), else the area of the
/// case the browser last ran (`tabs_case`) and the account it signed in as
/// (`signed_in`, a key too). `None` when no project is chosen.
pub fn discovery_sighting(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    discovery: Option<&crate::commands::autorun::DiscoveryState>,
    tabs_case: Option<i32>,
    signed_in: Option<&str>,
) -> Option<Sighting> {
    if project.trim().is_empty() {
        return None;
    }
    // An explore session files nothing in the discovery map.
    if discovery.is_some_and(|s| s.explore.is_some()) {
        return None;
    }
    Some(Sighting {
        area: recording_area(root, discovery.and_then(|s| s.area.as_deref()), tabs_case),
        account: discovery.and_then(|s| s.account.clone()).or_else(|| signed_in.map(str::to_string)),
        discovering: discovery.map(|s| s.started_at),
        policy: recording_policy(root, organization, project),
        root: root.to_path_buf(),
        org: organization.to_string(),
        project: project.to_string(),
    })
}

/// The page the browser is on: its address's path (no host, query or
/// fragment; empty when it cannot be read) and its title. The address is
/// read as `nav::go_to_module` reads it to compare with `arrived`.
pub async fn current_page<D: crate::browser::cdp::Driver>(d: &mut D) -> (String, String) {
    let (href, title) = page_address(d).await;
    let path = if href.trim().is_empty() { String::new() } else { crate::autorun::discovery_map::path_only(&href) };
    (path, title)
}

/// The page's full address (empty when it cannot be read) and its title.
/// The address is only checked against the allowed origins and cut to its
/// path: it is never logged or answered whole.
pub(crate) async fn page_address<D: crate::browser::cdp::Driver>(d: &mut D) -> (String, String) {
    let href = crate::browser::page::eval_value(d, "location.href").await;
    let href = href.ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    let title = crate::browser::page::eval_value(d, "document.title").await;
    let title = title.ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    (href, title)
}

/// A recording that could not be written is said in the log and never
/// fails the route that made it.
pub(crate) fn unrecorded(why: &str) {
    crate::applog::warn(format!("Discovery map: what the page showed could not be recorded: {why}"));
}

/// The page as text, `limit` lines at most, with every locator it printed
/// filed at `at`. The text is the same with or without `at`.
pub async fn read_page<D: crate::browser::cdp::Driver>(
    d: &mut D,
    limit: usize,
    at: Option<&Sighting>,
) -> (u16, String) {
    let (status, text, _) = read_and_file(d, limit, at).await;
    (status, text)
}

/// What a page read filed in the discovery map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filed {
    /// Nowhere to file it (`at` was none), or the read failed.
    Not,
    /// This many distinct elements recorded as seen.
    Recorded(usize),
    /// The page is not on the application's own origins (`Sighting::policy`).
    OffOrigin,
    /// It could not be written; the log says why.
    Unrecorded,
}

/// `read_page`, saying what it filed. The single path every page read
/// records through: only the lines the read returned are filed, a page the
/// application's origins do not allow files nothing, and a read the line
/// limit cut short files what it returned without exploring the page
/// (`discovery_map::record_read`).
pub async fn read_and_file<D: crate::browser::cdp::Driver>(
    d: &mut D,
    limit: usize,
    at: Option<&Sighting>,
) -> (u16, String, Filed) {
    read_and_file_within(d, limit, None, at).await
}

/// [`read_and_file`], listing only the subtree of the one element `within`
/// matches when it is given (`snapshot::within_node`): 400 with the count
/// when it matches none or several, and nothing filed. Each listed line is
/// counted against the whole page when every frame of it was read
/// (`discovery_map::record_read_counted`); a `within` read is never whole.
pub async fn read_and_file_within<D: crate::browser::cdp::Driver>(
    d: &mut D,
    limit: usize,
    within: Option<&crate::browser::locator::Target>,
    at: Option<&Sighting>,
) -> (u16, String, Filed) {
    let node = match within {
        None => None,
        Some(t) => match crate::browser::snapshot::within_node(d, t).await {
            Ok(Ok(node)) => Some(node),
            Ok(Err(why)) => return (400, why, Filed::Not),
            Err(e) => return (503, format!("the browser did not answer: {e}"), Filed::Not),
        },
    };
    let read = match crate::browser::snapshot::snapshot_read_within(d, limit, node).await {
        Ok(read) => read,
        Err(e) => return (503, format!("the browser did not answer: {e}"), Filed::Not),
    };
    let Some(at) = at else {
        return (200, read.text, Filed::Not);
    };
    let (href, title) = page_address(d).await;
    if href.trim().is_empty() {
        unrecorded("the page's address could not be read");
        return (200, read.text, Filed::Unrecorded);
    }
    if !may_record(&href, &at.policy) {
        crate::applog::info(format!(
            "Discovery map: a page read was not filed, as the page is not on {}",
            crate::browser::actions::ALLOWED_ORIGINS
        ));
        return (200, read.text, Filed::OffOrigin);
    }
    let recorded = crate::autorun::discovery_map::record_read_counted(
        &at.root,
        &at.org,
        &at.project,
        at.area.as_deref(),
        &crate::autorun::discovery_map::path_only(&href),
        &title,
        &read.lines,
        read.page_lines.as_deref(),
        at.account.as_deref(),
        at.discovering,
        !read.cut,
        crate::autorun::sessions::now_ms(),
    );
    match recorded {
        Ok(n) => (200, read.text, Filed::Recorded(n)),
        Err(why) => {
            unrecorded(&why);
            (200, read.text, Filed::Unrecorded)
        }
    }
}

/// Can a page at `href` (its full address) be filed as seen: only a page
/// of the application counts. An address that is not an http, https or
/// file page (about:blank, a browser error page) never is, even when no
/// recipe limits the origins; one `policy` does not allow never is either.
pub fn may_record(href: &str, policy: &crate::browser::actions::Policy) -> bool {
    crate::autorun::recipe::origin_of(href).is_some() && policy.allows(href)
}

/// One locator to file, with how many elements it matched and how many
/// with hidden ones included (`discovery_map::NOT_ONE` when not known).
pub(crate) type Matched<'a> = (&'a crate::browser::locator::Target, usize, usize);

/// Files each locator in `targets` at `at` under the page at `href` (its
/// full address, cut to its path here), with how many elements it matched,
/// only when `may_record` allows that page. Says whether every locator was
/// filed.
pub(crate) fn record_matched_targets(at: &Sighting, href: &str, targets: &[Matched]) -> bool {
    if targets.is_empty() || at.project.trim().is_empty() {
        return false;
    }
    if href.trim().is_empty() {
        unrecorded("the page's address could not be read");
        return false;
    }
    if !may_record(href, &at.policy) {
        crate::applog::info(format!(
            "Discovery map: a matched locator was not filed, as the page is not on {}",
            crate::browser::actions::ALLOWED_ORIGINS
        ));
        return false;
    }
    let path = crate::autorun::discovery_map::path_only(href);
    let now = crate::autorun::sessions::now_ms();
    let mut filed = true;
    for (target, matched, all) in targets {
        let area = at.area.as_deref();
        if let Err(why) = crate::autorun::discovery_map::record_matched_counts(
            &at.root, &at.org, &at.project, area, &path, target, *matched, *all, now,
        ) {
            unrecorded(&why);
            filed = false;
        }
    }
    filed
}

/// What a step's components found on the page: the locator of each
/// `wait_for` and `expect_text` a `use_component` ran that passed, with
/// how many elements it is filed as matching, as a probe would file it. An
/// `expect_text` reads one element and fails on several, so one (a plain
/// css link's hidden matches, and a legacy string's count, are not known:
/// `NOT_ONE`); a `wait_for` passes on one or several, so not known. What
/// it waited for may be gone a moment later (a toast), so this is filed
/// from the passed action itself, never a probe after it. `expanded` is
/// the step's actions as the runner ran them (`components::expand_step`),
/// paired by position with `outcomes`; only an outcome a component gave,
/// beside an action of a component, counts.
pub fn component_finds(
    expanded: &[(crate::browser::actions::Action, Option<String>)],
    outcomes: &[crate::browser::actions::ActionOutcome],
) -> Vec<(crate::browser::locator::Target, usize, usize)> {
    use crate::autorun::discovery_map::{plain_target, NOT_ONE};
    use crate::browser::actions::Action;
    let unknown = NOT_ONE as usize;
    expanded
        .iter()
        .zip(outcomes)
        .filter(|((_, from), o)| from.is_some() && o.component.is_some() && o.ok)
        .filter_map(|((action, _), _)| match action {
            Action::WaitFor { selector, .. } => Some((selector.clone(), unknown, unknown)),
            Action::ExpectText { selector, .. } | Action::ExpectTextMoved { selector, .. } => {
                let css = plain_target(selector) && selector.links().first().is_some_and(|l| l.css.is_some());
                Some(if selector.is_legacy() {
                    (selector.clone(), unknown, unknown)
                } else if css {
                    (selector.clone(), 1, unknown)
                } else {
                    (selector.clone(), 1, 1)
                })
            }
            _ => None,
        })
        .collect()
}

/// What a guard (`when_visible`, `when_hidden`) that acted and worked found on
/// the page: each locator of its `then` actions, filed as a worked try files
/// it (matched one, `matched_targets`). A guard that did not act (its target
/// showed up for a `when_hidden`, never did for a `when_visible`) or whose
/// `then` failed at any action files nothing: its outcome is not ok then, or
/// its `GuardTrace` says it did not act. A guard inside a guard's `then` is
/// read one level down, from the trace's `inner` in the order those guards
/// stand in the `then`: its own `then` is filed only when that inner guard
/// acted. A legacy string target is left out, as a try probes it first.
/// `expanded` and `outcomes` pair as in `component_finds`.
pub fn guard_finds(
    expanded: &[(crate::browser::actions::Action, Option<String>)],
    outcomes: &[crate::browser::actions::ActionOutcome],
) -> Vec<(crate::browser::locator::Target, usize, usize)> {
    use crate::autorun::discovery_map::NOT_ONE;
    use crate::browser::actions::{Action, GuardTrace};
    let unknown = NOT_ONE as usize;
    // The targets a guard's `then` proved, when `trace` says it acted.
    fn proved<'a>(
        then: &'a [Action],
        trace: &GuardTrace,
        unknown: usize,
        out: &mut Vec<(&'a crate::browser::locator::Target, usize, usize)>,
    ) {
        use crate::autorun::discovery_map::plain_target;
        if !trace.acted {
            return;
        }
        // The traces of the guards in this `then` are kept in order.
        let mut inner = trace.inner.iter();
        for a in then {
            if a.is_guard() {
                if let (Some(t), Some((_, _, inner_then))) = (inner.next(), a.guard_parts()) {
                    proved(inner_then, t, unknown, out);
                }
                continue;
            }
            match a {
                // It passes on one match or several and does not say
                // which: filed as found, its count not known.
                Action::WaitFor { selector, .. } => out.push((selector, unknown, unknown)),
                _ => out.extend(matched_targets(a).into_iter().filter(|(t, _)| !t.is_legacy()).map(|(t, n)| {
                    let css = plain_target(t) && t.links().first().is_some_and(|l| l.css.is_some());
                    (t, n, if css { unknown } else { n })
                })),
            }
        }
    }
    let mut found = Vec::new();
    for ((action, _), o) in expanded.iter().zip(outcomes) {
        let (Some((_, _, then)), Some(trace)) = (action.guard_parts(), &o.guard) else { continue };
        if o.ok {
            proved(then, trace, unknown, &mut found);
        }
    }
    found
        .into_iter()
        // A locator holding a data placeholder is not what the page showed.
        .filter(|(t, _, _)| crate::autorun::seen_check::fire_can_file(t, true))
        .map(|(t, n, all)| (t.clone(), n, all))
        .collect()
}

/// Files `component_finds` and `guard_finds` of a step that ran `actions`
/// with `outcomes`, at the page the browser is on now, under `area`: a
/// try's, a discovery's or a replay's. Nothing is filed for a step that
/// uses no component and no guard, and a map that could not be written is
/// said in the log.
#[allow(clippy::too_many_arguments)]
pub async fn file_component_finds<D: crate::browser::cdp::Driver>(
    d: &mut D,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    area: Option<String>,
    actions: &[crate::browser::actions::Action],
    outcomes: &[crate::browser::actions::ActionOutcome],
) {
    file_step_finds(d, root, organization, project, area, actions, outcomes, true).await;
}

/// Files only the `guard_finds` of a step a run ran: a guard that fired and
/// worked has proved its `then` targets were there.
#[allow(clippy::too_many_arguments)]
pub async fn file_guard_finds<D: crate::browser::cdp::Driver>(
    d: &mut D,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    area: Option<String>,
    actions: &[crate::browser::actions::Action],
    outcomes: &[crate::browser::actions::ActionOutcome],
) {
    file_step_finds(d, root, organization, project, area, actions, outcomes, false).await;
}

/// Takes off the saved script's `unfired_guards` each target a guard of `step` just proved: the
/// ones `file_guard_finds` files (`guard_finds`, under the same conditions - a project and an
/// area). The script is read again and saved through the store, which removes only these
/// entries from what is on disk now. A failure to save is said in the log only.
pub fn forget_fired_guards(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    script: &crate::autorun::CaseScript,
    step: &crate::autorun::StepScript,
    outcomes: &[crate::browser::actions::ActionOutcome],
) {
    let has_area = script.area.as_deref().is_some_and(|a| !a.trim().is_empty());
    if script.unfired_guards.is_empty() || !has_area || project.trim().is_empty() {
        return;
    }
    let Ok((expanded, _)) = crate::autorun::components::expand_step(root, organization, project, &step.actions) else { return };
    let entries: Vec<String> = guard_finds(&expanded, outcomes)
        .iter()
        .map(|(t, _, _)| crate::autorun::seen_check::unfired_entry(step.step_number, &t.describe()))
        .filter(|e| script.unfired_guards.contains(e))
        .collect();
    if entries.is_empty() {
        return;
    }
    if let Err(why) = crate::autorun::store::remove_unfired_guards(root, script.case_id, &entries) {
        crate::applog::warn(format!("Auto Run: case {} could not drop a clean-up guard that fired from its script: {why}", script.case_id));
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn file_step_finds<D: crate::browser::cdp::Driver>(
    d: &mut D,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    area: Option<String>,
    actions: &[crate::browser::actions::Action],
    outcomes: &[crate::browser::actions::ActionOutcome],
    components: bool,
) {
    use crate::browser::actions::Action;
    let uses = actions.iter().any(|a| matches!(a, Action::UseComponent { .. } | Action::WhenVisible { .. } | Action::WhenHidden { .. }));
    if !uses || project.trim().is_empty() {
        return;
    }
    let Ok((expanded, _)) = crate::autorun::components::expand_step(root, organization, project, actions) else { return };
    let mut finds = if components { component_finds(&expanded, outcomes) } else { Vec::new() };
    // A guard's finds are filed under the script's area: with none, they
    // would count for every area, so nothing is filed.
    if area.as_deref().is_some_and(|a| !a.trim().is_empty()) {
        finds.extend(guard_finds(&expanded, outcomes));
    }
    if finds.is_empty() {
        return;
    }
    let at = Sighting {
        root: root.to_path_buf(),
        org: organization.to_string(),
        project: project.to_string(),
        area,
        account: None,
        discovering: None,
        policy: recording_policy(root, organization, project),
    };
    let (href, _) = page_address(d).await;
    let matched: Vec<Matched> = finds.iter().map(|(t, n, all)| (t, *n, *all)).collect();
    record_matched_targets(&at, &href, &matched);
}

/// The locators an action that WORKED must have matched at least once,
/// each with how many elements it matched. Kinds that pass while matching
/// nothing give none: `expect_hidden`, an `expect_count` of 0,
/// `expect_no_row`, a `expect_row_count` with no positive bound, and
/// `when_visible`, whose outcome does not say whether its selector was
/// there. A click, a fill and the checks that read one element fail on
/// several, so theirs matched one; an `expect_count` matched the number it
/// counted. A `wait_for` passes on one or more and its outcome does not
/// say how many, so it is left out here and probed once the try worked
/// (`try_action`), as is any legacy string target: a run takes its first
/// match, so it passes on several too, and so are the list of an
/// `expect_each` or `expect_order` and an `expect_each`'s child chain
/// (`list_targets`, `list_chain_of`), whose outcome does not say how many
/// items it read. Matched without a catch-all, so a
/// new kind will not compile until it is classified here.
pub(crate) fn matched_targets(action: &crate::browser::actions::Action) -> Vec<(&crate::browser::locator::Target, usize)> {
    use crate::browser::actions::Action;
    match action {
        Action::Click { selector }
        | Action::Hover { selector, .. }
        | Action::Fill { selector, .. }
        | Action::SelectOption { selector, .. }
        | Action::Upload { selector, .. }
        | Action::ExpectVisible { selector, .. }
        | Action::ExpectText { selector, .. }
        | Action::ExpectTextMoved { selector, .. }
        | Action::RememberText { selector, .. }
        | Action::ExpectContainsText { selector, .. }
        | Action::ExpectAttribute { selector, .. }
        | Action::ExpectStyle { selector, .. }
        | Action::ExpectEnabled { selector, .. }
        | Action::ExpectDisabled { selector, .. }
        | Action::ExpectChecked { selector, .. }
        | Action::ExpectFocused { selector, .. } => vec![(selector, 1)],
        // Only an exact count says how many elements it saw; a bound
        // (`at_least`, `at_most`) does not.
        Action::ExpectCount { selector, equals, .. } => match equals {
            Some(n) if *n >= 1 => vec![(selector, *n as usize)],
            _ => vec![],
        },
        Action::Drag { from, to, .. } => vec![(from, 1), (to, 1)],
        Action::ExpectPosition { selector, relative_to, .. } => vec![(selector, 1), (relative_to, 1)],
        // The clicked element, and the region when it names one: a pass
        // read each as one element.
        Action::ExpectNoChange { selector, region, .. } => {
            std::iter::once((selector, 1)).chain(region.as_ref().map(|r| (r, 1))).collect()
        }
        Action::ExpectRow { table, .. } | Action::ExpectSorted { table, .. } => vec![(table, 1)],
        Action::ExpectRowCount { table, equals, at_least, .. } => {
            if equals.is_some_and(|n| n >= 1) || at_least.is_some_and(|n| n >= 1) {
                vec![(table, 1)]
            } else {
                vec![]
            }
        }
        Action::ExpectHidden { .. }
        | Action::WaitFor { .. }
        | Action::ExpectEach { .. }
        | Action::ExpectOrder { .. }
        | Action::UseComponent { .. }
        | Action::ExpectNoRow { .. }
        | Action::WhenVisible { .. }
        | Action::WhenHidden { .. }
        | Action::RepeatWhileVisible { .. }
        | Action::Navigate { .. }
        | Action::CheckText { .. }
        | Action::CheckUrl { .. }
        | Action::RememberUrl { .. }
        | Action::SignIn { .. }
        | Action::ExpectResponse { .. }
        | Action::ApiRequest { .. }
        | Action::Reload
        | Action::ExpireSession
        | Action::ReturnToArea { .. }
        | Action::PressKey { .. }
        | Action::ExpectDownload { .. }
        | Action::ExpectTab { .. }
        | Action::OpenTab { .. }
        | Action::SwitchTab { .. }
        | Action::CloseTab { .. }
        | Action::ExpectTabClosed { .. }
        | Action::ExpectDialog { .. }
        // It may pass without ever clicking `next`, and says nothing of how
        // many elements an `until` locator matched on the page it was seen.
        | Action::PageUntil { .. } => vec![],
    }
}

/// The list an `expect_each` or `expect_order` reads: probed once the try
/// passed (`try_action`).
pub(crate) fn list_targets(action: &crate::browser::actions::Action) -> Vec<&crate::browser::locator::Target> {
    use crate::browser::actions::Action;
    match action {
        Action::ExpectEach { selector, .. } | Action::ExpectOrder { selector, .. } if !selector.is_legacy() => vec![selector],
        _ => vec![],
    }
}

/// An `expect_each`'s child as it is looked for inside the list: the list's
/// links and then its own, the chain the save needs seen whole.
pub(crate) fn list_chain_of(action: &crate::browser::actions::Action) -> Option<crate::browser::locator::Target> {
    match action {
        crate::browser::actions::Action::ExpectEach { selector, child: Some(child), .. } if !selector.is_legacy() => {
            Some(crate::browser::locator::Target::Chain(selector.links().into_iter().chain(child.links()).collect()))
        }
        _ => None,
    }
}

/// A probe's answer read back (`snapshot::probe`): how many elements
/// matched (`"matches: N ..."`), how many of them it listed (at most ten),
/// and how many of those listed are visible (a line `<tag> "<text>" visible
/// at ...`).
pub(crate) struct ProbeAnswer {
    pub(crate) matches: usize,
    pub(crate) listed: usize,
    pub(crate) visible: usize,
}

pub(crate) fn read_probe_answer(text: &str) -> ProbeAnswer {
    let matches = text
        .strip_prefix("matches: ")
        .map(|rest| rest.chars().take_while(char::is_ascii_digit).collect::<String>())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    // After the text's closing quote, which the text itself cannot follow;
    // the name the page read gives the element, after it, is cut off first.
    let shown = |line: &str| {
        let line = match line.rfind(crate::browser::snapshot::PAGE_READ_NAME) {
            Some(i) if line.ends_with('"') => &line[..i],
            _ => line,
        };
        line.rfind('"').map(|i| &line[i + 1..]).is_some_and(|rest| rest.starts_with(" visible "))
    };
    let listed = matches.min(10);
    let visible = text.lines().skip(1).take(listed).filter(|l| shown(l)).count();
    ProbeAnswer { matches, listed, visible }
}

/// How many visible elements a probe of a structured locator says
/// matched. `None` when none of them is visible, which files nothing. One
/// visible beside any number of hidden ones is one, as a run finds only
/// visible ones. When the probe matched more than it lists and one listed
/// is visible, the ones it left out may be visible too, so that is at
/// least two, never one.
pub fn visible_matches(text: &str) -> Option<usize> {
    let answer = read_probe_answer(text);
    if answer.visible == 0 {
        return None;
    }
    if answer.matches > answer.listed {
        return Some(answer.visible.max(2));
    }
    Some(answer.visible)
}

/// What a probe of `target` (its answer `text`) files the target as
/// matching: how many elements, and how many with hidden ones included.
/// `None` files nothing.
///
/// A legacy string target resolves to at most one element (`LEGACY_JS`
/// keeps the first css match or the last `text=` one), so its probe never
/// shows more than one: it is counted for real with `LEGACY_COUNT_JS`,
/// hidden ones included, as a run does not leave them out. It is filed
/// only when the element a run takes is visible.
///
/// A structured locator files its visible matches (`visible_matches`).
/// When it is one plain css link, whose key a legacy string shares, the
/// hidden ones are counted too by probing it with them included, so a
/// legacy use of that key is held to every match; any other files the
/// visible count for both.
pub(crate) async fn counted<D: crate::browser::cdp::Driver>(
    d: &mut D,
    target: &crate::browser::locator::Target,
    text: &str,
) -> Option<(usize, usize)> {
    use crate::autorun::discovery_map::{plain_target, NOT_ONE};
    if let crate::browser::locator::Target::Legacy(sel) = target {
        if read_probe_answer(text).visible == 0 {
            return None;
        }
        let all = crate::browser::locator::legacy_count(d, sel).await.ok().filter(|n| *n > 0)?;
        return Some((all, all));
    }
    let matched = visible_matches(text)?;
    let css = plain_target(target) && target.links().first().is_some_and(|l| l.css.is_some());
    if !css {
        return Some((matched, matched));
    }
    let all = match crate::browser::snapshot::probe_exact(d, &target.including_hidden()).await {
        Ok(t) => read_probe_answer(&t).matches.max(matched),
        Err(_) => NOT_ONE as usize,
    };
    Some((matched, all))
}

/// What a locator matches on the page, as the probe answers it; a locator
/// that matched at least one visible element is filed at `at`, with how
/// many visible ones it matched (`visible_matches`).
pub async fn probe_page<D: crate::browser::cdp::Driver>(
    d: &mut D,
    target: &crate::browser::locator::Target,
    at: Option<&Sighting>,
) -> (u16, String) {
    let text = match crate::browser::snapshot::probe(d, target).await {
        Ok(text) => text,
        Err(e) => return (503, format!("the browser did not answer: {e}")),
    };
    if let Some(at) = at {
        // Filed as a run takes the target (a plain string: its first match), not as shown.
        let as_run = match target {
            crate::browser::locator::Target::Legacy(_) => crate::browser::snapshot::probe_exact(d, target).await.unwrap_or_else(|_| text.clone()),
            _ => text.clone(),
        };
        match counted(d, target, &as_run).await {
            Some((matched, all)) => {
                let (href, _) = page_address(d).await;
                record_matched_targets(at, &href, &[(target, matched, all)]);
            }
            // Nothing seen: what no page read can print of it is taken off
            // this page (`discovery_map::record_matched_counts` with 0).
            None if read_probe_answer(&as_run).visible == 0 => {
                let (href, _) = page_address(d).await;
                record_matched_targets(at, &href, &[(target, 0, 0)]);
            }
            None => {}
        }
    }
    (200, text)
}

/// How a locator recorded on the page is named in a save's answer: as it
/// describes itself, with how many elements it matched when that was more
/// than one.
pub(crate) fn recorded_as(described: &str, matched: usize) -> String {
    if matched > 1 {
        format!("{described} (matched {matched} elements)")
    } else {
        described.to_string()
    }
}

/// Can a locator be probed as it is written: no placeholder (`{{...}}`)
/// and no link left for a component's target input?
pub(crate) fn probeable(target: &crate::browser::locator::Target) -> bool {
    let no_placeholder = serde_json::to_string(target).is_ok_and(|json| !json.contains("{{"));
    no_placeholder && target.links().iter().all(|l| l.input.is_none())
}

/// What a save refused only for unseen locators does while a discovery is
/// open: each refused locator in `targets` that can be probed as written
/// is probed on the discovery browser's current page, as
/// `probe_autorun_locator` probes it (it never clicks or types), and one
/// that matches one or more visible elements is recorded under the
/// discovery's current area, with how many it matched. Whether that is
/// enough for the save is the seen check's to say: a check that may match
/// several takes it, any other use needs one (`seen_check::takes_many`).
/// Hands back the locators recorded, as each describes itself, with
/// "(matched N elements)" after one that matched several; `None` when no
/// discovery is going, and then nothing is probed.
pub async fn record_refused_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    targets: &[crate::browser::locator::Target],
) -> Option<Vec<String>> {
    let browser = slot.as_mut()?;
    let p = browser.parts();
    let state = p.discovery.as_ref()?;
    let at = discovery_sighting(root, organization, project, Some(state), None, p.signed_in.as_deref());
    let mut probed: Vec<String> = Vec::new();
    let mut recorded: Vec<String> = Vec::new();
    // Counted, never named, in the log: a locator is the assistant's
    // writing and can hold an address.
    let mut unchecked = 0usize;
    for target in targets.iter().filter(|t| probeable(t)) {
        let described = target.describe();
        if probed.contains(&described) {
            continue;
        }
        probed.push(described.clone());
        let Ok(text) = crate::browser::snapshot::probe_exact(p.driver, target).await else {
            unchecked += 1;
            continue;
        };
        let Some((matched, all)) = counted(p.driver, target, &text).await else {
            continue;
        };
        let Some(at) = at.as_ref() else {
            continue;
        };
        let (href, _) = page_address(p.driver).await;
        if record_matched_targets(at, &href, &[(target, matched, all)]) {
            recorded.push(recorded_as(&described, matched));
        }
    }
    if unchecked > 0 {
        crate::applog::warn(format!("Auto Run save: {unchecked} refused locator(s) could not be checked on the page"));
    }
    if !recorded.is_empty() {
        crate::applog::info(format!(
            "Auto Run save: recorded {} of {} refused locator(s) on the current page",
            recorded.len(),
            targets.len()
        ));
    }
    Some(recorded)
}

/// [`record_refused_in`] for a script save, whose check reads each script's
/// own area: only when every one of `script_areas` is the discovery's
/// current area (trimmed, blank as none, compared as `nav::module_key`
/// compares names). Otherwise a sighting would be filed where the check
/// does not look, so nothing is probed and the answer is `None`.
pub async fn record_refused_for_scripts_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    script_areas: &[Option<&str>],
    targets: &[crate::browser::locator::Target],
) -> Option<Vec<String>> {
    let key = |a: Option<&str>| a.map(str::trim).filter(|a| !a.is_empty()).map(crate::autorun::nav::module_key);
    let here = key(slot.as_mut()?.parts().discovery.as_ref()?.area.as_deref());
    if script_areas.iter().any(|a| key(*a) != here) {
        return None;
    }
    record_refused_in(slot, root, organization, project, targets).await
}

/// [`record_refused_for_scripts_in`] in the supervised browser, holding its
/// lock as `probe_autorun_locator` does. `None` when an unattended run has
/// the browser, no discovery is going, or it is on another area.
pub(crate) async fn record_refused_on_page(
    ctx: &BridgeContext,
    root: &std::path::Path,
    script_areas: &[Option<&str>],
    targets: &[crate::browser::locator::Target],
    n: SlotNo,
) -> Option<Vec<String>> {
    if unattended_run_is_using_the_browser().is_some() || crate::autorun::replay_to::state(n).is_running() {
        return None;
    }
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    record_refused_for_scripts_in(&mut slot, root, &ctx.org, &ctx.project, script_areas, targets).await
}

/// A save's own answer, after the line that says what a refusal checked on
/// the page recorded.
pub fn after_recording(recorded: &[String], (status, text): (u16, String)) -> (u16, String) {
    let line = if recorded.is_empty() {
        "Recorded on the current page: nothing - no refused locator matched a visible element.".to_string()
    } else {
        format!("Recorded on the current page: {}.", recorded.join(", "))
    };
    (status, format!("{line}\n{text}"))
}

/// One named field out of a small JSON body, or a refusal that says what
/// the body should have carried.
pub(crate) fn body_field(body: &str, key: &str, shape: &str) -> Result<serde_json::Value, (u16, String)> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| (400, format!("that is not readable JSON: {e}. Expected {shape}.")))?;
    match v.get(key) {
        Some(found) if !found.is_null() => Ok(found.clone()),
        _ => Err((400, format!("this call needs a \"{key}\". Expected {shape}."))),
    }
}

/// What a probe body's `selector` may be.
pub(crate) const PROBE_SHAPE: &str = "{ \"selector\": <a script's target: a CSS string, one locator object, or a list of them (a chain, outermost first)> }";

/// The locator a probe names: a string, one locator object, or the chain
/// form scripts use, a list of links (outermost first). A string holding
/// such a list or object as JSON text - how a client whose schema allowed
/// only a string or an object sent a chain - is read as that list or
/// object, never as CSS: a selector that is valid JSON of either shape is
/// not valid CSS. `Err` is the refusal's sentence.
pub fn probe_target(selector: serde_json::Value) -> Result<crate::browser::locator::Target, String> {
    let selector = match selector {
        serde_json::Value::String(text) => match serde_json::from_str::<serde_json::Value>(text.trim()) {
            Ok(v @ (serde_json::Value::Array(_) | serde_json::Value::Object(_))) => v,
            _ => serde_json::Value::String(text),
        },
        other => other,
    };
    let target: crate::browser::locator::Target =
        serde_json::from_value(selector).map_err(|e| format!("that is not a locator: {e}"))?;
    target.validate()?;
    if target.links().iter().filter_map(|l| l.css.as_deref()).any(is_playwright_selector) {
        return Err(NOT_A_PLAYWRIGHT_SELECTOR.to_string());
    }
    Ok(target)
}

/// Said, with 400, to a probe whose CSS holds a Playwright pseudo-selector.
pub const NOT_A_PLAYWRIGHT_SELECTOR: &str =
    "that is a Playwright selector, which Auto Run does not take: use a CSS selector or a role and name (see get_guide topic autorun, Selectors).";

/// Is `css` written with a Playwright-only selector (`:text-is`,
/// `:has-text`, `:text(`, or the `>>` chain), which no browser's CSS takes?
pub(crate) fn is_playwright_selector(css: &str) -> bool {
    let lower = css.to_ascii_lowercase();
    [":text-is", ":has-text", ":text(", ">>"].iter().any(|p| lower.contains(p))
}

/// Said, with 409, to a page read, a probe, a try or the assistant's End
/// discovery while a replay holds the browser: it holds the session's lock
/// for its whole run, so waiting on it could outlast the call.
pub const REPLAY_GOING: &str =
    "a replay is going - call replay_autorun_to_step with the same case and step to wait for it";

/// Said, with 409, to a page read while a replay holds the browser. The
/// page is not read beside the replay: the replay holds the browser's one
/// connection for its whole run, and that connection is what answers the
/// page's dialogs and the requests a no-save guard pauses, so a second
/// reader would either wait out the replay or act behind it. The replay's
/// own call says where it is, and hands back the page once it stands
/// before its step. It is slot `n`'s replay that is going, and slot `n`'s
/// run that names the case and step.
pub fn replay_going_for_a_page_read(n: SlotNo) -> String {
    let call = call_the_replay_going(n);
    format!(
        "the page cannot be read while a replay drives the browser - {call}: it waits for the replay, says the step and the action it is on, and hands back the page once the browser stands before the step"
    )
}

/// How to wait on the replay going: the assistant's own names its case and
/// step; the person's, or one not known, says "the same case and step".
/// Read from the run kept in slot `n`, the one whose replay is going.
pub(crate) fn call_the_replay_going(n: SlotNo) -> String {
    let which = assistant_run(n).as_ref().filter(|r| r.answer.borrow().is_none()).map(|r| (r.case_id, r.step));
    match which {
        Some((case_id, step)) => format!("call replay_autorun_to_step with case {case_id} and step {step}"),
        None => "call replay_autorun_to_step with the same case and step".to_string(),
    }
}

/// Said, with 409, to the assistant's release while a replay holds the
/// browser: a release never stops a replay, so it says how to wait for it.
pub fn replay_going_for_a_release(n: SlotNo) -> String {
    let stop = if assistant_replay_going(n) { ", or replay_autorun_to_step with stop: true to stop it" } else { "" };
    format!(
        "a replay is going and holds the browser, and a release never stops one - {} to wait for it{stop}, then release",
        call_the_replay_going(n)
    )
}

/// Whether the replay going in slot `n` is the assistant's own: its call's
/// slot holds no answer yet. Only then does a refusal point at the replay
/// tool's `stop`, which never stops the person's.
pub fn assistant_replay_going(n: SlotNo) -> bool {
    assistant_run(n).as_ref().is_some_and(|r| r.answer.borrow().is_none())
}

/// The case and step, `(case_id, step)`, of the assistant's replay kept in
/// slot `n` - running, or ended with its answer not yet taken - if there is
/// one.
pub fn assistant_replay_of(n: SlotNo) -> Option<(i32, i32)> {
    assistant_run(n).as_ref().map(|r| (r.case_id, r.step))
}

/// `commands::autorun::REPLAY_RUNNING_AT_RESET`, as the assistant's reset,
/// cookie and header tools say it: naming the replay tool's `stop` when the
/// replay going is the assistant's own.
pub fn replay_running_refusal(n: SlotNo) -> String {
    let said = crate::commands::autorun::REPLAY_RUNNING_AT_RESET;
    if assistant_replay_going(n) {
        format!("{said} Or stop it first with replay_autorun_to_step and stop: true.")
    } else {
        said.to_string()
    }
}

/// `REPLAY_GOING` while a replay to a step holds slot `n`'s browser.
pub fn replay_is_holding_the_browser(n: SlotNo) -> Option<(u16, String)> {
    crate::autorun::replay_to::state(n).is_running().then(|| (409, REPLAY_GOING.to_string()))
}

/// What a locator matches on the open page right now.
pub(crate) async fn autorun_probe(ctx: &BridgeContext, body: &str, n: SlotNo) -> (u16, String) {
    let selector = match body_field(body, "selector", PROBE_SHAPE) {
        Ok(v) => v,
        Err(refused) => return refused,
    };
    let target = match probe_target(selector) {
        Ok(t) => t,
        Err(why) => return (400, why),
    };
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    if let Some(going) = replay_is_holding_the_browser(n) {
        return going;
    }
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    let Some(session) = slot.as_mut() else {
        return (409, NO_SUPERVISED_BROWSER.to_string());
    };
    let at = supervised_sighting(session, &ctx.org, &ctx.project);
    let answer = probe_page(&mut session.cdp, &target, at.as_ref()).await;
    let answer = let_go_if_silent(&mut slot, answer).await;
    crate::commands::autorun::publish_discovery_in(n, &slot);
    answer
}

/// What an inspect body carries.
pub(crate) const INSPECT_SHAPE: &str = "{ \"locator\": <a script's target: a CSS string, one locator object, or a list of them (a chain, outermost first)>, \"show\"?: [\"styles\", \"listeners\", \"animations\", \"structure\"], \"properties\"?: [<CSS property names>] }";

/// Said to an inspect whose locator still waits for a value.
pub const INSPECT_PLACEHOLDER: &str =
    "a locator with a placeholder ({{...}}) or a component input cannot be inspected as written - put in the value it stands for";

/// The locator and what to show, out of an inspect's body: everything that
/// can be refused without the browser.
pub(crate) fn inspect_ask(body: &str) -> Result<(crate::browser::locator::Target, crate::browser::inspect::Ask), (u16, String)> {
    let locator = body_field(body, "locator", INSPECT_SHAPE)?;
    let target = probe_target(locator).map_err(|why| (400, why))?;
    if !probeable(&target) {
        return Err((400, INSPECT_PLACEHOLDER.to_string()));
    }
    let args: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let ask = crate::browser::inspect::read_ask(&args).map_err(|why| (400, why))?;
    Ok((target, ask))
}

/// `probe_autorun_locator` with `show` on the page `d` shows: the body read
/// (`inspect_ask`), then the one element its locator matches inspected
/// (`browser::inspect`). Read only.
pub async fn inspect_page<D: crate::browser::cdp::Driver>(d: &mut D, body: &str) -> (u16, String) {
    match inspect_ask(body) {
        Ok((target, ask)) => inspected(d, &target, &ask).await,
        Err(refused) => refused,
    }
}

pub(crate) async fn inspected<D: crate::browser::cdp::Driver>(
    d: &mut D,
    target: &crate::browser::locator::Target,
    ask: &crate::browser::inspect::Ask,
) -> (u16, String) {
    match crate::browser::inspect::inspect(d, target, ask).await {
        Ok(v) => (200, v.to_string()),
        Err(refused) => refused,
    }
}

/// `/autorun-inspect`: one element in the supervised browser, as a probe
/// reaches it - any supervised browser, the person's too, since nothing is
/// changed. Refused at once while an unattended run or a replay holds it.
pub(crate) async fn autorun_inspect(body: &str, n: SlotNo) -> (u16, String) {
    let (target, ask) = match inspect_ask(body) {
        Ok(asked) => asked,
        Err(refused) => return refused,
    };
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    if let Some(going) = replay_is_holding_the_browser(n) {
        return going;
    }
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    let Some(session) = slot.as_mut() else {
        return (409, NO_SUPERVISED_BROWSER.to_string());
    };
    let answer = inspected(&mut session.cdp, &target, &ask).await;
    let answer = let_go_if_silent(&mut slot, answer).await;
    // A browser let go above may have ended a discovery: the window hears.
    crate::commands::autorun::publish_discovery_in(n, &slot);
    // Never the locator: it is the assistant's writing and can hold an address.
    crate::applog::info(format!("AI inspected an element in the supervised browser: {}", answer.0));
    answer
}

