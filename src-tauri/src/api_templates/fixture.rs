//! Fixtures: a saved, ordered list of proven API templates that makes the
//! things a test script needs (a cycle, a suite, a set of records) before
//! the script runs. This file is the model and the save rules; running a
//! fixture lives elsewhere. See the design doc, "Fixtures" section 1.
//!
//! A step's params and a fixture's `outputs` and `creates` are text that
//! may hold `{{steps.<m>.<capture>}}`, `{{now:<format>}}` and `{{prefix}}`.
//! `<capture>` is one of the earlier template's declared `outputs`: those
//! are the values a template run hands back (see `runner`, which reports
//! only the declared outputs), so they are all a later step can read.

use super::{exec, ApiTemplate, Effect};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct FixtureStep {
    pub template: String,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

/// One thing a fixture makes: its kind and the placeholders that name its
/// id and its name once the fixture has run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct Creates {
    pub kind: String,
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub id: String,
    pub name: String,
    /// An Auto Run account key, as a template run takes.
    pub account: String,
    pub steps: Vec<FixtureStep>,
    #[serde(default)]
    pub outputs: BTreeMap<String, String>,
    #[serde(default)]
    pub creates: Vec<Creates>,
}

/// One running of a fixture, as its history keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct FixtureRun {
    pub at: String,
    pub ok: bool,
    /// The 1-based step that failed, when one did.
    #[serde(default)]
    pub failed_step: Option<u32>,
    #[serde(default)]
    pub detail: Option<String>,
    // See `RunRecord::outputs` on why the value side is `unknown`.
    #[serde(default)]
    #[specta(type = BTreeMap<String, specta_typescript::Unknown>)]
    pub outputs: BTreeMap<String, Value>,
}

pub const NEEDS_A_STEP: &str = "a fixture needs at least one step";
pub const NEEDS_PREFIX: &str = "a fixture with creates must use {{prefix}} in a step's params";

/// `Some((m, capture))` for a name of the form `steps.<m>.<capture>`.
fn steps_ref(name: &str) -> Option<(usize, &str)> {
    let rest = name.strip_prefix("steps.")?;
    let (m, capture) = rest.split_once('.')?;
    // All ASCII digits: `usize`'s parse also takes `+1`.
    if m.is_empty() || !m.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let m: usize = m.parse().ok()?;
    (!capture.is_empty()).then_some((m, capture))
}

/// True when step `m` (1-based, no later than `limit`) exists and its
/// template declares `capture` among its outputs.
fn comes_from(f: &Fixture, templates: &dyn Fn(&str) -> Option<ApiTemplate>, name: &str, limit: usize) -> bool {
    let Some((m, capture)) = steps_ref(name) else { return false };
    if m < 1 || m > limit || m > f.steps.len() {
        return false;
    }
    templates(&f.steps[m - 1].template).is_some_and(|t| t.outputs.iter().any(|o| o == capture))
}

/// A whole value that is exactly one `{{steps.<m>.<capture>}}` placeholder
/// from some step of the fixture.
fn is_step_value(f: &Fixture, templates: &dyn Fn(&str) -> Option<ApiTemplate>, value: &str) -> bool {
    let v = value.trim();
    let names = exec::placeholders(v);
    names.len() == 1
        && v.starts_with("{{")
        && v.ends_with("}}")
        && v.matches("{{").count() == 1
        && comes_from(f, templates, &names[0], f.steps.len())
}

/// Every problem with `f`, one sentence each. `templates` looks a template
/// up by id; a template with no `proven` is not proven. `Ok` is fit to
/// save.
pub fn validate(f: &Fixture, templates: &dyn Fn(&str) -> Option<ApiTemplate>) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();

    if f.steps.is_empty() {
        problems.push(NEEDS_A_STEP.to_string());
    }

    let mut uses_prefix = false;
    for (i, step) in f.steps.iter().enumerate() {
        let n = i + 1;
        match templates(&step.template) {
            Some(t) if t.proven.is_some() => {
                if t.effect == Effect::Delete {
                    problems.push(format!(
                        "step {n}: template {} deletes, and a fixture never deletes",
                        step.template
                    ));
                }
            }
            _ => problems.push(format!("step {n}: template {} is not proven", step.template)),
        }
        for value in step.params.values() {
            for name in exec::placeholders(value) {
                if name == "prefix" {
                    uses_prefix = true;
                } else if name.starts_with("steps.") && !comes_from(f, templates, &name, n - 1) {
                    problems.push(format!("step {n}: {{{{{name}}}}} does not come from an earlier step"));
                }
            }
        }
    }

    for (name, value) in &f.outputs {
        if !is_step_value(f, templates, value) {
            problems.push(format!("output {name}: {} does not come from a step", value.trim()));
        }
    }

    for c in &f.creates {
        for value in [&c.id, &c.name] {
            if !is_step_value(f, templates, value) {
                problems.push(format!("creates: {} does not come from a step", value.trim()));
            }
        }
    }

    if !f.creates.is_empty() && !uses_prefix {
        problems.push(NEEDS_PREFIX.to_string());
    }

    // The same bad placeholder in several params is one problem, said once.
    let mut seen = std::collections::HashSet::new();
    problems.retain(|p| seen.insert(p.clone()));

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}
