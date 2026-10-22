//! The save check: a script may name only what the app has seen on the
//! live page (`discovery_map`), so a script is never saved against a
//! guessed locator.
//!
//! Two kinds of name are allowed without a sighting. One is a locator
//! whose text or name contains a value the script typed in an earlier
//! step (a record it just created): typing "AutoTest Leave 7" exempts a
//! row "AutoTest Leave 7 Pending", never a "Leave" button. The other is a
//! name a check looks for that the test case itself says (the expected
//! result the step is checking for). Both match whole words only, and a
//! typed value shorter than 3 characters exempts nothing.
//!
//! The script's own data is exempt the same way, from the step that brings
//! it in: the name of one of the project's Test files that the script
//! uploads in that step or an earlier one (and a name holding it), and that
//! file's size as the app shows it ("240.0 KB" or "0.2 MB"); a date-picker
//! day (`DAY_ROLES`) whose name is a date the script picked (a component's
//! text input) or typed; and a `dd/mm/yyyy` day inside a seen date picker.
//! With the Test files unknown, no file name or size is exempt.
//!
//! Some locators are built from data, so they are compared with a
//! sighting by more than their text. None of this lets through a locator
//! that could not exist on the seen page:
//!
//! - A data placeholder (`{{fixture.<id>.<output>}}`, `{{setup.<output>}}`;
//!   see `seen_match`) in a name or a text, or inside a quoted attribute
//!   value of a css selector, stands for a non-empty run of a seen value
//!   with no quote in it. Inside an id or class token beside a literal
//!   part of it (`#c{{setup.cycle_id}}`) it stands for a run of letters,
//!   digits, `-` and `_` of a seen token of that kind with the same literal
//!   parts; a whole token (`#{{setup.x}}`) or any other place in a
//!   selector stays literal. The text around it must match the sighting as
//!   written, and so must the role (or the rest of the selector).
//!   A name or a text that is nothing but a placeholder fits nothing
//!   (`ONLY_A_PLACEHOLDER`): it would stand for any element of its role.
//!   `{{prefix}}` and `{{now:...}}` are not data placeholders: Auto Run
//!   never fills them in, so they stay literal and are refused. Once the
//!   run has filled a placeholder in, the locator is checked again
//!   (`check_resolved_inputs`): each value must have the shape of the seen
//!   value it stands for, all digits where that was all digits. A filled
//!   link that is the script's own data, read from the filled steps as the
//!   save reads the saved ones, passes there too. The case's text is not
//!   known at run time, so at save it never lets a link holding a
//!   placeholder through on its own.
//! - A css selector's `:checked`, `:disabled`, `:enabled` and `:focus` on
//!   an element it names are left out before it is compared; one that
//!   starts the selector or follows a space or a combinator stands for an
//!   element of its own and is kept. A `:not(X)` passes only when the rest
//!   was seen and so was X in the same areas: X as a selector, or, when X
//!   is only attribute filters, the rest seen carrying each of them. A
//!   `:has(X)` passes only when X was seen inside the rest: a seen
//!   selector or chain with the rest as an ancestor of X.
//! - Names and texts compare with whitespace collapsed, case folded, and
//!   an em dash, an en dash and a hyphen as one dash, with no space beside
//!   it (`norm_name`).
//! - For a check that may match several (`takes_many`), a role or text
//!   locator whose name (or text) is a part of a seen one of the same role
//!   (a text of a seen text), folded the same way, counts as seen, as a run
//!   matches a name that contains it (`locator::name_matches`). Only for a
//!   plain one-link locator (no `nth`, no `"visible": false`, not
//!   `"exact": true`), whose part is at least `MIN_TYPED_LEN` characters
//!   once folded, and never one holding a data placeholder. Every other use
//!   needs the whole name, as seen: the map holds only what reads listed,
//!   and a read cut at its limit or listing one element may have left out
//!   a second name holding the part, so "one seen name holds it" can never
//!   say a run would match one element. Its refusal says to use the full
//!   name as seen, and names a seen one that holds the part.
//!
//! A link a checked saved script of one of the checked areas uses, written
//! exactly as it is there, counts as seen (`load_checked_map`): the save
//! that wrote it checked it against the live app, so a sighting the map
//! has since let go does not refuse it again. Only a script that vouches
//! counts (`vouches`): one saved in this organization and project by a
//! save that ran this check (the assistant's or an import, `checked`),
//! never one saved from the editor. A script saved before scripts were
//! stamped counts only while exactly one project has recorded areas and it
//! is this one. Only a script whose own area is one the check reads counts,
//! never another area's or one with no area, and a saved link that holds a
//! placeholder or a component input counts for nothing.
//!
//! A refusal names the closest locator seen in the same areas, in the same
//! role (a text locator offers only text), when one is close enough
//! (`SUGGEST_WITHIN`). One seen on the page the refused step's area arrives
//! on (`add_area_pages`) is offered before one seen only elsewhere.
//!
//! Each refusal also says, in one clause, what that use needs (`wants`):
//! "this click needs one seen matching exactly one element", "this
//! expect_hidden needs it seen at least once". Where the reason is more
//! particular it says that instead: seen only matching several
//! (`SEEN_ONLY_MANY`, followed by up to 3 of the seen names it matched on
//! the pages it was seen on, `named_matches`; with `"exact": true` and only
//! its own name among them, `NEVER_COUNTED`), never counted (`NEVER_COUNTED`, or
//! `NEVER_COUNTED_NTH` for a locator with an `nth`, which is counted only
//! as written), or seen only in saved checks (`SEEN_ONLY_IN_CHECKS`). A
//! part of a seen name, refused for a use that needs one, says to use the
//! full name as seen (`use_full_name`).
//!
//! A `when_visible` guards what may not show up where a run is: a cookie
//! banner, a session prompt, both of which appear on the sign-in page and
//! anywhere else. So its selector, and each target in its `then` that is
//! that same locator, is checked against what was seen in every area of
//! the project, not only the areas the script reads. Its selector is a
//! guard, skipped when it matches nothing, so it counts seen in any form
//! and any count, none included (`seen_many`, `takes_none`); a target in
//! its `then` keeps its own action's rule.
//!
//! A refused save names every locator never seen at once, in step order,
//! then action order, then link order, each with its own hint, up to
//! `MAX_LISTED` lines and then how many more. A refusal of any other kind
//! (a page address never seen, a component use the step cannot make)
//! is answered alone, as it always was (`SeenVerdict`).
//!
//! How many elements a locator was seen matching (`Sighting::matched`)
//! depends on how it is used (`takes_many`). A check a run can pass with
//! zero or several matches (`expect_hidden`, `expect_count`, `wait_for`)
//! may use a locator seen matching one or more elements, as may a
//! `when_visible`'s own selector. Every other use needs one seen matching
//! exactly one: an action that acts on the element
//! (click, fill, select, check, upload, press, drag, ...), and the checks
//! whose run reads one element and fails on several (`expect_visible`,
//! `expect_text`, `expect_contains_text`, `expect_attribute`, `expect_style`,
//! `expect_enabled`, `expect_disabled`, `expect_checked`, `expect_focused`, and the table
//! checks `expect_row`, `expect_no_row`,
//! `expect_sorted` and `expect_row_count`, whose table must be one). A
//! locator a probe matched none of (`Sighting::matched` 0) counts only
//! for a use that passes with nothing there (`takes_none`):
//! `expect_hidden`, an `expect_count` of 0 and a `when_visible`'s selector.
//! Every use that needs it present refuses it as never seen. The rule is
//! per locator: each link of a chain is held to it, the links a chain is
//! scoped by included. A run resolves a scope across every
//! element it matches, so the scope of a check that may match several may
//! have been seen matching several too; an action's scope keeps the rule it
//! always had, one. A link of a saved script that vouches (see below)
//! counts for an action only when that script used it outside such a
//! check, and only as it wrote it: a plain one-link locator by its key, a
//! chain or a narrowed locator only whole, written the same way (its
//! links alone count only for such a check). A link a saved script used
//! only where a run passes with nothing there (`takes_none`, a
//! `when_visible`'s own locator included) counts only for such a use: it
//! may have been saved on a probe of none, or on a count of several. A script saved before
//! sightings kept their counts is grandfathered: what it vouches for is
//! read the same way, though it was checked under the older rule.
//!
//! A legacy string target acts on the first match, hidden or not, so its
//! use that needs one needs its key seen matching exactly one element
//! with hidden ones included (`Sighting::all_matched`); a structured one
//! needs exactly one visible match. A saved script's link vouches for a
//! legacy use only when that script wrote it as a legacy string too.
//!
//! Only exactly the locator that was seen matching one counts as one. A
//! plain one-link locator (`discovery_map::plain_link`) is compared by its
//! key, as always; a chain or a narrowed locator (`nth`, `exact`,
//! `visible`) that matched one counts as one only as that whole locator,
//! written the same way (`Sightings::whole`). A data placeholder in such a
//! locator stands for the value the seen one holds in that place, fitted
//! per name, text or selector, anchored, with the rest written exactly as
//! seen (`same_link_filled`): a placeholder locator cannot be probed as
//! written, so probing it with a real value counts for it, and the probed
//! one must still have matched exactly one. The run-time check takes such
//! a locator as a whole the same way (`SeenBoth::whole_filled`), each
//! value the run filled in shaped as the probed one was. A css selector seen with a
//! state pseudo-class (`:checked`) does not stand for the selector without
//! it here.
//!
//! A step that uses a component must name one the project has and give it
//! every input, each of its kind. Every locator of the component an input
//! goes into (a target input, or a text input written into a locator) is
//! checked like any other, as it runs; the component's fixed locators are
//! not, as they were checked when the component was saved.

use super::components::{expand, find, not_saved, Component, ComponentFile};
use super::discovery_map::{
    area_key, counted, load_map, only_many, page_path, path_only, plain_target, seen_keys, seen_links, seen_locators, seen_pages, seen_paths,
    AreaMap, DiscoveryMap,
};
use super::edits::Edit;
use super::nav::NavFile;
use super::recipe::project_slug;
use super::seen_match::{
    attribute_tails, css_pieces, descendant_splits, filter_attributes, fit, has_wild, is_ddmmyyyy, name_pattern, parse_date,
    safe_filled, same_shape, split_filters, strip_states, wild_fits, without_placeholders, Date, Filter, Piece,
};
pub use super::seen_match::{holds_data_placeholder, is_data_placeholder, norm_name, only_data_placeholders};
use super::{CaseScript, StepScript};
use crate::browser::actions::Action;
use crate::browser::locator::{fold_name, LocatorStep, SeenKey, Target};
use crate::test_files::TestFile;
use serde_json::Value;
use std::collections::HashSet;

/// A typed value, or a name taken from the test case, shorter than this
/// exempts nothing: two characters are inside far too many names to say
/// where they came from.
const MIN_TYPED_LEN: usize = 3;

/// How close a seen name must be to the refused one to be offered as "did
/// you mean": at most one edit (a character added, dropped or changed) for
/// every three characters of the longer of the two names, compared as
/// `norm_name` makes them. "Publsh" offers "Publish" (1 edit in 7); "Add
/// Rating Method" does not offer "Add Method" (7 edits in 17).
const SUGGEST_WITHIN: (usize, usize) = (1, 3);

/// The roles a date picker's day has: a button (most pickers), a gridcell
/// (a calendar grid), an option (a listbox of days, as react-datepicker
/// marks them) or a link. A day named by its text alone, or in any other
/// role (a table cell holding a due date), is not a picked day.
const DAY_ROLES: [&str; 4] = ["button", "gridcell", "option", "link"];

/// The roles a date picker itself has, when it is named by role: the
/// popup (dialog), its calendar grid, an application widget, a listbox of
/// days, or a group around them.
const PICKER_ROLES: [&str; 5] = ["dialog", "grid", "application", "listbox", "group"];

/// The steps a changed script's check reads: the declared ones, or every
/// step (`None`) when nothing is declared, so a missing declaration can
/// never skip the check.
pub fn steps_to_check(declared: Option<&Edit>) -> Option<Vec<i32>> {
    declared.map(|e| e.steps.clone())
}

/// Is `phrase` in `text` as whole words: bounded on each side by a
/// character that is not a letter or digit, or by an end of the text?
pub(crate) fn has_phrase(text: &str, phrase: &str) -> bool {
    text.match_indices(phrase).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + phrase.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// "<what> was never seen on the live app", with the closest seen locator
/// after it when there is one; or, when the hint is `SEEN_ONLY_MANY`, that
/// it was seen only matching several elements and this use needs one.
pub fn never_seen(what: &str, hint: Option<&str>) -> String {
    match hint {
        Some(h) if is_own_sentence(h) => format!("{what} {h}"),
        Some(h) => format!("{what} was never seen on the live app; {h}"),
        None => format!("{what} was never seen on the live app."),
    }
}

fn refusal(step: i32, what: &str, hint: Option<&str>) -> String {
    format!(
        "Step {step}: {} Find it on the page first with probe_autorun_locator or discover_autorun_actions, then save again.",
        never_seen(what, hint)
    )
}

/// The hint a refusal gives a locator a use needs to have seen matching
/// one element, when it was only seen matching several: said after the
/// locator in place of "was never seen on the live app" (`never_seen`).
pub const SEEN_ONLY_MANY: &str = "was seen on the live app only matching several elements, and this use needs one seen matching exactly one: narrow it with a name, \"exact\": true, a scope or \"nth\".";

/// [`SEEN_ONLY_MANY`] for a locator seen, but never counted as matching
/// one element: as a link of a chain, narrowed, or on a read the line
/// limit cut short.
pub const NEVER_COUNTED: &str = "was seen on the live app, but never counted as matching exactly one element: probe it as written, then save again.";

/// [`SEEN_ONLY_MANY`] for a locator only a saved script's check vouches
/// for: that check may match several, so it says nothing of one.
pub const SEEN_ONLY_IN_CHECKS: &str = "was used only in saved checks that may match several elements, and this use needs one seen matching exactly one: probe it on the page, narrowed to one element if it matches several.";

/// [`NEVER_COUNTED`] for a locator with an `nth`: only that whole locator,
/// `nth` included, is counted when it is probed.
pub const NEVER_COUNTED_NTH: &str = "was seen on the live app, but never counted as matching exactly one element: probe it with its nth as written, then save again.";

/// [`NEVER_COUNTED`] for a locator holding a data placeholder, which cannot
/// be probed as written: the same locator probed with a real value in each
/// placeholder's place counts for it (`same_link_filled`).
pub const NEVER_COUNTED_FILLED: &str = "was seen on the live app, but never counted as matching exactly one element: probe the same locator with a real value in place of each {{...}} placeholder (and its nth, if it has one), then save again.";

/// An `expect_each` child seen, link by link, but never as the chain: a
/// list and an `h3` seen apart do not say there is an `h3` inside the list.
pub const SEEN_ONLY_APART: &str = "was seen on the live app only piece by piece, and an expect_each child needs the whole of it seen inside its list: probe it written the same way, then save again.";

/// Said of a locator whose whole name or text is one data placeholder
/// (`button "{{setup.label}}"`): it would match any element of that role,
/// so nothing seen, probed or not, counts for it.
pub const ONLY_A_PLACEHOLDER: &str = "is named only by a placeholder, which would fit any name: write the words around it as the page shows them, or reach it by a css attribute that holds the value, such as div[data-cycle-id=\"{{setup.cycle_id}}\"].";

/// Is `raw`, a name or a text, a data placeholder and nothing else: no
/// literal word around it once folded (`name_pattern`)?
fn only_placeholder(raw: &str) -> bool {
    let pieces = name_pattern(raw);
    has_wild(&pieces) && pieces.iter().all(|p| !matches!(p, Piece::Lit(l) if !l.trim().is_empty()))
}

/// Is `link`'s name or text only a data placeholder (`only_placeholder`)?
fn named_only_by_placeholder(link: &LocatorStep) -> bool {
    [&link.name, &link.text].into_iter().flatten().any(|v| only_placeholder(v))
}

/// Is `hint` one of the sentences said after the locator in place of "was
/// never seen on the live app" (`never_seen`), rather than a clause after
/// it?
pub fn is_own_sentence(hint: &str) -> bool {
    [SEEN_ONLY_IN_CHECKS, NEVER_COUNTED, NEVER_COUNTED_NTH, NEVER_COUNTED_FILLED, ONLY_A_PLACEHOLDER, SEEN_ONLY_APART]
        .contains(&hint)
        || hint.starts_with(SEEN_ONLY_MANY)
}

/// What a locator seen only matching several matched, said after
/// `SEEN_ONLY_MANY`: up to 3 of the distinct seen names it matches, by role
/// and name (`Sightings::matching`), then "and more" past 3. One name only
/// is twins of it. Nothing when no name is known (a css locator).
fn named_matches(matched: &[&LocatorStep]) -> String {
    let quoted = |s: &str| s.replace('"', "\\\"");
    let named = |l: &&LocatorStep| match (&l.role, &l.name, &l.text) {
        (Some(r), Some(n), _) => format!("{r} \"{}\"", quoted(n)),
        (_, _, Some(t)) => format!("text \"{}\"", quoted(t)),
        _ => String::new(),
    };
    let names: Vec<String> = matched.iter().take(3).map(named).filter(|n| !n.is_empty()).collect();
    match (names.as_slice(), matched.len()) {
        ([], _) => String::new(),
        ([one], 1) => format!(" It matched {one} more than once."),
        (some, n) if n > 3 => format!(" It matched {} and more.", some.join(", ")),
        (some, _) => {
            let (last, rest) = some.split_last().expect("not empty");
            format!(" It matched {} and {last}.", rest.join(", "))
        }
    }
}

/// The hint for a part of a name refused for a use that needs one: the
/// seen name holding it, as `seen` was written, to use in its place.
pub fn use_full_name(seen: &LocatorStep) -> String {
    let quoted = |s: &str| s.replace('"', "\\\"");
    match (&seen.role, &seen.text) {
        (Some(r), _) => format!("use the full name as seen, such as {r} \"{}\".", quoted(seen.name.as_deref().unwrap_or(""))),
        (None, Some(t)) => format!("use the full text as seen, such as text \"{}\".", quoted(t)),
        _ => "use the full name as seen.".to_string(),
    }
}

/// What a use of a locator needs, in one clause, for a refusal of one never
/// seen: `kind` is the action's own word, `many` whether it may match
/// several (`takes_many`), `everywhere` whether it is a `when_visible`'s
/// locator, checked against every area, and `none` whether it also takes a
/// probe that matched nothing (`takes_none`: a guard, `expect_hidden`).
pub fn wants(kind: &str, many: bool, everywhere: bool, none: bool) -> String {
    let anywhere = if everywhere { ", in any area" } else { "" };
    let or_none = if none { " (or a probe that matched none)" } else { "" };
    if many {
        format!("this {kind} needs it seen on the live app at least once, matching any number of elements{or_none}{anywhere}.")
    } else {
        format!("this {kind} needs one seen matching exactly one element{anywhere}.")
    }
}

/// Can this action's locator have been seen matching several elements:
/// a check a run passes with zero or several matches? `expect_hidden`
/// (nothing of it visible), `expect_count` (any number) and `wait_for`
/// (at least one). Every other action, the checks that read one element
/// and fail on several included, needs a locator seen matching exactly
/// one (see the module's notes). Matched without a catch-all, so a new
/// kind will not compile until it is classified here.
pub fn takes_many(action: &Action) -> bool {
    match action {
        // A loop's selector matches every row it works through.
        Action::ExpectHidden { .. }
        | Action::ExpectCount { .. }
        | Action::ExpectEach { .. }
        | Action::ExpectOrder { .. }
        | Action::WaitFor { .. }
        | Action::RepeatWhileVisible { .. } => true,
        Action::Click { .. }
        | Action::Fill { .. }
        | Action::Upload { .. }
        | Action::Drag { .. }
        | Action::PressKey { .. }
        | Action::WhenVisible { .. }
        | Action::ExpectVisible { .. }
        | Action::ExpectText { .. }
        | Action::ExpectContainsText { .. }
        | Action::ExpectAttribute { .. }
        | Action::ExpectStyle { .. }
        | Action::ExpectEnabled { .. }
        | Action::ExpectDisabled { .. }
        | Action::ExpectChecked { .. }
        | Action::ExpectFocused { .. }
        | Action::ExpectPosition { .. }
        | Action::ExpectRow { .. }
        | Action::ExpectNoRow { .. }
        | Action::ExpectSorted { .. }
        | Action::ExpectRowCount { .. }
        | Action::UseComponent { .. }
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
        | Action::ExpectDownload { .. }
        | Action::ExpectTab { .. }
        | Action::OpenTab { .. }
        | Action::SwitchTab { .. }
        | Action::CloseTab { .. }
        | Action::ExpectTabClosed { .. }
        | Action::ExpectDialog { .. }
        // Its `next` is clicked and its `until` locators looked for: held
        // per target (`many_at`).
        | Action::PageUntil { .. } => false,
    }
}

/// Can this action pass with its locator matching nothing on the page:
/// `expect_hidden`, an `expect_count` of 0 (`equals: 0` or `at_most: 0`),
/// and a `when_visible`, a guard that is skipped when its locator matches
/// nothing? Such a use takes a
/// locator a probe matched none of (`Sighting::matched` 0); every other
/// use needs it seen present.
pub fn takes_none(action: &Action) -> bool {
    matches!(
        action,
        Action::ExpectHidden { .. }
            | Action::WhenVisible { .. }
            | Action::ExpectCount { equals: Some(0), .. }
            | Action::ExpectCount { at_most: Some(0), .. }
    )
}

/// Does the seen check take this action's own locator seen matching any
/// number of elements: a check that may match several (`takes_many`), or a
/// `when_visible`'s own locator, a guard skipped when it does not match
/// (its `then` actions are held to their own rule)?
fn seen_many(action: &Action) -> bool {
    takes_many(action) || matches!(action, Action::WhenVisible { .. })
}

/// `seen_many` for the target at `index` of `own_targets(action)`. The same
/// for every action but a `page_until`, whose `next` is clicked (seen
/// matching exactly one) and whose `until` locators are each looked for, so
/// may have been seen matching several.
fn many_at(action: &Action, index: usize) -> bool {
    match action {
        Action::PageUntil { .. } => index > 0,
        _ => seen_many(action),
    }
}

/// `is_check` for the target at `index` of `own_targets(action)`: a
/// `page_until`'s `until` locators are looked for, so the case's own words
/// exempt them as they do any check's; its `next` is acted on.
fn check_at(action: &Action, index: usize) -> bool {
    match action {
        Action::PageUntil { .. } => index > 0,
        _ => is_check(action),
    }
}

/// `map` with no sighting or whole seen matching none (`matched` 0): what a
/// use that needs its locator present is checked against.
fn present_only(map: &DiscoveryMap) -> DiscoveryMap {
    let mut out = map.clone();
    for a in &mut out.areas {
        a.sightings.retain(|s| s.matched != 0);
        a.wholes.retain(|w| w.matched != 0);
        a.saved_none_links.clear();
    }
    out
}

/// `map` with only what was seen matching exactly one element: no sighting
/// or element seen only matching several, and no saved link used only in a
/// check that may match several. What a use that needs one is checked
/// against.
/// With `legacy`, the count read is the one with hidden matches included
/// (`all_matched`), as a legacy string target acts on the first match.
fn one_only(map: &DiscoveryMap, legacy: bool) -> DiscoveryMap {
    let mut out = map.clone();
    for a in &mut out.areas {
        a.sightings.retain(|s| {
            let n = if legacy { s.all_matched() } else { s.matched };
            n != 0 && !only_many(n)
        });
        // A legacy use takes only what a saved script wrote as a legacy
        // string: a structured css locator vouched for its visible match.
        if legacy {
            a.saved_links = std::mem::take(&mut a.saved_legacy_links);
        }
        for p in &mut a.pages {
            // A chain or a narrowed locator counts only as a whole
            // (`whole_matched`), never by the keys of its links.
            p.elements
                .retain(|e| !only_many(if legacy { e.all_matched() } else { e.matched }) && plain_target(&e.locator));
        }
        a.saved_check_links.clear();
    }
    out
}

/// The keys in the named areas (and the bucket for no area) actually
/// counted matching several elements, never one: what `SEEN_ONLY_MANY`
/// is said of. One only not counted (`NOT_ONE`) is `NEVER_COUNTED`.
fn counted_many(map: &DiscoveryMap, areas: &[&str], legacy: bool) -> HashSet<SeenKey> {
    let wanted: Vec<String> = areas.iter().map(|a| area_key(a)).collect();
    let several = |n: u32| only_many(n) && counted(n);
    let mut out = HashSet::new();
    for a in map.areas.iter().filter(|a| {
        let key = area_key(&a.area);
        key.is_empty() || wanted.contains(&key)
    }) {
        for s in &a.sightings {
            if several(if legacy { s.all_matched() } else { s.matched }) {
                out.insert(s.key.clone());
            }
        }
        for e in a.pages.iter().flat_map(|p| p.elements.iter()).filter(|e| plain_target(&e.locator)) {
            if several(if legacy { e.all_matched() } else { e.matched }) {
                out.insert(e.key.clone());
            }
        }
    }
    out
}

/// The chains and narrowed locators seen matching exactly one element in
/// the named areas and in the bucket for no area, as their links.
fn whole_matched(map: &DiscoveryMap, areas: &[&str]) -> Vec<Vec<LocatorStep>> {
    let wanted: Vec<String> = areas.iter().map(|a| area_key(a)).collect();
    map.areas
        .iter()
        .filter(|a| {
            let key = area_key(&a.area);
            key.is_empty() || wanted.contains(&key)
        })
        .flat_map(|a| {
            let saved = if area_key(&a.area).is_empty() { Vec::new() } else { a.saved_wholes.clone() };
            let elements = a
                .pages
                .iter()
                .flat_map(|p| p.elements.iter())
                .filter(|e| !only_many(e.matched) && !plain_target(&e.locator))
                .map(|e| e.locator.links());
            let wholes = a.wholes.iter().filter(|w| w.matched == 1).map(|w| w.locator.links());
            elements.chain(wholes).chain(saved)
        })
        .collect()
}

/// The chains seen whole, matching at least one element, in the named
/// areas and in the bucket for no area, and the chains the named areas'
/// saved `expect_each` scripts use: as their links.
fn each_wholes_in(map: &DiscoveryMap, areas: &[&str]) -> Vec<Vec<LocatorStep>> {
    let wanted: Vec<String> = areas.iter().map(|a| area_key(a)).collect();
    map.areas
        .iter()
        .filter(|a| {
            let key = area_key(&a.area);
            key.is_empty() || wanted.contains(&key)
        })
        .flat_map(|a| {
            let saved = if area_key(&a.area).is_empty() { Vec::new() } else { a.saved_each_wholes.clone() };
            let elements = a
                .pages
                .iter()
                .flat_map(|p| p.elements.iter())
                .filter(|e| e.matched != 0 && !plain_target(&e.locator))
                .map(|e| e.locator.links());
            let wholes = a.wholes.iter().filter(|w| w.matched != 0).map(|w| w.locator.links());
            elements.chain(wholes).chain(saved)
        })
        .collect()
}

/// The chains and narrowed locators seen whole in the named areas and in
/// the bucket for no area whose count `keep` takes, as their links.
fn wholes_in(map: &DiscoveryMap, areas: &[&str], keep: impl Fn(u32) -> bool) -> Vec<Vec<LocatorStep>> {
    let wanted: Vec<String> = areas.iter().map(|a| area_key(a)).collect();
    map.areas
        .iter()
        .filter(|a| {
            let key = area_key(&a.area);
            key.is_empty() || wanted.contains(&key)
        })
        .flat_map(|a| a.wholes.iter())
        .filter(|w| keep(w.matched))
        .map(|w| w.locator.links())
        .collect()
}

/// Are `a` and `b` the same link, as a locator is matched: the same key,
/// and the same `nth`, `exact` and `visible`, an exact name or text the
/// same as written.
fn same_link(a: &LocatorStep, b: &LocatorStep) -> bool {
    a.seen_key().is_some()
        && a.seen_key() == b.seen_key()
        && a.nth == b.nth
        && a.exact == b.exact
        && a.visible == b.visible
        && (!a.exact || (a.name == b.name && a.text == b.text))
}

/// `same_link` for `written`, a link of a script, against `seen`, a link
/// seen as part of a whole: a data placeholder in `written`'s name, text or
/// css stands for the value `seen` holds there, each one fitted on its own
/// and anchored at both ends (`fit`, the text around it as written), and
/// what is left must then be the same link exactly. So a chain probed with
/// a real id (`div[data-cycle-id="274"]`, then button "Edit") counts as
/// seen matching one for the same chain written with
/// `{{setup.cycle_id}}`. A placeholder in a role never fits.
fn same_link_filled(seen: &LocatorStep, written: &LocatorStep) -> bool {
    if !link_holds_placeholder(written) {
        return same_link(seen, written);
    }
    if written.role.as_deref().is_some_and(holds_data_placeholder) || named_only_by_placeholder(written) {
        return false;
    }
    let mut filled = written.clone();
    let named = |w: &Option<String>, s: &Option<String>, css: bool| -> Option<Option<String>> {
        match (w.as_deref(), s.as_deref()) {
            (Some(w), Some(s)) if holds_data_placeholder(w) => {
                let fits = if css {
                    wild_fits(&css_pieces(w.trim()), s.trim())
                } else {
                    wild_fits(&name_pattern(w), &norm_name(s))
                };
                fits.then(|| Some(s.to_string()))
            }
            (Some(w), None) if holds_data_placeholder(w) => None,
            _ => Some(w.clone()),
        }
    };
    let (Some(name), Some(text), Some(css)) =
        (named(&written.name, &seen.name, false), named(&written.text, &seen.text, false), named(&written.css, &seen.css, true))
    else {
        return false;
    };
    filled.name = name;
    filled.text = text;
    filled.css = css;
    same_link(seen, &filled)
}

/// The values `pieces` (a saved name, text or selector with placeholders)
/// take in `seen` and in `got`, when both fit, each `got` value shaped as
/// its `seen` one (`same_shape`) and, in a selector, unable to leave its
/// value (`safe_filled`).
fn filled_like(pieces: &[Piece], seen: &str, got: &str, css: bool) -> bool {
    match (fit(pieces, seen), fit(pieces, got)) {
        (Some(stood), Some(took)) => same_shape(&stood, &took) && (!css || took.iter().all(|v| safe_filled(v))),
        _ => false,
    }
}

/// Run time: does `filled`, the run's copy of the saved link `saved`, fit
/// `seen`, a link of a whole seen matching one: `saved` fits it as the
/// save fitted it (`same_link_filled`), and every value the run put in
/// has the shape the probed value had there. A link with no placeholder
/// must be the seen one as written.
fn filled_fits(seen: &LocatorStep, saved: &LocatorStep, filled: &LocatorStep) -> bool {
    if !link_holds_placeholder(saved) {
        return same_link(seen, filled);
    }
    if link_holds_placeholder(filled) || !same_link_filled(seen, saved) {
        return false;
    }
    let field = |w: &Option<String>, s: &Option<String>, f: &Option<String>, css: bool| match (w.as_deref(), s.as_deref(), f.as_deref()) {
        (Some(w), Some(s), Some(f)) if holds_data_placeholder(w) => {
            if css {
                filled_like(&css_pieces(w.trim()), s.trim(), f.trim(), true)
            } else {
                filled_like(&name_pattern(w), &norm_name(s), &norm_name(f), false)
            }
        }
        (Some(w), _, _) if holds_data_placeholder(w) => false,
        _ => true,
    };
    field(&saved.name, &seen.name, &filled.name, false)
        && field(&saved.text, &seen.text, &filled.text, false)
        && field(&saved.css, &seen.css, &filled.css, true)
}

/// What the map has seen, read both ways: everything, for a use that may
/// match several (`takes_many`), and only what matched one, for any other.
/// `vouched` is everything but what only saved checks vouch for, to say
/// which of the two a refusal is about.
struct SeenBoth {
    any: Sightings,
    /// `any` with what a probe matched none of: for a use that passes with
    /// nothing there (`takes_none`).
    none: Sightings,
    one: Sightings,
    /// `one` for a legacy string target: hidden matches counted.
    one_legacy: Sightings,
    vouched: Sightings,
    many: HashSet<SeenKey>,
    many_legacy: HashSet<SeenKey>,
    /// Every chain and narrowed locator seen whole, at any count, none
    /// included: what a use that passes with nothing there takes whole
    /// (a probe of none files no link alone).
    none_wholes: Vec<Vec<LocatorStep>>,
    /// The chains and narrowed locators seen whole matching several,
    /// counted: what `SEEN_ONLY_MANY` is said of, written that way.
    many_wholes: Vec<Vec<LocatorStep>>,
    /// Every chain seen whole matching at least one element, and every
    /// chain a saved `expect_each` used: what an `expect_each` child is
    /// taken as.
    each_wholes: Vec<Vec<LocatorStep>>,
}

impl SeenBoth {
    fn new(map: &DiscoveryMap, areas: &[&str]) -> Self {
        // What was seen inside what (`holds_inside`, for a `:has`) is where
        // elements sit, not how many a locator matched: every seen chain.
        let chains = Sightings::new_strict(map, areas).chains;
        let none = Sightings::new(map, areas);
        let none_wholes = wholes_in(map, areas, |_| true);
        let map = &present_only(map);
        let many_wholes = wholes_in(map, areas, |m| only_many(m) && counted(m));
        let each_wholes = each_wholes_in(map, areas);
        let any = Sightings::new(map, areas);
        let mut one = Sightings::new_strict(&one_only(map, false), areas);
        one.whole = whole_matched(map, areas);
        one.chains = chains.clone();
        let mut one_legacy = Sightings::new_strict(&one_only(map, true), areas);
        one_legacy.chains = chains;
        let mut without_checks = map.clone();
        for a in &mut without_checks.areas {
            a.saved_check_links.clear();
        }
        SeenBoth {
            any,
            none,
            one,
            one_legacy,
            vouched: Sightings::new(&without_checks, areas),
            many: counted_many(map, areas, false),
            many_legacy: counted_many(map, areas, true),
            none_wholes,
            many_wholes,
            each_wholes,
        }
    }

    /// Is `chain`, an `expect_each` child inside its list, seen as that
    /// chain, written the same way? Its links seen apart are not enough.
    fn each_whole(&self, chain: &[LocatorStep]) -> bool {
        self.each_wholes.iter().any(|w| w.len() == chain.len() && w.iter().zip(chain).all(|(a, b)| same_link_filled(a, b)))
    }

    /// Is `chain`, used where a run passes with nothing there (`none`),
    /// seen as a whole at any count, none included, written the same way?
    fn none_whole(&self, chain: &[LocatorStep], none: bool) -> bool {
        none && self.none_wholes.iter().any(|w| w.len() == chain.len() && w.iter().zip(chain).all(|(a, b)| same_link_filled(a, b)))
    }

    /// Run time: [`SeenBoth::none_whole`] for `filled`, the run's copy of
    /// the saved chain `saved`, as `whole_filled` fits one.
    fn none_whole_filled(&self, saved: &[LocatorStep], filled: &[LocatorStep], none: bool) -> bool {
        none && saved.len() == filled.len()
            && self.none_wholes.iter().any(|w| {
                w.len() == saved.len() && w.iter().zip(saved).zip(filled).all(|((s, sv), f)| filled_fits(s, sv, f))
            })
    }

    /// Is `chain`, used this way (`many`), seen as a whole: a chain or a
    /// narrowed locator seen matching one, written the same way? Only a
    /// use that needs one asks; any other reads every link anyway.
    fn whole(&self, chain: &[LocatorStep], many: bool) -> bool {
        !many
            && self.one.whole.iter().any(|w| w.len() == chain.len() && w.iter().zip(chain).all(|(a, b)| same_link_filled(a, b)))
    }

    /// Run time: is `filled`, the run's copy of the saved chain `saved`,
    /// a whole seen matching one, each link fitting as `filled_fits` says?
    /// What lets a placeholder chain the save took as a whole
    /// (`same_link_filled`) through at run time too: its scope links were
    /// never counted on their own.
    fn whole_filled(&self, saved: &[LocatorStep], filled: &[LocatorStep], many: bool) -> bool {
        !many
            && saved.len() == filled.len()
            && self.one.whole.iter().any(|w| {
                w.len() == saved.len() && w.iter().zip(saved).zip(filled).all(|((s, sv), f)| filled_fits(s, sv, f))
            })
    }

    /// The sightings a locator used this way is checked against: `none`
    /// for a use that passes with nothing there (`takes_none`), `many` for
    /// a check that may match several, `legacy` for a legacy string target.
    fn for_use(&self, many: bool, legacy: bool, none: bool) -> &Sightings {
        match (many, legacy) {
            _ if none => &self.none,
            (true, _) => &self.any,
            (false, false) => &self.one,
            (false, true) => &self.one_legacy,
        }
    }

    /// The hint for `link` (one of `links`), refused for a use (`many`) of
    /// the action `kind`: when every one of `links` was seen but this use
    /// needs one, why it was not one (`SEEN_ONLY_IN_CHECKS`,
    /// `SEEN_ONLY_MANY`, `NEVER_COUNTED_NTH` or `NEVER_COUNTED`); for a use
    /// that needs one, a seen name holding the refused one as a part
    /// (`use_full_name`); else the closest seen locator, for that use, if
    /// there is one. Each but the first kind is followed by what this use
    /// needs (`wants`).
    #[allow(clippy::too_many_arguments)]
    fn hint(
        &self,
        link: &LocatorStep,
        links: &[LocatorStep],
        many: bool,
        legacy: bool,
        none: bool,
        page: Option<&str>,
        kind: &str,
        everywhere: bool,
    ) -> Option<String> {
        if links.iter().any(|l| link_holds_placeholder(l) && named_only_by_placeholder(l)) {
            return Some(ONLY_A_PLACEHOLDER.to_string());
        }
        if !many && links.iter().all(|l| self.any.has(l, true)) {
            if !links.iter().all(|l| self.vouched.has(l, true)) {
                return Some(SEEN_ONLY_IN_CHECKS.to_string());
            }
            // Probed as written and found matching several: said so, with
            // what it matched, never "probe it" again.
            // A placeholder one may fit another value matching one: it is
            // told to probe one (`NEVER_COUNTED_FILLED`) below.
            let whole_many = !links.iter().any(link_holds_placeholder)
                && self.many_wholes.iter().any(|w| w.len() == links.len() && w.iter().zip(links).all(|(a, b)| same_link(a, b)));
            if whole_many {
                return Some(format!("{SEEN_ONLY_MANY}{}", named_matches(&self.any.matching(link))));
            }
            let several = if legacy { &self.many_legacy } else { &self.many };
            if link.seen_key().is_some_and(|k| several.contains(&k)) {
                let matched = self.any.matching(link);
                // Exact, only one name is its own: the count was of the
                // names holding it, so it was never counted as written.
                if link.exact && matched.len() <= 1 {
                    let nth = links.iter().any(|l| l.nth.is_some());
                    return Some(if nth { NEVER_COUNTED_NTH } else { NEVER_COUNTED }.to_string());
                }
                return Some(format!("{SEEN_ONLY_MANY}{}", named_matches(&matched)));
            }
            if links.iter().any(link_holds_placeholder) {
                return Some(NEVER_COUNTED_FILLED.to_string());
            }
            if links.iter().any(|l| l.nth.is_some()) {
                return Some(NEVER_COUNTED_NTH.to_string());
            }
            return Some(NEVER_COUNTED.to_string());
        }
        let needs = wants(kind, many, everywhere, none);
        let full = if many { None } else { self.any.holders(link).first().map(|l| use_full_name(l)) };
        Some(match full.or_else(|| self.for_use(many, legacy, none).closest(link, page)) {
            Some(closest) => format!("{closest} {}{}", needs[..1].to_uppercase(), &needs[1..]),
            None => needs,
        })
    }
}

/// Does this action look for something rather than act on it?
fn is_check(action: &Action) -> bool {
    matches!(
        action,
        Action::WaitFor { .. }
            | Action::ExpectVisible { .. }
            | Action::ExpectHidden { .. }
            | Action::ExpectText { .. }
            | Action::ExpectContainsText { .. }
            | Action::ExpectCount { .. }
            | Action::ExpectEach { .. }
            | Action::ExpectOrder { .. }
            | Action::ExpectPosition { .. }
            | Action::ExpectAttribute { .. }
            | Action::ExpectStyle { .. }
            | Action::ExpectEnabled { .. }
            | Action::ExpectDisabled { .. }
            | Action::ExpectChecked { .. }
            | Action::ExpectFocused { .. }
            | Action::ExpectRow { .. }
            | Action::ExpectNoRow { .. }
            | Action::ExpectSorted { .. }
            | Action::ExpectRowCount { .. }
    )
}

/// The link's own words: its text or its accessible name, folded.
fn words(link: &LocatorStep) -> Vec<String> {
    [link.text.as_deref(), link.name.as_deref()]
        .into_iter()
        .flatten()
        .map(fold_name)
        .filter(|w| !w.is_empty())
        .collect()
}

/// Does any field of `link` hold a data placeholder?
fn link_holds_placeholder(link: &LocatorStep) -> bool {
    [&link.role, &link.name, &link.text, &link.css].into_iter().flatten().any(|v| holds_data_placeholder(v))
}

/// Is `link` in one of `roles`?
fn in_roles(link: &LocatorStep, roles: &[&str]) -> bool {
    link.role.as_deref().is_some_and(|r| roles.contains(&fold_name(r).as_str()))
}

/// Does this link name a date picker: one of `PICKER_ROLES` whose name has
/// "date" as a word, or "calendar" or "datepicker"; or a css selector
/// holding "datepicker", "date-picker" or "calendar"? Read without its
/// placeholders: a word a run would put in names nothing that was seen.
fn is_date_picker(link: &LocatorStep) -> bool {
    let by_role = in_roles(link, &PICKER_ROLES)
        && link.name.as_deref().is_some_and(|n| {
            let n = without_placeholders(n).to_lowercase();
            has_phrase(&n, "date") || n.contains("calendar") || n.contains("datepicker")
        });
    let by_css = link.css.as_deref().is_some_and(|c| {
        let c = without_placeholders(c).to_lowercase();
        c.contains("datepicker") || c.contains("date-picker") || c.contains("calendar")
    });
    by_role || by_css
}

// ---- what was seen ----

/// What the map has seen in the areas a check reads (and in the bucket
/// for no area), ready to compare.
struct Sightings {
    keys: HashSet<SeenKey>,
    /// Role folded, name as `norm_name` makes it, and the link as seen.
    roles: Vec<(String, String, LocatorStep)>,
    /// Text as `norm_name` makes it, and the link as seen.
    texts: Vec<(String, LocatorStep)>,
    /// Css selectors, state pseudo-classes left out.
    css: Vec<String>,
    /// Each seen chain's links' css (state pseudo-classes left out), in
    /// order, outermost first; `None` for a link that is not css.
    chains: Vec<Vec<Option<String>>>,
    /// The links the areas' saved scripts use, as written there.
    saved: Vec<LocatorStep>,
    /// The pages (`page_path`) each key was seen on.
    pages: std::collections::HashMap<SeenKey, HashSet<String>>,
    /// Chains and narrowed locators seen as a whole matching one, as their
    /// links (`whole_matched`): read only by a use that needs one.
    whole: Vec<Vec<LocatorStep>>,
    /// Every name seen in the areas: its role folded (`"text"` for a
    /// text), its name as `norm_name` makes it, and the link as seen. What
    /// a part of a name is read against (`holders`).
    names: Vec<(String, String, LocatorStep)>,
    /// Whether a part of a seen name counts at all: only for a use that may
    /// match several (`part_of`).
    parts_many: bool,
}

impl Sightings {
    fn new(map: &DiscoveryMap, areas: &[&str]) -> Self {
        Self::build(map, areas, false)
    }

    /// [`Sightings::new`] for a use that needs one: a css selector seen
    /// with a state pseudo-class is kept with it, so the selector without
    /// the state is not taken as seen matching one.
    fn new_strict(map: &DiscoveryMap, areas: &[&str]) -> Self {
        Self::build(map, areas, true)
    }

    fn build(map: &DiscoveryMap, areas: &[&str], strict: bool) -> Self {
        let states = |c: &str| if strict { c.trim().to_string() } else { strip_states(c) };
        let mut s = Sightings {
            whole: Vec::new(),
            names: Vec::new(),
            parts_many: !strict,
            keys: seen_keys(map, areas),
            roles: Vec::new(),
            texts: Vec::new(),
            css: Vec::new(),
            chains: Vec::new(),
            saved: Vec::new(),
            pages: seen_pages(map, areas),
        };
        let wanted: Vec<String> = areas.iter().map(|a| area_key(a)).filter(|k| !k.is_empty()).collect();
        for a in map.areas.iter().filter(|a| wanted.contains(&area_key(&a.area))) {
            s.saved.extend(a.saved_links.iter().cloned());
            s.saved.extend(a.saved_check_links.iter().cloned());
            s.saved.extend(a.saved_none_links.iter().cloned());
        }
        for l in seen_links(map, areas) {
            if let Some(role) = &l.role {
                s.roles.push((fold_name(role), norm_name(l.name.as_deref().unwrap_or("")), l.clone()));
            } else if let Some(t) = &l.text {
                s.texts.push((norm_name(t), l.clone()));
            } else if let Some(c) = &l.css {
                s.css.push(states(c));
            }
        }
        for (r, n, l) in s.roles.iter().filter(|(_, n, _)| !n.is_empty()) {
            s.names.push((r.clone(), n.clone(), l.clone()));
        }
        for (n, l) in s.texts.iter().filter(|(n, _)| !n.is_empty()) {
            s.names.push(("text".to_string(), n.clone(), l.clone()));
        }
        for t in seen_locators(map, areas) {
            let links = t.links();
            if links.len() > 1 {
                s.chains.push(links.iter().map(|l| l.css.as_deref().map(states)).collect());
            }
        }
        s
    }

    /// Was `link` seen: its exact key, or a sighting its name or selector
    /// matches. With `wild`, a data placeholder in it stands for a seen
    /// value. A part of a seen name is not this (`part_of`).
    fn has(&self, link: &LocatorStep, wild: bool) -> bool {
        if link.seen_key().is_some_and(|k| self.keys.contains(&k)) || self.saved.contains(link) {
            return true;
        }
        // The placeholders are found in the raw name before it is folded
        // (`name_pattern`): folding first would turn a `{{Setup.x}}` the
        // run never fills into one it does.
        // A name that is only a placeholder would fit anything: never.
        let fits = |raw: &str, seen: &str| {
            norm_name(raw) == seen || (wild && !only_placeholder(raw) && wild_fits(&name_pattern(raw), seen))
        };
        if let Some(role) = &link.role {
            let (r, n) = (fold_name(role), link.name.as_deref().unwrap_or(""));
            return self.roles.iter().any(|(sr, sn, _)| *sr == r && fits(n, sn));
        }
        if let Some(t) = &link.text {
            return self.texts.iter().any(|(sn, _)| fits(t, sn));
        }
        link.css.as_deref().is_some_and(|c| self.css_seen(c, wild, 0))
    }

    /// The distinct seen names of `link`'s role (or texts, for a text) that
    /// hold its name as a part, folded by `norm_name`, the way a run
    /// matches a name (`locator::name_matches`), as they were seen. None
    /// for a css link, an empty name, or one holding a data placeholder.
    fn holders(&self, link: &LocatorStep) -> Vec<&LocatorStep> {
        let (kind, raw) = match (&link.role, &link.name, &link.text) {
            (Some(r), Some(n), _) => (fold_name(r), n.as_str()),
            (None, _, Some(t)) => ("text".to_string(), t.as_str()),
            _ => return Vec::new(),
        };
        let want = norm_name(raw);
        if want.is_empty() || holds_data_placeholder(raw) {
            return Vec::new();
        }
        let mut out: Vec<&LocatorStep> = Vec::new();
        let mut held: Vec<&str> = Vec::new();
        for (k, n, l) in &self.names {
            if *k == kind && n.contains(&want) && !held.contains(&n.as_str()) {
                held.push(n);
                out.push(l);
            }
        }
        out
    }

    /// The distinct seen names `link` matches as a run matches a name
    /// (`locator::name_matches`, `"exact"` as written), in its role (a
    /// text, for a text), seen on a page its own key was seen on, as they
    /// were seen. None for a css link or one with no name.
    fn matching(&self, link: &LocatorStep) -> Vec<&LocatorStep> {
        let (kind, raw) = match (&link.role, &link.name, &link.text) {
            (Some(r), Some(n), _) => (fold_name(r), n.as_str()),
            (None, _, Some(t)) => ("text".to_string(), t.as_str()),
            _ => return Vec::new(),
        };
        let pages = link.seen_key().and_then(|k| self.pages.get(&k));
        let mut out: Vec<&LocatorStep> = Vec::new();
        let mut held: Vec<&str> = Vec::new();
        for (k, n, l) in &self.names {
            let words = l.name.as_deref().or(l.text.as_deref()).unwrap_or("");
            let same_page = match (pages, l.seen_key().and_then(|key| self.pages.get(&key))) {
                (Some(mine), Some(theirs)) => !mine.is_disjoint(theirs),
                _ => true,
            };
            if *k == kind
                && same_page
                && crate::browser::locator::name_matches(words, raw, link.exact)
                && !held.contains(&n.as_str())
            {
                held.push(n);
                out.push(l);
            }
        }
        out
    }

    /// Does `chain`, a plain one-link locator (no `nth`, no `"visible":
    /// false`, not `"exact"`) whose name or text is at least
    /// `MIN_TYPED_LEN` characters once folded, name a part of a seen name
    /// of its role? Only for a use that may match several (`parts_many`):
    /// any other needs the whole name (see the module's notes).
    fn part_of(&self, chain: &[LocatorStep]) -> bool {
        let [link] = chain else { return false };
        let raw = link.name.as_deref().or(link.text.as_deref()).unwrap_or("");
        self.parts_many
            && link.nth.is_none()
            && link.visible != Some(false)
            && !link.exact
            && norm_name(raw).chars().count() >= MIN_TYPED_LEN
            && !self.holders(link).is_empty()
    }

    /// Does the selector `base` (state pseudo-classes and filters already
    /// left out) match the seen selector `seen`?
    fn base_fits(base: &str, seen: &str, wild: bool) -> bool {
        base == seen || (wild && wild_fits(&css_pieces(base), seen))
    }

    /// A css selector: its state pseudo-classes left out, the rest seen,
    /// and each `:not`/`:has` filter's inner part seen as that filter
    /// needs.
    fn css_seen(&self, css: &str, wild: bool, depth: u8) -> bool {
        let (base, inners) = split_filters(&strip_states(css));
        if base.is_empty() || !self.css.iter().any(|s| Self::base_fits(&base, s, wild)) {
            return false;
        }
        inners.iter().all(|(kind, x)| match kind {
            Filter::Not => match filter_attributes(x) {
                Some(names) => names.iter().all(|n| self.carries(&base, n, wild)),
                None => depth < 3 && self.css_seen(x, wild, depth + 1),
            },
            Filter::Has => self.holds_inside(&base, x, wild),
        })
    }

    /// Was `base` seen with attribute `attr` on it: a seen selector that is
    /// `base` followed by attribute filters, one of them `attr`'s?
    fn carries(&self, base: &str, attr: &str, wild: bool) -> bool {
        self.css.iter().any(|s| {
            attribute_tails(s).into_iter().any(|(before, tail)| {
                !before.is_empty()
                    && filter_attributes(tail).is_some_and(|names| names.iter().any(|n| n == attr))
                    && Self::base_fits(base, before, wild)
            })
        })
    }

    /// Does the seen selector `t` (one element) match the inner part `x` of
    /// a `:has`: carrying each attribute when `x` is only attribute
    /// filters, or matching `x` otherwise?
    fn inner_fits(x: &str, t: &str, wild: bool) -> bool {
        match filter_attributes(x) {
            Some(names) => attribute_tails(t)
                .into_iter()
                .chain(std::iter::once(("", t)).filter(|(_, t)| t.starts_with('[')))
                .any(|(_, tail)| filter_attributes(tail).is_some_and(|got| names.iter().all(|n| got.contains(n)))),
            None => Self::base_fits(&strip_states(x), t, wild),
        }
    }

    /// Was `x` seen inside `base`: a seen selector naming `base` as an
    /// ancestor of an element matching `x` (`.card .badge`), or a seen
    /// chain with a link matching `base` outside one matching `x`?
    fn holds_inside(&self, base: &str, x: &str, wild: bool) -> bool {
        let in_selector = self.css.iter().any(|s| {
            descendant_splits(s).into_iter().any(|(ancestor, inside)| {
                Self::base_fits(base, ancestor, wild)
                    && std::iter::once(inside)
                        .chain(descendant_splits(inside).into_iter().map(|(_, d)| d))
                        .any(|d| Self::inner_fits(x, d, wild))
            })
        });
        let in_chain = self.chains.iter().any(|chain| {
            chain.iter().enumerate().any(|(i, outer)| {
                outer.as_deref().is_some_and(|o| Self::base_fits(base, o, wild))
                    && chain[i + 1..].iter().flatten().any(|inner| Self::inner_fits(x, inner, wild))
            })
        });
        in_selector || in_chain
    }

    /// Every seen selector, and every descendant part of one: what a
    /// filled `:has` part may be shaped by.
    fn css_parts(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for s in &self.css {
            out.push(s);
            out.extend(descendant_splits(s).into_iter().map(|(_, d)| d));
        }
        out.extend(self.chains.iter().flatten().flatten().map(String::as_str));
        out
    }

    /// Every attribute-filter tail of a seen selector: what a filled
    /// attribute filter may be shaped by.
    fn attribute_parts(&self) -> Vec<&str> {
        self.css.iter().flat_map(|s| attribute_tails(s).into_iter().map(|(_, t)| t)).collect()
    }

    /// Does `filled`, the run's copy of `template` with its data
    /// placeholders filled in, match a sighting `template` matches, each
    /// value a placeholder took the shape of the seen value it stands for
    /// there? A placeholder left unfilled never does.
    fn fills(&self, template: &LocatorStep, filled: &LocatorStep) -> bool {
        if link_holds_placeholder(filled) {
            return false;
        }
        // Seen exactly as filled. A css selector's filters are not taken
        // from `has`, which reads an attribute filter by its name alone:
        // the value filled into one must be shaped below.
        if filled.seen_key().is_some_and(|k| self.keys.contains(&k))
            || (filled.css.is_none() && self.has(filled, false))
        {
            return true;
        }
        if let Some(role) = &filled.role {
            let r = fold_name(role);
            let seen = self.roles.iter().filter(|(sr, ..)| *sr == r).map(|(_, n, _)| n.as_str());
            return shaped(
                &name_pattern(template.name.as_deref().unwrap_or("")),
                &norm_name(filled.name.as_deref().unwrap_or("")),
                seen,
            );
        }
        if let Some(t) = &filled.text {
            let seen = self.texts.iter().map(|(n, _)| n.as_str());
            return shaped(&name_pattern(template.text.as_deref().unwrap_or("")), &norm_name(t), seen);
        }
        let (Some(written), Some(c)) = (template.css.as_deref(), filled.css.as_deref()) else { return false };
        if !self.css_seen(written, true, 0) {
            return false;
        }
        let (want_base, want_inners) = split_filters(&strip_states(written));
        let (got_base, got_inners) = split_filters(&strip_states(c));
        // A value filled into a selector must stay inside the value or the
        // token it was put in (`safe_filled`).
        let stays = |p: &[Piece], got: &str| fit(p, got).is_some_and(|took| took.iter().all(|v| safe_filled(v)));
        let base_pieces = css_pieces(&want_base);
        if want_inners.len() != got_inners.len()
            || !stays(&base_pieces, &got_base)
            || !shaped(&base_pieces, &got_base, self.css.iter().map(String::as_str))
        {
            return false;
        }
        want_inners.iter().zip(&got_inners).all(|((kind, want), (got_kind, got))| {
            let p = css_pieces(want);
            if kind != got_kind {
                return false;
            }
            if !has_wild(&p) {
                return want == got;
            }
            let Some(took) = fit(&p, got) else { return false };
            if !took.iter().all(|v| safe_filled(v)) {
                return false;
            }
            let parts = if filter_attributes(want).is_some() { self.attribute_parts() } else { self.css_parts() };
            let stood: Vec<Vec<String>> = parts.into_iter().filter_map(|s| fit(&p, s)).collect();
            // An attribute filter is checked by its attribute's name at
            // save (`carries`), so a value may have nothing seen to be
            // shaped by (`*=` against a seen `=`): then filled, non-empty
            // and unable to leave its value is all that can be asked.
            stood.is_empty() || stood.iter().any(|s| same_shape(s, &took))
        })
    }

    /// The seen locator closest to `link`, as "did you mean <role>
    /// "<name>"?": only one in the same role (a text locator offers only
    /// text), within `SUGGEST_WITHIN`; one seen on `page` (the refused
    /// step's area page, `page_path`) first when there is one that close,
    /// else the fewest edits anywhere. `None` when no name in that role is
    /// that close, or for a css link (a selector has no name to offer).
    fn closest(&self, link: &LocatorStep, page: Option<&str>) -> Option<String> {
        let (role, want) = match (&link.role, &link.text) {
            (Some(r), _) => (fold_name(r), norm_name(link.name.as_deref().unwrap_or(""))),
            (None, Some(t)) => ("text".to_string(), norm_name(t)),
            _ => return None,
        };
        if want.is_empty() {
            return None;
        }
        let candidates = self
            .roles
            .iter()
            .map(|(r, n, l)| (r.as_str(), n.as_str(), l))
            .chain(self.texts.iter().map(|(n, l)| ("text", n.as_str(), l)));
        let on_page = |l: &LocatorStep| {
            page.filter(|p| !p.is_empty()).is_some_and(|p| {
                l.seen_key().and_then(|k| self.pages.get(&k)).is_some_and(|pages| pages.contains(p))
            })
        };
        // Ranked by being off the page first, then by edits: one seen on
        // the page wins over a closer one seen only elsewhere.
        let mut best: Option<((bool, usize), &LocatorStep)> = None;
        for (r, n, l) in candidates {
            if n.is_empty() || r != role {
                continue;
            }
            let edits = edit_distance(&want, n);
            let longer = want.chars().count().max(n.chars().count());
            if edits * SUGGEST_WITHIN.1 > longer * SUGGEST_WITHIN.0 {
                continue;
            }
            let rank = (!on_page(l), edits);
            if best.as_ref().is_none_or(|(b, _)| rank < *b) {
                best = Some((rank, l));
            }
        }
        let quoted = |s: &str| s.replace('"', "\\\"");
        best.and_then(|(_, l)| match (&l.role, &l.text) {
            (Some(r), _) => Some(format!("did you mean {r} \"{}\"?", quoted(l.name.as_deref().unwrap_or("")))),
            (None, Some(t)) => Some(format!("did you mean text \"{}\"?", quoted(t))),
            _ => None,
        })
    }
}

/// Does `got` fit `pieces`, and some `seen` value fit them too, with each
/// value a placeholder took in `got` the shape of the one it stood for in
/// that seen value?
fn shaped<'a>(pieces: &[Piece], got: &str, seen: impl Iterator<Item = &'a str>) -> bool {
    let Some(took) = fit(pieces, got) else { return false };
    seen.into_iter().any(|s| fit(pieces, s).is_some_and(|stood| same_shape(&stood, &took)))
}

/// How many characters must be added, dropped or changed to turn `a` into
/// `b`.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let here = row[j + 1];
            row[j + 1] = if ca == *cb { prev } else { 1 + prev.min(here).min(row[j]) };
            prev = here;
        }
    }
    row[b.len()]
}

// ---- the script's own data ----

/// What exempts a locator at one place in a script, besides a sighting.
struct Own<'a> {
    /// Values typed before it, folded, each at least `MIN_TYPED_LEN`.
    typed: &'a [String],
    case_text: &'a [String],
    /// Whether the locator is only looked for.
    check: bool,
    /// The project's Test files the script has uploaded by this step,
    /// folded, each at least `MIN_TYPED_LEN`.
    files: &'a [String],
    /// Their sizes as the app shows them, folded.
    sizes: &'a [String],
    /// The dates the script picked or typed so far.
    dates: &'a [Date],
}

/// Is `link` the script's own data: a value typed earlier, a name the case
/// says (a check only, of a link with no data placeholder), a Test file it uploaded or that file's size, or a
/// date-picker day that is a date it picked or typed?
fn exempt(link: &LocatorStep, own: &Own) -> bool {
    let own_words = words(link);
    let has = |list: &[String]| list.iter().any(|t| own_words.iter().any(|w| has_phrase(w, t)));
    // A value typed earlier (already at least 3 characters) as whole words
    // inside the locator's text or name: the record the script created.
    let typed_here = has(own.typed);
    // The locator's whole name, of at least 3 characters, as whole words in
    // what the case says. Never for a link holding a data placeholder: the
    // run checks such a link again once filled in, without the case's text
    // (`check_resolved_inputs_with`), so it would pass here and be Blocked
    // there.
    let in_case = own.check
        && !link_holds_placeholder(link)
        && own_words
            .iter()
            .any(|w| w.chars().count() >= MIN_TYPED_LEN && own.case_text.iter().any(|t| has_phrase(t, w)));
    let own_file = has(own.files) || has(own.sizes);
    let picked = in_roles(link, &DAY_ROLES)
        && own_words.iter().filter_map(|w| parse_date(w)).any(|d| own.dates.contains(&d));
    typed_here || in_case || own_file || picked
}

/// Is `link` a date-picker day (`DAY_ROLES`) named `dd/mm/yyyy`, inside a
/// date picker seen in these areas: an earlier link of its chain?
fn in_seen_date_picker(link: &LocatorStep, chain: &[LocatorStep], seen: &Sightings) -> bool {
    let Some(at) = chain.iter().position(|l| l == link) else { return false };
    let name = link.name.as_deref().unwrap_or("");
    in_roles(link, &DAY_ROLES)
        && is_ddmmyyyy(name.trim())
        && chain[..at].iter().any(|o| is_date_picker(o) && seen.has(o, true))
}

/// The first of `links` (links of `chain`) neither seen nor exempt.
/// A plain one-link locator naming a part of a seen name counts for a use
/// that may match several (`Sightings::part_of`).
fn first_unseen<'a>(links: &'a [LocatorStep], chain: &[LocatorStep], seen: &Sightings, own: &Own) -> Option<&'a LocatorStep> {
    if links.len() == chain.len() && seen.part_of(chain) {
        return None;
    }
    links.iter().find(|l| !seen.has(l, true) && !exempt(l, own) && !in_seen_date_picker(l, chain, seen))
}

/// The files `actions` upload, as named.
fn uploads<'a>(actions: impl Iterator<Item = &'a Action>) -> Vec<String> {
    actions
        .filter_map(|a| match a {
            Action::Upload { file, .. } => Some(file.clone()),
            _ => None,
        })
        .collect()
}

/// The `files` among `uploaded`: what the script uploads that is one of
/// the project's Test files.
fn own_test_files<'a>(uploaded: &[String], files: &'a [TestFile]) -> Vec<&'a TestFile> {
    files.iter().filter(|f| uploaded.iter().any(|n| n.trim().eq_ignore_ascii_case(f.name.trim()))).collect()
}

/// Their names folded, those long enough to exempt anything.
fn exempting(files: &[&TestFile]) -> Vec<String> {
    files.iter().map(|f| fold_name(&f.name)).filter(|n| n.chars().count() >= MIN_TYPED_LEN).collect()
}

/// Their sizes as the app shows a size, folded: one decimal, in KB and in
/// MB (1 KB = 1024 bytes), as "240.0 KB" and "0.2 MB"
/// (`test_files::human_size`'s form).
fn shown_sizes(files: &[&TestFile]) -> Vec<String> {
    files
        .iter()
        .flat_map(|f| {
            let b = f.size as f64;
            [format!("{:.1} kb", b / 1024.0), format!("{:.1} mb", b / (1024.0 * 1024.0))]
        })
        .collect()
}

/// The text inputs a `use_component` gives, as text.
fn text_inputs(action: &Action) -> Vec<String> {
    match action {
        Action::UseComponent { inputs, .. } => inputs
            .values()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

// ---- the checks ----

/// One locator, or one page path, a script names that the map has never
/// seen: the step that names it and how it reads (`Target::describe`, or
/// the path). Or a use of a component the step cannot make: then
/// `refused` says why, without the step ("Pick a date needs day"), and
/// `locator` is the component's name as the script writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unseen {
    pub step: i32,
    pub locator: String,
    pub refused: Option<String>,
}

impl Unseen {
    /// The sentence a component change is refused with for a script that
    /// uses it, after "Case <id>, ".
    pub fn broken_by_change(&self) -> String {
        self.broken_by_change_hinted(None)
    }

    /// [`Unseen::broken_by_change`], with the refusal's hint: a locator
    /// seen only matching several (`SEEN_ONLY_MANY`, `SEEN_ONLY_IN_CHECKS`)
    /// says so, as `never_seen` puts it.
    pub fn broken_by_change_hinted(&self, hint: Option<&str>) -> String {
        match (&self.refused, hint) {
            (Some(why), _) => format!("step {}: {why}; this change would break it.", self.step),
            (None, Some(h)) if is_own_sentence(h) => {
                format!("step {}: {} This change would break it.", self.step, never_seen(&self.locator, Some(h)))
            }
            (None, _) => format!(
                "step {}: {} was never seen on the live app; this change would break it.",
                self.step, self.locator
            ),
        }
    }

    /// The sentence a save refuses this with, naming the closest seen
    /// locator (`hint`) when there is one.
    fn refusal(&self, hint: Option<&str>) -> String {
        match &self.refused {
            Some(why) => format!("Step {}: {why}", self.step),
            None => refusal(self.step, &self.locator, hint),
        }
    }
}

/// A failure of the check, with the closest seen locator to offer, and
/// the locator itself when the failure is only that it was never seen
/// (`None` for a page address or a component use the step cannot make).
struct Found {
    unseen: Unseen,
    hint: Option<String>,
    target: Option<Target>,
}

/// Adds to `map`, in memory, the links each of `scripts` uses, under the
/// script's own area: a link a saved script of an area uses, written
/// exactly as it is there, counts as seen in that area. A script with no
/// area adds nothing (it would count everywhere), and neither does a link
/// that holds a placeholder (`{{...}}`) or is a component's input place.
/// `map` is never written back: these links are not sightings.
pub fn add_saved_scripts(map: &mut DiscoveryMap, scripts: &[CaseScript], components: &ComponentFile) {
    for script in scripts {
        let Some(area) = script.area_name() else { continue };
        let key = area_key(area);
        if key.is_empty() {
            continue;
        }
        let i = match map.areas.iter().position(|a| area_key(&a.area) == key) {
            Some(i) => i,
            None => {
                map.areas.push(AreaMap { area: area.to_string(), ..AreaMap::default() });
                map.areas.len() - 1
            }
        };
        let a = &mut map.areas[i];
        for (target, links, class) in script_links(script, components) {
            // A use that passes with nothing there vouches only for such
            // uses: it may have been saved on a probe of none, or, for a
            // `when_visible`'s own locator, on a count of several.
            if class == SavedUse::None {
                for link in links {
                    let known = a.saved_links.contains(&link) || a.saved_check_links.contains(&link);
                    if !known && !a.saved_none_links.contains(&link) {
                        a.saved_none_links.push(link);
                    }
                }
                continue;
            }
            if class == SavedUse::Whole {
                if links.len() == target.links().len() && !a.saved_each_wholes.contains(&links) {
                    a.saved_each_wholes.push(links);
                }
                continue;
            }
            let many = class == SavedUse::Many;
            // A plain one-link target outside a check that may match several
            // vouches by its key; a chain or a narrowed one only whole, its
            // links alone only for such a check.
            let plain = plain_target(&target);
            if !many && !plain && links.len() == target.links().len() && !a.saved_wholes.contains(&links) {
                a.saved_wholes.push(links.clone());
            }
            for link in links {
                if !many && plain {
                    if target.is_legacy() && !a.saved_legacy_links.contains(&link) {
                        a.saved_legacy_links.push(link.clone());
                    }
                    if !a.saved_links.contains(&link) {
                        a.saved_check_links.retain(|l| *l != link);
                        a.saved_none_links.retain(|l| *l != link);
                        a.saved_links.push(link);
                    }
                } else if !a.saved_links.contains(&link) && !a.saved_check_links.contains(&link) {
                    a.saved_none_links.retain(|l| *l != link);
                    a.saved_check_links.push(link);
                }
            }
        }
    }
}

/// The strictest use `component`'s actions make of its input `name`, as a
/// save checks the locator the input gives there (`input_locators`): one
/// that needs one seen matching exactly one, else a check that may match
/// several, else a use that passes with nothing there. A use inside a
/// `repeat_while_visible`'s `then` may match several, one per row. `None`
/// when the project has no such component, or the component never puts
/// the input into a locator.
fn input_use(components: &ComponentFile, component: &str, name: &str) -> Option<SavedUse> {
    let c = find(components, component)?;
    let name = name.trim();
    let rank = |u: SavedUse| match u {
        SavedUse::One => 2,
        SavedUse::Many => 1,
        SavedUse::None => 0,
        SavedUse::Whole => 1,
    };
    let mut strictest: Option<SavedUse> = None;
    for top in &c.actions {
        for w in top.each() {
            let in_loop = matches!(top, Action::RepeatWhileVisible { .. }) && !std::ptr::eq(top, w);
            // Each target the input feeds, held as its own place in the action.
            for (i, t) in own_targets(w).into_iter().enumerate() {
                if !t.links().iter().any(|l| l.input.as_deref().is_some_and(|n| n.trim() == name)) {
                    continue;
                }
                let class = if takes_none(w) {
                    SavedUse::None
                } else if in_loop || many_at(w, i) {
                    SavedUse::Many
                } else {
                    SavedUse::One
                };
                if strictest.is_none_or(|s| rank(class) > rank(s)) {
                    strictest = Some(class);
                }
            }
        }
    }
    strictest
}

/// How a saved script used a locator, for what it vouches for
/// (`add_saved_scripts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SavedUse {
    /// A use that needs one seen matching exactly one.
    One,
    /// A check that may match several (`takes_many`).
    Many,
    /// A use that passes with nothing there (`takes_none`), a
    /// `when_visible`'s own locator included.
    None,
    /// An `expect_each` child inside its list: vouches for that chain as a
    /// whole, never for its links alone.
    Whole,
}

/// Every link `script` names as written, a component's target inputs
/// included, leaving out any that holds a placeholder or an input place;
/// each with how its action used it (`SavedUse`). A component's target
/// input counts as the strictest use the component makes of it
/// (`input_use`), as a save checked it; one of a component the project
/// does not have, or never puts into a locator, is left out: it vouches
/// for nothing. Each action's own targets only (`own_targets`): a
/// `when_visible`'s `then` actions are listed on their own.
fn script_links(script: &CaseScript, components: &ComponentFile) -> Vec<(Target, Vec<LocatorStep>, SavedUse)> {
    let mut out: Vec<(Target, Vec<LocatorStep>, SavedUse)> = Vec::new();
    for action in script.steps.iter().flat_map(|s| s.actions.iter()).flat_map(Action::each) {
        let mut targets: Vec<(Target, SavedUse)> = own_targets(action)
            .into_iter()
            .enumerate()
            .map(|(i, t)| {
                let class = if takes_none(action) {
                    SavedUse::None
                } else if many_at(action, i) {
                    SavedUse::Many
                } else {
                    SavedUse::One
                };
                (t.clone(), class)
            })
            .collect();
        if let Action::UseComponent { component, inputs } = action {
            // Each target input as strictly as its uses inside the
            // component checked it; one the project cannot place vouches
            // for nothing.
            for (name, v) in inputs.iter().filter(|(_, v)| v.is_object() || v.is_array()) {
                let Some(class) = input_use(components, component, name) else { continue };
                if let Ok(t) = serde_json::from_value::<Target>(v.clone()) {
                    targets.push((t, class));
                }
            }
        }
        targets.extend(each_child_chain(action).map(|chain| (chain, SavedUse::Whole)));
        for (target, class) in targets {
            let links: Vec<LocatorStep> = target
                .links()
                .into_iter()
                .filter(|link| {
                    let placeholder = serde_json::to_string(link).map_or(true, |j| j.contains("{{"));
                    link.input.is_none() && !placeholder
                })
                .collect();
            out.push((target, links, class));
        }
    }
    out
}

/// The project's map as a save checks it: `load_map`, with the links of
/// every saved script under `root` that vouches for them here (`vouches`)
/// added to its area (`add_saved_scripts`), the replaced one included,
/// read as `store::list_scripts` reads them.
pub fn load_checked_map(root: &std::path::Path, org: &str, project: &str) -> Result<DiscoveryMap, String> {
    load_checked_map_with(root, org, project, &super::store::list_scripts(root))
}

/// [`load_checked_map`] with the saved scripts already read, so a save
/// that checks more than once reads them once.
pub fn load_checked_map_with(
    root: &std::path::Path,
    org: &str,
    project: &str,
    saved: &[CaseScript],
) -> Result<DiscoveryMap, String> {
    let mut map = load_map(root, org, project)?;
    // What was not seen for 30 days does not count, though its area has
    // not been written since to let it go.
    super::discovery_map::without_stale(&mut map, super::sessions::now_ms());
    let legacy = legacy_scripts_count(root, org, project);
    let vouching: Vec<CaseScript> = saved.iter().filter(|s| vouches(s, org, project, legacy)).cloned().collect();
    // Its components, for how a script's target inputs were checked: one
    // that cannot be read places no input, so none vouches.
    let components = super::components::load_components(root, org, project).unwrap_or_default();
    add_saved_scripts(&mut map, &vouching, &components);
    // Where each area arrives, for "did you mean" to prefer that page. An
    // areas file that cannot be read only costs the preference.
    if let Ok(nav) = super::nav::load_nav(root, org, project) {
        add_area_pages(&mut map, &nav);
    }
    Ok(map)
}

/// Each recorded area's arrival page (`page_path` of its `arrived`), kept
/// in memory on its area of `map` (`AreaMap::area_page`), an area the map
/// has no entry for added empty. An area with no arrival recorded is left
/// without one.
pub fn add_area_pages(map: &mut DiscoveryMap, nav: &NavFile) {
    for m in &nav.modules {
        let key = area_key(&m.area);
        let page = page_path(&m.arrived);
        if key.is_empty() || m.arrived.trim().is_empty() {
            continue;
        }
        match map.areas.iter_mut().find(|a| area_key(&a.area) == key) {
            Some(a) => a.area_page = page,
            None => map.areas.push(AreaMap { area: m.area.clone(), area_page: page, ..AreaMap::default() }),
        }
    }
}

/// The arrival page of the area named `area` in `map`, if it is known.
fn area_page<'a>(map: &'a DiscoveryMap, area: Option<&str>) -> Option<&'a str> {
    let key = area_key(area?);
    map.areas
        .iter()
        .find(|a| !key.is_empty() && area_key(&a.area) == key)
        .map(|a| a.area_page.as_str())
        .filter(|p| !p.is_empty())
}

/// Is this script stamped as saved in `org`/`project`, compared as the
/// project's files are named (`project_slug`)?
fn same_project(script: &CaseScript, org: &str, project: &str) -> bool {
    match (&script.organization, &script.project) {
        (Some(o), Some(p)) => project_slug(o, p) == project_slug(org, project),
        _ => false,
    }
}

/// Was this script saved before scripts were stamped?
fn unstamped(script: &CaseScript) -> bool {
    script.organization.is_none() && script.project.is_none() && !script.checked
}

/// Do `script`'s own locators count as seen for a check in `org`/`project`:
/// a checked save of this project, or, with `legacy` (see
/// `legacy_scripts_count`), one saved before scripts were stamped?
pub fn vouches(script: &CaseScript, org: &str, project: &str, legacy: bool) -> bool {
    if unstamped(script) {
        return legacy;
    }
    script.checked && same_project(script, org, project)
}

/// Whether a save that keeps steps of `old` unchecked (an unchanged resave,
/// or a repair, which checks only the steps it declares) still vouches:
/// when `old` did for this project, or, saved before scripts were stamped,
/// when `legacy` says such scripts count here (`legacy_scripts_count`).
/// Otherwise the save is stamped unchecked.
pub fn vouch_carries_over(old: &CaseScript, org: &str, project: &str, legacy: bool) -> bool {
    if unstamped(old) {
        return legacy;
    }
    old.checked && same_project(old, org, project)
}

/// Scripts saved before they were stamped name no project, so they count
/// only where they can belong to no other: exactly one project under
/// `root` has recorded areas, and it is `org`/`project`. An areas file
/// that cannot be read makes none of them count: it could be any project's.
pub fn legacy_scripts_count(root: &std::path::Path, org: &str, project: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(root.join("projects")) else { return false };
    let mut with_areas: Vec<String> = Vec::new();
    for e in entries.flatten() {
        let Some(name) = e.file_name().to_str().filter(|n| n.ends_with(".nav.json")).map(str::to_string) else {
            continue;
        };
        let nav = std::fs::read_to_string(e.path())
            .ok()
            .and_then(|s| serde_json::from_str::<NavFile>(s.strip_prefix('\u{feff}').unwrap_or(&s)).ok());
        let Some(nav) = nav else { return false };
        if !nav.modules.is_empty() {
            with_areas.push(name);
        }
    }
    with_areas.len() == 1 && with_areas[0] == format!("{}.nav.json", project_slug(org, project))
}

/// How many refusals a refused save lists before it says only how many
/// more there are.
pub const MAX_LISTED: usize = 50;

/// `lines`, one to a line: the first `MAX_LISTED`, then "and N more" for
/// the rest. One line is exactly that line.
pub fn refusal_list(lines: &[String]) -> String {
    let mut out: Vec<String> = lines.iter().take(MAX_LISTED).cloned().collect();
    if lines.len() > MAX_LISTED {
        out.push(format!("and {} more", lines.len() - MAX_LISTED));
    }
    out.join("\n")
}

/// What the save check says of a script or a component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeenVerdict {
    /// Nothing is refused.
    Passed,
    /// Refused for something other than a locator never seen (a page
    /// address never seen, or a component use the step cannot make): the
    /// first such refusal, alone, as a save has always answered it.
    Other(String),
    /// Refused only for locators never seen: every one, in step (or
    /// action) order and then link order, each with its "did you mean".
    Unseen(Vec<String>),
}

impl SeenVerdict {
    /// The refusal a save answers with: `Other`'s sentence, or the
    /// `Unseen` lines as `refusal_list` lists them.
    pub fn into_result(self) -> Result<(), String> {
        match self {
            SeenVerdict::Passed => Ok(()),
            SeenVerdict::Other(why) => Err(why),
            SeenVerdict::Unseen(lines) => Err(refusal_list(&lines)),
        }
    }

    /// From the check's failures in order, each with whether it is a
    /// locator never seen.
    fn of(failures: impl IntoIterator<Item = (String, bool)>) -> SeenVerdict {
        let mut unseen: Vec<String> = Vec::new();
        for (line, locator) in failures {
            if !locator {
                return SeenVerdict::Other(line);
            }
            unseen.push(line);
        }
        if unseen.is_empty() {
            SeenVerdict::Passed
        } else {
            SeenVerdict::Unseen(unseen)
        }
    }
}

/// Checks `script` against what `map` has seen, in step order: every
/// step, or only `only_steps` on a repair. `case_text` is the test case's
/// own step actions and expected results; `components` are the project's,
/// for the steps that use one. Refused, it names every locator never seen
/// (`seen_verdict`).
pub fn check_seen(
    map: &DiscoveryMap,
    components: &ComponentFile,
    script: &CaseScript,
    case_text: &[String],
    only_steps: Option<&[i32]>,
) -> Result<(), String> {
    check_seen_with_files(map, components, script, case_text, only_steps, &[])
}

/// [`check_seen`], knowing the project's Test files (`files`), so the name
/// and the size the app shows of one the script uploads are exempt.
pub fn check_seen_with_files(
    map: &DiscoveryMap,
    components: &ComponentFile,
    script: &CaseScript,
    case_text: &[String],
    only_steps: Option<&[i32]>,
    files: &[TestFile],
) -> Result<(), String> {
    seen_verdict(map, components, script, case_text, only_steps, files).into_result()
}

/// What [`check_seen_with_files`] says of `script`, before it is put as
/// one refusal: a bundle save lists the lines of all its scripts together.
pub fn seen_verdict(
    map: &DiscoveryMap,
    components: &ComponentFile,
    script: &CaseScript,
    case_text: &[String],
    only_steps: Option<&[i32]>,
    files: &[TestFile],
) -> SeenVerdict {
    SeenVerdict::of(
        scan(map, components, script, case_text, only_steps, None, files)
            .into_iter()
            .map(|f| (f.unseen.refusal(f.hint.as_deref()), f.target.is_some())),
    )
}

/// The locators [`check_seen_with_files`] refuses `script` for, in step
/// order, when being unseen is all it refuses: `None` when it also refuses
/// a page address or a component use the step cannot make. Empty when it
/// refuses nothing.
pub fn unseen_targets(
    map: &DiscoveryMap,
    components: &ComponentFile,
    script: &CaseScript,
    case_text: &[String],
    only_steps: Option<&[i32]>,
    files: &[TestFile],
) -> Option<Vec<Target>> {
    scan(map, components, script, case_text, only_steps, None, files).into_iter().map(|f| f.target).collect()
}

/// [`check_seen`], but every failure in step order rather than the first:
/// an import names all of them at once.
pub fn check_seen_all(
    map: &DiscoveryMap,
    components: &ComponentFile,
    script: &CaseScript,
    case_text: &[String],
    only_steps: Option<&[i32]>,
) -> Vec<Unseen> {
    scan(map, components, script, case_text, only_steps, None, &[]).into_iter().map(|f| f.unseen).collect()
}

/// [`check_seen_all`], knowing the project's Test files, each failure with
/// the closest seen locator to offer ("did you mean ...?") when there is
/// one.
pub fn check_seen_all_hinted(
    map: &DiscoveryMap,
    components: &ComponentFile,
    script: &CaseScript,
    case_text: &[String],
    only_steps: Option<&[i32]>,
    files: &[TestFile],
) -> Vec<(Unseen, Option<String>)> {
    scan(map, components, script, case_text, only_steps, None, files)
        .into_iter()
        .map(|f| (f.unseen, f.hint))
        .collect()
}

/// Every use of `component` in `script`, checked the way the script's
/// save checks it, against `components` as they would be (the version
/// being saved): each locator an input goes into, read with what the
/// script typed before it and `case_text`, and whether the use can still
/// be expanded at all. The script's other locators are not checked again.
pub fn check_component_uses(
    map: &DiscoveryMap,
    components: &ComponentFile,
    script: &CaseScript,
    case_text: &[String],
    component: &str,
) -> Vec<Unseen> {
    scan(map, components, script, case_text, None, Some(component), &[]).into_iter().map(|f| f.unseen).collect()
}

/// [`check_component_uses`], each failure with its hint, for
/// `Unseen::broken_by_change_hinted`.
pub fn check_component_uses_hinted(
    map: &DiscoveryMap,
    components: &ComponentFile,
    script: &CaseScript,
    case_text: &[String],
    component: &str,
) -> Vec<(Unseen, Option<String>)> {
    scan(map, components, script, case_text, None, Some(component), &[]).into_iter().map(|f| (f.unseen, f.hint)).collect()
}

/// Does this script use a component anywhere? Its checks need the
/// project's components file only then.
pub fn uses_components(script: &CaseScript) -> bool {
    script
        .steps
        .iter()
        .flat_map(|s| s.actions.iter())
        .flat_map(Action::each)
        .any(|a| matches!(a, Action::UseComponent { .. }))
}

// ---- the assistant's save: rules for what a save changes ----
//
// These three run only in the assistant's save (`ai_bridge::save_autorun_
// scripts`), on the steps it checks (a new script's, a repair's declared
// ones), after the seen check passed. Nothing else calls them: not an
// import, a component save, a load or a run, so no script saved before is
// ever judged by them.

/// Said of an absence check whose locator was only ever seen matching
/// nothing: the one rule of the three that refuses.
pub const ABSENCE_UNPROVEN: &str = "was only ever seen matching nothing on the live app, so this check that it is absent could pass with the locator wrong: probe it where it is shown (in any area), or check it present elsewhere in this script with the same last part, then save again. If the app never shows it anywhere, leave the check out, and when the step then checks nothing, say why in its \"unchecked\".";

/// Is this a check that passes when its locator matches nothing:
/// `expect_hidden`, an `expect_count` of 0 (`equals` or `at_most`)? A
/// `when_visible` guard, skipped when its locator is absent, is not.
fn is_absence_check(action: &Action) -> bool {
    takes_none(action) && !matches!(action, Action::WhenVisible { .. })
}

/// `link` as a name only: the same control however it is narrowed, so a
/// click on `{name, exact: true}` shows the same button as `{name}`.
fn loosened(link: &LocatorStep) -> LocatorStep {
    LocatorStep { exact: false, nth: None, visible: None, ..link.clone() }
}

/// Does this action, written at the top of a step, fail when its locator
/// matches nothing? Not a use that passes with nothing there (`takes_none`),
/// a loop (it may not run at all), or a count with no lower bound
/// (`at_most`, or a bound of 0). Whatever a guard or a loop runs in its
/// `then` is left out by the caller: it runs only when the guard is there.
fn needs_it_present(action: &Action) -> bool {
    match action {
        Action::RepeatWhileVisible { .. } => false,
        Action::ExpectCount { equals, at_least, .. } => equals.is_some_and(|n| n >= 1) || at_least.is_some_and(|n| n >= 1),
        other => !takes_none(other),
    }
}

/// Every absence check of `script`'s checked steps (`only_steps`, else all)
/// whose locator could be wrong without the check ever failing, one line
/// each in step order, a repeated line once: "Step {n}: {locator}
/// {ABSENCE_UNPROVEN}". The last part of its locator (the chain's last
/// link) must have been shown: seen matching something in any area, or a
/// chain ending in it seen whole matching something, or named by an action
/// of the script that fails when it is not there (`needs_it_present`),
/// however that name is narrowed (`loosened`).
///
/// An absence action identical to one the `saved` script already has is not
/// judged again, whatever step it is in now: what was saved is not this
/// save's to refuse.
pub fn absence_refusals(
    map: &DiscoveryMap,
    script: &CaseScript,
    only_steps: Option<&[i32]>,
    saved: Option<&CaseScript>,
) -> Vec<String> {
    let before: Vec<&Action> = saved
        .into_iter()
        .flat_map(|s| s.steps.iter())
        .flat_map(|s| s.actions.iter())
        .flat_map(Action::each)
        .filter(|a| is_absence_check(a))
        .collect();
    let judged: Vec<(i32, &Action)> = script
        .steps
        .iter()
        .filter(|s| only_steps.is_none_or(|only| only.contains(&s.step_number)))
        .flat_map(|s| s.actions.iter().flat_map(Action::each).map(move |a| (s.step_number, a)))
        .filter(|(_, a)| is_absence_check(a) && !before.contains(a))
        .collect();
    if judged.is_empty() {
        return Vec::new();
    }
    let areas = every_area(map);
    let shown = present_only(map);
    let present = Sightings::new(&shown, &areas);
    let wholes = each_wholes_in(map, &areas);
    // What the script itself needs present, in any step, checked or not.
    let used: Vec<LocatorStep> = script
        .steps
        .iter()
        .flat_map(|s| s.actions.iter())
        .filter(|top| needs_it_present(top))
        .flat_map(|top| checked_targets(top))
        .filter_map(|(t, _)| t.links().pop())
        .map(|l| loosened(&l))
        .collect();
    let mut out: Vec<String> = Vec::new();
    for (step, action) in judged {
        for target in own_targets(action) {
            let Some(last) = target.links().pop() else { continue };
            let loose = loosened(&last);
            let proven = present.has(&last, true)
                || wholes.iter().any(|w| w.last().is_some_and(|l| same_link_filled(&loosened(l), &loose)))
                || used.iter().any(|u| same_link(u, &loose));
            let line = format!("Step {step}: {} {ABSENCE_UNPROVEN}", target.describe());
            if !proven && !out.contains(&line) {
                out.push(line);
            }
        }
    }
    out
}

/// The most loose names one case's notes list before saying how many more.
const MAX_LOOSE_NOTES: usize = 5;

/// Notes, never refusals: a name in a click or another use that needs one
/// element which matches loosely (a run matches a name as a part of the
/// page's, ignoring case), when the live app also showed a longer name that
/// holds it, so the use would be ambiguous whenever both are on the page.
/// Only names that read like a record (three words or more, or a digit),
/// not `exact`, with no `nth` or `visible`, one note per name in step
/// order, `MAX_LOOSE_NOTES` of them and then "and {k} more loose names like
/// these".
pub fn loose_name_notes(
    map: &DiscoveryMap,
    components: &ComponentFile,
    script: &CaseScript,
    only_steps: Option<&[i32]>,
) -> Vec<String> {
    // The names worth looking at, first: most cases have none, and then
    // nothing of the map is read.
    let mut named: Vec<(String, String)> = Vec::new();
    let mut found: Vec<(i32, LocatorStep, String)> = Vec::new();
    for step in script.steps.iter().filter(|s| only_steps.is_none_or(|only| only.contains(&s.step_number))) {
        for top in &step.actions {
            for action in top.each() {
                // A loop's own work runs once per row: not one element.
                let in_loop = matches!(top, Action::RepeatWhileVisible { .. }) && !std::ptr::eq(top, action);
                if in_loop {
                    continue;
                }
                let one_each = own_targets(action).into_iter().enumerate().filter(|(i, _)| !many_at(action, *i));
                for link in one_each.flat_map(|(_, t)| t.links()) {
                    let (kind, raw) = match (&link.role, &link.name, &link.text) {
                        (Some(r), Some(n), _) => (fold_name(r), n.as_str()),
                        (None, _, Some(t)) => ("text".to_string(), t.as_str()),
                        _ => continue,
                    };
                    if link.exact || link.nth.is_some() || link.visible.is_some() || link.input.is_some() || raw.contains("{{") {
                        continue;
                    }
                    let want = norm_name(raw);
                    if want.split(' ').count() < 3 && !want.chars().any(|c| c.is_ascii_digit()) {
                        continue;
                    }
                    if named.iter().any(|(k, n)| *k == kind && *n == want) {
                        continue;
                    }
                    named.push((kind, want.clone()));
                    found.push((step.step_number, link.clone(), want));
                }
            }
        }
    }
    if found.is_empty() {
        return Vec::new();
    }
    let areas = script_areas(components, script.area_name(), &script.steps);
    let areas: Vec<&str> = areas.iter().map(String::as_str).collect();
    let seen = Sightings::new(&present_only(map), &areas);
    let mut notes: Vec<String> = Vec::new();
    for (step, link, want) in found {
        let Some(longer) = seen
            .holders(&link)
            .into_iter()
            .find(|h| norm_name(h.name.as_deref().or(h.text.as_deref()).unwrap_or("")) != want)
        else {
            continue;
        };
        let (what, longer_what) = match (&link.role, &link.name) {
            (Some(r), Some(n)) => (format!("{r} \"{n}\""), format!("{r} \"{}\"", longer.name.as_deref().unwrap_or(""))),
            _ => (
                format!("text \"{}\"", link.text.as_deref().unwrap_or("")),
                format!("text \"{}\"", longer.text.as_deref().unwrap_or("")),
            ),
        };
        notes.push(format!(
            "step {step}: {what} matches loosely, and the live app also showed {longer_what}, which it would match too when both are on the page: add \"exact\": true."
        ));
    }
    if notes.len() > MAX_LOOSE_NOTES {
        let more = notes.len() - MAX_LOOSE_NOTES;
        notes.truncate(MAX_LOOSE_NOTES);
        notes.push(format!("and {more} more loose names like these"));
    }
    notes
}

/// The targets an action's own check reads: a `when_visible`'s own
/// selector only, as `each` lists its guarded actions after it.
fn own_targets(action: &Action) -> Vec<&Target> {
    match action {
        Action::WhenVisible { selector, .. }
        | Action::RepeatWhileVisible { selector, .. }
        | Action::ExpectEach { selector, .. } => vec![selector],
        _ => action.targets(),
    }
}

/// Every target an action's save and run-time checks read: its own
/// (`own_targets`) and an `expect_each`'s child chain (`each_child_chain`),
/// each with whether it must be seen as that whole chain. The callers that
/// walk a component's or a filled step's locators all take this, so the
/// child is never left out of one.
fn checked_targets(action: &Action) -> Vec<(Target, bool)> {
    own_targets(action)
        .into_iter()
        .map(|t| (t.clone(), false))
        .chain(each_child_chain(action).map(|chain| (chain, true)))
        .collect()
}

/// An `expect_each`'s child as it is looked for: inside the list, so the
/// list's links and then its own. The chain is what a save checks against
/// the map: seen whole, as that chain (`SeenBoth::each_whole`) - a list
/// and a bare child seen apart do not say the child is inside the list -
/// and what a saved script vouches for, as that chain only. `None` for any other action, and for an
/// `expect_each` without a child.
fn each_child_chain(action: &Action) -> Option<Target> {
    match action {
        Action::ExpectEach { selector, child: Some(child), .. } => {
            Some(Target::Chain(selector.links().into_iter().chain(child.links()).collect()))
        }
        _ => None,
    }
}

/// The locator a save checks for `target`, used by `action` inside the
/// top-level `top`, and whether it may have been seen matching several:
/// as written, except inside a `repeat_while_visible`'s `then`, where one
/// that starts in the pass's row (`repeat::ROW_SCOPE`) is checked without
/// that link - the row is the loop's own, never on the map - and may have
/// been seen matching several, one per row. `None` for one that names the
/// row itself, which the loop's selector already stands for.
fn as_checked(top: &Action, action: &Action, target: &Target) -> Option<(Target, bool)> {
    use crate::browser::repeat::{is_row_scoped, within_row};
    let in_loop = matches!(top, Action::RepeatWhileVisible { .. }) && !std::ptr::eq(top, action);
    if in_loop && is_row_scoped(target) {
        return within_row(target).map(|t| (t, true));
    }
    Some((target.clone(), false))
}

/// A locator a component's inputs went into, as it runs: the links an
/// input put there (a fixed link it already had is not re-checked; that
/// was done when the component was saved), whether its action is a check,
/// and what the component typed before it.
struct InputLocator {
    target: Target,
    links: Vec<LocatorStep>,
    check: bool,
    /// Whether its action may match several (`takes_many`), or it is a
    /// `when_visible`'s own locator (`seen_many`).
    many: bool,
    /// Whether its action passes with nothing there (`takes_none`).
    none: bool,
    typed_before: Vec<String>,
    /// Its action's own word (`Action::kind`), for what a refusal says it
    /// needs (`wants`).
    kind: String,
    /// Whether it is a `when_visible`'s locator, checked against every
    /// area (see the module's notes).
    everywhere: bool,
    /// Whether it must be seen as this whole chain (an `expect_each`
    /// child inside its list), not link by link.
    whole: bool,
}

/// Is `target`, used by `action` inside the top-level `top`, a
/// `when_visible`'s locator: its own selector, or that same locator used by
/// an action in its `then`?
fn guards(top: &Action, target: &Target) -> bool {
    matches!(top, Action::WhenVisible { selector, .. } if selector == target)
}

/// Every area name `map` holds: what a `when_visible`'s locator is checked
/// against.
fn every_area(map: &DiscoveryMap) -> Vec<&str> {
    map.areas.iter().map(|a| a.area.as_str()).collect()
}

/// Every locator of `c` an input changed, from its `expanded` actions
/// (`expand`'s, which pairs one for one with `c.actions`): a target
/// input's locator, or a fixed one a text input was written into.
fn input_locators(c: &Component, expanded: &[Action]) -> Vec<InputLocator> {
    let mut out = Vec::new();
    let mut typed_before: Vec<String> = Vec::new();
    for (written, ran) in c.actions.iter().zip(expanded) {
        for (w, r) in written.each().into_iter().zip(ran.each()) {
            for (i, ((wt, whole), (rt, _))) in checked_targets(w).into_iter().zip(checked_targets(r)).enumerate() {
                if wt == rt {
                    continue;
                }
                let fixed = wt.links();
                let links: Vec<LocatorStep> = rt.links().into_iter().filter(|l| !fixed.contains(l)).collect();
                out.push(InputLocator {
                    target: rt,
                    links,
                    check: check_at(r, i),
                    many: many_at(r, i),
                    none: takes_none(r),
                    typed_before: typed_before.clone(),
                    kind: r.kind(),
                    everywhere: false,
                    whole,
                });
            }
            push_typed(&mut typed_before, r);
        }
    }
    out
}

/// The actions a `use_component` the project can expand runs as, for what
/// they type and the areas they go to; nothing for any other action, or
/// for a use that cannot be expanded (its own step refuses that).
fn as_run(components: &ComponentFile, action: &Action) -> Vec<Action> {
    match action {
        Action::UseComponent { component, inputs } => {
            find(components, component).and_then(|c| expand(c, inputs).ok()).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// The areas a check of these steps reads: `area` (the script's own) and
/// every area an action of the steps returns to, a component's included.
pub fn script_areas(components: &ComponentFile, area: Option<&str>, steps: &[StepScript]) -> Vec<String> {
    let mut areas: Vec<String> = area.into_iter().map(str::to_string).collect();
    for step in steps {
        for action in step.actions.iter().flat_map(Action::each) {
            let ran = as_run(components, action);
            for a in std::iter::once(action).chain(ran.iter().flat_map(Action::each)) {
                if let Some(a) = a.area_named() {
                    areas.push(a.to_string());
                }
            }
        }
    }
    areas
}

/// Every failure of the check, in step order, then action order, then
/// link order; with `only_component`, of that component's uses alone.
fn scan(
    map: &DiscoveryMap,
    components: &ComponentFile,
    script: &CaseScript,
    case_text: &[String],
    only_steps: Option<&[i32]>,
    only_component: Option<&str>,
    files: &[TestFile],
) -> Vec<Found> {
    let only_key = only_component.map(super::nav::module_key);
    let mut unseen: Vec<Found> = Vec::new();
    let areas = script_areas(components, script.area_name(), &script.steps);
    let areas: Vec<&str> = areas.iter().map(String::as_str).collect();
    let both = SeenBoth::new(map, &areas);
    // What every area saw, for a `when_visible`'s locator: built only when
    // a script has one to check.
    let mut everywhere: Option<SeenBoth> = None;
    let paths = seen_paths(map);
    let case_text: Vec<String> = case_text.iter().map(|t| fold_name(t)).collect();
    let mut so_far = SoFar::default();
    // The area the browser is in as each step runs: the script's own, until
    // a `return_to_area` names another.
    let mut here: Option<String> = script.area_name().map(str::to_string);

    for step in &script.steps {
        so_far.uploads_of(components, step);
        let own = own_test_files(&so_far.uploaded, files);
        let (own_files, sizes) = (exempting(&own), shown_sizes(&own));
        let checked = only_steps.is_none_or(|only| only.contains(&step.step_number));
        if checked {
            for (top, action) in step.actions.iter().flat_map(|top| top.each().into_iter().map(move |a| (top, a))) {
                if let Some(a) = action.area_named() {
                    here = Some(a.to_string());
                }
                if let Some(k) = &only_key {
                    let this_one = matches!(
                        action,
                        Action::UseComponent { component, .. } if super::nav::module_key(component) == *k
                    );
                    if !this_one {
                        continue;
                    }
                }
                if let Action::Navigate { url } | Action::OpenTab { url, .. } = action {
                    // Compared as the map files a page; named as written.
                    let path = path_only(url);
                    if !paths.contains(&page_path(url)) {
                        unseen.push(Found {
                            unseen: Unseen { step: step.step_number, locator: path, refused: None },
                            hint: None,
                            target: None,
                        });
                    }
                }
                // The locators the script names: the action's own, each
                // with whether it is only looked for and what was typed
                // before it; or every locator of a component its inputs
                // went into.
                let mut named: Vec<InputLocator> = own_targets(action)
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, t)| {
                        let (checked, in_row) = as_checked(top, action, t)?;
                        Some(InputLocator {
                            links: checked.links(),
                            target: checked,
                            check: check_at(action, i),
                            many: in_row || many_at(action, i),
                            none: takes_none(action),
                            typed_before: Vec::new(),
                            kind: action.kind(),
                            everywhere: guards(top, t),
                            whole: false,
                        })
                    })
                    .collect();
                if let Some(chain) = each_child_chain(action) {
                    named.push(InputLocator {
                        links: chain.links(),
                        target: chain,
                        check: true,
                        many: true,
                        none: false,
                        typed_before: Vec::new(),
                        kind: action.kind(),
                        everywhere: false,
                        whole: true,
                    });
                }
                if let Action::UseComponent { component, inputs } = action {
                    let used = find(components, component)
                        .ok_or_else(|| not_saved(component))
                        .and_then(|c| expand(c, inputs).map(|ex| (c, ex)));
                    let (c, expanded) = match used {
                        Ok(used) => used,
                        Err(why) => {
                            unseen.push(Found {
                                unseen: Unseen { step: step.step_number, locator: component.clone(), refused: Some(why) },
                                hint: None,
                                target: None,
                            });
                            continue;
                        }
                    };
                    named.extend(input_locators(c, &expanded));
                }
                // A date this action picks: one of its own component inputs.
                let picked_here = text_inputs(action);
                for n in named {
                    let typed: Vec<String> = so_far.typed.iter().cloned().chain(n.typed_before).collect();
                    let dates = so_far.dates(&typed, &picked_here);
                    let own = Own {
                        typed: &typed,
                        case_text: &case_text,
                        check: n.check,
                        files: &own_files,
                        sizes: &sizes,
                        dates: &dates,
                    };
                    // One line per target, however many of its links are
                    // unseen.
                    let chain = n.target.links();
                    let legacy = n.target.is_legacy();
                    let seen = if n.everywhere {
                        &*everywhere.get_or_insert_with(|| SeenBoth::new(map, &every_area(map)))
                    } else {
                        &both
                    };
                    if n.whole {
                        // A child is seen inside its list, as that chain.
                        if seen.each_whole(&chain) {
                            continue;
                        }
                        let hint = match first_unseen(&n.links, &chain, seen.for_use(n.many, legacy, n.none), &own) {
                            Some(link) => {
                                let page = area_page(map, here.as_deref());
                                seen.hint(link, &n.links, n.many, legacy, n.none, page, &n.kind, n.everywhere)
                            }
                            None => Some(SEEN_ONLY_APART.to_string()),
                        };
                        unseen.push(Found {
                            unseen: Unseen { step: step.step_number, locator: n.target.describe(), refused: None },
                            hint,
                            target: Some(n.target.clone()),
                        });
                        continue;
                    }
                    if seen.whole(&chain, n.many) || seen.none_whole(&chain, n.none) {
                        continue;
                    }
                    if let Some(link) = first_unseen(&n.links, &chain, seen.for_use(n.many, legacy, n.none), &own) {
                        let page = area_page(map, here.as_deref());
                        unseen.push(Found {
                            unseen: Unseen { step: step.step_number, locator: n.target.describe(), refused: None },
                            hint: seen.hint(link, &n.links, n.many, legacy, n.none, page, &n.kind, n.everywhere),
                            target: Some(n.target.clone()),
                        });
                    }
                }
            }
        }
        so_far.typed_by(components, step);
        if !checked {
            for a in step.actions.iter().flat_map(Action::each).filter_map(Action::area_named) {
                here = Some(a.to_string());
            }
        }
    }
    unseen
}

/// The script's own data its steps have given so far, read the same way at
/// save (`scan`) and at run time (`check_resolved_inputs_with`).
#[derive(Default)]
struct SoFar {
    /// Values typed by the steps before the one being checked, folded,
    /// each at least `MIN_TYPED_LEN`.
    typed: Vec<String>,
    /// The text inputs the steps before it gave components.
    picked: Vec<String>,
    /// The files uploaded by this step and the ones before it, a
    /// component's uploads included.
    uploaded: Vec<String>,
}

impl SoFar {
    /// Adds what `step` uploads: a file is the script's own from the step
    /// that uploads it.
    fn uploads_of(&mut self, components: &ComponentFile, step: &StepScript) {
        for action in step.actions.iter().flat_map(Action::each) {
            let ran = as_run(components, action);
            self.uploaded.extend(uploads(std::iter::once(action).chain(ran.iter().flat_map(Action::each))));
        }
    }

    /// Adds what `step` typed and picked, for the steps after it. A
    /// component types what its actions type, its text inputs put in.
    fn typed_by(&mut self, components: &ComponentFile, step: &StepScript) {
        for action in step.actions.iter().flat_map(Action::each) {
            self.picked.extend(text_inputs(action));
            let ran = as_run(components, action);
            for a in std::iter::once(action).chain(ran.iter().flat_map(Action::each)) {
                push_typed(&mut self.typed, a);
            }
        }
    }

    /// The dates among `typed`, the text inputs picked before, and
    /// `picked_here` (the checked action's own).
    fn dates(&self, typed: &[String], picked_here: &[String]) -> Vec<Date> {
        typed.iter().chain(&self.picked).chain(picked_here).filter_map(|v| parse_date(v)).collect()
    }
}

/// Adds the value `action` types to `typed`, folded, when it is long
/// enough to exempt anything.
fn push_typed(typed: &mut Vec<String>, action: &Action) {
    if let Some(v) = action.typed_value() {
        let v = fold_name(v);
        if v.chars().count() >= MIN_TYPED_LEN {
            typed.push(v);
        }
    }
}

/// Is this link left to the script: a target input's place, or one that
/// holds a complete `{{x}}` text placeholder?
fn input_link(link: &LocatorStep) -> bool {
    link.input.is_some()
        || serde_json::to_value(link).is_ok_and(|v| super::components::holds_text_placeholder(&v))
}

/// A component's own locators and the pages it goes to, checked against
/// what `map` has seen in `area` (and in any area its actions return to),
/// in order; the first unseen one is refused, named by the component's
/// action. Two kinds of link are exempt, as only a script knows them: one
/// a target input fills (`{"input": ...}`), and one a text input is
/// written into (`{{x}}`). Every other link, beside one of those in a
/// chain too, must be on the map. A script's save checks the exempt ones
/// as they expand. With no Test files known here, no file name or size is
/// exempt.
pub fn check_component_seen(map: &DiscoveryMap, area: Option<&str>, actions: &[Action]) -> Result<(), String> {
    component_verdict(map, area, actions).into_result()
}

/// What [`check_component_seen`] says of a component's actions: refused
/// only for locators never seen, every one in action order, each with its
/// "did you mean"; refused for a page address, that refusal alone.
pub fn component_verdict(map: &DiscoveryMap, area: Option<&str>, actions: &[Action]) -> SeenVerdict {
    SeenVerdict::of(component_unseen(map, area, actions).into_iter().map(|(why, t)| (why, t.is_some())))
}

/// The locators [`check_component_seen`] refuses, in order, when being
/// unseen is all it refuses: `None` when it also refuses a page address.
/// Empty when it refuses nothing.
pub fn unseen_component_targets(map: &DiscoveryMap, area: Option<&str>, actions: &[Action]) -> Option<Vec<Target>> {
    component_unseen(map, area, actions).into_iter().map(|(_, t)| t).collect()
}

/// Every refusal of a component's check, in action order, each with the
/// locator it names (`None` for a page address).
fn component_unseen(map: &DiscoveryMap, area: Option<&str>, actions: &[Action]) -> Vec<(String, Option<Target>)> {
    let mut areas: Vec<&str> = area.into_iter().collect();
    areas.extend(actions.iter().flat_map(Action::each).filter_map(Action::area_named));
    let both = SeenBoth::new(map, &areas);
    let mut everywhere: Option<SeenBoth> = None;
    let paths = seen_paths(map);
    let refused = |i: usize, what: &str, hint: Option<&str>| {
        format!(
            "Action {}: {} Find it on the page first with probe_autorun_locator or discover_autorun_actions, then save again.",
            i + 1,
            never_seen(what, hint)
        )
    };
    let mut out: Vec<(String, Option<Target>)> = Vec::new();
    // Values typed by the component's earlier actions.
    let mut typed: Vec<String> = Vec::new();
    for (i, action) in actions.iter().enumerate() {
        for a in action.each() {
            if let Action::Navigate { url } | Action::OpenTab { url, .. } = a {
                if !paths.contains(&page_path(url)) {
                    out.push((refused(i, &path_only(url), None), None));
                }
            }
            let dates: Vec<Date> = typed.iter().filter_map(|v| parse_date(v)).collect();
            for (ti, (written, whole)) in checked_targets(a).iter().enumerate() {
                let own = Own { typed: &typed, case_text: &[], check: check_at(a, ti), files: &[], sizes: &[], dates: &dates };
                let Some((checked, in_row)) = as_checked(action, a, written) else { continue };
                let t = &checked;
                let chain = t.links();
                let links: Vec<LocatorStep> = chain.iter().filter(|l| !input_link(l)).cloned().collect();
                let many = in_row || many_at(a, ti);
                let none = takes_none(a);
                let banner = guards(action, t);
                let seen = if banner {
                    &*everywhere.get_or_insert_with(|| SeenBoth::new(map, &every_area(map)))
                } else {
                    &both
                };
                // A child is seen inside its list, as that chain - unless an
                // input fills part of it, which only a script knows.
                if *whole && !chain.iter().any(input_link) {
                    if seen.each_whole(&chain) {
                        continue;
                    }
                    let hint = match first_unseen(&links, &chain, seen.for_use(many, t.is_legacy(), none), &own) {
                        Some(link) => {
                            seen.hint(link, &links, many, t.is_legacy(), none, area_page(map, area), &a.kind(), banner)
                        }
                        None => Some(SEEN_ONLY_APART.to_string()),
                    };
                    out.push((refused(i, &t.describe(), hint.as_deref()), Some(t.clone())));
                    continue;
                }
                if seen.whole(&chain, many) || seen.none_whole(&chain, none) {
                    continue;
                }
                let legacy = t.is_legacy();
                if let Some(link) = first_unseen(&links, &chain, seen.for_use(many, legacy, none), &own) {
                    let hint = seen.hint(link, &links, many, legacy, none, area_page(map, area), &a.kind(), banner);
                    out.push((refused(i, &t.describe(), hint.as_deref()), Some(t.clone())));
                }
            }
            push_typed(&mut typed, a);
        }
    }
    out
}

// ---- at run time ----

/// Does `v` hold a data placeholder in any of its strings?
fn value_holds_placeholder(v: &Value) -> bool {
    match v {
        Value::String(s) => holds_data_placeholder(s),
        Value::Array(items) => items.iter().any(value_holds_placeholder),
        Value::Object(map) => map.values().any(value_holds_placeholder),
        _ => false,
    }
}

/// Does a `use_component` in `steps` give an input holding a data
/// placeholder?
pub fn has_placeholder_inputs(steps: &[StepScript]) -> bool {
    steps.iter().flat_map(|s| s.actions.iter()).flat_map(Action::each).any(|a| {
        matches!(a, Action::UseComponent { inputs, .. } if inputs.values().any(value_holds_placeholder))
    })
}

/// Does any locator in `steps` hold a data placeholder: one a step names
/// itself, or one a component input gives? Only then is there anything
/// for `check_resolved_inputs`.
pub fn has_data_placeholders(steps: &[StepScript]) -> bool {
    has_placeholder_inputs(steps)
        || steps
            .iter()
            .flat_map(|s| s.actions.iter())
            .flat_map(Action::each)
            .flat_map(checked_targets)
            .any(|(t, _)| t.links().iter().any(link_holds_placeholder))
}

/// The run-time half of the check for every locator that held a data
/// placeholder when the script was saved: one a step names itself, and
/// one a component input gives. `saved` are the steps as saved, `filled`
/// the same steps once the run filled the placeholders in. Each such
/// locator, as filled, must match a sighting in `areas` that the saved one
/// matches, every value a placeholder took the shape of the seen value it
/// stands for there (all digits where that was); a placeholder the run
/// left unfilled fails. The failure names the step, and for a component
/// the component and its input. With no earlier steps and no Test files
/// known: [`check_resolved_inputs_with`].
pub fn check_resolved_inputs(
    map: &DiscoveryMap,
    components: &ComponentFile,
    areas: &[&str],
    saved: &[StepScript],
    filled: &[StepScript],
) -> Result<(), String> {
    check_resolved_inputs_with(map, components, areas, &[], saved, filled, &[])
}

/// [`check_resolved_inputs`], knowing the steps that ran before `filled`
/// (`before`, as filled in) and the project's Test files (`files`). A
/// filled link the save would have let through as the script's own data
/// passes here too, read from the filled steps the way the save reads the
/// saved ones: a value typed earlier or put into a component's text input,
/// a Test file uploaded by then or its size, a date picked or typed, or a
/// `dd/mm/yyyy` day inside a seen date picker. The case's own text is not
/// known here; the save never lets it alone pass a link holding a
/// placeholder.
pub fn check_resolved_inputs_with(
    map: &DiscoveryMap,
    components: &ComponentFile,
    areas: &[&str],
    before: &[StepScript],
    saved: &[StepScript],
    filled: &[StepScript],
    files: &[TestFile],
) -> Result<(), String> {
    let both = SeenBoth::new(map, areas);
    let mut so_far = SoFar::default();
    for s in before {
        so_far.uploads_of(components, s);
        so_far.typed_by(components, s);
    }
    for (ss, fs) in saved.iter().zip(filled) {
        let step = ss.step_number;
        so_far.uploads_of(components, fs);
        let own = own_test_files(&so_far.uploaded, files);
        let (own_files, sizes) = (exempting(&own), shown_sizes(&own));
        for (sa, fa) in ss.actions.iter().flat_map(Action::each).zip(fs.actions.iter().flat_map(Action::each)) {
            let picked_here = text_inputs(fa);
            // Is this filled link, in its filled chain, the script's own
            // data, with `typed` the values typed before it?
            let own_data = |link: &LocatorStep, chain: &[LocatorStep], typed: &[String], check: bool, seen: &Sightings| {
                let dates = so_far.dates(typed, &picked_here);
                let own = Own { typed, case_text: &[], check, files: &own_files, sizes: &sizes, dates: &dates };
                !link_holds_placeholder(link) && (exempt(link, &own) || in_seen_date_picker(link, chain, seen))
            };
            for (ti, ((st, _), (ft, _))) in checked_targets(sa).into_iter().zip(checked_targets(fa)).enumerate() {
                let chain = ft.links();
                // A placeholder chain or narrowed locator the save took as
                // a whole (`SeenBoth::whole`) is fitted as a whole here too.
                if both.whole_filled(&st.links(), &chain, many_at(fa, ti))
                    || both.none_whole_filled(&st.links(), &chain, takes_none(fa))
                {
                    continue;
                }
                let seen = both.for_use(many_at(fa, ti), ft.is_legacy(), takes_none(fa));
                let off = st.links().iter().zip(&chain).any(|(sl, fl)| {
                    link_holds_placeholder(sl)
                        && !own_data(fl, &chain, &so_far.typed, check_at(fa, ti), seen)
                        && !seen.fills(sl, fl)
                });
                if off {
                    return Err(format!(
                        "Step {step}: {}, as filled in, does not fit what was seen on the live app. {RUN_TIME_FIX}",
                        ft.describe()
                    ));
                }
            }
            let (Action::UseComponent { component, inputs: given }, Action::UseComponent { inputs: filled_in, .. }) = (sa, fa)
            else {
                continue;
            };
            if !given.values().any(value_holds_placeholder) {
                continue;
            }
            let at_step = |why: String| format!("Step {step}: {why}");
            let c = find(components, component).ok_or_else(|| at_step(not_saved(component)))?;
            let ran = expand(c, filled_in).map_err(at_step)?;
            for (name, v) in given.iter().filter(|(_, v)| value_holds_placeholder(v)) {
                // The same use with only this input as it was saved: the
                // locators that differ from `ran` are the ones it fed.
                let mut marked = filled_in.clone();
                marked.insert(name.clone(), v.clone());
                let as_saved = expand(c, &marked).map_err(at_step)?;
                // What the steps before typed, then what the component's
                // own actions typed before each of its locators.
                let mut typed = so_far.typed.clone();
                for (m, r) in as_saved.iter().flat_map(Action::each).zip(ran.iter().flat_map(Action::each)) {
                    for (ti, ((mt, _), (rt, _))) in checked_targets(m).into_iter().zip(checked_targets(r)).enumerate() {
                        let chain = rt.links();
                        if both.whole_filled(&mt.links(), &chain, many_at(r, ti))
                            || both.none_whole_filled(&mt.links(), &chain, takes_none(r))
                        {
                            continue;
                        }
                        let seen = both.for_use(many_at(r, ti), rt.is_legacy(), takes_none(r));
                        let fed = mt.links().iter().zip(&chain).any(|(ml, rl)| {
                            let changed = ml != rl || link_holds_placeholder(rl);
                            changed && !own_data(rl, &chain, &typed, check_at(r, ti), seen) && !seen.fills(ml, rl)
                        });
                        if fed {
                            return Err(format!(
                                "Step {step}: {}: its input {} gave {}, which does not fit what was seen on the live app. {RUN_TIME_FIX}",
                                c.name,
                                name.trim(),
                                rt.describe()
                            ));
                        }
                    }
                    push_typed(&mut typed, r);
                }
            }
        }
        so_far.typed_by(components, fs);
    }
    Ok(())
}

/// What a person can do about a run-time Blocked of a filled locator.
const RUN_TIME_FIX: &str = "Explore that screen again with discovery, or check the value the setup or fixture gives.";
