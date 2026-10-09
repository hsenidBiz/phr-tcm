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
//! The script's own data is exempt the same way: the name of a Test file
//! the script uploads (and a name holding it), that file's size as the app
//! shows it ("240.0 KB" or "1.2 MB"), a name that is a date the script
//! picked (a component's text input) or typed, and a `dd/mm/yyyy` name
//! inside a date picker that was seen.
//!
//! Some locators are built from data, so they are compared with a
//! sighting by more than their text. None of this lets through a locator
//! that could not exist on the seen page:
//!
//! - A placeholder the run fills in (`{{fixture.<id>.<output>}}`,
//!   `{{setup.<output>}}`, `{{prefix}}`, `{{now:...}}`) inside a name, a
//!   text or a css selector stands for a non-empty run of a seen value
//!   with no quote in it. The text around it must match the sighting as
//!   written, and so must the role (or the rest of the selector). A
//!   component input holding one is checked again once the run has filled
//!   it in (`check_resolved_inputs`): the value must have the seen value's
//!   shape, all digits where that was all digits.
//! - A css selector's `:checked`, `:disabled`, `:enabled` and `:focus` are
//!   left out before it is compared. A `:not(X)` or `:has(X)` passes only
//!   when the rest was seen and X was seen in the same areas too: X as a
//!   selector, or, when X is only attribute filters, the rest seen
//!   carrying each of those attributes.
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
use super::discovery_map::{page_path, path_only, seen_keys, seen_links, seen_paths, DiscoveryMap};
use super::edits::Edit;
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

// ---- names, placeholders and selectors ----

/// A name or a text as the check compares it: whitespace collapsed, case
/// folded, an em dash or an en dash read as a hyphen, and no space beside
/// a hyphen. Both sides of a comparison go through it.
pub fn norm_name(s: &str) -> String {
    let dashed: String = s.chars().map(|c| if matches!(c, '\u{2014}' | '\u{2013}') { '-' } else { c }).collect();
    fold_name(&dashed).replace(" -", "-").replace("- ", "-")
}

/// Is `name` (what sits between the braces) a placeholder the run fills in
/// from data: a fixture's output, the setup's, the run's prefix, or a date
/// and time?
pub fn is_data_placeholder(name: &str) -> bool {
    let n = name.trim();
    n.starts_with("fixture.") || n.starts_with("setup.") || n == "prefix" || n.starts_with("now:")
}

/// A name or a selector cut where its data placeholders are.
#[derive(Debug, Clone, PartialEq)]
enum Piece {
    Lit(String),
    Wild,
}

fn pieces(s: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut rest = s;
    while let Some(at) = rest.find("{{") {
        let after = &rest[at + 2..];
        match after.find("}}") {
            Some(end) if is_data_placeholder(&after[..end]) => {
                lit.push_str(&rest[..at]);
                if !lit.is_empty() {
                    out.push(Piece::Lit(std::mem::take(&mut lit)));
                }
                out.push(Piece::Wild);
                rest = &after[end + 2..];
            }
            _ => {
                lit.push_str(&rest[..at + 2]);
                rest = after;
            }
        }
    }
    lit.push_str(rest);
    if !lit.is_empty() {
        out.push(Piece::Lit(lit));
    }
    out
}

/// Does `s` hold a data placeholder?
pub fn holds_data_placeholder(s: &str) -> bool {
    pieces(s).contains(&Piece::Wild)
}

/// Is every `{{` and `}}` in `s` part of a data placeholder? A component's
/// text input may carry one; any other brace pair would be read as the
/// component's own placeholder.
pub fn only_data_placeholders(s: &str) -> bool {
    pieces(s).iter().all(|p| match p {
        Piece::Lit(l) => !l.contains("{{") && !l.contains("}}"),
        Piece::Wild => true,
    })
}

fn is_quote(c: char) -> bool {
    matches!(c, '"' | '\'')
}

/// `pieces` matched against the whole of `seen`: what each placeholder
/// stands for there (a non-empty run with no quote), or `None`.
fn fit(pieces: &[Piece], seen: &str) -> Option<Vec<String>> {
    fn go(pieces: &[Piece], s: &str, caps: &mut Vec<String>) -> bool {
        match pieces.split_first() {
            None => s.is_empty(),
            Some((Piece::Lit(l), rest)) => match s.strip_prefix(l.as_str()) {
                Some(after) => go(rest, after, caps),
                None => false,
            },
            Some((Piece::Wild, rest)) => {
                for (i, c) in s.char_indices() {
                    if is_quote(c) {
                        break;
                    }
                    let end = i + c.len_utf8();
                    caps.push(s[..end].to_string());
                    if go(rest, &s[end..], caps) {
                        return true;
                    }
                    caps.pop();
                }
                false
            }
        }
    }
    let mut caps = Vec::new();
    go(pieces, seen, &mut caps).then_some(caps)
}

/// Does `want`, which holds a data placeholder, fit `seen`?
fn wild_fits(want: &str, seen: &str) -> bool {
    let p = pieces(want);
    p.contains(&Piece::Wild) && fit(&p, seen).is_some()
}

/// Was each value a placeholder stood for at run time (`got`) the shape of
/// what it stood for in the sighting (`seen`): all digits where that was?
fn same_shape(seen: &[String], got: &[String]) -> bool {
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    seen.len() == got.len() && seen.iter().zip(got).all(|(s, g)| !digits(s) || digits(g))
}

const STATES: [&str; 4] = ["checked", "disabled", "enabled", "focus"];

fn ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}

/// `css` with each state pseudo-class (`STATES`) left out, outside quotes
/// and attribute brackets. `:focus-visible` and `::checked` are not one.
fn strip_states(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut quote: Option<char> = None;
    let mut bracket = 0usize;
    let mut i = 0;
    while let Some(c) = css[i..].chars().next() {
        let width = c.len_utf8();
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            out.push(c);
            i += width;
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '[' => bracket += 1,
            ']' => bracket = bracket.saturating_sub(1),
            ':' if bracket == 0 && !out.ends_with(':') && !css[i + 1..].starts_with(':') => {
                let rest = &css[i + 1..];
                let state = STATES
                    .iter()
                    .find(|s| rest.starts_with(**s) && !rest[s.len()..].chars().next().is_some_and(ident_char));
                if let Some(s) = state {
                    i += 1 + s.len();
                    continue;
                }
            }
            _ => {}
        }
        out.push(c);
        i += width;
    }
    out.trim().to_string()
}

/// The length of what comes before the `)` that closes a paren already
/// open, quotes and nested parens skipped; `None` when it never closes.
fn paren_len(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' => depth += 1,
            ')' if depth == 0 => return Some(i),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// `css` without its `:not(...)` and `:has(...)`, and what each held. One
/// that never closes is left in, so it matches nothing.
fn split_filters(css: &str) -> (String, Vec<String>) {
    let mut base = String::with_capacity(css.len());
    let mut inners = Vec::new();
    let mut quote: Option<char> = None;
    let mut bracket = 0usize;
    let mut i = 0;
    while let Some(c) = css[i..].chars().next() {
        let width = c.len_utf8();
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            base.push(c);
            i += width;
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '[' => bracket += 1,
            ']' => bracket = bracket.saturating_sub(1),
            ':' if bracket == 0 => {
                let rest = &css[i..];
                if let Some(head) = [":not(", ":has("].iter().find(|h| rest.starts_with(**h)) {
                    if let Some(len) = paren_len(&rest[head.len()..]) {
                        inners.push(rest[head.len()..head.len() + len].trim().to_string());
                        i += head.len() + len + 1;
                        continue;
                    }
                }
            }
            _ => {}
        }
        base.push(c);
        i += width;
    }
    (base.trim().to_string(), inners)
}

/// The attribute names of a selector that is attribute filters only
/// (`[data-x*="a"]`, `[a][b="c"]`); `None` for any other.
fn filter_attributes(x: &str) -> Option<Vec<String>> {
    let mut names = Vec::new();
    let mut rest = x.trim();
    if rest.is_empty() {
        return None;
    }
    while !rest.is_empty() {
        rest = rest.strip_prefix('[')?;
        let mut quote: Option<char> = None;
        let close = rest.char_indices().find_map(|(i, c)| {
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
                return None;
            }
            if is_quote(c) {
                quote = Some(c);
                return None;
            }
            (c == ']').then_some(i)
        })?;
        let name: String = rest[..close]
            .trim_start()
            .chars()
            .take_while(|c| !matches!(c, '=' | '~' | '|' | '^' | '$' | '*') && !c.is_whitespace())
            .collect();
        if name.is_empty() {
            return None;
        }
        names.push(name);
        rest = rest[close + 1..].trim_start();
    }
    Some(names)
}

// ---- dates ----

/// A date as (year, month, day).
type Date = (u32, u32, u32);

/// A date written `dd/mm/yyyy`, `d-m-yyyy`, `dd.mm.yyyy` or `yyyy-mm-dd`
/// (any one of `/`, `-` or `.` between the parts), and nothing else.
fn parse_date(s: &str) -> Option<Date> {
    let s = s.trim();
    let sep = s.chars().find(|c| matches!(c, '/' | '-' | '.'))?;
    let parts: Vec<&str> = s.split(sep).collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty() || p.len() > 4 || !p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    let n = |p: &str| p.parse::<u32>().ok();
    let short = |p: &str| p.len() <= 2;
    let (y, m, d) = if parts[0].len() == 4 && short(parts[1]) && short(parts[2]) {
        (n(parts[0])?, n(parts[1])?, n(parts[2])?)
    } else if parts[2].len() == 4 && short(parts[0]) && short(parts[1]) {
        (n(parts[2])?, n(parts[1])?, n(parts[0])?)
    } else {
        return None;
    };
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then_some((y, m, d))
}

/// Exactly `dd/mm/yyyy`, a real day and month.
fn is_ddmmyyyy(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[2] == b'/'
        && b[5] == b'/'
        && [0, 1, 3, 4, 6, 7, 8, 9].iter().all(|&i| b[i].is_ascii_digit())
        && parse_date(s).is_some()
}

/// Does this link name a date picker: "date" as a word, or "datepicker",
/// "date-picker" or "calendar", in its name, text or selector?
fn is_date_picker(link: &LocatorStep) -> bool {
    [link.name.as_deref(), link.text.as_deref(), link.css.as_deref()].into_iter().flatten().any(|v| {
        let v = v.to_lowercase();
        has_phrase(&v, "date") || v.contains("datepicker") || v.contains("calendar")
    })
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
}

impl Sightings {
    fn new(map: &DiscoveryMap, areas: &[&str]) -> Self {
        let mut s = Sightings { keys: seen_keys(map, areas), roles: Vec::new(), texts: Vec::new(), css: Vec::new() };
        for l in seen_links(map, areas) {
            if let Some(role) = &l.role {
                s.roles.push((fold_name(role), norm_name(l.name.as_deref().unwrap_or("")), l.clone()));
            } else if let Some(t) = &l.text {
                s.texts.push((norm_name(t), l.clone()));
            } else if let Some(c) = &l.css {
                s.css.push(strip_states(c));
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
        let fits = |want: &str, seen: &str| want == seen || (wild && wild_fits(want, seen));
        if let Some(role) = &link.role {
            let (r, n) = (fold_name(role), norm_name(link.name.as_deref().unwrap_or("")));
            return self.roles.iter().any(|(sr, sn, _)| *sr == r && fits(&n, sn));
        }
        if let Some(t) = &link.text {
            let n = norm_name(t);
            return self.texts.iter().any(|(sn, _)| fits(&n, sn));
        }
        link.css.as_deref().is_some_and(|c| self.css_seen(c, wild, 0))
    }

    fn base_fits(base: &str, seen: &str, wild: bool) -> bool {
        base == seen || (wild && wild_fits(base, seen))
    }

    /// A css selector: its state pseudo-classes left out, the rest seen,
    /// and each `:not`/`:has` filter's inner part seen too.
    fn css_seen(&self, css: &str, wild: bool, depth: u8) -> bool {
        let (base, inners) = split_filters(&strip_states(css));
        if base.is_empty() || !self.css.iter().any(|s| Self::base_fits(&base, s, wild)) {
            return false;
        }
        inners.iter().all(|x| match filter_attributes(x) {
            Some(names) => names.iter().all(|n| self.carries(&base, n, wild)),
            None => depth < 3 && self.css_seen(x, wild, depth + 1),
        })
    }

    /// Was `base` seen with attribute `attr` on it: a seen selector that is
    /// `base` followed by attribute filters, one of them `attr`'s?
    fn carries(&self, base: &str, attr: &str, wild: bool) -> bool {
        self.css.iter().any(|s| {
            s.char_indices().filter(|(_, c)| *c == '[').any(|(k, _)| {
                k > 0
                    && filter_attributes(&s[k..]).is_some_and(|names| names.iter().any(|n| n == attr))
                    && Self::base_fits(base, s[..k].trim_end(), wild)
            })
        })
    }

    /// Does `filled`, a link a run filled a data placeholder of `template`
    /// in, match a sighting `template` matches, each value the placeholder
    /// took the shape of the seen value it stands for there?
    fn fills(&self, template: &LocatorStep, filled: &LocatorStep) -> bool {
        let unfilled = [&filled.role, &filled.name, &filled.text, &filled.css]
            .into_iter()
            .flatten()
            .any(|v| holds_data_placeholder(v));
        if unfilled {
            return false;
        }
        if self.has(filled, false) {
            return true;
        }
        let (want, got, seen): (String, String, Vec<&str>) = if let Some(role) = &filled.role {
            let r = fold_name(role);
            (
                norm_name(template.name.as_deref().unwrap_or("")),
                norm_name(filled.name.as_deref().unwrap_or("")),
                self.roles.iter().filter(|(sr, ..)| *sr == r).map(|(_, n, _)| n.as_str()).collect(),
            )
        } else if let Some(t) = &filled.text {
            (
                norm_name(template.text.as_deref().unwrap_or("")),
                norm_name(t),
                self.texts.iter().map(|(n, _)| n.as_str()).collect(),
            )
        } else if let Some(c) = &filled.css {
            let written = template.css.as_deref().unwrap_or("");
            if !self.css_seen(written, true, 0) {
                return false;
            }
            (
                split_filters(&strip_states(written)).0,
                split_filters(&strip_states(c)).0,
                self.css.iter().map(String::as_str).collect(),
            )
        } else {
            return false;
        };
        let p = pieces(&want);
        let Some(took) = fit(&p, &got) else { return false };
        seen.into_iter().any(|s| fit(&p, s).is_some_and(|stood| same_shape(&stood, &took)))
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
        best.and_then(|(_, l)| match (&l.role, &l.text) {
            (Some(r), _) => Some(format!("did you mean {r} \"{}\"?", l.name.as_deref().unwrap_or(""))),
            (None, Some(t)) => Some(format!("did you mean text \"{t}\"?")),
            _ => None,
        })
    }
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
    /// The Test files the script uploads, folded, each at least
    /// `MIN_TYPED_LEN`.
    files: &'a [String],
    /// Their sizes as the app shows them, folded.
    sizes: &'a [String],
    /// The dates the script picked or typed so far.
    dates: &'a [Date],
}

/// Is `link` the script's own data: a value typed earlier, a name the case
/// says (a check only), a Test file it uploads or that file's size, or a
/// date it picked or typed?
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
    let picked = own_words.iter().filter_map(|w| parse_date(w)).any(|d| own.dates.contains(&d));
    typed_here || in_case || own_file || picked
}

/// Is `link` a `dd/mm/yyyy` day inside a date picker seen in these areas:
/// an earlier link of its chain?
fn in_seen_date_picker(link: &LocatorStep, chain: &[LocatorStep], seen: &Sightings) -> bool {
    let Some(at) = chain.iter().position(|l| l == link) else { return false };
    let name = link.name.as_deref().or(link.text.as_deref()).unwrap_or("");
    is_ddmmyyyy(name.trim()) && chain[..at].iter().any(|o| is_date_picker(o) && seen.has(o, true))
}

/// The first of `links` (links of `chain`) neither seen nor exempt.
fn first_unseen<'a>(links: &'a [LocatorStep], chain: &[LocatorStep], seen: &Sightings, own: &Own) -> Option<&'a LocatorStep> {
    links.iter().find(|l| !seen.has(l, true) && !exempt(l, own) && !in_seen_date_picker(l, chain, seen))
}

/// The Test files `actions` upload, as named.
fn uploads<'a>(actions: impl Iterator<Item = &'a Action>) -> Vec<String> {
    actions
        .filter_map(|a| match a {
            Action::Upload { file, .. } => Some(file.clone()),
            _ => None,
        })
        .collect()
}

/// `names` folded, those long enough to exempt anything.
fn exempting(names: &[String]) -> Vec<String> {
    names.iter().map(|n| fold_name(n)).filter(|n| n.chars().count() >= MIN_TYPED_LEN).collect()
}

/// The sizes of the uploaded `names` among `files`, as the app shows a
/// size, folded: one decimal, in KB and in MB (1 KB = 1024 bytes), as
/// "240.0 KB" and "0.2 MB" (`test_files::human_size`'s form).
fn shown_sizes(names: &[String], files: &[TestFile]) -> Vec<String> {
    files
        .iter()
        .filter(|f| names.iter().any(|n| n.trim().eq_ignore_ascii_case(f.name.trim())))
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

/// [`check_seen`], knowing the project's Test files (`files`), so the size
/// the app shows for a file the script uploads is exempt too.
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
    // The Test files the script uploads, anywhere in it, a component's
    // uploads included, and their sizes as the app shows them.
    let uploaded = {
        let ran: Vec<Action> = script
            .steps
            .iter()
            .flat_map(|s| s.actions.iter())
            .flat_map(Action::each)
            .flat_map(|a| as_run(components, a))
            .collect();
        let own = script.steps.iter().flat_map(|s| s.actions.iter()).flat_map(Action::each);
        uploads(own.chain(ran.iter().flat_map(Action::each)))
    };
    let own_files = exempting(&uploaded);
    let sizes = shown_sizes(&uploaded, files);
    // Values typed by the steps before the one being checked.
    let mut typed: Vec<String> = Vec::new();
    // The text inputs the steps before it gave components.
    let mut picked_before: Vec<String> = Vec::new();

    for step in &script.steps {
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
/// as they expand.
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
    let own_files = exempting(&uploads(actions.iter().flat_map(Action::each)));
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
            let own = Own { typed: &typed, case_text: &[], check: is_check(a), files: &own_files, sizes: &[], dates: &dates };
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
/// placeholder? Only then is there anything for `check_resolved_inputs`.
pub fn has_placeholder_inputs(steps: &[StepScript]) -> bool {
    steps.iter().flat_map(|s| s.actions.iter()).flat_map(Action::each).any(|a| {
        matches!(a, Action::UseComponent { inputs, .. } if inputs.values().any(value_holds_placeholder))
    })
}

/// The run-time half of the check for a component input that held a data
/// placeholder when the script was saved. `saved` are the steps as saved,
/// `filled` the same steps once the run filled the placeholders in; each
/// locator such an input goes into must match a sighting in `areas` that
/// the saved locator matches, every value the placeholder took the shape
/// of the seen value it stands for there (all digits where that was). A
/// placeholder the run left unfilled fails. The failure names the
/// component, the input and the locator it gave.
pub fn check_resolved_inputs(
    map: &DiscoveryMap,
    components: &ComponentFile,
    areas: &[&str],
    saved: &[StepScript],
    filled: &[StepScript],
) -> Result<(), String> {
    let seen = Sightings::new(map, areas);
    for (ss, fs) in saved.iter().zip(filled) {
        for (sa, fa) in ss.actions.iter().flat_map(Action::each).zip(fs.actions.iter().flat_map(Action::each)) {
            let (Action::UseComponent { component, inputs: given }, Action::UseComponent { inputs: filled_in, .. }) = (sa, fa)
            else {
                continue;
            };
            if !given.values().any(value_holds_placeholder) {
                continue;
            }
            let c = find(components, component).ok_or_else(|| not_saved(component))?;
            let ran = expand(c, filled_in)?;
            for (name, v) in given.iter().filter(|(_, v)| value_holds_placeholder(v)) {
                // The same use with only this input as it was saved: the
                // locators that differ from `ran` are the ones it fed.
                let mut marked = filled_in.clone();
                marked.insert(name.clone(), v.clone());
                let as_saved = expand(c, &marked)?;
                for (m, r) in as_saved.iter().flat_map(Action::each).zip(ran.iter().flat_map(Action::each)) {
                    for (mt, rt) in own_targets(m).into_iter().zip(own_targets(r)) {
                        let fed = mt.links().iter().zip(rt.links()).any(|(ml, rl)| {
                            let changed = *ml != rl || serde_json::to_value(&rl).is_ok_and(|v| value_holds_placeholder(&v));
                            changed && !seen.fills(ml, &rl)
                        });
                        if fed {
                            return Err(format!(
                                "{}: its input {} gave {}, which does not fit what was seen on the live app",
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
