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
//!   `{{prefix}}` and `{{now:...}}` are not data placeholders: Auto Run
//!   never fills them in, so they stay literal and are refused. Once the
//!   run has filled a placeholder in, the locator is checked again
//!   (`check_resolved_inputs`): each value must have the shape of the seen
//!   value it stands for, all digits where that was all digits.
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
//!
//! A refusal names the closest locator seen in the same areas when one is
//! close enough (`SUGGEST_WITHIN`).
//!
//! A step that uses a component must name one the project has and give it
//! every input, each of its kind. Every locator of the component an input
//! goes into (a target input, or a text input written into a locator) is
//! checked like any other, as it runs; the component's fixed locators are
//! not, as they were checked when the component was saved.

use super::components::{expand, find, not_saved, Component, ComponentFile};
use super::discovery_map::{page_path, path_only, seen_keys, seen_links, seen_locators, seen_paths, DiscoveryMap};
use super::edits::Edit;
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
fn has_phrase(text: &str, phrase: &str) -> bool {
    text.match_indices(phrase).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + phrase.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// "<what> was never seen on the live app", with the closest seen locator
/// after it when there is one.
fn never_seen(what: &str, hint: Option<&str>) -> String {
    match hint {
        Some(h) => format!("{what} was never seen on the live app; {h}"),
        None => format!("{what} was never seen on the live app."),
    }
}

fn refusal(step: i32, what: &str, hint: Option<&str>) -> String {
    format!(
        "Step {step}: {} Find it on the page first with probe_autorun_locator or discover_autorun_action, then save again.",
        never_seen(what, hint)
    )
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
            | Action::ExpectAttribute { .. }
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
}

impl Sightings {
    fn new(map: &DiscoveryMap, areas: &[&str]) -> Self {
        let mut s = Sightings {
            keys: seen_keys(map, areas),
            roles: Vec::new(),
            texts: Vec::new(),
            css: Vec::new(),
            chains: Vec::new(),
        };
        for l in seen_links(map, areas) {
            if let Some(role) = &l.role {
                s.roles.push((fold_name(role), norm_name(l.name.as_deref().unwrap_or("")), l.clone()));
            } else if let Some(t) = &l.text {
                s.texts.push((norm_name(t), l.clone()));
            } else if let Some(c) = &l.css {
                s.css.push(strip_states(c));
            }
        }
        for t in seen_locators(map, areas) {
            let links = t.links();
            if links.len() > 1 {
                s.chains.push(links.iter().map(|l| l.css.as_deref().map(strip_states)).collect());
            }
        }
        s
    }

    /// Was `link` seen: its exact key, or a sighting its name or selector
    /// matches. With `wild`, a data placeholder in it stands for a seen
    /// value.
    fn has(&self, link: &LocatorStep, wild: bool) -> bool {
        if link.seen_key().is_some_and(|k| self.keys.contains(&k)) {
            return true;
        }
        // The placeholders are found in the raw name before it is folded
        // (`name_pattern`): folding first would turn a `{{Setup.x}}` the
        // run never fills into one it does.
        let fits = |raw: &str, seen: &str| norm_name(raw) == seen || (wild && wild_fits(&name_pattern(raw), seen));
        if let Some(role) = &link.role {
            let (r, n) = (fold_name(role), link.name.as_deref().unwrap_or(""));
            return self.roles.iter().any(|(sr, sn, _)| *sr == r && fits(n, sn));
        }
        if let Some(t) = &link.text {
            return self.texts.iter().any(|(sn, _)| fits(t, sn));
        }
        link.css.as_deref().is_some_and(|c| self.css_seen(c, wild, 0))
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
    /// "<name>"?": the same role first, then the fewest edits, within
    /// `SUGGEST_WITHIN`; `None` when nothing is that close, or for a css
    /// link (a selector has no name to offer).
    fn closest(&self, link: &LocatorStep) -> Option<String> {
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
        let mut best: Option<((bool, usize), &LocatorStep)> = None;
        for (r, n, l) in candidates {
            if n.is_empty() {
                continue;
            }
            let edits = edit_distance(&want, n);
            let longer = want.chars().count().max(n.chars().count());
            if edits * SUGGEST_WITHIN.1 > longer * SUGGEST_WITHIN.0 {
                continue;
            }
            let rank = (r != role, edits);
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
/// says (a check only), a Test file it uploaded or that file's size, or a
/// date-picker day that is a date it picked or typed?
fn exempt(link: &LocatorStep, own: &Own) -> bool {
    let own_words = words(link);
    let has = |list: &[String]| list.iter().any(|t| own_words.iter().any(|w| has_phrase(w, t)));
    // A value typed earlier (already at least 3 characters) as whole words
    // inside the locator's text or name: the record the script created.
    let typed_here = has(own.typed);
    // The locator's whole name, of at least 3 characters, as whole words in
    // what the case says.
    let in_case = own.check
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
fn first_unseen<'a>(links: &'a [LocatorStep], chain: &[LocatorStep], seen: &Sightings, own: &Own) -> Option<&'a LocatorStep> {
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
        match &self.refused {
            Some(why) => format!("step {}: {why}; this change would break it.", self.step),
            None => format!(
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

/// A failure of the check, with the closest seen locator to offer.
struct Found {
    unseen: Unseen,
    hint: Option<String>,
}

/// Checks `script` against what `map` has seen, in step order: every
/// step, or only `only_steps` on a repair. `case_text` is the test case's
/// own step actions and expected results; `components` are the project's,
/// for the steps that use one. The first failure is returned.
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
    match scan(map, components, script, case_text, only_steps, true, None, files).into_iter().next() {
        Some(f) => Err(f.unseen.refusal(f.hint.as_deref())),
        None => Ok(()),
    }
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
    scan(map, components, script, case_text, only_steps, false, None, &[]).into_iter().map(|f| f.unseen).collect()
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
    scan(map, components, script, case_text, only_steps, false, None, files)
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
    scan(map, components, script, case_text, None, false, Some(component), &[]).into_iter().map(|f| f.unseen).collect()
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

/// The targets an action's own check reads: a `when_visible`'s own
/// selector only, as `each` lists its guarded actions after it.
fn own_targets(action: &Action) -> Vec<&Target> {
    match action {
        Action::WhenVisible { selector, .. } => vec![selector],
        _ => action.targets(),
    }
}

/// A locator a component's inputs went into, as it runs: the links an
/// input put there (a fixed link it already had is not re-checked; that
/// was done when the component was saved), whether its action is a check,
/// and what the component typed before it.
struct InputLocator {
    target: Target,
    links: Vec<LocatorStep>,
    check: bool,
    typed_before: Vec<String>,
}

/// Every locator of `c` an input changed, from its `expanded` actions
/// (`expand`'s, which pairs one for one with `c.actions`): a target
/// input's locator, or a fixed one a text input was written into.
fn input_locators(c: &Component, expanded: &[Action]) -> Vec<InputLocator> {
    let mut out = Vec::new();
    let mut typed_before: Vec<String> = Vec::new();
    for (written, ran) in c.actions.iter().zip(expanded) {
        for (w, r) in written.each().into_iter().zip(ran.each()) {
            for (wt, rt) in own_targets(w).into_iter().zip(own_targets(r)) {
                if wt == rt {
                    continue;
                }
                let fixed = wt.links();
                let links: Vec<LocatorStep> = rt.links().into_iter().filter(|l| !fixed.contains(l)).collect();
                out.push(InputLocator { target: rt.clone(), links, check: is_check(r), typed_before: typed_before.clone() });
            }
            if let Some(v) = r.typed_value() {
                let v = fold_name(v);
                if v.chars().count() >= MIN_TYPED_LEN {
                    typed_before.push(v);
                }
            }
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

/// The failures of the check, stopping at the first when `first_only`;
/// with `only_component`, of that component's uses alone.
#[allow(clippy::too_many_arguments)]
fn scan(
    map: &DiscoveryMap,
    components: &ComponentFile,
    script: &CaseScript,
    case_text: &[String],
    only_steps: Option<&[i32]>,
    first_only: bool,
    only_component: Option<&str>,
    files: &[TestFile],
) -> Vec<Found> {
    let only_key = only_component.map(super::nav::module_key);
    let mut unseen: Vec<Found> = Vec::new();
    let areas = script_areas(components, script.area_name(), &script.steps);
    let areas: Vec<&str> = areas.iter().map(String::as_str).collect();
    let seen = Sightings::new(map, &areas);
    let paths = seen_paths(map);
    let case_text: Vec<String> = case_text.iter().map(|t| fold_name(t)).collect();
    // Values typed by the steps before the one being checked.
    let mut typed: Vec<String> = Vec::new();
    // The text inputs the steps before it gave components.
    let mut picked_before: Vec<String> = Vec::new();
    // The files uploaded by this step and the ones before it, a
    // component's uploads included.
    let mut uploaded: Vec<String> = Vec::new();

    for step in &script.steps {
        for action in step.actions.iter().flat_map(Action::each) {
            let ran = as_run(components, action);
            uploaded.extend(uploads(std::iter::once(action).chain(ran.iter().flat_map(Action::each))));
        }
        let own = own_test_files(&uploaded, files);
        let (own_files, sizes) = (exempting(&own), shown_sizes(&own));
        let checked = only_steps.is_none_or(|only| only.contains(&step.step_number));
        if checked {
            for action in step.actions.iter().flat_map(Action::each) {
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
                        unseen.push(Found { unseen: Unseen { step: step.step_number, locator: path, refused: None }, hint: None });
                        if first_only {
                            return unseen;
                        }
                    }
                }
                // The locators the script names: the action's own, each
                // with whether it is only looked for and what was typed
                // before it; or every locator of a component its inputs
                // went into.
                let mut named: Vec<InputLocator> = own_targets(action)
                    .into_iter()
                    .map(|t| InputLocator {
                        target: t.clone(),
                        links: t.links(),
                        check: is_check(action),
                        typed_before: Vec::new(),
                    })
                    .collect();
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
                            });
                            if first_only {
                                return unseen;
                            }
                            continue;
                        }
                    };
                    named.extend(input_locators(c, &expanded));
                }
                // A date this action picks: one of its own component inputs.
                let picked_here = text_inputs(action);
                for n in named {
                    let typed: Vec<String> = typed.iter().cloned().chain(n.typed_before).collect();
                    let dates: Vec<Date> =
                        typed.iter().chain(&picked_before).chain(&picked_here).filter_map(|v| parse_date(v)).collect();
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
                    if let Some(link) = first_unseen(&n.links, &chain, &seen, &own) {
                        unseen.push(Found {
                            unseen: Unseen { step: step.step_number, locator: n.target.describe(), refused: None },
                            hint: seen.closest(link),
                        });
                        if first_only {
                            return unseen;
                        }
                    }
                }
            }
        }
        for action in step.actions.iter().flat_map(Action::each) {
            picked_before.extend(text_inputs(action));
            // A component types what its actions type, its text inputs
            // put in.
            let ran = as_run(components, action);
            for a in std::iter::once(action).chain(ran.iter().flat_map(Action::each)) {
                if let Some(v) = a.typed_value() {
                    let v = fold_name(v);
                    if v.chars().count() >= MIN_TYPED_LEN {
                        typed.push(v);
                    }
                }
            }
        }
    }
    unseen
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
    let mut areas: Vec<&str> = area.into_iter().collect();
    areas.extend(actions.iter().flat_map(Action::each).filter_map(Action::area_named));
    let seen = Sightings::new(map, &areas);
    let paths = seen_paths(map);
    let refused = |i: usize, what: &str, hint: Option<&str>| {
        format!(
            "Action {}: {} Find it on the page first with probe_autorun_locator or discover_autorun_action, then save again.",
            i + 1,
            never_seen(what, hint)
        )
    };
    // Values typed by the component's earlier actions.
    let mut typed: Vec<String> = Vec::new();
    for (i, action) in actions.iter().enumerate() {
        for a in action.each() {
            if let Action::Navigate { url } | Action::OpenTab { url, .. } = a {
                if !paths.contains(&page_path(url)) {
                    return Err(refused(i, &path_only(url), None));
                }
            }
            let dates: Vec<Date> = typed.iter().filter_map(|v| parse_date(v)).collect();
            let own = Own { typed: &typed, case_text: &[], check: is_check(a), files: &[], sizes: &[], dates: &dates };
            for t in own_targets(a) {
                let chain = t.links();
                let links: Vec<LocatorStep> = chain.iter().filter(|l| !input_link(l)).cloned().collect();
                if let Some(link) = first_unseen(&links, &chain, &seen, &own) {
                    return Err(refused(i, &t.describe(), seen.closest(link).as_deref()));
                }
            }
            if let Some(v) = a.typed_value() {
                let v = fold_name(v);
                if v.chars().count() >= MIN_TYPED_LEN {
                    typed.push(v);
                }
            }
        }
    }
    Ok(())
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
            .flat_map(own_targets)
            .any(|t| t.links().iter().any(link_holds_placeholder))
}

/// The run-time half of the check for every locator that held a data
/// placeholder when the script was saved: one a step names itself, and
/// one a component input gives. `saved` are the steps as saved, `filled`
/// the same steps once the run filled the placeholders in. Each such
/// locator, as filled, must match a sighting in `areas` that the saved one
/// matches, every value a placeholder took the shape of the seen value it
/// stands for there (all digits where that was); a placeholder the run
/// left unfilled fails. The failure names the step, and for a component
/// the component and its input.
pub fn check_resolved_inputs(
    map: &DiscoveryMap,
    components: &ComponentFile,
    areas: &[&str],
    saved: &[StepScript],
    filled: &[StepScript],
) -> Result<(), String> {
    let seen = Sightings::new(map, areas);
    for (ss, fs) in saved.iter().zip(filled) {
        let step = ss.step_number;
        for (sa, fa) in ss.actions.iter().flat_map(Action::each).zip(fs.actions.iter().flat_map(Action::each)) {
            for (st, ft) in own_targets(sa).into_iter().zip(own_targets(fa)) {
                let off = st
                    .links()
                    .iter()
                    .zip(ft.links())
                    .any(|(sl, fl)| link_holds_placeholder(sl) && !seen.fills(sl, &fl));
                if off {
                    return Err(format!(
                        "Step {step}: {}, as filled in, does not fit what was seen on the live app",
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
                for (m, r) in as_saved.iter().flat_map(Action::each).zip(ran.iter().flat_map(Action::each)) {
                    for (mt, rt) in own_targets(m).into_iter().zip(own_targets(r)) {
                        let fed = mt.links().iter().zip(rt.links()).any(|(ml, rl)| {
                            let changed = *ml != rl || link_holds_placeholder(&rl);
                            changed && !seen.fills(ml, &rl)
                        });
                        if fed {
                            return Err(format!(
                                "Step {step}: {}: its input {} gave {}, which does not fit what was seen on the live app",
                                c.name,
                                name.trim(),
                                rt.describe()
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
