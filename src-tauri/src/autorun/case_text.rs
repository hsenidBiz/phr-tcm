//! A test case's own words, as plain text for comparisons: what a reported
//! divergence quotes, and what a script's check values are held against.
//!
//! Pure: nothing here reads a file, a page or Azure DevOps.

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

    pub fn preconditions_folded(&self) -> String {
        fold(&self.preconditions)
    }

    /// Whether the folded `phrase` (at least 1 char) is in `all_folded` as
    /// whole words.
    pub fn holds(&self, phrase: &str) -> bool {
        let phrase = fold(phrase);
        !phrase.is_empty() && has_phrase(&self.all_folded(), &phrase)
    }
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
