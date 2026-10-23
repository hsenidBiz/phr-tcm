//! A test case's own words, as plain text for comparisons: what a reported
//! divergence quotes, and what a script's check values are held against.
//!
//! Pure: nothing here reads a file, a page or Azure DevOps.

use std::collections::BTreeSet;

use super::seen_check::has_phrase;

/// One test case's words as plain text: title, each step's action and
/// expected result, preconditions.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CaseText {
    pub title: String,
    /// Step 1 first: (action, expected result).
    pub steps: Vec<(String, String)>,
    pub preconditions: String,
}

impl CaseText {
    /// Steps through `steps_xml::plain_texts` (pairs: action, expected);
    /// preconditions as the field reads.
    pub fn from_case(case: &crate::ado::TestCaseFull) -> CaseText {
        let texts = crate::steps_xml::plain_texts(&case.steps);
        let steps = texts.chunks(2).map(|p| (p[0].clone(), p.get(1).cloned().unwrap_or_default())).collect();
        CaseText { title: case.title.clone(), steps, preconditions: case.preconditions.clone() }
    }

    /// Title, every step's two texts and the preconditions, each folded,
    /// joined with "\n".
    pub fn all_folded(&self) -> String {
        let mut parts = vec![fold(&self.title)];
        for (action, expected) in &self.steps {
            parts.push(fold(action));
            parts.push(fold(expected));
        }
        parts.push(fold(&self.preconditions));
        parts.join("\n")
    }

    /// Step `n`'s action and expected result, folded, joined with "\n"
    /// (empty for a step the case has not).
    pub fn step_folded(&self, n: i32) -> String {
        if n < 1 {
            return String::new();
        }
        match self.steps.get(n as usize - 1) {
            Some((action, expected)) => format!("{}\n{}", fold(action), fold(expected)),
            None => String::new(),
        }
    }

    /// A short stable print of the steps' text (each step's action and
    /// expected result with whitespace collapsed), for noticing that a case
    /// was rewritten. Case and punctuation count: a rewrite may change only
    /// those. The title and preconditions are not in it.
    pub fn steps_fingerprint(&self) -> String {
        let collapse = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut text = String::new();
        for (action, expected) in &self.steps {
            text.push_str(&collapse(action));
            text.push('\u{1f}');
            text.push_str(&collapse(expected));
            text.push('\u{1e}');
        }
        crate::filewatch::fingerprint(text.as_bytes())
    }

    /// Step `n`'s action alone, folded; none for a step the case has not or
    /// one with no action text.
    pub fn step_action_folded(&self, n: i32) -> Option<String> {
        if n < 1 {
            return None;
        }
        let folded = fold(&self.steps.get(n as usize - 1)?.0);
        (!folded.is_empty()).then_some(folded)
    }

    pub fn preconditions_folded(&self) -> String {
        fold(&self.preconditions)
    }

    /// Whether the folded `phrase` (at least 1 char) is in `all_folded` as
    /// whole words.
    pub fn holds(&self, phrase: &str) -> bool {
        let phrase = fold(phrase);
        if phrase.is_empty() {
            return false;
        }
        let all = self.all_folded();
        has_phrase(&all, &phrase) || range_members(&all).iter().any(|m| *m == phrase)
    }

    /// Every number the case holds: the digit runs of its words, plus the
    /// size of each range it spells out ("Level 1 to Level 4" holds 4).
    pub fn case_numbers(&self) -> BTreeSet<u32> {
        let all = self.all_folded();
        let mut set = numbers(&all);
        set.extend(range_sizes(&all));
        set.extend(
            all.split(|c: char| !c.is_alphanumeric())
                .filter_map(|w| NUMBER_WORDS.iter().position(|n| *n == w))
                .map(|i| i as u32 + 1),
        );
        set
    }

    /// Every duration the case states, in whole ms (`stated_durations`).
    pub fn stated_durations(&self) -> BTreeSet<u64> {
        stated_durations(&self.all_folded())
    }
}

/// Every duration the text states: a number (digits, with thousands separators ("1,000") and an optional `.`
/// or `,` and more digits), optional spaces, then a unit word. `ms`, `msec`, `msecs`, `millisecond` and
/// `milliseconds` are ms; `s`, `sec`, `secs`, `second` and `seconds` are x 1000. Each is rounded to whole ms,
/// so "800 ms", "800ms" and "0.8 s" all give 800, and "1,000 ms" gives 1000. A range ("600 to 900 ms",
/// "600-900 ms") states both ends, in the unit its last number carries. A number that sits against a letter
/// ("q4 ms") is not one.
pub fn stated_durations(folded: &str) -> BTreeSet<u64> {
    let chars: Vec<char> = folded.chars().collect();
    let mut out = BTreeSet::new();
    let mut put = |n: f64, per_unit: f64| {
        let ms = (n * per_unit).round();
        if ms.is_finite() && (0.0..9.0e15).contains(&ms) {
            out.insert(ms as u64);
        }
    };
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        let Some((first, end)) = read_number(&chars, i) else {
            i += 1;
            continue;
        };
        i = end;
        if start > 0 && chars[start - 1].is_alphabetic() {
            continue;
        }
        if let Some((per_unit, _)) = read_unit(&chars, end) {
            put(first, per_unit);
            continue;
        }
        // A range: "600 to 900 ms", "600-900 ms". Both ends carry the unit the last one has.
        let Some(second_at) = range_connector(&chars, end) else { continue };
        let Some((second, second_end)) = read_number(&chars, second_at) else { continue };
        if let Some((per_unit, _)) = read_unit(&chars, second_end) {
            put(first, per_unit);
            put(second, per_unit);
            i = second_end;
        }
    }
    out
}

/// A number starting at `at`: digits, thousands separators (a `,` and exactly three digits), and a decimal part
/// (a `.` or `,` and more digits). Its value and where it ends.
fn read_number(chars: &[char], at: usize) -> Option<(f64, usize)> {
    let digits = |from: usize| chars.get(from..).map_or(0, |rest| rest.iter().take_while(|c| c.is_ascii_digit()).count());
    let mut i = at + digits(at);
    if i == at {
        return None;
    }
    let mut text: String = chars[at..i].iter().collect();
    // "1,000": a comma and exactly three digits is a thousands separator.
    while i < chars.len() && chars[i] == ',' && digits(i + 1) == 3 {
        text.extend(&chars[i + 1..i + 4]);
        i += 4;
    }
    if i + 1 < chars.len() && matches!(chars[i], '.' | ',') && chars[i + 1].is_ascii_digit() {
        let frac = digits(i + 1);
        text.push('.');
        text.extend(&chars[i + 1..i + 1 + frac]);
        i += 1 + frac;
    }
    text.parse::<f64>().ok().map(|n| (n, i))
}

/// The unit word at `at` (after optional spaces): how many ms one of it is, and where it ends.
fn read_unit(chars: &[char], at: usize) -> Option<(f64, usize)> {
    let mut j = at;
    while j < chars.len() && chars[j] == ' ' {
        j += 1;
    }
    let unit_start = j;
    while j < chars.len() && chars[j].is_alphabetic() {
        j += 1;
    }
    let unit: String = chars[unit_start..j].iter().collect();
    let per_unit = match unit.as_str() {
        "ms" | "msec" | "msecs" | "millisecond" | "milliseconds" => 1.0,
        "s" | "sec" | "secs" | "second" | "seconds" => 1000.0,
        _ => return None,
    };
    Some((per_unit, j))
}

/// Where the second number of a range starts, when a range's connector ("to" or a dash, with optional
/// spaces) follows `at`.
fn range_connector(chars: &[char], at: usize) -> Option<usize> {
    let mut j = at;
    while j < chars.len() && chars[j] == ' ' {
        j += 1;
    }
    if chars.get(j..j + 2).is_some_and(|w| w == ['t', 'o']) {
        j += 2;
    } else if chars.get(j).is_some_and(|c| matches!(c, '-' | '\u{2013}' | '\u{2014}')) {
        j += 1;
    } else {
        return None;
    }
    while j < chars.len() && chars[j] == ' ' {
        j += 1;
    }
    chars.get(j).is_some_and(|c| c.is_ascii_digit()).then_some(j)
}

/// Every maximal ASCII digit run whose neighbours are not alphabetic ("level
/// 4" gives 4, "q4" gives nothing); runs over 9 digits are skipped.
pub fn numbers(folded: &str) -> BTreeSet<u32> {
    let chars: Vec<char> = folded.chars().collect();
    let mut out = BTreeSet::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        let before_ok = start == 0 || !chars[start - 1].is_alphabetic();
        let after_ok = i >= chars.len() || !chars[i].is_alphabetic();
        if before_ok && after_ok && i - start <= 9 {
            if let Ok(n) = chars[start..i].iter().collect::<String>().parse::<u32>() {
                out.insert(n);
            }
        }
    }
    out
}

/// "one" is 1 ... "twenty" is 20.
const NUMBER_WORDS: [&str; 20] = [
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve", "thirteen",
    "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen", "twenty",
];

/// "levels" -> "level": the word a range's members are written with.
fn singular(w: &str) -> String {
    match w.strip_suffix('s') {
        Some(s) if s.chars().count() >= 3 && !s.ends_with('s') => s.to_string(),
        _ => w.to_string(),
    }
}

fn is_dash(t: &str) -> bool {
    matches!(t, "-" | "\u{2013}" | "\u{2014}")
}

/// One range the case spells out: the word, its two ends.
fn ranges(folded: &str) -> Vec<(String, u32, u32)> {
    let raw: Vec<&str> = folded.split_whitespace().collect();
    let clean = |t: &str| t.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
    let int = |t: &str| -> Option<u32> {
        let c = clean(t);
        if !c.is_empty() && c.chars().all(|ch| ch.is_ascii_digit()) && c.len() <= 9 {
            c.parse().ok()
        } else {
            None
        }
    };
    let is_sep = |t: &str| is_dash(t) || matches!(clean(t).as_str(), "to" | "through" | "until");
    let word_of = |t: &str| {
        let c = clean(t);
        (c.chars().count() >= 2 && c.chars().all(char::is_alphabetic)).then(|| singular(&c))
    };
    let ok = |a: u32, b: u32| a < b && b - a <= 50;
    let mut out = Vec::new();
    for i in 0..raw.len() {
        let Some(x) = word_of(raw[i]) else { continue };
        let Some(&t1) = raw.get(i + 1) else { continue };
        // "X a-b"
        let c1 = clean(t1);
        if let Some((l, r)) = c1.split_once('-') {
            if let (Ok(a), Ok(b)) = (l.parse::<u32>(), r.parse::<u32>()) {
                if ok(a, b) {
                    out.push((x.clone(), a, b));
                    continue;
                }
            }
        }
        let Some(a) = int(t1) else { continue };
        let Some(&t2) = raw.get(i + 2) else { continue };
        if !is_sep(t2) {
            continue;
        }
        let Some(&t3) = raw.get(i + 3) else { continue };
        // "X a to b" / "X a - b": a bare end
        if let Some(b) = int(t3) {
            if ok(a, b) {
                out.push((x.clone(), a, b));
            }
            continue;
        }
        // "X a to X b"
        if word_of(t3).as_deref() == Some(x.as_str()) {
            if let Some(b) = raw.get(i + 4).and_then(|t| int(t)) {
                if ok(a, b) {
                    out.push((x.clone(), a, b));
                }
            }
        }
    }
    out
}

/// The members of each range the case spells out: "level 1 to level 4"
/// gives "level 1", "level 2", "level 3", "level 4".
pub fn range_members(folded: &str) -> Vec<String> {
    ranges(folded).into_iter().flat_map(|(x, a, b)| (a..=b).map(move |k| format!("{x} {k}"))).collect()
}

/// How many members each range holds.
fn range_sizes(folded: &str) -> Vec<u32> {
    ranges(folded).into_iter().map(|(_, a, b)| b - a + 1).collect()
}

/// A number the case states about a kind of thing: "exactly 4 levels".
#[derive(Debug, Clone, PartialEq)]
pub struct StatedCount {
    pub n: u32,
    /// The qualifier (when there is one), the number and the noun, as written.
    pub phrase: String,
    /// The noun lowercased, plural off.
    pub stem: String,
}

const STOP: &[&str] = &[
    "to", "and", "or", "of", "through", "until", "than", "in", "on", "at", "by", "per", "for", "from", "is", "are",
    "was", "were", "x", "times", "second", "seconds", "minute", "minutes", "hour", "hours", "day", "days", "week",
    "weeks", "month", "months", "year", "years", "am", "pm", "percent",
];

fn trim_punct(t: &str) -> &str {
    t.trim_matches(|c: char| !c.is_alphanumeric())
}

/// The counts the ORIGINAL text states: a whole number of 2 or more, then a
/// noun.
pub fn stated_counts(text: &str) -> Vec<StatedCount> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let mut out = Vec::new();
    for i in 0..tokens.len() {
        let tok = tokens[i];
        if tok.is_empty() || tok.len() > 9 || !tok.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(n) = tok.parse::<u32>() else { continue };
        // A year is not a count.
        if n < 2 || (1900..=2099).contains(&n) {
            continue;
        }
        let Some(next) = tokens.get(i + 1) else { continue };
        if !next.chars().next().is_some_and(char::is_alphabetic) {
            continue;
        }
        let noun = trim_punct(next);
        let lower = noun.to_lowercase();
        if lower.is_empty() || STOP.contains(&lower.as_str()) {
            continue;
        }
        let stem = if lower.ends_with("es") && lower.chars().count() > 4 {
            lower[..lower.len() - 2].to_string()
        } else if let Some(s) = lower.strip_suffix('s') {
            s.to_string()
        } else {
            lower.clone()
        };
        if stem.chars().count() < 3 {
            continue;
        }
        let qualifier = i
            .checked_sub(1)
            .map(|p| tokens[p])
            .filter(|q| matches!(trim_punct(q).to_lowercase().as_str(), "exactly" | "only" | "precisely" | "all"));
        let phrase = match qualifier {
            Some(q) => format!("{} {n} {noun}", trim_punct(q)),
            None => format!("{n} {noun}"),
        };
        out.push(StatedCount { n, phrase, stem });
    }
    out
}

/// Lowercase; curly quotes to straight; `*`, backtick, `#` and `>` removed;
/// whitespace collapsed to one space; trimmed; then leading and trailing
/// `"'.,:;!?()[]` trimmed.
pub fn fold(s: &str) -> String {
    let straight: String = s
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => Some('"'),
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => Some('\''),
            '*' | '`' | '#' | '>' => None,
            c => Some(c),
        })
        .collect();
    let collapsed = straight.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.trim_matches(|c: char| "\"'.,:;!?()[]".contains(c)).to_string()
}
