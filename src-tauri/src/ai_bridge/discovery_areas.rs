//! Saving and repairing the areas a discovery or a mapping run records.

use super::*;

/// How many lines of the page a refused area hands back.
pub(crate) const AREA_PAGE_LINES: usize = 40;

/// How many areas one mapping run may save, added and updated together.
pub const MAPPING_CAP: usize = 150;

/// Said to a mapping run's save past `MAPPING_CAP`.
pub const MAPPING_CAP_REACHED: &str = "This mapping run has saved 150 screens; end it and start another for the rest.";

/// Said to a mapping run's save of a name a person's area already has.
pub(crate) fn person_area_kept(name: &str) -> String {
    format!("An area named '{name}' was recorded by a person, so this mapping run leaves it as it is")
}

/// Save the clicks a discovery found as an area named `name` under the
/// test-case Module `module`, once a replay of them from home has arrived
/// where the browser stands now: `{saved, arrived}`, and the discovery
/// moves to the new area. The replay is the run's own trip (`go_to_module`
/// from a fresh home, signed in again as the discovery's account when the
/// session has gone), so a saved area is one a run can take. Clicks that
/// do not arrive save nothing, and the refusal says where they stopped and
/// what the page showed. A name already taken is refused before the
/// browser is touched: the person replaces an area, never the assistant -
/// except to repair a path a run cannot take (`repair_area_in`).
///
/// In a mapping run (spec 2026-10-09 section 2) the area is saved as the
/// run's own (`MadeBy::Mapping`), and every save is counted on the run:
/// past `MAPPING_CAP` saves it is refused before the browser is touched; a
/// name a person's area has is refused and counted unchanged; a name the
/// run's own area has is replayed and saved again only when its clicks or
/// where it arrives changed (`{saved: false, unchanged: true}` and no
/// write otherwise); clicks that do not arrive are counted unreached.
/// Outside one, every area saved is the person's.
///
/// The saves the guard blocked on the replay's trip are counted on the run
/// whatever the answer (`count_blocked_writes`).
#[allow(clippy::too_many_arguments)]
pub async fn discover_area_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    name: &str,
    module: &str,
    clicks: Vec<crate::browser::locator::Target>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    let answer = save_discovered_area(slot, root, organization, project, name, module, clicks, None, timing).await;
    if let Some(browser) = slot.as_mut() {
        let p = browser.parts();
        count_blocked_writes(p.driver, p.discovery.as_mut(), &[]);
    }
    answer
}

/// An assistant's repair of an existing area's menu path
/// (`save_autorun_area` with `repair: { reason }`, owner decision
/// 2026-10-20). The name must be an area the project has - whoever made
/// it, a person included - and the repair keeps it under its own module
/// (`module` may be blank; another one is refused), and new clicks equal
/// to the saved ones are refused (`area_repair::SAME_PATH`), judged after the
/// new clicks are pinned (`nav::go_to_module_pinning`). It is allowed
/// only while the area is failing to be reached, decided live every time:
/// the area's saved path is replayed from a fresh home as a run takes it
/// (`nav::reach_module`, two tries with a reload between) before anything
/// is written, and the repair is refused while it still arrives
/// (`area_repair::NOTHING_TO_REPAIR`) or when the browser stops answering
/// during that check. The new clicks are then replayed from a fresh home
/// exactly as a new area's are, and must arrive where the discovery browser
/// stands. The change is written to the areas file as it is after those
/// trips (`area_repair::save_repair`), so a Remove or a put-back made
/// meanwhile is never overwritten. Only the clicks, where they arrive and
/// the page they begin on change; the area keeps its name, module and
/// maker, and records the path it replaced with the scrubbed reason
/// (`repaired_by_assistant`) so the person can put it back.
#[allow(clippy::too_many_arguments)]
pub async fn repair_area_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    name: &str,
    module: &str,
    clicks: Vec<crate::browser::locator::Target>,
    reason: &str,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    let answer =
        save_discovered_area(slot, root, organization, project, name, module, clicks, Some(reason), timing).await;
    if let Some(browser) = slot.as_mut() {
        let p = browser.parts();
        count_blocked_writes(p.driver, p.discovery.as_mut(), &[]);
    }
    answer
}

/// A fresh home page for an area's replay, never where the page stands:
/// PeoplesHR's menu remembers whether a person left it open, so the clicks
/// would otherwise depend on it (spec 2026-10-09, mapped areas).
/// `load_home` runs `after_sign_in` once, on the fresh load. A session that
/// has gone is signed in again as `account`; a sign-in ends on a fresh,
/// signed-in page with `after_sign_in` run, so that is the start: loading
/// home again would run it a second time, and a toggle run twice closes the
/// menu it opened. The outcome says whether home was reached; `Err` is the
/// answer to give instead.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn fresh_home_for_area<D: crate::browser::cdp::Driver>(
    d: &mut D,
    home: &crate::autorun::nav::Home,
    timing: &crate::browser::timing::Timing,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    account: Option<&str>,
    lease: &mut crate::autorun::lease::Held,
    signed_in: &mut Option<String>,
) -> Result<crate::browser::actions::ActionOutcome, (u16, String)> {
    let went = crate::autorun::nav::load_home(d, home, timing).await;
    let signed = went.ok
        && crate::browser::expect::expect(
            d,
            &home.signed_in,
            home.signed_in_check(),
            timing.nav_ms,
            timing.poll_ms,
        )
        .await
        .ok;
    if signed {
        return Ok(went);
    }
    let Some(key) = account else {
        return Err((409, "the discovery has no account to sign in again with - start it again".to_string()));
    };
    match crate::autorun::signin::sign_in_leased(d, root, organization, project, key, lease, signed_in, timing).await {
        Err(why) => Err((409, why)),
        Ok(out) if !out.ok => Err((409, out.detail)),
        Ok(_) => Ok(crate::browser::actions::ActionOutcome::passed("signed in again")),
    }
}

/// The discovery now stands in the area `moved`. What it has seen since it
/// started, in the area it leaves, is kept in its own state for the new one
/// (`discovery_map::carry_seen`), so a script saved while this discovery goes
/// on can use locators met before the move. Never written to the map. A mapping run reads and saves nothing, so it
/// carries nothing.
pub(crate) fn stand_in_area(
    state: &mut crate::commands::autorun::DiscoveryState,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    moved: &str,
) {
    let to = crate::autorun::discovery_map::canonical_area(root, organization, project, moved);
    if state.mapping.is_none() {
        if let Some(from) = state.area.as_deref() {
            match crate::autorun::discovery_map::load_map(root, organization, project) {
                Ok(map) => {
                    if let Some(more) = crate::autorun::discovery_map::carry_seen(&map, from, &to, state.started_at) {
                        crate::autorun::discovery_map::keep_carried(&mut state.carried, more);
                    }
                }
                Err(why) => unrecorded(&why),
            }
        }
    }
    state.area = Some(to);
}

/// Where a discovery stands once the area `kept` is saved or found as it
/// was. An ordinary discovery goes on to explore that area, so it becomes
/// current. A mapping run files the screen it arrived on under `kept` now,
/// then stands in no area: the pages it reads on its way to the next screen
/// belong to none of the screens it saved.
pub(crate) async fn stand_in_saved_area<D: crate::browser::cdp::Driver>(
    d: &mut D,
    discovery: &mut Option<crate::commands::autorun::DiscoveryState>,
    signed_in: Option<&str>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    kept: String,
) {
    let Some(state) = discovery.as_mut() else { return };
    let mapping = state.mapping.is_some();
    stand_in_area(state, root, organization, project, &kept);
    if !mapping {
        return;
    }
    let at = discovery_sighting(root, organization, project, discovery.as_ref(), None, signed_in);
    let _ = read_page(d, crate::browser::snapshot::DEFAULT_LIMIT, at.as_ref()).await;
    if let Some(state) = discovery.as_mut() {
        state.area = None;
    }
}

/// `save_autorun_area` with `variant: true`: the clicks the discovery found
/// are added to the existing area `name` as another menu path
/// (`ModulePath::variants`), for the other menu shape. They are replayed from
/// a fresh home exactly as a new area's are and must arrive where the
/// discovery browser stands; the area's primary path, name, module and
/// scripts are untouched. A path the area already has is not added twice
/// (`{saved: false, unchanged: true}`). Not in a mapping run, which adds and
/// updates its own areas only.
pub async fn variant_area_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    name: &str,
    module: &str,
    clicks: Vec<crate::browser::locator::Target>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    let answer = save_area_variant(slot, root, organization, project, name, module, clicks, timing).await;
    if let Some(browser) = slot.as_mut() {
        let p = browser.parts();
        count_blocked_writes(p.driver, p.discovery.as_mut(), &[]);
    }
    answer
}

pub(crate) async fn save_area_variant<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    name: &str,
    module: &str,
    clicks: Vec<crate::browser::locator::Target>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    use crate::autorun::nav;
    let Some(browser) = slot.as_mut() else {
        return (409, NO_DISCOVERY.to_string());
    };
    let p = browser.parts();
    let Some(state) = p.discovery.as_ref() else {
        return (409, NO_DISCOVERY.to_string());
    };
    let name = name.trim();
    if name.is_empty() || clicks.is_empty() {
        return (400, "a menu path needs the name of an existing area and at least one click".to_string());
    }
    if state.mapping.is_some() {
        return (409, "a mapping run does not add menu paths; end it and add the menu path in an ordinary discovery".to_string());
    }
    let navfile = match nav::load_nav(root, organization, project) {
        Ok(n) => n,
        Err(why) => return (409, why),
    };
    let Some(found) = nav::find_area(&navfile, name).cloned() else {
        return (409, format!("there is no area named '{name}' to add a menu path to - save it first, without variant"));
    };
    if !module.trim().is_empty() && nav::module_key(module) != nav::module_key(&found.module) {
        return (
            409,
            format!("area '{name}' belongs to module '{}', not '{}' - a menu path never moves an area", found.module, module.trim()),
        );
    }
    let recipe = match crate::autorun::recipe::load_effective_recipe(root, organization, project) {
        Ok(r) => r,
        Err(why) => return (409, why),
    };
    let account = state.account.clone().or_else(|| p.signed_in.clone());
    let d = p.driver;
    let href = crate::browser::page::eval_value(d, "location.href").await;
    let href = href.ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    if href.trim().is_empty() {
        return (409, "the page would not say where it is - read the page and try again".to_string());
    }
    let arrived = nav::path_of(&href);
    let home = nav::Home::of(&recipe);
    let went = match fresh_home_for_area(
        d,
        &home,
        timing,
        root,
        organization,
        project,
        account.as_deref(),
        p.lease,
        p.signed_in,
    )
    .await
    {
        Ok(went) => went,
        Err(refused) => return refused,
    };
    let tried = if went.ok {
        let path = nav::ModulePath {
            clicks,
            arrived,
            start: String::new(),
            variants: Vec::new(),
            repaired_by_assistant: None,
            ..found.clone()
        };
        nav::go_to_module_pinning(d, &nav::Route::new(&recipe, path), timing).await
    } else {
        Err(nav::PathFailure { at: nav::Where::Home, reason: went.detail, harness: went.harness })
    };
    let (at, pinned_clicks, pinned) = match tried {
        Ok(found) => found,
        Err(failure) => {
            let (status, page) = read_page(d, crate::browser::snapshot::DEFAULT_LIMIT, None).await;
            let showed = if status == 200 {
                page.lines().take(AREA_PAGE_LINES).collect::<Vec<_>>().join("\n")
            } else {
                "nothing - the page could not be read".to_string()
            };
            return (409, format!("The clicks did not arrive: {}. The page showed: {showed}", failure.for_dialog()));
        }
    };
    let kept = found.name().to_string();
    let same = |clicks: &[crate::browser::locator::Target], arrived: &str| clicks == pinned_clicks.as_slice() && arrived == at;
    let held = if same(&found.clicks, &found.arrived) {
        Some(1)
    } else {
        found.variants.iter().position(|v| same(&v.clicks, &v.arrived)).map(|i| i + 2)
    };
    if let Some(n) = held {
        stand_in_saved_area(d, p.discovery, p.signed_in.as_deref(), root, organization, project, kept.clone()).await;
        let message = format!("already menu path {n} of \"{kept}\"");
        return (200, serde_json::json!({ "saved": false, "unchanged": true, "arrived": at, "message": message }).to_string());
    }
    let mut updated = found.clone();
    updated.variants.push(nav::PathVariant { clicks: pinned_clicks, arrived: at.clone() });
    let n = updated.variants.len() + 1;
    if let Err(why) = nav::put_path_if(root, organization, project, updated, nav::AreaExpect::Was(&found)) {
        return (409, why);
    }
    stand_in_saved_area(d, p.discovery, p.signed_in.as_deref(), root, organization, project, kept.clone()).await;
    crate::applog::info(format!("Auto Run discovery added menu path {n} to an area"));
    let message = format!("saved as menu path {n} of \"{kept}\"");
    let mut answer = serde_json::json!({ "saved": true, "arrived": at, "message": message });
    if !pinned.is_empty() {
        answer["pinned"] = serde_json::json!(pinned);
    }
    (200, answer.to_string())
}

/// `discover_area_in` and `repair_area_in`, before the blocked saves are
/// counted. `repair` is the repair's reason, as the assistant sent it.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn save_discovered_area<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    name: &str,
    module: &str,
    clicks: Vec<crate::browser::locator::Target>,
    repair: Option<&str>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    use crate::autorun::{area_repair, nav};
    let Some(browser) = slot.as_mut() else {
        return (409, NO_DISCOVERY.to_string());
    };
    let p = browser.parts();
    let Some(state) = p.discovery.as_ref() else {
        return (409, NO_DISCOVERY.to_string());
    };
    let (name, module) = (name.trim().to_string(), module.trim().to_string());
    let reason = match repair.map(area_repair::check_reason) {
        None => None,
        Some(Ok(reason)) => Some(reason),
        Some(Err(why)) => return (400, why),
    };
    // A repair may leave the module out: it keeps the area's own.
    if name.is_empty() || (module.is_empty() && reason.is_none()) || clicks.is_empty() {
        return (400, "an area needs a name, the module it belongs to and at least one click".to_string());
    }
    let saved_this_run = state.mapping.as_ref().map(|run| run.added.len() + run.updated.len());
    if saved_this_run.is_some_and(|n| n >= MAPPING_CAP) {
        return (409, MAPPING_CAP_REACHED.to_string());
    }
    let mapping = saved_this_run.is_some();
    let navfile = match nav::load_nav(root, organization, project) {
        Ok(n) => n,
        Err(why) => return (409, why),
    };
    // The area a repair replaces the path of: one the project has, under
    // the module it already has.
    let repairing = match &reason {
        None => None,
        Some(_) => match nav::find_area(&navfile, &name) {
            None => return (409, area_repair::no_area_to_repair(&name)),
            Some(found) if !module.is_empty() && nav::module_key(&module) != nav::module_key(&found.module) => {
                return (409, area_repair::other_module(found))
            }
            Some(found) => Some(found.clone()),
        },
    };
    // The clicks as sent: an area is compared with them, not with the clicks
    // as they are saved once pinned (`nav::go_to_module_pinning`), and a
    // repair is the same path only if pinning would not change them either.
    let sent = clicks.clone();
    let module = repairing.as_ref().map_or(module, |old| old.module.clone());
    // The run's own area of this name, which this save may update.
    let existing = match nav::find_area(&navfile, &name) {
        _ if repairing.is_some() => None,
        None => None,
        Some(found) if mapping && found.made_by == nav::MadeBy::Mapping => {
            if let Err(why) = nav::check_area_free(&navfile, &name, &module) {
                return (409, why);
            }
            Some(found.clone())
        }
        Some(found) if mapping => {
            let kept = found.name().to_string();
            // Located where the person's area is, so it can stand for an
            // unreached entry of the same screen under another name.
            let arrived = found.arrived.trim();
            let screen = crate::commands::autorun::MappingScreen {
                arrived: (!arrived.is_empty()).then(|| arrived.to_string()),
                menu: nav::menu_path(&found.clicks),
            };
            if let Some(run) = p.discovery.as_mut().and_then(|s| s.mapping.as_mut()) {
                run.record_unchanged(kept.clone());
                run.locate_last(screen);
            }
            return (409, person_area_kept(&kept));
        }
        Some(_) => {
            return (
                409,
                format!(
                    "An area named '{name}' already exists; pick another name or ask the person to replace it in Auto Run"
                ),
            )
        }
    };
    let Some(state) = p.discovery.as_ref() else {
        return (409, NO_DISCOVERY.to_string());
    };
    let recipe = match crate::autorun::recipe::load_effective_recipe(root, organization, project) {
        Ok(r) => r,
        Err(why) => return (409, why),
    };
    let account = state.account.clone().or_else(|| p.signed_in.clone());
    let d = p.driver;
    // Where the clicks must arrive: where the assistant clicked its way to,
    // read as a run's trip reads it - and read before a repair's try of the
    // saved path moves the page.
    let href = crate::browser::page::eval_value(d, "location.href").await;
    let href = href.ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    let arrived = if href.trim().is_empty() { String::new() } else { nav::path_of(&href) };
    if arrived.is_empty() {
        return (409, "the page would not say where it is - read the page and try again".to_string());
    }
    // Where the screen is, for a mapping run's summary, however this ends.
    let screen = crate::commands::autorun::MappingScreen { arrived: Some(arrived.clone()), menu: nav::menu_path(&clicks) };
    let home = nav::Home::of(&recipe);
    // A repair first takes the area's saved path from a fresh home, as a
    // run would, and goes no further when it still arrives.
    if let Some(old) = repairing.as_ref() {
        let went = match fresh_home_for_area(
            d,
            &home,
            timing,
            root,
            organization,
            project,
            account.as_deref(),
            p.lease,
            p.signed_in,
        )
        .await
        {
            Ok(went) => went,
            Err(refused) => return refused,
        };
        if !went.ok {
            return (409, format!("the area's saved path could not be tried: {}", went.detail));
        }
        let route = nav::Route::new(&recipe, old.clone());
        let tried = nav::reach_module(d, &route, nav::TripFrom::SignIn, timing, "Auto Run area repair", account.as_deref()).await;
        if tried.ok {
            return (409, area_repair::NOTHING_TO_REPAIR.to_string());
        }
        if tried.harness {
            return (
                409,
                format!(
                    "the browser stopped answering while the area's saved path was tried, so nothing was saved: {}",
                    tried.detail
                ),
            );
        }
    }
    // The check starts from a fresh home page, never from where the page
    // stands (`fresh_home_for_area`).
    let went = match fresh_home_for_area(
        d,
        &home,
        timing,
        root,
        organization,
        project,
        account.as_deref(),
        p.lease,
        p.signed_in,
    )
    .await
    {
        Ok(went) => went,
        Err(refused) => return refused,
    };
    let reached = if went.ok {
        let start = match crate::browser::page::eval_value(d, "location.href").await {
            Ok(v) => nav::path_of(v.as_str().unwrap_or("")),
            Err(_) => String::new(),
        };
        // The menu preferences in force, which the area's clicks are for: a
        // project whose recipe names them records them with the area. A
        // dialog that cannot be read leaves them out; the area is saved
        // without.
        let preferences = match recipe.menu_preferences.as_ref() {
            Some(prefs) => match crate::autorun::menu_setup::read_preferences(d, prefs, timing).await {
                Ok(read) => Some(read),
                Err(why) => {
                    crate::applog::warn(format!("area {name}: the menu preferences could not be read: {why}"));
                    None
                }
            },
            None => None,
        };
        // The mode the account is in (a project whose recipe names modes):
        // the area is for that one.
        let mode = crate::autorun::menu_setup::read_mode(d, &recipe.modes, timing).await;
        let mut path = match (&repairing, &reason) {
            (Some(old), Some(reason)) => area_repair::repaired(
                old,
                clicks,
                arrived,
                start,
                reason.clone(),
                crate::commands::autorun_record::now_iso(),
            ),
            _ => nav::ModulePath {
                // An update keeps the area's name as it was saved.
                area: existing.as_ref().map_or_else(|| name.clone(), |old| old.name().to_string()),
                module,
                clicks,
                arrived,
                recorded: crate::commands::autorun_record::now_iso(),
                start,
                made_by: if mapping { nav::MadeBy::Mapping } else { nav::MadeBy::Person },
                // A mapping run writes its own area again only when the path
                // changed, and an assistant's repair note describes the path
                // it replaced: it goes with that path.
                repaired_by_assistant: None,
                // What an area already saved keeps when a read fails below.
                preferences: existing.as_ref().map(|old| old.preferences.clone()).unwrap_or_default(),
                mode: existing.as_ref().and_then(|old| old.mode.clone()),
                // The menu paths for other menu shapes stay with the area.
                variants: existing.as_ref().map(|old| old.variants.clone()).unwrap_or_default(),
            },
        };
        if let Some(read) = preferences {
            path.preferences = read;
        }
        if mode.is_some() {
            path.mode = mode;
        }
        // The clicks are saved as the trip made them: each finds exactly
        // one element on the page (`nav::go_to_module_pinning`).
        let mut route = nav::Route::new(&recipe, path);
        match nav::go_to_module_pinning(d, &route, timing).await {
            Ok((at, clicks, notes)) => {
                route.path.clicks = clicks;
                Ok((at, route.path, notes))
            }
            Err(failure) => Err(failure),
        }
    } else {
        Err(nav::PathFailure { at: nav::Where::Home, reason: went.detail, harness: went.harness })
    };
    let (at, path, pinned) = match reached {
        Ok(found) => found,
        Err(failure) => {
            // The saved path again fails the way it did: nothing new to try.
            let refused_pin = failure.reason.contains("counting hidden ones");
            if !refused_pin && repairing.as_ref().is_some_and(|old| old.clicks == sent) {
                return (409, area_repair::SAME_PATH.to_string());
            }
            let reason = failure.for_dialog();
            let (status, page) = read_page(d, crate::browser::snapshot::DEFAULT_LIMIT, None).await;
            let showed = if status == 200 {
                page.lines().take(AREA_PAGE_LINES).collect::<Vec<_>>().join("\n")
            } else {
                "nothing - the page could not be read".to_string()
            };
            if let Some(run) = p.discovery.as_mut().and_then(|s| s.mapping.as_mut()) {
                run.record_unreached(name, &reason);
                run.locate_last(screen);
            }
            return (409, format!("The clicks did not arrive: {reason}. The page showed: {showed}"));
        }
    };
    if repairing.as_ref().is_some_and(|old| old.clicks == path.clicks) {
        return (409, area_repair::SAME_PATH.to_string());
    }
    // The run's own area, found where it was: nothing to write. Its saved
    // clicks stand when they are the clicks found now, even when pinning
    // would only make a name exact; they do not when one now matches more
    // than one element and had to be scoped.
    let scoped = pinned.iter().any(|note| note.contains(nav::SCOPED_NOTE));
    let found_as_saved = |old: &nav::ModulePath| old.clicks == path.clicks || (old.clicks == sent && !scoped);
    if let Some(old) = existing.as_ref().filter(|old| found_as_saved(old) && old.arrived == path.arrived) {
        let kept = old.name().to_string();
        if let Some(state) = p.discovery.as_mut() {
            if let Some(run) = state.mapping.as_mut() {
                run.record_unchanged(kept.clone());
                run.locate_last(screen);
            }
        }
        // The same clicks, but the mode or the menu preferences read now are
        // not what the area holds (an area saved before they were kept, or
        // one recorded in another state): only those are written, the saved
        // clicks stay as they are.
        let (mode_differs, prefs_differ) = (old.mode != path.mode, old.preferences != path.preferences);
        if mode_differs || prefs_differ {
            let mut updated = old.clone();
            updated.preferences = path.preferences.clone();
            updated.mode = path.mode.clone();
            if let Err(why) = nav::put_path_if(root, organization, project, updated, nav::AreaExpect::Was(old)) {
                return (409, why);
            }
            stand_in_saved_area(d, p.discovery, p.signed_in.as_deref(), root, organization, project, kept.clone()).await;
            let what = match (mode_differs, prefs_differ) {
                (true, true) => "mode/menu preferences",
                (true, false) => "mode",
                _ => "menu preferences",
            };
            crate::applog::info(format!("Auto Run area {kept}: recorded its {what}"));
            let message = format!("unchanged path; recorded its {what}");
            return (200, serde_json::json!({ "saved": true, "arrived": at, "message": message }).to_string());
        }
        stand_in_saved_area(d, p.discovery, p.signed_in.as_deref(), root, organization, project, kept).await;
        crate::applog::info("Auto Run mapping run found an area unchanged");
        return (200, serde_json::json!({ "saved": false, "unchanged": true, "arrived": at }).to_string());
    }
    let saved = path.clicks.len();
    let preferences = path.preferences.clone();
    let mode = path.mode.clone();
    let new_menu = nav::menu_path(&path.clicks);
    let written = match (&repairing, path.repaired_by_assistant.as_ref()) {
        // A repair goes to the file as it is now (`area_repair::save_repair`).
        (Some(old), Some(note)) => area_repair::save_repair_recording(
            root,
            organization,
            project,
            old,
            path.clicks.clone(),
            path.arrived.clone(),
            path.start.clone(),
            note.reason.clone(),
            note.at.clone(),
            Some((path.preferences.clone(), path.mode.clone())),
        ),
        // The area as it was read before the trips decides the write: an
        // area saved or changed since is not written over.
        _ => match existing.as_ref() {
            Some(old) => nav::put_path_if(root, organization, project, path, nav::AreaExpect::Was(old)),
            None => nav::put_path_if(root, organization, project, path, nav::AreaExpect::New),
        },
    };
    if let Err(why) = written {
        return (409, why);
    }
    // The area whose path this save replaced, if any.
    let replaced = existing.as_ref().or(repairing.as_ref());
    if let Some(state) = p.discovery.as_mut() {
        if let Some(run) = state.mapping.as_mut() {
            match replaced {
                Some(old) => run.record_updated(old.name().to_string(), nav::menu_path(&old.clicks), new_menu),
                None => run.record_added(name.clone()),
            }
            run.locate_last(screen);
        }
    }
    let kept = match replaced {
        Some(old) => old.name().to_string(),
        None => name,
    };
    stand_in_saved_area(d, p.discovery, p.signed_in.as_deref(), root, organization, project, kept.clone()).await;
    if let Some(reason) = reason {
        crate::applog::info(format!("AI repaired area {kept}: {reason}"));
        let message = area_repair::repaired_answer(&kept, &reason);
        let mut answer = serde_json::json!({ "saved": true, "repaired": true, "arrived": at, "message": message });
        if !pinned.is_empty() {
            answer["pinned"] = serde_json::json!(pinned);
        }
        return (200, answer.to_string());
    }
    crate::applog::info(format!("Auto Run discovery saved an area ({saved} clicks)"));
    let mut answer = serde_json::json!({ "saved": true, "arrived": at });
    if !pinned.is_empty() {
        answer["pinned"] = serde_json::json!(pinned);
    }
    if !preferences.is_empty() {
        answer["preferences"] = serde_json::json!(preferences);
    }
    if let Some(mode) = mode {
        answer["mode"] = serde_json::json!(mode);
    }
    (200, answer.to_string())
}

/// `/autorun-discover-area`: save the clicks a discovery found as an area
/// (`discover_area_in`), once a replay of them arrives.
pub(crate) async fn autorun_discover_area(ctx: &BridgeContext, body: &str, n: SlotNo) -> (u16, String) {
    const SHAPE: &str = "{ \"name\": <the area's name>, \"module\": <the test-case Module it belongs to>, \"clicks\": [<a script's click selector>, ...], \"repair\": <optional: { \"reason\": <one line> } to replace an existing area's path>, \"variant\": <optional: true to add these clicks to the existing area as another menu path> }";
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {SHAPE}.")),
    };
    let Some(name) = named(v.get("name").and_then(|a| a.as_str())) else {
        return (400, format!("this call needs a \"name\" for the area. Expected {SHAPE}."));
    };
    let repair = match v.get("repair") {
        None | Some(serde_json::Value::Null) => None,
        Some(r) => match r.get("reason").and_then(|a| a.as_str()) {
            Some(reason) => Some(reason.to_string()),
            None => return (400, format!("\"repair\" is {{ \"reason\": <one line> }}. Expected {SHAPE}.")),
        },
    };
    let variant = match v.get("variant") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(_) => return (400, format!("\"variant\" is true or false. Expected {SHAPE}.")),
    };
    if variant && repair.is_some() {
        return (400, format!("a menu path is added with \"variant\" or an area is repaired with \"repair\", not both. Expected {SHAPE}."));
    }
    // A repair keeps the area's own module, and so does a menu path added to
    // an existing area: either may leave it out.
    let module = match named(v.get("module").and_then(|a| a.as_str())) {
        Some(module) => module,
        None if repair.is_some() || variant => String::new(),
        None => return (400, format!("this call needs the \"module\" the area belongs to. Expected {SHAPE}.")),
    };
    let clicks: Vec<crate::browser::locator::Target> = match v.get("clicks").cloned().map(serde_json::from_value) {
        Some(Ok(clicks)) => clicks,
        Some(Err(e)) => return (400, format!("those are not clicks: {e}. Expected {SHAPE}.")),
        None => return (400, format!("this call needs the \"clicks\" from the home page. Expected {SHAPE}.")),
    };
    if clicks.is_empty() {
        return (400, format!("an area needs at least one click from the home page. Expected {SHAPE}."));
    }
    for (i, click) in clicks.iter().enumerate() {
        if let Err(e) = click.validate() {
            return (400, format!("click {}: {e}", i + 1));
        }
    }
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    if variant {
        return variant_area_in(
            &mut slot,
            &root,
            &ctx.org,
            &ctx.project,
            &name,
            &module,
            clicks,
            &crate::browser::timing::Timing::supervised(),
        )
        .await;
    }
    if let Some(reason) = repair {
        return repair_area_in(
            &mut slot,
            &root,
            &ctx.org,
            &ctx.project,
            &name,
            &module,
            clicks,
            &reason,
            &crate::browser::timing::Timing::supervised(),
        )
        .await;
    }
    discover_area_in(
        &mut slot,
        &root,
        &ctx.org,
        &ctx.project,
        &name,
        &module,
        clicks,
        &crate::browser::timing::Timing::supervised(),
    )
    .await
}

