//! The person's own writing style for the AI writing guide: Markdown they
//! edit on the AI Bridge tab, which the guide carries in place of its
//! standard granularity and edge-case sections while it is switched on.
//!
//! When the team's testing policy changes, the person updates it here
//! instead of waiting for a release. One small JSON file in the app data
//! dir, beside `app-settings.json`, and like that one not `crate::cache`
//! (wiped when a different account signs in): it belongs to the machine.
//!
//! The guide reads it from disk on every call - no copy in memory - so a
//! save takes effect the next time an assistant reads the guide.
//!
//! The text is not a secret, but it is the person's own words: logs name
//! only that it changed and its size, never the text.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

pub const FILE: &str = "writing-style.json";

/// The largest text a save accepts, in UTF-8 bytes.
pub const MAX_BYTES: usize = 65_536;

pub const TOO_LARGE: &str = "the writing style is larger than 64 KB";
pub const EMPTY: &str = "the writing style is empty";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct WritingStyle {
    /// The guide carries `text` instead of its standard sections.
    pub enabled: bool,
    /// Markdown, as the person wrote it.
    pub text: String,
}

impl WritingStyle {
    /// What a machine starts with: the converted trial rules, switched off.
    pub fn starting() -> Self {
        Self { enabled: false, text: DEFAULT_RISK_TIERED.to_string() }
    }
}

/// Why `style` cannot be saved, as the sentence the person reads.
pub fn refusal(style: &WritingStyle) -> Option<&'static str> {
    if style.text.len() > MAX_BYTES {
        return Some(TOO_LARGE);
    }
    if style.enabled && style.text.trim().is_empty() {
        return Some(EMPTY);
    }
    None
}

/// What `dir` holds, or `None` when the file is missing, unreadable or the
/// wrong shape.
pub fn load(dir: &Path) -> Option<WritingStyle> {
    let raw = std::fs::read_to_string(dir.join(FILE)).ok()?;
    match serde_json::from_str::<WritingStyle>(&raw) {
        Ok(s) => Some(s),
        Err(e) => {
            crate::applog::warn(format!("the saved writing style could not be read, so it is not used: {e}"));
            None
        }
    }
}

/// Write `style` into `dir` (created if missing), atomically, after the
/// same checks a save from the screen gets.
pub fn save(dir: &Path, style: &WritingStyle) -> Result<(), String> {
    if let Some(why) = refusal(style) {
        return Err(why.to_string());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
    let body = serde_json::to_string(style).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&dir.join(FILE), &body)?;
    crate::applog::info(format!(
        "writing style saved: {}, {} bytes",
        if style.enabled { "on" } else { "off" },
        style.text.len()
    ));
    Ok(())
}

/// The style in `dir`, creating the starting one when there is no file
/// yet. A file that is there but unreadable is left alone (the person may
/// want it back) and reads as the starting style.
pub fn get_or_create(dir: &Path) -> WritingStyle {
    if let Some(s) = load(dir) {
        return s;
    }
    let starting = WritingStyle::starting();
    if !dir.join(FILE).exists() {
        if let Err(e) = save(dir, &starting) {
            crate::applog::warn(format!("creating the starting writing style failed: {e}"));
        }
    }
    starting
}

/// The text the guide carries, when the style is switched on and has any.
/// `None` (the standard sections) when it is off, missing or empty.
pub fn active_text(dir: &Path) -> Option<String> {
    load(dir).filter(|s| s.enabled && !s.text.trim().is_empty()).map(|s| s.text)
}

/// A Markdown file's text for the editor (Upload .md): only `.md` and
/// `.markdown`, no larger than a save accepts. Errors name no path.
pub fn read_markdown(path: &Path) -> Result<String, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
    if !matches!(ext.as_deref(), Some("md") | Some("markdown")) {
        return Err("choose a .md or .markdown file".to_string());
    }
    let bytes = std::fs::read(path).map_err(|e| {
        crate::applog::warn(format!("reading a writing style file failed: {e}"));
        "the file could not be read. Settings → Logs has the details.".to_string()
    })?;
    if bytes.len() > MAX_BYTES {
        return Err(TOO_LARGE.to_string());
    }
    let text = String::from_utf8(bytes).map_err(|_| "the file is not UTF-8 text".to_string())?;
    // A byte order mark belongs to the editor that wrote the file, not the text.
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_string())
}

static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Called once from setup with the app data dir: the starting style is
/// written there if this machine has none yet.
pub fn init(dir: PathBuf) {
    get_or_create(&dir);
    set_dir(Some(dir));
}

/// Where the style lives, or `None` for no style at all (the `--mcp` proxy
/// never runs setup). Tests point it at a temp dir and back.
pub fn set_dir(dir: Option<PathBuf>) {
    *DIR.lock().unwrap_or_else(|e| e.into_inner()) = dir;
}

pub fn dir() -> Option<PathBuf> {
    DIR.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// What the guide carries right now, read from disk.
pub fn current_text() -> Option<String> {
    dir().and_then(|d| active_text(&d))
}

/// The first custom style: the AI Bridge tab's old risk-tiered trial rules,
/// as one Markdown document. Wording kept; the trial's two workflow steps
/// are sections of their own.
pub const DEFAULT_RISK_TIERED: &str = r#"## Risk-tiered design (trial rules)
These rules replace the plain granularity and edge-case guidance while
the developer trials them. They cut the number of cases without
cutting coverage: every case earns its place against an acceptance
criterion or a named risk. If a request conflicts with these rules,
follow the rules and say which rule the request conflicts with.

### Tier every scenario
- T1 - Critical: financial, legal or statutory, data isolation between
companies, or security. Payroll calculations, tax, EPF/ETF, access
control.
- T2 - Core: a core business workflow whose failure is visible but
recoverable. Leave, attendance, onboarding, integrations.
- T3 - Low: cosmetic or configuration, with a small blast radius.
Labels, report layout, settings screens.

Tier each SCENARIO, not the whole story. A check that shows or depends
on a T1 behaviour - the label that displays a calculated figure, the
report column that carries it - is RELATED: it belongs in the T1 case
and is T1. A change that stands on its own - an unrelated label renamed
in the same story - is UNRELATED: its own case, at its own tier.

### Design techniques
- Equivalence partitioning: one case per partition, not per value.
- Boundary values: the minimum, the maximum and just outside - nothing
in between.
- Pairwise for three or more interacting inputs. Never every
combination unless the developer asks for it on a T1 scenario.
- T1 combinations: when three or more inputs feed one calculation or
rule, say so in the scenario list with the count - for example "tax
band x employee type x join date feed the EPF calculation: pairwise
covers 12 of 48 combinations - generate all 48?" - and let the
developer decide.
- Negative cases: one per distinct validation rule, not one per
invalid input.

### Budget per story
New scenarios at most: T1 25, T2 12, T3 5. When that is not enough,
STOP and list the extra scenarios with a one-line justification each,
instead of writing them.

### Consolidate
- Before adding a case, read the PBI's existing cases (`get_test_cases`)
and extend a matching one - keeping its `id` - rather than writing a
near-duplicate.
- Checks that differ only in their data are ONE case: one step per data
row, each with its own expected result. Different branches still stay
separate cases (see One branch per case).
- Similar checks on the same screen with the same setup belong in one
case. A padded case count is not coverage.
- Do not write a case that only checks what the browser or the
application's framework does on its own - a link opens, a field takes
typing, a page scrolls - unless the story changes that behaviour.

### Tags on every case
These three are required; a genuinely new tag is fine for them.
- A trace: the acceptance criterion or named risk the case covers, as a
tag like `AC-3` or `Risk-payroll-rounding`. Never write a case without
one.
- Its tier: exactly one of `T1`, `T2`, `T3`.
- Exactly one run category: `Smoke` (the critical path, runnable in a
few minutes with no special data), `Regression` (the default) or
`Extended` (slow, data-heavy, or across companies).

### Ready to automate
- Deterministic: name exact data and fixed dates in preconditions and
steps - never "today", "any employee" or an order the tester cannot
see.
- Isolated: a case sets up what it needs and never depends on another
case having run first.
- Data isolation: a case that reads or changes one company's data also
checks another company's data is not shown or touched (T1).

## Edge cases, the tiered way
Choose edge cases with the techniques above, not by habit - each one
still traced, tiered and inside the budget:

- Access: open the page's address without signing in, or as a role
that should not see it; the expected result is what the application
shows instead (the sign-in page, a permission message), named
exactly.
- Validation: one negative case per validation rule. A blank required
field and a value over the maximum length are two rules; if the
application trims input, blank and only spaces are one.
- Boundaries: the minimum, the maximum and just outside, for each field
the form bounds.
- State: the same action twice (double submit, refresh after saving,
back button after a save), where the flow allows it.
- Absence: the list with nothing in it, a search with no matches; the
expected result is the empty state's own words.

Do NOT write cases that need developer tools, a modified request,
a database change, a disconnected network, or a clock change: a
tester cannot run them from the application, and a case nobody can
run is worse than none. If a spec names such a behaviour, put it in
`reviewer_notes` as a note for the developers instead.

## Scenario list before drafting
After reading the PBI's existing cases and before drafting, write the
SCENARIO LIST in the conversation - one line each: the scenario, its
trace (`AC-n` or the named risk), its tier, RELATED or UNRELATED where
the story mixes tiers, and any T1 combination question. Wait for the
developer to approve it, unless they said the scenarios are
pre-approved. Then write cases ONLY from the approved list.

## Closing summary
At the end of the workflow, end with a summary: each approved scenario
and the acceptance criterion or risk it covers; cases added against
existing cases extended; and every scenario deferred over the budget,
with its justification.

## Regression suite review (trial rules)
When the developer asks you to review the test cases under a test plan
or a PBI and decide which belong in the Regression suite, follow this
protocol. The aim is FEWER Regression cases: keep only the
high-critical, high-risk and high-value ones.

1. Read every case: `get_suite_test_cases` for a plan or suite,
`get_test_cases` for a PBI.
2. Weigh each case on: business criticality; functional importance;
regression risk; impact on core functionality; and its value for
future regression testing. Apply the rules above alongside them:
- The tier sets how strict to be. T1 (payroll, statutory, data
isolation between companies, security) justifies keeping more; a T3
case (labels, report layout, settings) should rarely be Regression.
- Duplicates and near-duplicates (the same screen and the same
scenario) are REMOVE candidates, or should be merged into one case
with a step per data row.
- A case that only checks what the browser or framework does on its
own is a REMOVE candidate.
- A case with no acceptance criterion or risk tag is a weak candidate.
- A slow, data-heavy or cross-company case belongs in `Extended`, not
`Regression`.
3. Label every case with exactly one of: `KEEP_REGRESSION` (its
Regression tag is justified), `ADD_REGRESSION` (it has no Regression
tag but should), `REMOVE_REGRESSION` (its Regression tag is not
justified).
4. While reviewing, change NOTHING: add or remove no tag, edit or
delete no case, and write no file. Report a summary only - no
per-case listing beyond the id lists below.
5. Report exactly these, in this order:
1) Total test cases reviewed. 2) Current Regression count.
3) Regression tags to keep. 4) Regression tags to add.
5) Regression tags to remove. 6) Proposed final Regression count.
7) Test case ids to ADD. 8) Test case ids to REMOVE.
9) A brief reason for the ADD and REMOVE decisions.
Before reporting, check that Proposed final = Current - Remove + Add
and Keep = Current - Remove.
6. Then ask exactly:
"Would you like me to apply these Regression tag changes?"
Make no change until the developer says yes in so many words.
7. Only after that yes: read the approved cases with `get_test_cases`
and use `transform_cases` to `add_tags` `Regression` on the ADD ids
and `remove_tags` `Regression` on the REMOVE ids. Change nothing else:
every other field of each case - its title, steps, other tags and the
rest - stays exactly as it was, and each case keeps its `id`, so the
file updates those cases and no others. Tell the developer to import
the file through Import Test Cases, where they see every change
before anything reaches Azure DevOps. Then report the ids added and
removed and the final Regression count.
"#;
