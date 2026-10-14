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
//!
//! An output and a `creates` name may also be what a step SENT (`sent_ref`):
//! `{{steps.<n>.params.<param>}}`, or the exact text of one step's param as
//! written (`"{{prefix}} cycle {{now:yyyyMMddHHmmss}}"`). Either resolves,
//! once per run, to the text that step sent for that param, so a script can
//! open its own draft by a unique name when no template hands the name back.

use super::flow::{creating_stage, required_before, Flow, Stage};
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
    /// Stages of a flow that the record this fixture makes does not
    /// include (a cycle with Competencies off), by stage title or id: the
    /// flow gate (`flow_problems`) does not ask an earlier step to perform
    /// them. Never the creating stage, and never one a step performs.
    #[serde(default)]
    pub skip_stages: Vec<String>,
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

/// The step (1-based) and the param whose SENT text `value` stands for:
/// `{{steps.<m>.params.<param>}}` naming a param step m has, or the exact
/// text (trimmed) of a param as some step writes it. With `step`, only that
/// step's params are read; else the first step that sends it.
pub fn sent_ref(f: &Fixture, value: &str, step: Option<usize>) -> Option<(usize, String)> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    let names = exec::placeholders(v);
    if let [one] = names.as_slice() {
        if v.starts_with("{{") && v.ends_with("}}") && v.matches("{{").count() == 1 {
            if let Some((m, capture)) = steps_ref(one) {
                let param = capture.strip_prefix("params.")?;
                let has = m >= 1 && m <= f.steps.len() && f.steps[m - 1].params.contains_key(param);
                return (has && step.is_none_or(|s| s == m)).then(|| (m, param.to_string()));
            }
        }
    }
    f.steps
        .iter()
        .enumerate()
        .map(|(i, s)| (i + 1, s))
        .filter(|(m, _)| step.is_none_or(|s| s == *m))
        .find_map(|(m, s)| s.params.iter().find(|(_, raw)| raw.trim() == v).map(|(p, _)| (m, p.clone())))
}

/// Said when an output is neither a step's capture nor what a step sent.
pub fn output_not_from_a_step(name: &str, value: &str) -> String {
    format!(
        "output {name}: {} does not come from a step - an output is one {{{{steps.<n>.<output>}}}} (a value step n captured), one {{{{steps.<n>.params.<param>}}}} (the text step n sent for that param), or the exact text of one step's param as written",
        value.trim()
    )
}

/// Said when a `creates` name is neither a capture nor what the step that
/// captured its id sent.
pub fn creates_name_not_from_a_step(kind: &str, value: &str) -> String {
    format!(
        "creates: {kind}'s name {} does not come from a step - use the capture holding the name the creating step gave it, {{{{steps.<n>.params.<param>}}}} for the name that step sent, or the exact text of that param as written",
        value.trim()
    )
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

/// The step (1-based, before `n`) and the capture a whole
/// `{{steps.<m>.<capture>}}` value reads.
fn step_ref_of(value: &str, n: usize) -> Option<(usize, String)> {
    let v = value.trim();
    let names = exec::placeholders(v);
    if names.len() != 1 || !v.starts_with("{{") || !v.ends_with("}}") || v.matches("{{").count() != 1 {
        return None;
    }
    // `{{steps.<m>.params.<param>}}` is what a step sent, not a capture.
    steps_ref(&names[0])
        .filter(|(m, c)| *m >= 1 && *m < n && !c.starts_with("params."))
        .map(|(m, c)| (m, c.to_string()))
}

/// A step whose template performs a stage of a flow. Its flow must still
/// be saved with that stage in it. For a stage that is not the creating
/// stage: with no database to ask, the fixture must perform the flow's
/// earlier stages itself - every stage the gate would ask about
/// (`flow::required_before`) performed by an earlier step - and every stage
/// must act on the one record the flow's creating step made. So its
/// subject (the flow's `subject.name`, which the creating stage's template
/// captures and outputs, `check_stage_ref`) is exactly
/// `{{steps.<m>.<subject>}}` with step m that creating step, and every
/// earlier step on the flow is step m itself or takes that same value.
fn flow_problems(
    f: &Fixture,
    n: usize,
    t: &ApiTemplate,
    templates: &dyn Fn(&str) -> Option<ApiTemplate>,
    flows: &dyn Fn(&str) -> Option<Flow>,
) -> Vec<String> {
    let Some(r) = &t.stage else { return vec![] };
    let Some(flow) = flows(&r.flow) else {
        return vec![format!("step {n}: template {} belongs to flow {}, which is no longer saved", t.id, r.flow)];
    };
    let Some(stage) = flow.stages.iter().find(|s| s.id == r.id) else {
        return vec![format!(
            "step {n}: template {} belongs to flow {}, but stage {} is no longer in flow {}",
            t.id, r.flow, r.id, r.flow
        )];
    };
    let creating = creating_stage(&flow).map(|s| s.id.clone());
    if creating.as_deref() == Some(stage.id.as_str()) {
        return vec![];
    }
    // The stages the earlier steps perform on this flow, by step.
    let on_flow = |m: usize| -> Option<String> {
        let earlier = templates(&f.steps[m - 1].template)?;
        earlier.stage.filter(|s| s.flow == flow.id).map(|s| s.id)
    };
    let performed: Vec<String> = (1..n).filter_map(on_flow).collect();
    // The stages this record does not include, each in the flow it is of.
    let skipped = skipped_ids(f, templates, flows);
    let mut problems = Vec::new();
    let missing: Vec<&str> = required_before(&flow, &stage.id)
        .into_iter()
        .filter(|s| !performed.iter().any(|p| p == &s.id) && !skipped.iter().any(|(fl, st)| fl == &flow.id && st == &s.id))
        .map(|s| s.title.as_str())
        .collect();
    if !missing.is_empty() {
        problems.push(format!(
            "step {n}: template {} performs {} of flow {}, so the steps before it must perform {} - or, for a stage this record does not include, name it in the fixture's skip_stages",
            t.id,
            stage.title,
            flow.title,
            missing.join(", ")
        ));
    }
    let subject = &flow.subject.name;
    let subject_of = |k: usize| f.steps[k - 1].params.get(subject).and_then(|v| step_ref_of(v, k));
    let creates = |m: usize| creating.is_some() && on_flow(m) == creating;
    let one_record = match subject_of(n) {
        Some((m, x)) if &x == subject && creates(m) => (1..n)
            .filter(|k| on_flow(*k).is_some())
            .all(|k| k == m || (!creates(k) && subject_of(k) == Some((m, x.clone())))),
        _ => false,
    };
    if !one_record {
        problems.push(format!(
            "step {n}: template {} performs {} of flow {}, so its {subject} must be one {{{{steps.<m>.<output>}}}} of an earlier step on that flow",
            t.id, stage.title, flow.title
        ));
    }
    problems
}

/// Each `skip_stages` entry resolved to the one stage it names, in the flow
/// that owns it: `(entry, Some((flow id, stage id)))`, or `None` with the
/// sentence that refuses it. An entry is a stage's title or id (case and
/// the space around it ignored), or `<flow id>/<stage id>`; it is read
/// against the flows `f`'s steps are on, and must name exactly one stage
/// of them - a title or an id two of them share is refused, asking for the
/// stage id or the flow-qualified form.
fn resolve_skips(f: &Fixture, on: &[Flow]) -> Vec<(String, Result<(String, String), String>)> {
    f.skip_stages
        .iter()
        .map(|entry| {
            let n = entry.trim();
            let hits: Vec<(&Flow, &Stage)> = match n.split_once('/') {
                Some((flow, stage)) => on
                    .iter()
                    .filter(|fl| fl.id.eq_ignore_ascii_case(flow.trim()))
                    .flat_map(|fl| fl.stages.iter().filter(|s| s.id.eq_ignore_ascii_case(stage.trim())).map(move |s| (fl, s)))
                    .collect(),
                None => {
                    let by = |m: &dyn Fn(&Stage) -> bool| -> Vec<(&Flow, &Stage)> {
                        on.iter().flat_map(|fl| fl.stages.iter().filter(|s| m(s)).map(move |s| (fl, s))).collect()
                    };
                    let by_id = by(&|s: &Stage| s.id.eq_ignore_ascii_case(n));
                    if by_id.len() == 1 {
                        by_id
                    } else {
                        let by_title = by(&|s: &Stage| s.title.trim().eq_ignore_ascii_case(n));
                        if by_id.is_empty() { by_title } else { by_id }
                    }
                }
            };
            let said = match hits.as_slice() {
                [(fl, s)] => Ok((fl.id.clone(), s.id.clone())),
                [] => {
                    let titles: Vec<String> = on.iter().map(|fl| fl.title.clone()).collect();
                    let where_ = if titles.is_empty() { "no flow: no step performs a flow's stage".to_string() } else { titles.join(", ") };
                    Err(format!("skip_stages: {n} is not a stage of {where_}"))
                }
                many => {
                    let named: Vec<String> = many.iter().map(|(fl, s)| format!("{}/{}", fl.id, s.id)).collect();
                    Err(format!(
                        "skip_stages: {n} names more than one stage ({}) - name the one you mean by its stage id, or as <flow id>/<stage id>",
                        named.join(", ")
                    ))
                }
            };
            (entry.clone(), said)
        })
        .collect()
}

/// The stages `f` skips, as `(flow id, stage id)`, each the one its entry
/// names (`resolve_skips`); an entry that names none or several skips
/// nothing.
fn skipped_ids(f: &Fixture, templates: &dyn Fn(&str) -> Option<ApiTemplate>, flows: &dyn Fn(&str) -> Option<Flow>) -> Vec<(String, String)> {
    if f.skip_stages.is_empty() {
        return vec![];
    }
    resolve_skips(f, &flows_of(f, templates, flows)).into_iter().filter_map(|(_, r)| r.ok()).collect()
}

/// The flows the steps of `f` perform stages of, each once, in step order.
fn flows_of(f: &Fixture, templates: &dyn Fn(&str) -> Option<ApiTemplate>, flows: &dyn Fn(&str) -> Option<Flow>) -> Vec<Flow> {
    let mut out: Vec<Flow> = Vec::new();
    for step in &f.steps {
        let Some(r) = templates(&step.template).and_then(|t| t.stage) else { continue };
        if out.iter().any(|fl| fl.id == r.flow) {
            continue;
        }
        if let Some(fl) = flows(&r.flow) {
            out.push(fl);
        }
    }
    out
}

/// What is wrong with `f`'s `skip_stages`: an entry naming no stage of a
/// flow its steps are on, or more than one (`resolve_skips`), the creating
/// stage (a fixture always makes its record), or a stage one of its steps
/// performs.
fn skip_problems(
    f: &Fixture,
    templates: &dyn Fn(&str) -> Option<ApiTemplate>,
    flows: &dyn Fn(&str) -> Option<Flow>,
) -> Vec<String> {
    if f.skip_stages.is_empty() {
        return vec![];
    }
    let on = flows_of(f, templates, flows);
    let mut problems = Vec::new();
    for (_, resolved) in resolve_skips(f, &on) {
        let (flow, stage) = match resolved {
            Ok(ids) => ids,
            Err(why) => {
                problems.push(why);
                continue;
            }
        };
        let Some(fl) = on.iter().find(|fl| fl.id == flow) else { continue };
        let Some(s) = fl.stages.iter().find(|s| s.id == stage) else { continue };
        if s.creates {
            problems.push(format!("skip_stages: {} makes the record in flow {}, so it cannot be skipped", s.title, fl.title));
        }
        let performing = f.steps.iter().enumerate().find(|(_, st)| {
            templates(&st.template).and_then(|t| t.stage).is_some_and(|r| r.flow == fl.id && r.id == s.id)
        });
        if let Some((i, _)) = performing {
            problems.push(format!(
                "skip_stages: step {} performs {}, so it is part of this record - take it out of skip_stages",
                i + 1,
                s.title
            ));
        }
    }
    problems
}

/// The stages `f` skips, as the answer to a save echoes them: `<stage
/// title> (flow <flow title>)`, each once.
pub fn skipped_stages(
    f: &Fixture,
    templates: &dyn Fn(&str) -> Option<ApiTemplate>,
    flows: &dyn Fn(&str) -> Option<Flow>,
) -> Vec<String> {
    let on = flows_of(f, templates, flows);
    let mut out = Vec::new();
    for (flow, stage) in skipped_ids(f, templates, flows) {
        let Some(fl) = on.iter().find(|fl| fl.id == flow) else { continue };
        let Some(s) = fl.stages.iter().find(|s| s.id == stage) else { continue };
        let line = format!("{} (flow {})", s.title, fl.title);
        if !out.contains(&line) {
            out.push(line);
        }
    }
    out
}

/// Said when a `creates` entry is not wholly the work of one step whose
/// template creates.
pub fn creates_one_step(kind: &str) -> String {
    format!("creates: {kind} must come from one step whose template creates")
}

/// Said when a `creates` names its thing by its id's own capture: Clean up
/// finds a test-made thing by its name starting with the test prefix, and
/// an id never does.
pub fn name_is_the_id(kind: &str, name: &str) -> String {
    format!(
        "creates: {kind}'s name {} is the same capture as its id - use the capture that holds the name the step gave it (one starting with {{{{prefix}}}}), so Clean up can find it",
        name.trim()
    )
}

/// Said when the step that makes a `creates` thing sends no value that
/// starts with `{{prefix}}`: its name cannot start with the test prefix, and
/// Clean up would never offer to delete it.
pub fn name_without_prefix(kind: &str, step: usize) -> String {
    format!(
        "creates: {kind}'s name cannot start with the test prefix - step {step} sends no value starting with {{{{prefix}}}}; give it a name such as \"{{{{prefix}}}} {kind}\" and capture that name"
    )
}

/// Said when a `creates` name is what its step sent for `param`, and that
/// text does not start with `{{prefix}}`.
pub fn sent_name_without_prefix(kind: &str, step: usize, param: &str) -> String {
    format!(
        "creates: {kind}'s name cannot start with the test prefix - step {step} sends {param} without {{{{prefix}}}} at its start; give it a name such as \"{{{{prefix}}}} {kind}\""
    )
}

/// What is wrong with how `c` (from one creating step, `from_one_creating_step`)
/// names its thing: the same capture as its id (`name_is_the_id`), or a step
/// that sends no value starting with `{{prefix}}` (`name_without_prefix`) -
/// said only when the fixture uses `{{prefix}}` somewhere (`uses_prefix`):
/// one that never does is told so once (`NEEDS_PREFIX`).
fn name_problem(f: &Fixture, c: &Creates, uses_prefix: bool) -> Option<String> {
    let id = step_ref_of(&c.id, f.steps.len() + 1)?;
    let starts_with_prefix = |v: &String| {
        let v = v.trim();
        v.strip_prefix("{{").and_then(|r| r.split_once("}}")).is_some_and(|(inner, _)| inner.trim() == "prefix")
    };
    // What the creating step sent: that param's own text is the name.
    if step_ref_of(&c.name, f.steps.len() + 1).is_none() {
        let (m, param) = sent_ref(f, &c.name, Some(id.0))?;
        let raw = &f.steps[m - 1].params[&param];
        if step_ref_of(raw, m) == Some(id) {
            return Some(name_is_the_id(&c.kind, &c.name));
        }
        return (uses_prefix && !starts_with_prefix(raw)).then(|| sent_name_without_prefix(&c.kind, m, &param));
    }
    let name = step_ref_of(&c.name, f.steps.len() + 1)?;
    if id == name {
        return Some(name_is_the_id(&c.kind, &c.name));
    }
    let step = &f.steps[name.0 - 1];
    (uses_prefix && !step.params.values().any(starts_with_prefix)).then(|| name_without_prefix(&c.kind, name.0))
}

/// How a fixture names what it creates, checked where a fixture is SAVED
/// (`fixture_store::save`), never where one runs: a fixture saved before
/// these rules keeps running (its run warns about a name without the
/// prefix, as it always did). Each `creates` from one creating step whose
/// name is its id's own capture (`name_is_the_id`), or whose step sends no
/// value starting with `{{prefix}}` (`name_without_prefix`, only when the
/// fixture uses `{{prefix}}` at all - one that never does is told so by
/// `validate`'s `NEEDS_PREFIX`).
pub fn naming_problems(f: &Fixture) -> Vec<String> {
    let uses_prefix =
        f.steps.iter().any(|s| s.params.values().any(|v| exec::placeholders(v).iter().any(|n| n == "prefix")));
    f.creates.iter().filter_map(|c| name_problem(f, c, uses_prefix)).collect()
}

/// Whether `c`'s id and name are both `{{steps.<m>.<x>}}` of the same step
/// m, and step m's template creates: its effect is `create` and it sends at
/// least one POST. A step that only reads (a GET) or edits found something
/// that was already there; recording that as test-made would let Clean up
/// offer to delete it.
fn from_one_creating_step(f: &Fixture, templates: &dyn Fn(&str) -> Option<ApiTemplate>, c: &Creates) -> bool {
    let step_of = |v: &str| step_ref_of(v, f.steps.len() + 1).map(|(m, _)| m);
    let id_step = step_of(&c.id);
    let name_step = step_of(&c.name).or_else(|| id_step.and_then(|m| sent_ref(f, &c.name, Some(m))).map(|(m, _)| m));
    match (id_step, name_step) {
        (Some(a), Some(b)) if a == b => templates(&f.steps[a - 1].template).is_some_and(|t| {
            t.effect == Effect::Create && t.steps.iter().any(|s| s.method == super::Method::Post)
        }),
        _ => false,
    }
}

/// Every problem with `f`, one sentence each. `templates` looks a template
/// up by id; a template with no `proven` is not proven. `flows` looks a
/// saved flow up by id, for the steps whose template performs a stage of
/// one. `Ok` is fit to save.
pub fn validate(
    f: &Fixture,
    templates: &dyn Fn(&str) -> Option<ApiTemplate>,
    flows: &dyn Fn(&str) -> Option<Flow>,
) -> Result<(), Vec<String>> {
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
                problems.extend(flow_problems(f, n, &t, templates, flows));
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
        if !is_step_value(f, templates, value) && sent_ref(f, value, None).is_none() {
            problems.push(output_not_from_a_step(name, value));
        }
    }

    for c in &f.creates {
        let mut from_steps = true;
        if !is_step_value(f, templates, &c.id) {
            problems.push(format!("creates: {} does not come from a step", c.id.trim()));
            from_steps = false;
        }
        // The name: a capture, or what the step that captured the id sent.
        let id_step = step_ref_of(&c.id, f.steps.len() + 1).map(|(m, _)| m);
        let sent_name = id_step.is_some_and(|m| sent_ref(f, &c.name, Some(m)).is_some());
        if !is_step_value(f, templates, &c.name) && !sent_name {
            problems.push(if step_ref_of(&c.name, f.steps.len() + 1).is_none() {
                creates_name_not_from_a_step(&c.kind, &c.name)
            } else {
                format!("creates: {} does not come from a step", c.name.trim())
            });
            from_steps = false;
        }
        // Only what a creating step made is recorded as test-made - so
        // Clean up only ever offers to delete a thing the tests created -
        // and the id and the name are that one step's.
        if from_steps && !from_one_creating_step(f, templates, c) {
            problems.push(creates_one_step(&c.kind));
        }
    }

    if !f.creates.is_empty() && !uses_prefix {
        problems.push(NEEDS_PREFIX.to_string());
    }

    problems.extend(skip_problems(f, templates, flows));

    // The same bad placeholder in several params is one problem, said once.
    let mut seen = std::collections::HashSet::new();
    problems.retain(|p| seen.insert(p.clone()));

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Why `raw`, sent as a fixture, does not read as one, named by where the
/// problem is: `creates[1].id is missing` rather than serde's bare "missing
/// field `id`", which reads as if the fixture's own id were missing. Each
/// `steps` and `creates` entry is read on its own first; `err` (the whole
/// fixture's error) is said when none of them is the problem.
pub fn shape_problem(raw: &Value, err: &serde_json::Error) -> String {
    fn entry<T: serde::de::DeserializeOwned>(list: &str, raw: &Value) -> Option<String> {
        let items = raw.get(list)?.as_array()?;
        items.iter().enumerate().find_map(|(i, item)| {
            let e = serde_json::from_value::<T>(item.clone()).err()?;
            let said = e.to_string();
            Some(match said.strip_prefix("missing field `").and_then(|r| r.split_once('`')) {
                Some((field, _)) => format!("{list}[{i}].{field} is missing"),
                None => format!("{list}[{i}]: {said}"),
            })
        })
    }
    let said = entry::<FixtureStep>("steps", raw).or_else(|| entry::<Creates>("creates", raw));
    match said {
        Some(where_) => where_,
        None => err.to_string(),
    }
}
