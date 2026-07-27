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
//!    action, no rationale, no asides.
//!
//! Everything is a pure function over `Vec<TestCase>`: no I/O, no ADO, no
//! writes. The AI passes a draft in and gets a draft back.

use crate::model::TestCase;
use crate::steps_xml::Step;

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
    pub duplicates_removed: usize,
    pub empty_steps_removed: usize,
    /// Anything a human should look at rather than trust blindly.
    pub notes: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct GroupSummary {
    /// Human-readable setup ("Payments · logged in as Admin"), or
    /// "No special setup".
    pub setup: String,
    pub cases: usize,
}

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
    "on the",
    "at the",
    "from the",
    "launch",
    "log in",
    "logged in",
    "sign in",
    "signed in",
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
    NAV_MARKERS.iter().any(|m| l.starts_with(m))
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

/// Reduce an expected result to the observable outcome. Bails out and
/// keeps the original whenever trimming would leave nothing useful -
/// losing information is worse than an untidy sentence.
pub fn clean_expected(raw: &str) -> String {
    let mut s = squash(raw);
    if s.is_empty() {
        return s;
    }

    // Drop parenthetical and bracketed asides.
    for (open, close) in [('(', ')'), ('[', ']')] {
        while let (Some(a), Some(b)) = (s.find(open), s.find(close)) {
            if b < a {
                break;
            }
            s = format!("{}{}", &s[..a], &s[b + close.len_utf8()..]);
            s = squash(&s);
        }
    }

    // Cut trailing rationale: notes, "because", "so that", "e.g.".
    let lower = s.to_lowercase();
    for marker in [" note:", " notes:", " because ", " so that ", " e.g.", " i.e."] {
        if let Some(i) = lower.find(marker) {
            s = s[..i].trim().to_string();
            break;
        }
    }

    // One sentence: the outcome.
    if let Some(i) = s.find(". ") {
        s = s[..i].to_string();
    }

    // Strip the openers that restate the act of testing.
    let mut changed = true;
    while changed {
        changed = false;
        let lower = s.to_lowercase();
        for noise in EXPECTED_NOISE {
            if lower.starts_with(noise) {
                s = s[noise.len()..].trim().to_string();
                changed = true;
                break;
            }
        }
    }

    let s = s.trim_end_matches(['.', ' ']).trim().to_string();
    if s.is_empty() {
        return squash(raw); // trimming ate everything - keep the original
    }
    format!("{}.", sentence_case(&s))
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

/// The preamble a tester needs to reach the page under test: the entry
/// point, any sign-in, then each navigation sentence lifted out of
/// preconditions, then the module itself.
fn preamble_steps(c: &TestCase, entry: &str) -> Vec<Step> {
    let all = sentences(&c.preconditions);
    let nav: Vec<&String> = all.iter().filter(|s| is_navigation(s)).collect();

    let mut out: Vec<Step> = vec![Step {
        action: entry.to_string(),
        expected: "The application opens.".to_string(),
    }];

    // Signing in is scanned across ALL preconditions, not just the ones
    // classed as navigation: "User is logged in as Admin" is a state that
    // defines the setup group AND an action the tester has to perform, so
    // it stays in preconditions and also earns a step.
    let signin = all.iter().find(|s| {
        let l = s.to_lowercase();
        ["log in", "logged in", "sign in", "signed in", "authenticated"]
            .iter()
            .any(|m| l.contains(m))
    });
    if let Some(sentence) = signin {
        let role = sentence
            .to_lowercase()
            .find(" as ")
            .map(|i| squash(&sentence[i + 4..]))
            .filter(|r| !r.is_empty());
        out.push(Step {
            action: match &role {
                Some(r) => format!("Sign in as {r}."),
                None => "Sign in.".to_string(),
            },
            expected: "The home page is displayed.".to_string(),
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
        if signin.map(|s| s == *sentence).unwrap_or(false) {
            continue;
        }
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
    }

    if out.len() == 1 && !c.module_value.trim().is_empty() {
        // Nothing was described at all - at least name the module, so the
        // tester is not left standing at the front door.
        out.push(Step {
            action: format!("Navigate to {}.", c.module_value.trim()),
            expected: format!("The {} page is displayed.", c.module_value.trim()),
        });
    }
    out
}

/// True if the case's own first steps already walk in from the entry
/// point, in which case a preamble would just duplicate them.
fn already_has_preamble(c: &TestCase) -> bool {
    let Some(first) = c.steps.first() else {
        return false;
    };
    let l = first.action.to_lowercase();
    ["launch", "open the app", "start the app", "log in", "sign in", "navigate to"]
        .iter()
        .any(|m| l.starts_with(m))
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

/// The default entry step when the caller doesn't name one.
pub const DEFAULT_ENTRY: &str = "Launch the application.";

/// Reorganise a draft. `entry` is the first step of every preamble.
pub fn optimize(cases: Vec<TestCase>, entry: Option<&str>) -> (Vec<TestCase>, OptimizeReport) {
    let entry = entry.map(str::trim).filter(|e| !e.is_empty()).unwrap_or(DEFAULT_ENTRY);
    let mut report = OptimizeReport {
        switches_before: count_switches(&cases),
        ..Default::default()
    };

    // 1. Drop exact duplicate titles, keeping the first.
    let mut deduped: Vec<TestCase> = vec![];
    for c in cases {
        let key = squash(&c.title).to_lowercase();
        if deduped.iter().any(|k| squash(&k.title).to_lowercase() == key) {
            report.duplicates_removed += 1;
            report.notes.push(format!("Removed a duplicate of '{}'.", squash(&c.title)));
            continue;
        }
        deduped.push(c);
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

        for s in c.steps.iter_mut() {
            s.action = squash(&s.action);
            let cleaned_expected = clean_expected(&s.expected);
            if cleaned_expected != squash(&s.expected) {
                report.expected_trimmed += 1;
            }
            s.expected = cleaned_expected;
        }

        if !already_has_preamble(&c) {
            let pre = preamble_steps(&c, entry);
            report.preamble_steps_added += pre.len();
            let mut steps = pre;
            steps.append(&mut c.steps);
            c.steps = steps;
        }

        // Navigation now lives in the steps; preconditions keep only the
        // state the tester has to arrange.
        let conditions = setup_conditions(&c.preconditions);
        c.preconditions = conditions
            .iter()
            .map(|s| sentence_case(s))
            .collect::<Vec<_>>()
            .join("; ");

        if c.steps.is_empty() {
            report.notes.push(format!("'{}' has no steps - it needs one.", c.title));
        }
        cleaned.push(c);
    }

    // 3. Group by setup, then chain the groups nearest-neighbour.
    let mut groups: Vec<(String, Vec<TestCase>)> = vec![];
    for c in cleaned {
        let key = setup_key(&c);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(c),
            None => groups.push((key, vec![c])),
        }
    }
    // Start with the biggest group: the longest uninterrupted run.
    groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));

    let mut ordered: Vec<TestCase> = vec![];
    if !groups.is_empty() {
        let mut remaining = groups;
        let mut current = remaining.remove(0);
        loop {
            report.groups.push(GroupSummary {
                setup: current.1.first().map(setup_label).unwrap_or_default(),
                cases: current.1.len(),
            });
            ordered.extend(current.1);
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

    report.switches_after = count_switches(&ordered);
    (ordered, report)
}
