//! What TCM knows about a case that the repo cannot see, handed over in the
//! case's md section: the records and settings it relies on (Preconditions
//! rows), what it appears to write, publish or must not touch (the
//! `**From TCM:**` block), and the cleanup it relies on (Cleanup rows).
//!
//! Pure: everything comes from the resolved script and the shared drafts it
//! reads. What a step writes is inferred from the names of the controls it
//! clicks, so every such line says "appears to".

use super::export::SharedDraft;
use crate::autorun::marks::normalise;
use crate::autorun::nav::{ProtectedKind, ProtectedRecord};
use crate::autorun::plan::{kept_constraints, suggest, Marks};
use crate::autorun::quirks::day_of;
use crate::autorun::CaseScript;
use crate::browser::actions::Action;
use crate::browser::locator::{LocatorStep, Target};
use std::collections::BTreeMap;

/// What one case hands over. Each list holds plain text; the md writer
/// (`test_case::section`) escapes it and marks the rows " (from TCM)".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Handover {
    /// Preconditions rows, after the ADO ones.
    pub preconditions: Vec<String>,
    /// Cleanup / Postconditions rows.
    pub cleanup: Vec<String>,
    /// The bullet lines of the `**From TCM:**` block.
    pub from_tcm: Vec<String>,
    /// The repo's common components the case uses: folder names under
    /// `suites/common-components/`, one `**Uses:**` line each
    /// (`uses_for`). Regenerated on every export.
    pub uses: Vec<String>,
    /// What the script alone shows is risky (`script_warnings`): the export
    /// dialog lists these before anything is written.
    pub warnings: Vec<String>,
}

pub struct HandoverInput<'a> {
    /// The script as the spec is written from it (`export::resolved_script`).
    pub script: &'a CaseScript,
    pub shared_drafts: &'a [SharedDraft],
    /// The project's protected baseline, which the repo keeps in
    /// `docs/tcm-protected-baseline.md` (`baseline_doc`).
    pub protected: &'a [ProtectedRecord],
    /// The sentence that blocks the case's runs while a divergence waits for
    /// a person (`divergences::needs_ruling`): the spec is `test.fixme`.
    pub needs_ruling: Option<String>,
}

/// Where the protected baseline lives in the Playwright repo.
pub const BASELINE_DOC_REL: &str = "docs/tcm-protected-baseline.md";

/// The repo's copy of the protected baseline: TCM-owned, rewritten whole on
/// every export. One section per kind present, in the kinds' own order,
/// each record a bullet. `None` for an empty baseline.
pub fn baseline_doc(protected: &[ProtectedRecord]) -> Option<String> {
    if protected.is_empty() {
        return None;
    }
    let mut out = String::from(
        "# Protected baseline (from TCM)\n\nTests must never edit or delete these records. \
         TCM rewrites this file on every export.\n",
    );
    let mut kinds: Vec<ProtectedKind> = protected.iter().map(|r| r.kind).collect();
    kinds.sort();
    kinds.dedup();
    for k in kinds {
        out.push_str(&format!("\n## {}\n\n", k.label()));
        for r in protected.iter().filter(|r| r.kind == k) {
            out.push_str(&format!("- {}\n", r.name));
        }
    }
    Some(out)
}

/// What a click appears to do, by the name of the control it clicks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    Delete,
    Save,
    Publish,
}

/// One control a step clicks that appears to change data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepEffect {
    pub step: i32,
    pub kind: EffectKind,
    /// The control's name as the script has it.
    pub control: String,
}

/// The words a control's name is read by, ignoring case. One name with
/// words of two kinds reads as the more harmful one: "Save & Publish"
/// publishes, "Save and delete" deletes.
const PUBLISH_WORDS: &[&str] = &["publish"];
const DELETE_WORDS: &[&str] = &["delete", "remove"];
const SAVE_WORDS: &[&str] = &["save", "submit", "create", "add", "update"];

/// A typed value shorter than this is never taken for the record a later
/// delete names: "1" or "QA" turns up in almost any row.
const MIN_CREATED_VALUE: usize = 3;

/// What a locator link is called: its accessible name, else its text. A
/// css link (and a legacy string, which is css) names nothing a person reads.
fn link_name(l: &LocatorStep) -> Option<&str> {
    l.name.as_deref().or(l.text.as_deref()).map(str::trim).filter(|n| !n.is_empty())
}

/// The name of the control `target` points at: its innermost link's.
fn control_name(target: &Target) -> Option<String> {
    target.links().last().and_then(link_name).map(str::to_string)
}

/// Everything `target` names, innermost last, for finding a value in it.
fn target_text(target: &Target) -> String {
    target.links().iter().filter_map(link_name).collect::<Vec<_>>().join(" ")
}

fn kind_of(name: &str) -> Option<EffectKind> {
    let words: Vec<String> =
        name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    let has = |list: &[&str]| words.iter().any(|w| list.contains(&w.as_str()));
    if has(PUBLISH_WORDS) {
        Some(EffectKind::Publish)
    } else if has(DELETE_WORDS) {
        Some(EffectKind::Delete)
    } else if has(SAVE_WORDS) {
        Some(EffectKind::Save)
    } else {
        None
    }
}

/// Every action of the script with its step number, in the order a run
/// meets them, the ones inside a `when_visible` or a `repeat_while_visible`
/// included.
fn each_action(script: &CaseScript) -> Vec<(i32, &Action)> {
    fn walk<'a>(step: i32, actions: &'a [Action], out: &mut Vec<(i32, &'a Action)>) {
        for a in actions {
            out.push((step, a));
            match a {
                Action::WhenVisible { then, .. } | Action::RepeatWhileVisible { then, .. } => walk(step, then, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for s in &script.steps {
        walk(s.step_number, &s.actions, &mut out);
    }
    out
}

/// A click on a control whose name reads as a write, with what it reads as.
fn click_effect(a: &Action) -> Option<(EffectKind, String, &Target)> {
    let Action::Click { selector } = a else { return None };
    let name = control_name(selector)?;
    kind_of(&name).map(|k| (k, name, selector))
}

/// Each control a step clicks that appears to delete, save or publish, once
/// per step, in script order. The one rule every hand-over line about
/// writes is built from.
pub fn step_effects(script: &CaseScript) -> Vec<StepEffect> {
    let mut out: Vec<StepEffect> = Vec::new();
    for (step, a) in each_action(script) {
        if let Some((kind, control, _)) = click_effect(a) {
            let e = StepEffect { step, kind, control };
            if !out.contains(&e) {
                out.push(e);
            }
        }
    }
    out
}

/// Whether a field reads as one that looks records up rather than writes
/// one: a searchbox, or anything named, labelled or selected with "search"
/// or "filter" in it, whatever the case.
fn is_search_field(target: &Target) -> bool {
    let mentions = |s: &str| {
        let s = s.to_lowercase();
        s.contains("search") || s.contains("filter")
    };
    target.links().iter().any(|l| {
        l.role.as_deref() == Some("searchbox")
            || [&l.name, &l.text, &l.css].into_iter().flatten().any(|v| mentions(v))
    })
}

/// The records the case deletes after making them itself: a delete whose
/// locator names a value the case typed and then saved. A value typed and
/// never saved (a search) is a record the case found, not one it made.
/// Only what was typed since the previous click that reads as a write
/// counts as made by a save, and a search or filter field never does.
/// One per step, the longest such value.
fn created_then_deleted(script: &CaseScript) -> Vec<(i32, String)> {
    let mut typed: Vec<String> = Vec::new();
    let mut created: Vec<String> = Vec::new();
    let mut out: Vec<(i32, String)> = Vec::new();
    for (step, a) in each_action(script) {
        if let Action::Fill { selector, value } = a {
            let v = value.trim();
            if v.chars().count() >= MIN_CREATED_VALUE && !is_search_field(selector) {
                typed.push(v.to_string());
            }
            continue;
        }
        let Some((kind, _, target)) = click_effect(a) else { continue };
        match kind {
            EffectKind::Save | EffectKind::Publish => created.append(&mut typed),
            EffectKind::Delete => {
                typed.clear();
                let text = target_text(target).to_lowercase();
                let found = created.iter().filter(|v| text.contains(&v.to_lowercase())).max_by_key(|v| v.len());
                if let Some(v) = found {
                    if !out.iter().any(|(s, _)| *s == step) {
                        out.push((step, v.clone()));
                    }
                }
            }
        }
    }
    out
}

/// The repo common components for the TCM components a case uses, in the
/// order it meets them, each once: what the person mapped them to
/// (`ExportMap.components`). A component that is not mapped, or is mapped to
/// a blank or to something that is no folder name, says nothing.
pub fn uses_for(used: &[String], mapped: &BTreeMap<String, String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in used {
        let Some(repo) = mapped.get(name).map(|r| r.trim()) else { continue };
        if crate::pw_repo::layout::valid_name(repo) && !out.iter().any(|o| o == repo) {
            out.push(repo.to_string());
        }
    }
    out
}

pub fn handover_for(input: &HandoverInput) -> Handover {
    let s = input.script;
    let mut h = Handover::default();

    for p in &s.preconditions {
        let mut row = format!("{} at {} stage", p.flow, p.stage);
        if let Some(why) = p.why.as_deref().map(str::trim).filter(|w| !w.is_empty()) {
            row.push_str(&format!(": {why}"));
        }
        h.preconditions.push(row);
    }
    h.preconditions.extend(s.needs_unchanged.iter().map(|m| format!("requires: {m} unchanged")));
    h.preconditions.extend(s.needs_state.iter().map(|(k, v)| format!("requires: {k} = {v}")));
    h.preconditions.extend(
        input
            .shared_drafts
            .iter()
            .map(|d| format!("shared draft \"{}\", rebuilt by TCM fixture `{}`", d.name, d.fixture)),
    );

    if s.no_save {
        h.from_tcm.push("Look-only: must not save anything.".to_string());
    }
    if let Some(reason) = &input.needs_ruling {
        h.from_tcm.push(format!("left as test.fixme: {reason}"));
    }
    for e in step_effects(s) {
        let at = step_name(s, e.step);
        let line = match e.kind {
            EffectKind::Delete => format!("{at} appears to delete data (\"{}\")", e.control),
            EffectKind::Save => format!("{at} appears to save data (\"{}\")", e.control),
            // Names no control, so two publish buttons in a step say it once.
            EffectKind::Publish => format!("{at} appears to publish - irreversible; run on demand only"),
        };
        if !h.from_tcm.contains(&line) {
            h.from_tcm.push(line);
        }
    }
    h.from_tcm.extend(s.changes.iter().map(|c| format!("changes shared state: {c}")));
    if !input.protected.is_empty() {
        h.from_tcm.push(format!("must never touch the protected records in {BASELINE_DOC_REL}"));
    }
    if let Some(d) = &s.suspected_defect {
        h.from_tcm.push(format!(
            "step {} is a known product defect (TCM mark {}): {}",
            d.step_number,
            day_of(&d.marked_at),
            d.note
        ));
    }
    for (step, value) in created_then_deleted(s) {
        let at = step_name(s, step);
        h.cleanup.push(format!("{at} deletes \"{value}\" it created"));
        h.from_tcm.push(format!("relies on {at} to put data back"));
    }
    h.warnings = script_warnings(s);
    h
}

/// How the hand-over names step `n` of `script`: `step N`, or `the clean-up`
/// for its clean-up step, which the spec writes under `// Clean-up:` with no
/// number.
fn step_name(script: &CaseScript, n: i32) -> String {
    match script.cleanup_step() {
        Some(c) if c.step_number == n => "the clean-up".to_string(),
        _ => format!("step {n}"),
    }
}

/// Whether a locator chooses its element by position among several matches
/// rather than by what it is called: an `nth`, or a css link that takes the
/// first or the Nth child.
fn picks_by_position(target: &Target) -> bool {
    target.links().iter().any(|l| {
        l.nth.is_some()
            || l.css.as_deref().is_some_and(|c| {
                let c = c.to_lowercase();
                c.contains(":first-child") || c.contains(":first-of-type") || c.contains(":nth-")
            })
    })
}

/// Whether any of `actions`, a `when_visible` or repeat inside them
/// included, is a click that appears to delete (`click_effect`'s rule).
fn deletes_any(actions: &[Action]) -> bool {
    actions.iter().any(|a| match a {
        Action::WhenVisible { then, .. } | Action::RepeatWhileVisible { then, .. } => deletes_any(then),
        _ => matches!(click_effect(a), Some((EffectKind::Delete, _, _))),
    })
}

/// What the script alone shows that a person should see before the case is
/// written, one plain line each, in the order a run meets it, each once:
/// - a delete-named click (`step_effects`' rule) whose locator picks by
///   position, so the row it deletes is not one the case named;
/// - a `repeat_while_visible` that appears to delete (its `then` clicks a
///   delete-named control) with no `except` list, so it deletes every
///   matching row;
/// - a publish (`step_effects`, the one rule).
pub fn script_warnings(script: &CaseScript) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |line: String| {
        if !out.contains(&line) {
            out.push(line);
        }
    };
    for (step, a) in each_action(script) {
        let at = step_name(script, step);
        match a {
            Action::RepeatWhileVisible { except, then, .. } if except.is_empty() && deletes_any(then) => {
                add(format!("{at} appears to delete every matching row (no protected rows listed)"))
            }
            _ => {
                if let Some((kind, _, target)) = click_effect(a) {
                    match kind {
                        EffectKind::Delete if picks_by_position(target) => {
                            add(format!("{at} appears to delete a row it did not name"))
                        }
                        EffectKind::Publish => add(format!("{at} appears to publish - irreversible")),
                        _ => {}
                    }
                }
            }
        }
    }
    out
}

/// The state this case needs unchanged that another of the listed cases
/// changes, one line each: `needs <item>, which #<case> changes`. `item`
/// is the spelling the needing case uses; the comparison is the planner's
/// (`normalise`). A case never conflicts with itself.
pub fn needs_warnings(id: i32, script: &CaseScript, listed: &[(i32, &CaseScript)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for need in &script.needs_unchanged {
        let key = normalise(need);
        for (other, o) in listed {
            if *other != id && o.changes.iter().any(|c| normalise(c) == key) {
                let line = format!("needs {}, which #{other} changes", need.trim());
                if !out.contains(&line) {
                    out.push(line);
                }
            }
        }
    }
    out
}

/// The one line every order note ends with. The repo has no such tags yet;
/// this only proposes them, so it is worded as a proposal.
pub const ORDER_PROPOSAL: &str =
    "Proposed for the repo owner: tag publishing cases @publishes and destructive ones @destructive and keep them out of routine runs.";

/// Whether the script appears to publish (`step_effects`, the one rule).
fn publishes(script: &CaseScript) -> bool {
    step_effects(script).iter().any(|e| e.kind == EffectKind::Publish)
}

/// What the cases of one test-case file need said about the order they run
/// in, one plain line each, from the marks in their scripts and the PBI's
/// saved order (`saved_order`, when there is one):
/// - a case that needs state unchanged runs before the case that changes it;
/// - a case that publishes, or changes shared state, runs alone and last;
/// - the saved TCM order of the cases in this file (two or more);
/// - a group, in the order TCM would suggest, for cases linked by the first
///   rule.
///
/// Always ends with `ORDER_PROPOSAL`; a note that is only that line says
/// nothing about these cases. A case listed twice counts once.
pub fn order_note(cases: &[(i32, &CaseScript)], saved_order: Option<&[i32]>) -> Vec<String> {
    let mut seen: Vec<i32> = Vec::new();
    let mut once: Vec<(i32, &CaseScript)> = Vec::new();
    for &(id, script) in cases {
        if !seen.contains(&id) {
            seen.push(id);
            once.push((id, script));
        }
    }
    let cases = once;
    let marks: Vec<(i32, Marks)> = cases.iter().map(|(id, s)| (*id, Marks::of(s))).collect();
    let mut out: Vec<String> = Vec::new();

    // (needer, changer, the state's name as the needer spells it), by file
    // position: only the constraints the planner keeps, so the notes never
    // say what `autorun::plan` would not do.
    let at = |id: i32| cases.iter().position(|(c, _)| *c == id).expect("a constraint names a listed case");
    let mut edges: Vec<(usize, usize, String)> = Vec::new();
    for (needer, changer) in kept_constraints(&marks) {
        let (i, j) = (at(needer), at(changer));
        let changed: Vec<String> = marks[j].1.changes.iter().map(|c| normalise(c)).collect();
        let item = marks[i].1.needs.iter().find(|n| changed.contains(&normalise(n)));
        edges.push((i, j, item.map(|n| n.trim().to_string()).unwrap_or_default()));
    }
    edges.sort_by_key(|(i, j, _)| (*i, *j));
    for (i, j, item) in &edges {
        out.push(format!(
            "run {} before {} ({} needs {item} unchanged and {} changes it)",
            cases[*i].0, cases[*j].0, cases[*i].0, cases[*j].0
        ));
    }

    for (id, script) in &cases {
        if publishes(script) {
            out.push(format!("run {id} alone and last: it publishes"));
        } else if !script.changes.is_empty() {
            out.push(format!("run {id} alone and last: it changes shared state ({})", script.changes.join(", ")));
        }
    }

    if let Some(saved) = saved_order {
        let here: Vec<String> =
            saved.iter().filter(|id| seen.contains(id)).map(|id| id.to_string()).collect();
        if here.len() >= 2 {
            out.push(format!("saved TCM order: {}", here.join(", ")));
        }
    }

    // Cases linked by those edges, whatever the direction, form one group.
    let mut group_of: Vec<usize> = (0..cases.len()).collect();
    for (i, j, _) in &edges {
        let (from, to) = (group_of[*j], group_of[*i]);
        group_of.iter_mut().filter(|g| **g == from).for_each(|g| *g = to);
    }
    let suggested = suggest(&marks);
    let mut done: Vec<usize> = Vec::new();
    for first in 0..cases.len() {
        let g = group_of[first];
        if done.contains(&g) {
            continue;
        }
        done.push(g);
        let members: Vec<i32> = suggested
            .iter()
            .copied()
            .filter(|id| cases.iter().position(|(c, _)| c == id).is_some_and(|p| group_of[p] == g))
            .collect();
        if members.len() >= 2 {
            let ids: Vec<String> = members.iter().map(i32::to_string).collect();
            out.push(format!("suggested group: {} (run together, in this order)", ids.join(", ")));
        }
    }

    out.push(ORDER_PROPOSAL.to_string());
    out
}
