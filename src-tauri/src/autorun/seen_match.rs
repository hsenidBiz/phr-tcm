//! The text side of the seen check (`seen_check`): how a name, a css
//! selector and a date are read before they are compared with what the
//! map has seen. Nothing here reads the map.
//!
//! A data placeholder is one the run fills in from a fixture or the setup
//! (`{{fixture.<id>.<output>}}`, `{{setup.<output>}}`). In a name or a text
//! it stands for a run of a seen value anywhere. In a css selector it
//! stands for one in two places only: inside a quoted attribute value
//! (`[data-cycle-id="{{setup.cycle_id}}"]`), and inside an id or class
//! token beside a literal part of that token (`#c{{setup.cycle_id}}`,
//! `.row-{{fixture.n}}`), where it stands for letters, digits, `-` and
//! `_` only. Anywhere else (a whole token, `#{{setup.x}}`; an element or
//! combinator position, `{{setup.sel}}`, `div{{setup.x}}`) it is literal
//! text, which no sighting holds.

use crate::browser::locator::fold_name;

/// A name or a text as the check compares it: whitespace collapsed, case
/// folded, an em dash or an en dash read as a hyphen, and no space beside
/// a hyphen. Both sides of a comparison go through it.
pub fn norm_name(s: &str) -> String {
    let dashed: String = s.chars().map(|c| if matches!(c, '\u{2014}' | '\u{2013}') { '-' } else { c }).collect();
    fold_name(&dashed).replace(" -", "-").replace("- ", "-")
}

/// Is `name` (what sits between the braces) a placeholder the run fills in
/// from data: a fixture's output or the setup's? `{{prefix}}` and
/// `{{now:...}}` are not: nothing in Auto Run fills them in.
pub fn is_data_placeholder(name: &str) -> bool {
    let n = name.trim();
    n.starts_with("fixture.") || n.starts_with("setup.")
}

/// A name or a selector cut where its data placeholders are.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Piece {
    Lit(String),
    /// A run with no quote in it.
    Wild,
    /// A run of the characters an id or a class is written with
    /// (`token_char`).
    Token,
}

/// Does `pieces` hold a placeholder of either kind?
pub(crate) fn has_wild(pieces: &[Piece]) -> bool {
    pieces.iter().any(|p| !matches!(p, Piece::Lit(_)))
}

/// A character an id or a class token is written with here: an ASCII
/// letter or digit, `-` or `_`.
fn token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Is the placeholder at byte `at` of `s`, ending at byte `end`, inside an
/// id or class token that has a literal part beside it: token characters
/// right before it back to a `#` or `.`, or right after it?
fn in_token(s: &str, at: usize, end: usize) -> bool {
    let before = &s[..at];
    let prefix = before.len() - before.trim_end_matches(token_char).len();
    let opens = before[..before.len() - prefix].ends_with(['#', '.']);
    let suffix = s[end..].chars().next().is_some_and(token_char);
    opens && (prefix > 0 || suffix)
}

fn push_lit(out: &mut Vec<Piece>, lit: &mut String) {
    if !lit.is_empty() {
        out.push(Piece::Lit(std::mem::take(lit)));
    }
}

/// A name or a text cut at every data placeholder in it.
pub(crate) fn name_pieces(s: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut rest = s;
    while let Some(at) = rest.find("{{") {
        let after = &rest[at + 2..];
        match after.find("}}") {
            Some(end) if is_data_placeholder(&after[..end]) => {
                lit.push_str(&rest[..at]);
                push_lit(&mut out, &mut lit);
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
    push_lit(&mut out, &mut lit);
    out
}

/// A css selector cut at each data placeholder inside a quoted attribute
/// value (`Piece::Wild`) or inside an id or class token beside a literal
/// part of it (`Piece::Token`); one anywhere else stays literal.
pub(crate) fn css_pieces(s: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut quote: Option<char> = None;
    let mut bracket = 0usize;
    let mut i = 0;
    while let Some(c) = s[i..].chars().next() {
        if quote.is_some() && bracket > 0 && s[i..].starts_with("{{") {
            let after = &s[i + 2..];
            if let Some(end) = after.find("}}") {
                let name = &after[..end];
                if is_data_placeholder(name) && !name.contains(['"', '\'']) {
                    push_lit(&mut out, &mut lit);
                    out.push(Piece::Wild);
                    i += 2 + end + 2;
                    continue;
                }
            }
        }
        if quote.is_none() && bracket == 0 && s[i..].starts_with("{{") {
            let after = &s[i + 2..];
            if let Some(end) = after.find("}}") {
                let close = i + 2 + end + 2;
                if is_data_placeholder(&after[..end]) && in_token(s, i, close) {
                    push_lit(&mut out, &mut lit);
                    out.push(Piece::Token);
                    i = close;
                    continue;
                }
            }
        }
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '"' | '\'' => quote = Some(c),
                '[' => bracket += 1,
                ']' => bracket = bracket.saturating_sub(1),
                _ => {}
            },
        }
        lit.push(c);
        i += c.len_utf8();
    }
    push_lit(&mut out, &mut lit);
    out
}

/// Does `s` hold a data placeholder anywhere?
pub fn holds_data_placeholder(s: &str) -> bool {
    name_pieces(s).contains(&Piece::Wild)
}

/// Is every `{{` and `}}` in `s` part of a data placeholder? A component's
/// text input may carry one; any other brace pair would be read as the
/// component's own placeholder.
pub fn only_data_placeholders(s: &str) -> bool {
    name_pieces(s).iter().all(|p| match p {
        Piece::Lit(l) => !l.contains("{{") && !l.contains("}}"),
        Piece::Wild | Piece::Token => true,
    })
}

fn is_quote(c: char) -> bool {
    matches!(c, '"' | '\'')
}

/// `pieces` matched against the whole of `seen`: what each placeholder
/// stands for there (a non-empty run: with no quote for `Piece::Wild`, of
/// `token_char`s for `Piece::Token`), or `None`.
pub(crate) fn fit(pieces: &[Piece], seen: &str) -> Option<Vec<String>> {
    fn go(pieces: &[Piece], s: &str, caps: &mut Vec<String>) -> bool {
        match pieces.split_first() {
            None => s.is_empty(),
            Some((Piece::Lit(l), rest)) => match s.strip_prefix(l.as_str()) {
                Some(after) => go(rest, after, caps),
                None => false,
            },
            Some((kind, rest)) => {
                let token = *kind == Piece::Token;
                for (i, c) in s.char_indices() {
                    if is_quote(c) || (token && !token_char(c)) {
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

/// Do `pieces`, which hold a placeholder, fit `seen`?
pub(crate) fn wild_fits(pieces: &[Piece], seen: &str) -> bool {
    has_wild(pieces) && fit(pieces, seen).is_some()
}

/// Was each value a placeholder took at run time (`got`) the shape of what
/// it stood for in the sighting (`seen`): all digits where that was?
pub(crate) fn same_shape(seen: &[String], got: &[String]) -> bool {
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    seen.len() == got.len() && seen.iter().zip(got).all(|(s, g)| !digits(s) || digits(g))
}

// ---- css ----

const STATES: [&str; 4] = ["checked", "disabled", "enabled", "focus"];

fn ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}

/// Does a pseudo-class at byte `i` of `css` qualify an element written
/// just before it? Not at the start, nor after whitespace, a combinator
/// (`>`, `+`, `~`), a comma or an open paren: there it stands for an
/// element of its own (`.card :checked` is any checked thing inside a
/// card), which must be seen as written.
fn qualifies(css: &str, i: usize) -> bool {
    css[..i].chars().next_back().is_some_and(|p| !p.is_whitespace() && !matches!(p, '>' | '+' | '~' | ',' | '('))
}

/// `css` with each state pseudo-class (`STATES`) that qualifies an element
/// left out, outside quotes and attribute brackets. `:focus-visible` and
/// `::checked` are not one.
pub(crate) fn strip_states(css: &str) -> String {
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
            ':' if bracket == 0 && qualifies(css, i) && !css[..i].ends_with(':') && !css[i + 1..].starts_with(':') => {
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

/// Which filter an inner part came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Filter {
    Not,
    Has,
}

/// `css` without the `:not(...)` and `:has(...)` that qualify an element,
/// and what each held. One that never closes, or that stands for an
/// element of its own (`qualifies`), is left in, so it matches nothing
/// that was not seen as written.
pub(crate) fn split_filters(css: &str) -> (String, Vec<(Filter, String)>) {
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
            ':' if bracket == 0 && qualifies(css, i) => {
                let rest = &css[i..];
                let head = [(":not(", Filter::Not), (":has(", Filter::Has)].into_iter().find(|(h, _)| rest.starts_with(h));
                if let Some((h, kind)) = head {
                    if let Some(len) = paren_len(&rest[h.len()..]) {
                        inners.push((kind, rest[h.len()..h.len() + len].trim().to_string()));
                        i += h.len() + len + 1;
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
pub(crate) fn filter_attributes(x: &str) -> Option<Vec<String>> {
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

/// Every tail of `s` that is attribute filters only, starting at one of its
/// `[`, with the part before it: `.card[data-x="1"]` gives (`.card`,
/// `[data-x="1"]`).
pub(crate) fn attribute_tails(s: &str) -> Vec<(&str, &str)> {
    s.char_indices()
        .filter(|(_, c)| *c == '[')
        .filter(|(k, _)| filter_attributes(&s[*k..]).is_some())
        .map(|(k, _)| (s[..k].trim_end(), &s[k..]))
        .collect()
}

/// Every way `s` reads as an ancestor and a descendant: split at each run
/// of whitespace or a `>` between them, outside quotes, brackets and
/// parens. `.card .row span` gives (`.card`, `.row span`) and (`.card
/// .row`, `span`). A `+` or `~` (a sibling) or a comma ends no split.
pub(crate) fn descendant_splits(s: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut quote: Option<char> = None;
    let (mut bracket, mut paren) = (0usize, 0usize);
    let mut run: Option<(usize, bool)> = None; // start, holds a sibling or a comma
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        let top = bracket == 0 && paren == 0;
        if top && (c.is_whitespace() || matches!(c, '>' | '+' | '~' | ',')) {
            let (start, odd) = run.unwrap_or((i, false));
            run = Some((start, odd || matches!(c, '+' | '~' | ',')));
            continue;
        }
        if let Some((start, odd)) = run.take() {
            if !odd && start > 0 {
                out.push((s[..start].trim_end(), s[i..].trim_start()));
            }
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '[' => bracket += 1,
            ']' => bracket = bracket.saturating_sub(1),
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            _ => {}
        }
    }
    out.retain(|(a, d)| !a.is_empty() && !d.is_empty());
    out
}

// ---- dates ----

/// A date as (year, month, day).
pub(crate) type Date = (u32, u32, u32);

/// A date written `dd/mm/yyyy`, `d-m-yyyy`, `dd.mm.yyyy` or `yyyy-mm-dd`
/// (any one of `/`, `-` or `.` between the parts), and nothing else.
pub(crate) fn parse_date(s: &str) -> Option<Date> {
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
pub(crate) fn is_ddmmyyyy(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[2] == b'/'
        && b[5] == b'/'
        && [0, 1, 3, 4, 6, 7, 8, 9].iter().all(|&i| b[i].is_ascii_digit())
        && parse_date(s).is_some()
}
