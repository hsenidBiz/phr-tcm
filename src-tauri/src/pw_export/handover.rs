//! What TCM knows about a case that the repo cannot see, handed over in the
//! case's md section: the records and settings it relies on (Preconditions
//! rows), what it appears to write, publish or must not touch (the
//! `**From TCM:**` block), and the cleanup it relies on (Cleanup rows).
//!
//! Pure: everything comes from the resolved script and the shared drafts it
//! reads. What a step writes is inferred from the names of the controls it
//! clicks, so every such line says "appears to".

use super::export::SharedDraft;
use crate::autorun::quirks::day_of;
use crate::autorun::CaseScript;
use crate::browser::actions::Action;
use crate::browser::locator::{LocatorStep, Target};

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
    pub uses: Vec<String>,
    pub warnings: Vec<String>,
}

pub struct HandoverInput<'a> {
    /// The script as the spec is written from it (`export::resolved_script`).
    pub script: &'a CaseScript,
    pub shared_drafts: &'a [SharedDraft],
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

/// The records the case deletes after making them itself: a delete whose
/// locator names a value the case typed and then saved. A value typed and
/// never saved (a search) is a record the case found, not one it made.
/// One per step, the longest such value.
fn created_then_deleted(script: &CaseScript) -> Vec<(i32, String)> {
    let mut typed: Vec<String> = Vec::new();
    let mut created: Vec<String> = Vec::new();
    let mut out: Vec<(i32, String)> = Vec::new();
    for (step, a) in each_action(script) {
        if let Action::Fill { value, .. } = a {
            let v = value.trim();
            if v.chars().count() >= MIN_CREATED_VALUE {
                typed.push(v.to_string());
            }
            continue;
        }
        match click_effect(a) {
            Some((EffectKind::Save | EffectKind::Publish, _, _)) => created.append(&mut typed),
            Some((EffectKind::Delete, _, target)) => {
                let text = target_text(target).to_lowercase();
                let found = created.iter().filter(|v| text.contains(&v.to_lowercase())).max_by_key(|v| v.len());
                if let Some(v) = found {
                    if !out.iter().any(|(s, _)| *s == step) {
                        out.push((step, v.clone()));
                    }
                }
            }
            None => {}
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
    for e in step_effects(s) {
        let line = match e.kind {
            EffectKind::Delete => format!("step {} appears to delete data (\"{}\")", e.step, e.control),
            EffectKind::Save => format!("step {} appears to save data (\"{}\")", e.step, e.control),
            // Names no control, so two publish buttons in a step say it once.
            EffectKind::Publish => format!("step {} appears to publish - irreversible; run on demand only", e.step),
        };
        if !h.from_tcm.contains(&line) {
            h.from_tcm.push(line);
        }
    }
    h.from_tcm.extend(s.changes.iter().map(|c| format!("changes shared state: {c}")));
    if let Some(d) = &s.suspected_defect {
        h.from_tcm.push(format!(
            "step {} is a known product defect (TCM mark {}): {}",
            d.step_number,
            day_of(&d.marked_at),
            d.note
        ));
    }
    for (step, value) in created_then_deleted(s) {
        h.cleanup.push(format!("step {step} deletes \"{value}\" it created"));
        h.from_tcm.push(format!("relies on step {step} to put data back"));
    }
    h
}
