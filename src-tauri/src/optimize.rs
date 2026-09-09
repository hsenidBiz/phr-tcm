//! Turning an AI's first draft into a run sheet a tester can actually
//! work through, in one pass instead of a dozen follow-up prompts.
//!
//! Four things happen here, and they feed each other:
//!
//! 1. **Navigation moves out of preconditions and into steps.** A draft
//!    that says "Preconditions: user is on the Payments page" hides the
//!    part a tester most needs spelled out. Those sentences become
//!    leading steps, from launching the app to arriving at the page.
//! 2. **What's left in preconditions is the real setup** - role, data,
//!    feature flags, device. That residue is the case's setup signature.
//! 3. **Cases are ordered by that signature**, so everything sharing a
//!    setup runs together and the tester changes environment as few times
//!    as possible. Groups are then chained nearest-neighbour, so each
//!    switch that does remain changes as little as possible.
//! 4. **Expected results are reduced to the outcome** - no restating the
//!    action, no rationale, no asides. A later sentence that still asserts
//!    something - a negation, an ordering, a value the first sentence did
//!    not name - is kept; only glosses go.
//!
//! Everything is a pure function over `Vec<TestCase>`: no I/O, no ADO, no
//! writes. The AI passes a draft in and gets a draft back.

use crate::model::TestCase;
use crate::steps_xml::Step;
use regex::Regex;

/// Sentence openers that mean "navigate", not "be in this state". These
/// are what gets promoted out of preconditions into real steps.
const NAV_MARKERS: &[&str] = &[
    "user is on",
    "user has navigated",
    "navigate to",
    "navigated to",
    "go to",
    "open the",
    "opened the",
    // "launch" on its own also claimed "Launch darkly flag PAY-42 is on".
    "launch the",
    "launches the",
];

/// Openers that mean navigation ONLY when the sentence also names a place.
///
/// "On the Payments page" is the elliptical form of "the user is on the
/// Payments page" and belongs in the steps. "On the second attempt the
/// lockout applies", "At the end of the billing cycle" and "From the
/// previous run the cart holds 3 items" are setup, and promoting them
/// turned real conditions into nonsense steps AND dropped them from the
/// case's setup signature, which is what decides the run order.
const NAV_MARKERS_NEEDING_PLACE: &[&str] = &["on the", "at the", "from the"];

/// Matched as WHOLE WORDS, never substrings. As a substring test this was
/// exactly backwards: "performance" contains "form", "review" contains
/// "view" and "table" contains "tab", so "At the end of the performance
/// review the rating is locked" counted as naming a place and got promoted
/// to a navigation step - the precise failure the marker split above was
/// added to prevent.
const PLACE_WORDS: &[&str] = &[
    "page", "screen", "tab", "module", "dialog", "form", "view", "menu", "panel", "portal",
    "window", "pane", "list", "grid", "board", "editor", "wizard",
];

/// Openers stripped from an expected result: they restate that we're
/// testing, rather than saying what the tester should see.
const EXPECTED_NOISE: &[&str] = &[
    "verify that ",
    "verify ",
    "ensure that ",
    "ensure ",
    "check that ",
    "check ",
    "confirm that ",
    "confirm ",
    "validate that ",
    "validate ",
    "it should be that ",
    "the system should ",
    "system should ",
    "the user should see ",
    "user should see ",
    "it should ",
    "should be ",
    "should ",
    "expected: ",
    "expected result: ",
];

/// Abbreviations that end in a full stop and are followed by more of the
/// SAME sentence. Without these, "Approx. 30 results are returned" was cut
/// at the first `". "` and became the single word "Approx".
const ABBREVIATIONS: &[&str] = &[
    "approx", "no", "vs", "etc", "fig", "ref", "min", "max", "sec", "mins", "secs", "hrs", "e.g",
    "i.e", "mr", "mrs", "ms", "dr", "st", "co", "inc", "ltd",
];

/// True if the case's own first steps already walk in from the entry
/// point, in which case a preamble would just duplicate them. The first
/// step matching `entry` itself counts - re-running the optimizer (or
/// passing an entry the draft already starts with) must be a no-op, not
/// a second copy of the same step.
/// How far in to look for a preamble the draft already wrote.
///
/// Only `steps.first()` was checked, so a draft that opened with a setup
/// line - "Ensure the seed data script has run." - and launched at step 2
/// was judged to have no preamble and got a whole second one prepended:
/// launch, sign in and navigate, all twice. Bounded rather than the whole
/// case, so a mid-case "navigate to the report tab" cannot suppress a
/// preamble that is genuinely needed.
const PREAMBLE_PROBE: usize = 4;

/// The default entry step when the caller doesn't name one.
pub const DEFAULT_ENTRY: &str = "Launch the application.";

#[derive(Debug, Default, serde::Serialize)]
pub struct OptimizeReport {
    /// Setup signatures in execution order - the run sheet's sections.
    pub groups: Vec<GroupSummary>,
    /// Environment/option switches the tester would have made before, and
    /// after. The headline number.
    pub switches_before: usize,
    pub switches_after: usize,
    pub preamble_steps_added: usize,
    pub expected_trimmed: usize,
    /// Trailing sentences kept because they carried an assertion - the
    /// count that used to be inside `expected_trimmed` as damage.
    pub assertions_kept: usize,
    pub duplicates_removed: usize,
    pub empty_steps_removed: usize,
    /// Anything a human should look at rather than trust blindly.
    pub notes: Vec<String>,
    /// Every precondition this run changed, with its before and after -
    /// so a caller can SEE what moved instead of diffing by hand.
    pub preconditions_rewritten: Vec<PreconditionChange>,
    /// Every expected result this run SHORTENED, same reasoning. A count
    /// alone said nothing about which case lost text, or what it said.
    pub expected_rewritten: Vec<ExpectedChange>,
}

/// One expected result this run shortened, with what it used to say.
#[derive(Debug, serde::Serialize)]
pub struct ExpectedChange {
    pub title: String,
    pub step_number: usize,
    pub before: String,
    pub after: String,
}

/// Whether the difference is more than tidying. Sentence-casing the first
/// letter and adding a trailing full stop happen to almost every step, and
/// counting those drowned the changes that actually removed something.
fn material_loss(before: &str, after: &str) -> bool {
    let cosmetic = {
        let t = before.trim_end_matches(['.', ' ']).trim();
        if t.is_empty() { String::new() } else { format!("{}.", sentence_case(t)) }
    };
    after != cosmetic
}

#[derive(Debug, serde::Serialize)]
pub struct PreconditionChange {
    pub title: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, serde::Serialize)]
pub struct GroupSummary {
    /// Human-readable setup ("Payments · logged in as Admin"), or
    /// "No special setup".
    pub setup: String,
    pub cases: usize,
}

fn names_a_place(lowered: &str) -> bool {
    lowered
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| PLACE_WORDS.contains(&word))
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn sentences(text: &str) -> Vec<String> {
    text.split(['\n', ';'])
        .flat_map(|line| line.split(". "))
        .map(|s| squash(s.trim_matches(|c: char| c == '.' || c.is_whitespace() || c == '-')))
        .filter(|s| !s.is_empty())
        .collect()
}

fn is_navigation(sentence: &str) -> bool {
    let l = sentence.to_lowercase();
    if NAV_MARKERS.iter().any(|m| l.starts_with(m)) {
        return true;
    }
    NAV_MARKERS_NEEDING_PLACE.iter().any(|m| l.starts_with(m)) && names_a_place(&l)
}

/// Capitalise the first character, leaving the rest alone (so acronyms
/// and product names survive).
fn sentence_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Case-insensitive search for an ASCII needle, returning a byte index
/// into `haystack` ITSELF.
///
/// The obvious version - lowercase the string, search that, slice the
/// original at the index it gives back - is wrong, because lowercasing
/// does not preserve byte length: 'S' is three bytes and 's' is one,
/// 'I' is two bytes and lowercases to three. The offsets then point
/// somewhere else in the original, and if they land inside a character
/// the slice panics and takes the optimize call down with it.
///
/// A match here is at an ASCII byte (`eq_ignore_ascii_case` only ever
/// folds A-Z/a-z, and never equates a UTF-8 continuation byte with an
/// ASCII one), so both ends of the match are character boundaries by
/// construction and the slice is always safe.
fn find_ascii_ci(haystack: &str, needle: &str) -> Option<usize> {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    if n.is_empty() || n.len() > h.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

/// `starts_with`, ASCII-case-insensitively, with the same guarantee:
/// when this is true, `&s[prefix.len()..]` is a valid slice.
fn starts_with_ascii_ci(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// Whether byte offset `at` falls inside a quotation.
///
/// Round 4: the trimmer was cutting expected results at sentence
/// boundaries INSIDE quoted message text - `It reads "...has now
/// started. Please complete..."` lost everything after the quoted full
/// stop, turning a case about the second half of a notification into one
/// that never mentions it. Anything between quotes is the requirement
/// being asserted, so no cut may land there.
///
/// Straight quotes have no direction, so parity stands in for nesting: an
/// odd count of `"` before `at` means a quote is open. Curly quotes are
/// directional and counted as open-minus-close.
fn inside_quotes(s: &str, at: usize) -> bool {
    let prefix = &s[..at];
    if prefix.matches('"').count() % 2 == 1 {
        return true;
    }
    prefix.matches('\u{201C}').count() > prefix.matches('\u{201D}').count()
}

/// The end of the first real sentence OUTSIDE any quotation, or None if
/// the whole string is one sentence.
fn sentence_break(s: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = s[from..].find(". ") {
        let at = from + rel;
        let word = s[..at]
            .rsplit(|c: char| c.is_whitespace())
            .next()
            .unwrap_or("")
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        // A single letter is an initial ("J. Smith"), never a sentence end.
        let is_abbrev = ABBREVIATIONS.contains(&word.as_str()) || word.chars().count() == 1;
        if !is_abbrev && !inside_quotes(s, at) {
            return Some(at);
        }
        from = at + 2;
    }
    None
}

/// A one-word opener, as opposed to a phrase that could not be content.
/// "verify " could start a real sentence about a Verify button; "verify
/// that " could not.
fn is_bare_verb(noise: &str) -> bool {
    matches!(
        noise,
        "verify " | "ensure " | "check " | "confirm " | "validate " | "should " | "should be "
    )
}

/// True when the text opens with a verb that has nothing in front of it -
/// what is left after a noise strip has eaten the subject. "Is cleared",
/// "are shown", "be displayed" name no thing for the tester to look at.
fn starts_with_a_bare_copula(s: &str) -> bool {
    const COPULAS: &[&str] = &["is ", "are ", "was ", "were ", "be ", "been ", "has ", "have "];
    COPULAS.iter().any(|c| starts_with_ascii_ci(s, c))
}

/// Reduce an expected result to the observable outcome. See
/// `clean_expected_keeping` for the one thing it will not cut.
pub fn clean_expected(raw: &str) -> String {
    clean_expected_keeping(raw).0
}

/// As `clean_expected`, but also says whether a trailing sentence survived
/// because it carried an assertion the first sentence did not. Bails out
/// and keeps the original whenever trimming would leave nothing useful -
/// losing information is worse than an untidy sentence.
pub fn clean_expected_keeping(raw: &str) -> (String, bool) {
    let mut s = squash(raw);
    if s.is_empty() {
        return (s, false);
    }

    // Drop parenthetical and bracketed asides - but only the ones long
    // enough to BE asides.
    //
    // A short parenthetical is usually part of a literal UI string, not
    // commentary on it: "The badge reads Rejected (Edit) in red" was being
    // trimmed to "Rejected", and "Rejected (Edit)" is the exact value from
    // the spec's status table. That does not make the expected result
    // shorter, it makes it WRONG - the tester now passes a badge reading
    // "Rejected". "Reason (optional)" is the same shape.
    //
    // The two mistakes are not equally bad. Keeping a genuine aside costs a
    // few words; dropping a UI string costs the assertion. So the rule errs
    // toward keeping: three words or more is an aside, one or two is a
    // label. `(e.g. "5 employees")` is three and still goes.
    for (open, close) in [('(', ')'), ('[', ']')] {
        let mut from = 0;
        while let Some(a) = s[from..].find(open).map(|i| i + from) {
            let Some(b) = s[a..].find(close).map(|i| i + a) else { break };
            let inner = &s[a + open.len_utf8()..b];
            // A parenthetical inside a quotation is message text whatever
            // its length - "(including any attachments you have added)"
            // in a quoted alert is part of what the tester reads.
            if inner.split_whitespace().count() >= 3 && !inside_quotes(&s, a) {
                s = format!("{}{}", &s[..a], &s[b + close.len_utf8()..]);
                s = squash(&s);
                from = 0; // indices moved
            } else {
                from = b + close.len_utf8();
            }
        }
    }

    // Cut trailing rationale: notes, "because", "so that", "e.g.".
    // A marker inside a quotation is part of the quoted message, not
    // commentary about it - "Approvals are locked because the cycle has
    // ended" is the alert's own wording. Scan past quoted matches to the
    // first one in open text.
    'rationale: for marker in [" note:", " notes:", " because ", " so that ", " e.g.", " i.e."] {
        let mut from = 0;
        while let Some(rel) = find_ascii_ci(&s[from..], marker) {
            let at = from + rel;
            if !inside_quotes(&s, at) {
                s = s[..at].trim().to_string();
                break 'rationale;
            }
            from = at + marker.len();
        }
    }

    // One sentence: the outcome - unless a later one carries an assertion
    // the first does not, in which case it is kept too. `". "` is not
    // always a sentence end - an abbreviation carries one too, and "Approx.
    // 30 results are returned" was being cut down to the single word
    // "Approx".
    let mut kept_assertion = false;
    if let Some(i) = sentence_break(&s) {
        let mut kept = s[..i].to_string();
        let mut rest = s[i + 2..].trim().to_string();
        while !rest.is_empty() {
            let (sentence, after) = match sentence_break(&rest) {
                Some(j) => (rest[..j].to_string(), rest[j + 2..].trim().to_string()),
                None => (rest.trim_end_matches('.').to_string(), String::new()),
            };
            if !carries_assertion(&kept, &sentence) {
                break;
            }
            kept = format!("{kept}. {sentence}");
            kept_assertion = true;
            rest = after;
        }
        s = kept;
    }

    // Strip the openers that restate the act of testing.
    //
    // Bounded, and checked. Looping without either meant the prefixes
    // CHAINED: "Ensure Check Number is displayed" lost "ensure " and then
    // "check " - because the subject's first word happened to be one of
    // the verbs - and came out as "Number is displayed." Banking fields
    // ("Check Number", "Check Date") and UI labels ("Confirm button") hit
    // that, and the result reaches a real Azure DevOps test case where the
    // tester can no longer tell which number to look at.
    let mut changed = true;
    let mut bare_verb_used = false;
    'stripping: while changed {
        changed = false;
        for noise in EXPECTED_NOISE {
            if !starts_with_ascii_ci(&s, noise) {
                continue;
            }
            // A BARE verb fires at most once. "Ensure Check Number is
            // displayed" is a sentence whose subject happens to begin with
            // one of these words; after "ensure " has gone, a second match
            // is the content, not another opener. The multi-word forms
            // ("verify that ", "the system should ") still chain, which is
            // what turns "Verify that the system should display an error"
            // into "Display an error."
            let bare = is_bare_verb(noise);
            if bare && bare_verb_used {
                continue;
            }
            let rest = s[noise.len()..].trim();
            // And a strip that ate the subject is not a strip at all: what
            // is left has to still name the thing the tester looks at.
            //
            // Rejecting a prefix STOPS the stripping - it does not fall
            // through to the next candidate. The list holds overlapping
            // prefixes, longest first ("verify that " before "verify "), so
            // skipping on to the shorter one strips the verb and leaves the
            // connective behind: "Verify that is shown" was refused on
            // "verify that ", matched "verify ", and came out as
            // "That is shown."
            if rest.is_empty() || starts_with_a_bare_copula(rest) {
                break 'stripping;
            }
            s = rest.to_string();
            bare_verb_used |= bare;
            changed = true;
            break;
        }
    }

    // `It reads "X".` ends quote-then-stop: that stop belongs to the OUTER
    // sentence (the quote carries none of its own) and is well-formed -
    // remember it BEFORE the tail-normalisation below strips it, or the
    // commonest expected-result shape in real sets comes back
    // unterminated (round 7 §6, the mirror of round 6 §2.1).
    let stopped_after_quote = {
        let tail = s.trim_end().trim_end_matches('.');
        tail.len() < s.trim_end().len()
            && tail.ends_with(['"', '\u{201d}', '\'', '\u{2019}'])
    };
    // A cut that landed just after a clause leaves its comma behind, and
    // "received,." is not a sentence - drop the severed connective before
    // the final full stop goes on.
    let s = s
        .trim_end_matches(['.', ' '])
        .trim_end_matches([',', ';', ':', ' '])
        .trim()
        .to_string();
    if s.is_empty() {
        return (squash(raw), false); // trimming ate everything - keep the original
    }
    // A closing quote PRECEDED by its own stop (`."`) is terminal
    // punctuation: appending another manufactures the `".` malformed tail
    // (round 6 §2.1 - the raw-last-character test saw `"` and thought the
    // sentence unfinished). But a quote the author already followed with a
    // stop (`".`) gets that stop back - the guard is against punctuation
    // this trimmer introduces, not against the author's own.
    let out = if s.ends_with(['"', '\u{201d}', '\'', '\u{2019}']) && !stopped_after_quote {
        sentence_case(&s)
    } else {
        format!("{}.", sentence_case(&s))
    };
    // Belt over the braces above: if anything still managed to sever a
    // quotation, the trim was wrong by construction - losing a tidy-up is
    // cheaper than shipping a case that asserts half a message.
    if out.matches('"').count() % 2 == 1 {
        return (squash(raw), false);
    }
    (out, kept_assertion)
}

/// Does the trimmed text still name something the title names?
///
/// Round 8 §12.2: "They differ." and "No rows are returned." retain nothing
/// a tester could fail. The trimmer keeps the first sentence, and when that
/// sentence is a pronoun and a verb the case is left asserting nothing at
/// all - the subject went with the sentence that was cut.
///
/// Deliberately generous: any shared word of four characters or more that
/// is not a common connective. The question is whether the trim left a
/// stub, not whether the two read alike, and keeping an untidy sentence
/// costs far less than shipping a case that tests nothing.
fn shares_a_subject(kept: &str, title: &str) -> bool {
    const NOISE: &[&str] = &[
        "that", "this", "with", "from", "when", "then", "than", "they", "them", "their", "there",
        "which", "while", "shown", "reads", "each", "into", "over", "only", "also", "same", "been",
        "does", "will", "must", "have", "has", "and", "the", "for", "are", "not",
    ];
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| !c.is_alphanumeric() && c != '_')
            .map(|w| w.trim().to_ascii_lowercase())
            .filter(|w| w.chars().count() >= 4 && !NOISE.contains(&w.as_str()))
            .collect()
    };
    let title_words = words(title);
    // A title with no substantial word of its own cannot judge anything -
    // say yes rather than refuse every trim on the set.
    if title_words.is_empty() {
        return true;
    }
    words(kept).iter().any(|w| title_words.contains(w))
}

/// Does a later sentence still TEST something, or only explain?
///
/// Round 8 §8/§12: on two independent drafts, nine trims in ten removed the
/// sentence carrying the assertion - house style puts the observation first
/// and the discriminating detail second, so "keep the first sentence" kept
/// the half that does no testing. A sentence is an assertion when it
/// negates (the "and nothing else" half of a check) or names something the
/// kept text does not - a placeholder, a parameter, an identifier, a quoted
/// value, a number, a proper noun, an ordering. Explanations do none of
/// those.
fn carries_assertion(kept: &str, sentence: &str) -> bool {
    let lowered = sentence.to_lowercase();
    let negation = Regex::new(r"(?i)\b(no|not|never|neither|none|nothing)\b|n't\b").unwrap();
    if negation.is_match(&lowered) {
        return true;
    }
    let ordering = Regex::new(r"(?i)\b(first|last|before|after|top|bottom|ascending|descending|order|only)\b").unwrap();
    if ordering.is_match(&lowered) {
        return true;
    }
    let kept_lower = kept.to_lowercase();
    let mut first_word = true;
    for raw in sentence.split_whitespace() {
        let tok = raw.trim_matches(|c: char| ",.;:()".contains(c));
        if tok.is_empty() {
            continue;
        }
        let names = tok.starts_with('{')
            || tok.starts_with('@')
            || tok.contains('_')
            || tok.chars().any(|c| c.is_ascii_digit())
            || tok.starts_with(['\'', '"', '\u{2018}', '\u{201c}'])
            || (!first_word && tok.chars().next().is_some_and(|c| c.is_uppercase()));
        first_word = false;
        if names && !kept_lower.contains(&tok.to_lowercase()) {
            return true;
        }
    }
    false
}

/// Everything in preconditions that is NOT navigation - the state a
/// tester has to arrange. This is what makes two cases share a setup.
fn setup_conditions(preconditions: &str) -> Vec<String> {
    sentences(preconditions)
        .into_iter()
        .filter(|s| !is_navigation(s))
        .collect()
}

/// A stable key for "the tester would have to change something between
/// these two cases": the module plus the non-navigation preconditions.
fn setup_key(c: &TestCase) -> String {
    let mut parts: Vec<String> = setup_conditions(&c.preconditions)
        .iter()
        .map(|s| s.to_lowercase())
        .collect();
    parts.sort();
    parts.dedup();
    let module = c.module_value.trim().to_lowercase();
    if module.is_empty() {
        parts.join(" | ")
    } else {
        format!("{module} | {}", parts.join(" | "))
    }
}

fn setup_label(c: &TestCase) -> String {
    let conditions = setup_conditions(&c.preconditions);
    let module = c.module_value.trim();
    match (module.is_empty(), conditions.is_empty()) {
        (true, true) => "No special setup".to_string(),
        (false, true) => module.to_string(),
        (true, false) => conditions.join(" · "),
        (false, false) => format!("{module} · {}", conditions.join(" · ")),
    }
}

fn tokens(key: &str) -> std::collections::BTreeSet<String> {
    key.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 2)
        .map(|t| t.to_lowercase())
        .collect()
}

/// How much two setups have in common (0..=1). Used to chain groups so
/// each remaining switch changes as little as possible.
fn similarity(a: &str, b: &str) -> f64 {
    let (ta, tb) = (tokens(a), tokens(b));
    if ta.is_empty() && tb.is_empty() {
        return 1.0;
    }
    let shared = ta.intersection(&tb).count() as f64;
    let total = ta.union(&tb).count() as f64;
    if total == 0.0 {
        0.0
    } else {
        shared / total
    }
}

fn count_switches(cases: &[TestCase]) -> usize {
    cases
        .windows(2)
        .filter(|w| setup_key(&w[0]) != setup_key(&w[1]))
        .count()
}

/// True when `entry` describes opening the application itself (the
/// default does). A caller may instead pass a post-sign-in navigation
/// ("In the PMS Module, open Performance Management...") - that must come
/// AFTER signing in, not before it.
fn entry_is_launch(entry: &str) -> bool {
    let l = entry.to_lowercase();
    l.contains("launch")
        || l.contains("open the app")
        || l.contains("start the app")
        || l.contains("browser")
}

/// Step-action equality that shrugs off whitespace, case and the final
/// full stop - "closely matches" for the idempotency check.
fn norm_step(s: &str) -> String {
    squash(s).to_lowercase().trim_end_matches('.').to_string()
}

/// The preamble a tester needs to reach the page under test, plus the
/// precondition sentences it CONSUMED. The two travel together on
/// purpose: a sentence may only leave preconditions if it is in the
/// returned steps - the move is atomic or it does not happen.
///
/// The sign-in sentence is NOT consumed: "Signed in as a manager" is
/// both an action (it earns a step) and the state that defines the
/// case setup group, so it stays in preconditions too.
fn preamble_steps(c: &TestCase, entry: &str) -> (Vec<Step>, Vec<String>) {
    let all = sentences(&c.preconditions);
    let nav: Vec<&String> = all.iter().filter(|s| is_navigation(s)).collect();
    let mut consumed: Vec<String> = vec![];

    // Signing in is scanned across ALL preconditions, not just the ones
    // classed as navigation.
    let signin = all.iter().find(|s| {
        let l = s.to_lowercase();
        ["log in", "logged in", "sign in", "signed in", "authenticated"]
            .iter()
            .any(|m| l.contains(m))
    });
    let signin_step = signin.map(|sentence| {
        // find_ascii_ci, not `to_lowercase().find(..)` - the offsets from a
        // lowercased copy do not address the original, so a sharp S or a
        // dotted capital I earlier in the sentence sliced the role off at
        // the wrong byte, or panicked.
        let role = find_ascii_ci(sentence, " as ")
            .map(|i| squash(&sentence[i + 4..]))
            .filter(|r| !r.is_empty());
        Step {
            action: match &role {
                Some(r) => format!("Sign in as {r}."),
                None => "Sign in.".to_string(),
            },
            expected: "The home page is displayed.".to_string(),
        }
    });

    // Sign in belongs immediately after the application opens. When the
    // caller entry is itself a navigation rather than a launch, the
    // order is sign in -> entry - never module-open before sign-in.
    let mut out: Vec<Step> = vec![];
    if entry_is_launch(entry) {
        out.push(Step {
            action: entry.to_string(),
            expected: "The application opens.".to_string(),
        });
        out.extend(signin_step);
    } else {
        out.extend(signin_step);
        out.push(Step {
            action: entry.to_string(),
            expected: "The module opens.".to_string(),
        });
    }

    // Each navigation sentence becomes its own step, phrased as an
    // instruction rather than a state. The target keeps its original
    // casing - product names matter.
    const PREFIXES: &[&str] = &[
        "user has navigated to",
        "user is on",
        "navigated to",
        "navigate to",
        "opened the",
        "open the",
        "from the",
        "go to",
        "on the",
        "at the",
    ];
    for sentence in &nav {
        let l = sentence.to_lowercase();
        let target = PREFIXES
            .iter()
            .find(|m| l.starts_with(**m))
            .map(|m| squash(&sentence[m.len()..]))
            .unwrap_or_else(|| squash(sentence));
        let target = target.trim_start_matches("the ").trim().to_string();
        if target.is_empty() {
            continue;
        }
        out.push(Step {
            action: format!("Navigate to the {target}."),
            expected: format!("The {target} is displayed."),
        });
        consumed.push((*sentence).clone());
    }

    if nav.is_empty() && signin.is_none() && !c.module_value.trim().is_empty() {
        // Nothing was described at all - at least name the module, so the
        // tester is not left standing at the front door.
        out.push(Step {
            action: format!("Navigate to {}.", c.module_value.trim()),
            expected: format!("The {} page is displayed.", c.module_value.trim()),
        });
    }
    (out, consumed)
}

fn already_has_preamble(c: &TestCase, entry: &str) -> bool {
    c.steps.iter().take(PREAMBLE_PROBE).any(|s| {
        // A step that OPENS with the entry phrase counts, not just one
        // that equals it: "In the PMS Module, open Performance Management
        // and confirm..." already walks in from the entry, and prepending
        // the entry again added ~500 redundant steps across one 267-case
        // set (round 6 §2.2).
        if norm_step(&s.action).starts_with(&norm_step(entry)) {
            return true;
        }
        // "As the manager, open the Review step" is the same walk-in with
        // a role prefix - a very common test-writing idiom, and skipping
        // past it re-added 18 preamble steps to an 11-case set (round 8
        // dogfooding). The clause is dropped before the prefix test.
        let l = s.action.to_lowercase();
        let l = match l.starts_with("as the ") || l.starts_with("as a ") || l.starts_with("as an ")
        {
            true => l.split_once(',').map(|(_, rest)| rest.trim_start().to_string()).unwrap_or(l),
            false => l,
        };
        if ["launch", "start the app", "log in", "sign in", "navigate to", "go to "]
            .iter()
            .any(|m| l.starts_with(m))
        {
            return true;
        }
        // "Open the Review step" is a walk-in; "Open the payment detail"
        // is a mid-flow action. Both start with "open ", so the verb alone
        // cannot decide (round 8: the blanket reading deleted legitimate
        // preambles). It counts only when the step names a navigation
        // CONTAINER - a place the tester goes, not a thing they act on.
        l.starts_with("open ")
            && l.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| {
                matches!(
                    w,
                    "app" | "application"
                        | "portal"
                        | "site"
                        | "browser"
                        | "url"
                        | "page"
                        | "screen"
                        | "tab"
                        | "module"
                        | "menu"
                        | "step"
                        | "dashboard"
                )
            })
    })
}

fn normalize_tags(raw: &str) -> String {
    let mut seen: Vec<String> = vec![];
    for t in raw.split([';', ',']) {
        let t = t.trim();
        if t.is_empty() || seen.iter().any(|s| s.eq_ignore_ascii_case(t)) {
            continue;
        }
        seen.push(t.to_string());
    }
    seen.join("; ")
}

/// Reorganise a draft. `entry` is the first step of every preamble.
/// Reorganise a draft, regrouping it for the tester.
pub fn optimize(cases: Vec<TestCase>, entry: Option<&str>) -> (Vec<TestCase>, OptimizeReport) {
    optimize_with(cases, entry, true)
}

/// As `optimize`, but `reorder` decides whether the cases are regrouped.
///
/// A set written to be read against a specification is in document order on
/// purpose, and regrouping it by setup destroys the one property that made
/// it reviewable - the reader can no longer walk the spec and the file side
/// by side. Which of the two a set is for is asked at intake (`ordering`),
/// because it is the developer's call and nothing in the cases reveals it.
///
/// Everything else still runs either way: navigation is still spelled out,
/// expected results are still trimmed, duplicate titles are still collapsed.
/// Only the final regrouping is skipped.
pub fn optimize_with(
    cases: Vec<TestCase>,
    entry: Option<&str>,
    reorder: bool,
) -> (Vec<TestCase>, OptimizeReport) {
    optimize_full(cases, entry, reorder, true)
}

/// As `optimize_with`, but `trim_expected` decides whether expected results
/// are shortened at all.
///
/// Round 8 §14: on a set whose value is arithmetic - one bespoke
/// configuration per case - there is nothing to regroup, so the reordering
/// half saves zero environment switches while the trimming half rewrites
/// every expected result. Welded together those two made the whole tool
/// unusable there, and the reported answer was to skip it entirely and lose
/// the navigation and ordering work as well. Pass false to keep those and
/// leave the assertions exactly as written.
pub fn optimize_full(
    cases: Vec<TestCase>,
    entry: Option<&str>,
    reorder: bool,
    trim_expected: bool,
) -> (Vec<TestCase>, OptimizeReport) {
    let entry = entry.map(str::trim).filter(|e| !e.is_empty()).unwrap_or(DEFAULT_ENTRY);
    let mut report = OptimizeReport {
        switches_before: count_switches(&cases),
        ..Default::default()
    };

    // 1. Collapse duplicate titles - but never at the cost of a work item.
    //
    // Two DIFFERENT test cases in Azure DevOps are allowed to share a title,
    // and dropping one here would quietly delete an update the caller asked
    // for: the id goes with it, so the survivor creates a new case and the
    // real one is never touched. Nobody asked for a dedupe either - it runs
    // on every optimize call. So a title clash is only collapsed when it
    // cannot cost anything: both sides id-less, or both the same work item.
    // Otherwise both are kept and the clash is reported for a human to
    // settle.
    let mut deduped: Vec<TestCase> = vec![];
    for c in cases {
        let key = squash(&c.title).to_lowercase();
        let clash = deduped
            .iter()
            .position(|k| squash(&k.title).to_lowercase() == key);
        match clash.map(|i| (i, deduped[i].update_id, c.update_id)) {
            None => deduped.push(c),
            // Same work item, or neither is one: a genuine duplicate.
            Some((_, a, b)) if a == b => {
                report.duplicates_removed += 1;
                report.notes.push(format!("Removed a duplicate of '{}'.", squash(&c.title)));
            }
            Some((_, a, b)) => {
                let which = |id: Option<i32>| match id {
                    Some(v) => format!("#{v}"),
                    None => "a new case".to_string(),
                };
                report.notes.push(format!(
                    "Two cases share the title '{}' - {} and {} - so both were kept.                      Rename one if that was accidental.",
                    squash(&c.title),
                    which(a),
                    which(b)
                ));
                deduped.push(c);
            }
        }
    }

    // 2. Per-case cleanup: preamble in, noise out.
    let mut cleaned: Vec<TestCase> = vec![];
    for mut c in deduped {
        c.title = squash(&c.title);
        c.tags = normalize_tags(&c.tags);
        if c.automation_status.trim() != "Planned" {
            c.automation_status = "Not Automated".to_string();
        }

        let before = c.steps.len();
        c.steps.retain(|s| !s.action.trim().is_empty() || !s.expected.trim().is_empty());
        report.empty_steps_removed += before - c.steps.len();

        for (i, s) in c.steps.iter_mut().enumerate() {
            s.action = squash(&s.action);
            let original = squash(&s.expected);
            // Round 8 §14: with trimming off the expected result is left
            // exactly as written - only the whitespace is normalised, which
            // every other field gets too.
            if !trim_expected {
                s.expected = original;
                continue;
            }
            let (mut cleaned_expected, kept) = clean_expected_keeping(&s.expected);
            // Round 8 §12.2: a trim that leaves nothing the title names has
            // removed the subject along with the gloss. "They differ." is
            // not something a tester can fail. The assertion rule above
            // saves the sentences that name a value; this catches the ones
            // that leave a contentless stub behind.
            // Only when a SENTENCE actually went. Stripping a "Verify that"
            // lead-in rewords one sentence and loses no assertion, and
            // judging that by the title would refuse tidying on any case
            // whose title happens to share no word with its own steps.
            if sentences(&cleaned_expected).len() < sentences(&original).len()
                && !shares_a_subject(&cleaned_expected, &c.title)
            {
                cleaned_expected = original.clone();
            }
            if kept {
                report.assertions_kept += 1;
            }
            if cleaned_expected != original {
                report.expected_trimmed += 1;
                // A count on its own could not tell "added a full stop"
                // from "deleted the second assertion" - and this function
                // does delete: an expected of "The status changes to
                // Shipped. A confirmation email is sent." keeps only the
                // first, so nobody is ever asked to check the email. The
                // preconditions record next to this one names every change
                // with its before and after; so does this one now, and a
                // dry run shows it before anything is imported.
                if material_loss(&original, &cleaned_expected) {
                    report.expected_rewritten.push(ExpectedChange {
                        title: c.title.clone(),
                        step_number: i + 1,
                        before: original,
                        after: cleaned_expected.clone(),
                    });
                }
            }
            s.expected = cleaned_expected;
        }

        // The move is ATOMIC: a precondition sentence leaves preconditions
        // only in the same pass that adds it as a step. When no preamble
        // is added - the draft already walks in, or its first step IS the
        // entry - preconditions are left completely untouched. The earlier
        // version stripped unconditionally, which silently destroyed
        // sign-in context that was never re-emitted.
        if already_has_preamble(&c, entry) {
            // Skipping is right - the draft already walks in, and adding a
            // second preamble is the duplication this check exists to stop.
            // But silence made a supplied `entry` look like it had been
            // ignored: preamble_steps_added: 0, no entry step, no reason
            // given. Say which case declined it and why.
            if !entry.trim().is_empty() {
                report.notes.push(format!(
                    "'{}': entry step omitted - the case already opens with navigation or sign-in.",
                    c.title
                ));
            }
        } else {
            let (pre, consumed) = preamble_steps(&c, entry);
            // Subtractive, as well as the probe above - but only against
            // the case's OPENING steps, not all of them.
            //
            // Scanning the whole case broke the atomic move that the
            // comment below promises. "Refund a payment" ends with
            // "Navigate to the Payments page." at step 6 to check the
            // result; that matched the generated nav step, so the step was
            // dropped - while the precondition sentence it had consumed was
            // still removed. The case lost both: no step telling the tester
            // to go there, and no precondition saying they should be. A
            // mid-case navigation is a real step, not a duplicate preamble.
            let existing: Vec<String> = c
                .steps
                .iter()
                .take(PREAMBLE_PROBE)
                .map(|s| norm_step(&s.action))
                .collect();
            let pre: Vec<Step> = pre
                .into_iter()
                .filter(|p| !existing.contains(&norm_step(&p.action)))
                .collect();
            report.preamble_steps_added += pre.len();
            let mut steps = pre;
            steps.append(&mut c.steps);
            c.steps = steps;

            // Rewrite the preconditions ONLY when a sentence was actually
            // consumed into the preamble. The join below reformats ". "
            // into "; " and drops the closing full stop, so comparing
            // rewritten-vs-original always differed and the report fired
            // on every case - which made an empty preconditions_rewritten
            // useless as the signal that nothing real happened (round 4;
            // round 2 had used exactly that emptiness as evidence).
            let kept: Vec<String> = sentences(&c.preconditions)
                .into_iter()
                .filter(|s| !consumed.contains(s))
                .collect();
            let removed_any = kept.len() != sentences(&c.preconditions).len();
            if removed_any {
                let before_text = squash(&c.preconditions);
                let after_text =
                    kept.iter().map(|s| sentence_case(s)).collect::<Vec<_>>().join("; ");
                report.preconditions_rewritten.push(PreconditionChange {
                    title: c.title.clone(),
                    before: before_text,
                    after: after_text.clone(),
                });
                c.preconditions = after_text;
            }
        }

        if c.steps.is_empty() {
            report.notes.push(format!("'{}' has no steps - it needs one.", c.title));
        }
        cleaned.push(c);
    }

    // 3. BOTH orders are recorded on every case, whatever the caller asked
    //    the array to be: spec_order is the order the draft was written in
    //    (the reading that walks down the specification), tester_order is
    //    the grouped sequence computed below. `reorder` only decides which
    //    of the two the returned ARRAY follows - it no longer costs the
    //    other one, which used to be simply destroyed.
    for (i, c) in cleaned.iter_mut().enumerate() {
        c.spec_order = Some(i as u32 + 1);
    }
    let (sequence, groups) = tester_sequence(&cleaned);
    for (rank, &i) in sequence.iter().enumerate() {
        cleaned[i].tester_order = Some(rank as u32 + 1);
    }

    if !reorder {
        report.switches_after = count_switches(&cleaned);
        note_the_trade(&mut report);
        return (cleaned, report);
    }
    report.groups = groups;
    let mut slots: Vec<Option<TestCase>> = cleaned.into_iter().map(Some).collect();
    let ordered: Vec<TestCase> = sequence.iter().map(|&i| slots[i].take().expect("sequence is a permutation")).collect();

    report.switches_after = count_switches(&ordered);
    note_the_trade(&mut report);
    (ordered, report)
}

/// Say what the run cost when it bought nothing.
///
/// Round 8 §14: on a set with one bespoke configuration per case there is
/// nothing to group, so reordering saves zero switches and the only effect
/// left is N rewritten expected results. `expected_trimmed: 59` reads as
/// tidy-up; the same number next to "saved 0 environment switches" is a
/// one-line decision instead of a 59-item diff review. It goes FIRST
/// because a note at the bottom of a long report is a note nobody read.
fn note_the_trade(report: &mut OptimizeReport) {
    if report.expected_trimmed == 0 || report.switches_after < report.switches_before {
        return;
    }
    report.notes.insert(
        0,
        format!(
            "This run rewrote {} expected result(s) and saved 0 environment switches - \
             reordering found nothing to group. If the set's value is in what each \
             expected result asserts rather than in which screen it happens on, run it \
             again with trim_expected: false to keep the navigation and ordering work \
             and leave the assertions alone.",
            report.expected_trimmed
        ),
    );
}

/// The tester's sequence over `cases`, as indices: grouped by setup, groups
/// chained nearest-neighbour, biggest group first. Pure - the caller
/// decides whether to reorder the array by it or only to record it.
fn tester_sequence(cases: &[TestCase]) -> (Vec<usize>, Vec<GroupSummary>) {
    let mut groups: Vec<(String, Vec<usize>)> = vec![];
    for (i, c) in cases.iter().enumerate() {
        let key = setup_key(c);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(i),
            None => groups.push((key, vec![i])),
        }
    }
    // Start with the biggest group: the longest uninterrupted run.
    groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));

    let mut sequence: Vec<usize> = vec![];
    let mut summaries: Vec<GroupSummary> = vec![];
    if !groups.is_empty() {
        let mut remaining = groups;
        let mut current = remaining.remove(0);
        loop {
            summaries.push(GroupSummary {
                setup: current.1.first().map(|&i| setup_label(&cases[i])).unwrap_or_default(),
                cases: current.1.len(),
            });
            sequence.extend(current.1);
            if remaining.is_empty() {
                break;
            }
            // Closest setup wins; ties break on the key so runs are stable.
            let mut best = 0usize;
            let mut best_score = f64::NEG_INFINITY;
            for (i, (key, _)) in remaining.iter().enumerate() {
                let score = similarity(&current.0, key);
                if score > best_score {
                    best_score = score;
                    best = i;
                }
            }
            current = remaining.remove(best);
        }
    }
    (sequence, summaries)
}
