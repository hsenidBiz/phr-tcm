//! Flows - the ordered stages of a module's wizard, each with a read-only
//! SQL check, so a template can be refused until the stages before it are
//! done for a record. See the design doc "API template flows" §3 (the
//! format and every rule below) and §4 (how a template names its stage).
//!
//! This module is the pure model: types, validation, typed substitution of
//! the record id into a check, and the "required before" set. It performs no
//! I/O - the store, the gate and the database live elsewhere.

use super::{valid_id, ApiTemplate, ParamType};
use crate::api_templates::exec;
use crate::db::guard::{classify, Verdict};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// The most stages one flow may have.
pub const MAX_STAGES: usize = 30;

/// What kind of value identifies the flow's record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum SubjectType {
    Number,
    String,
}

impl SubjectType {
    /// `number` or `string`, as a sentence names the type.
    pub fn word(self) -> &'static str {
        match self {
            SubjectType::Number => "number",
            SubjectType::String => "string",
        }
    }
}

/// The record a flow is about: the placeholder name a check uses for it and
/// the type of the value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: SubjectType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub creates: bool,
    pub check: String,
}

/// Written by the app when a flow is saved; a draft that carries one is
/// refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct FlowSaved {
    pub at: String,
    // See `ApiTemplate`'s `Expect::json` comment on why this is declared to
    // TypeScript as `unknown`.
    #[specta(type = specta_typescript::Unknown)]
    pub sample: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    pub id: String,
    pub title: String,
    pub module: String,
    pub subject: Subject,
    #[serde(default)]
    pub sources: Vec<String>,
    pub stages: Vec<Stage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved: Option<FlowSaved>,
}

/// A template's pointer at the flow and stage it performs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct StageRef {
    pub flow: String,
    pub id: String,
}

fn valid_placeholder_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Every problem with this flow, in flow order, all together. Empty means
/// the flow may be saved.
pub fn check_flow(f: &Flow) -> Vec<String> {
    let mut problems = Vec::new();

    if !valid_id(&f.id) {
        problems.push(format!(
            "id '{}' must be lowercase ASCII letters, digits, '-' or '_', 1-100 characters long",
            f.id
        ));
    }

    let name_ok = valid_placeholder_name(&f.subject.name);
    if !name_ok {
        problems.push(format!(
            "subject name '{}' must be letters, digits or '_' only, so a check can use it as a placeholder",
            f.subject.name
        ));
    }

    if f.stages.len() > MAX_STAGES {
        problems.push(format!("a flow has at most {MAX_STAGES} stages; this one has {}", f.stages.len()));
    }

    // Stage ids: valid, and each used once.
    let mut seen: HashSet<&str> = HashSet::new();
    let mut duplicated: HashSet<&str> = HashSet::new();
    for s in &f.stages {
        if !valid_id(&s.id) {
            problems.push(format!(
                "stage id '{}' must be lowercase ASCII letters, digits, '-' or '_', 1-100 characters long",
                s.id
            ));
        }
        if !seen.insert(s.id.as_str()) && duplicated.insert(s.id.as_str()) {
            problems.push(format!("stage id '{}' is used more than once", s.id));
        }
    }

    // Exactly one creating stage, with nothing before it.
    let creators: Vec<&Stage> = f.stages.iter().filter(|s| s.creates).collect();
    match creators.len() {
        1 => {}
        0 => problems.push("a flow needs exactly one stage with creates: true (the one that makes the record), found none".to_string()),
        n => problems.push(format!(
            "a flow needs exactly one stage with creates: true (the one that makes the record), found {n}: {}",
            creators.iter().map(|s| format!("'{}'", s.id)).collect::<Vec<_>>().join(", ")
        )),
    }

    let by_id: HashMap<&str, &Stage> = f.stages.iter().rev().map(|s| (s.id.as_str(), s)).collect();

    for s in &f.stages {
        if s.creates {
            if !s.requires.is_empty() {
                problems.push(format!("stage '{}' creates the record, so it cannot require another stage", s.id));
            }
        } else if s.requires.is_empty() {
            problems.push(format!("stage '{}' must require at least one stage (only the creating stage requires nothing)", s.id));
        }

        for r in &s.requires {
            if r == &s.id {
                problems.push(format!("stage '{}' requires itself", s.id));
            } else {
                match by_id.get(r.as_str()) {
                    None => problems.push(format!("stage '{}' requires '{}', which is not a stage of this flow", s.id, r)),
                    Some(target) if target.optional => problems.push(format!(
                        "stage '{}' requires '{}', which is optional - nothing may require a stage that can be skipped",
                        s.id, r
                    )),
                    Some(_) => {}
                }
            }
        }
    }

    // The recursion below is bounded by the stage count, so it is only run
    // for a flow within the limit (which is already reported above).
    if f.stages.len() <= MAX_STAGES {
        problems.extend(cycles(f, &by_id));
    }

    // Checks. Only worth substituting when the subject's name can be one.
    if name_ok {
        let sample = match f.subject.kind {
            SubjectType::Number => Value::from(0u64),
            SubjectType::String => Value::from("x"),
        };
        for s in &f.stages {
            let names = exec::placeholders(&s.check);
            let mut bad_placeholder = false;
            if !names.iter().any(|n| n == &f.subject.name) {
                problems.push(format!("stage '{}' check must contain {{{{{}}}}}", s.id, f.subject.name));
                bad_placeholder = true;
            }
            let mut reported = HashSet::new();
            for n in names.iter().filter(|n| **n != f.subject.name) {
                if reported.insert(n) {
                    problems.push(format!(
                        "stage '{}' check uses {{{{{}}}}}, but only {{{{{}}}}} is available",
                        s.id, n, f.subject.name
                    ));
                    bad_placeholder = true;
                }
            }
            if !bad_placeholder {
                if let Err(why) = substitute_check(&s.check, &f.subject, &sample) {
                    problems.push(format!("stage '{}' check: {why}", s.id));
                }
            }
        }
    }

    if f.saved.is_some() {
        problems.push("saved is written by the app - leave it out".to_string());
    }

    problems
}

/// One sentence per loop in the `requires` graph, each loop reported once.
fn cycles(f: &Flow, by_id: &HashMap<&str, &Stage>) -> Vec<String> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Unseen,
        Visiting,
        Done,
    }

    fn visit<'a>(
        id: &'a str,
        by_id: &HashMap<&'a str, &'a Stage>,
        marks: &mut HashMap<&'a str, Mark>,
        path: &mut Vec<&'a str>,
        out: &mut Vec<String>,
    ) {
        marks.insert(id, Mark::Visiting);
        path.push(id);
        if let Some(stage) = by_id.get(id) {
            for r in &stage.requires {
                // A stage requiring itself has its own sentence; a name that
                // is not a stage has one too.
                if r == id || !by_id.contains_key(r.as_str()) {
                    continue;
                }
                match marks.get(r.as_str()).copied().unwrap_or(Mark::Unseen) {
                    Mark::Unseen => visit(r.as_str(), by_id, marks, path, out),
                    Mark::Visiting => {
                        let start = path.iter().position(|p| *p == r.as_str()).unwrap_or(0);
                        let mut names: Vec<String> = path[start..].iter().map(|p| format!("'{p}'")).collect();
                        names.push(format!("'{r}'"));
                        out.push(format!("stages {} require each other in a loop", names.join(" -> ")));
                    }
                    Mark::Done => {}
                }
            }
        }
        path.pop();
        marks.insert(id, Mark::Done);
    }

    let mut marks: HashMap<&str, Mark> = HashMap::new();
    let mut out = Vec::new();
    for s in &f.stages {
        if marks.get(s.id.as_str()).copied().unwrap_or(Mark::Unseen) == Mark::Unseen {
            visit(s.id.as_str(), by_id, &mut marks, &mut Vec::new(), &mut out);
        }
    }
    out
}

/// Parses a draft into a `Flow` and runs `check_flow` on it. A serde error
/// (an unknown field, a wrong type, a missing field) becomes one sentence; a
/// structurally valid draft that fails `check_flow` comes back as every
/// problem found.
pub fn parse_flow(v: &Value) -> Result<Flow, Vec<String>> {
    let f: Flow = serde_json::from_value(v.clone()).map_err(|e| vec![e.to_string()])?;
    let problems = check_flow(&f);
    if problems.is_empty() {
        Ok(f)
    } else {
        Err(problems)
    }
}

/// Writes `value` into `check` in place of every `{{<subject name>}}`, as a
/// literal of the subject's type - never as text pasted in: a `number` must
/// be a non-negative JSON integer and is written as its digits, a `string`
/// as an `N'...'` literal with every `'` doubled. The result is classified
/// again and must be a read, so the guard sees exactly what would run.
pub fn substitute_check(check: &str, subject: &Subject, value: &Value) -> Result<String, String> {
    let literal = match subject.kind {
        SubjectType::Number => match value.as_u64() {
            Some(n) => n.to_string(),
            None => {
                return Err(format!(
                    "{} is a number subject: give a whole number, 0 or more",
                    subject.name
                ))
            }
        },
        SubjectType::String => match value.as_str() {
            Some(s) => format!("N'{}'", s.replace('\'', "''")),
            None => return Err(format!("{} is a string subject: give a string", subject.name)),
        },
    };

    let mut out = String::with_capacity(check.len() + literal.len());
    let mut rest = check;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let name = after[..end].trim();
        out.push_str(&rest[..start]);
        if name == subject.name {
            out.push_str(&literal);
        } else if name.is_empty() {
            out.push_str(&rest[start..start + 2 + end + 2]);
        } else {
            return Err(format!(
                "the check uses {{{{{name}}}}}, but only {{{{{}}}}} is available",
                subject.name
            ));
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);

    match classify(&out) {
        Verdict::Read => Ok(out),
        Verdict::Write => Err("the check is not a read".to_string()),
        Verdict::Refused(why) => Err(why),
    }
}

/// Every stage that must be done before `stage`, transitively, in the
/// flow's declaration order. The stage itself is never in the set, and an
/// unknown stage has none.
pub fn required_before<'a>(f: &'a Flow, stage: &str) -> Vec<&'a Stage> {
    let by_id: HashMap<&str, &Stage> = f.stages.iter().rev().map(|s| (s.id.as_str(), s)).collect();
    let mut wanted: HashSet<&str> = HashSet::new();
    let mut pending: Vec<&str> = match by_id.get(stage) {
        Some(s) => s.requires.iter().map(String::as_str).collect(),
        None => return Vec::new(),
    };
    while let Some(id) = pending.pop() {
        if !wanted.insert(id) {
            continue;
        }
        if let Some(s) = by_id.get(id) {
            pending.extend(s.requires.iter().map(String::as_str));
        }
    }
    f.stages.iter().filter(|s| s.id != stage && wanted.contains(s.id.as_str())).collect()
}

/// The stage that makes the record.
pub fn creating_stage(f: &Flow) -> Option<&Stage> {
    f.stages.iter().find(|s| s.creates)
}

fn param_word(k: ParamType) -> &'static str {
    match k {
        ParamType::String => "a string",
        ParamType::Number => "a number",
        ParamType::Boolean => "a boolean",
        ParamType::Date => "a date",
        ParamType::List => "a list",
    }
}

/// The problems with a template's `stage` reference, given the flow it
/// names (`None` when that flow is not saved). A template with no `stage`
/// has none. Each sentence is one a person can act on.
pub fn check_stage_ref(t: &ApiTemplate, f: Option<&Flow>) -> Vec<String> {
    let Some(r) = &t.stage else { return Vec::new() };
    let Some(f) = f else {
        return vec![format!("this template's flow {} is no longer saved", r.flow)];
    };
    let Some(stage) = f.stages.iter().find(|s| s.id == r.id) else {
        return vec![format!("stage \"{}\" is no longer in flow {}", r.id, r.flow)];
    };

    let name = &f.subject.name;
    let mut problems = Vec::new();
    if stage.creates {
        if !t.steps.iter().any(|s| s.capture.contains_key(name)) {
            problems.push(format!(
                "stage \"{}\" creates the record, so a step of this template must capture {name}",
                stage.id
            ));
        }
        if !t.outputs.iter().any(|o| o == name) {
            problems.push(format!(
                "stage \"{}\" creates the record, so {name} must be listed in this template's outputs",
                stage.id
            ));
        }
    } else {
        let want = match f.subject.kind {
            SubjectType::Number => ParamType::Number,
            SubjectType::String => ParamType::String,
        };
        match t.params.iter().find(|p| &p.name == name) {
            None => problems.push(format!(
                "stage \"{}\" acts on the flow's record, so this template must declare a parameter named {name} of type {}",
                stage.id,
                f.subject.kind.word()
            )),
            Some(p) if p.kind != want => problems.push(format!(
                "parameter {name} is declared as {}, but flow {}'s subject is a {}: declare it as a {}",
                param_word(p.kind),
                f.id,
                f.subject.kind.word(),
                f.subject.kind.word()
            )),
            Some(_) => {}
        }
    }
    problems
}
