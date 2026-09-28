//! The API template format - the JSON shape written to
//! `<Auto Run data>/templates/<org-project slug>/<template-id>.json` - and
//! every check a draft must pass before it may be proven. See the design
//! doc, "API templates" §4, for the format and its rules; the fixture in
//! `tests/suite/api_templates.rs` mirrors that section's example.
//!
//! `exec` holds placeholder scanning and capture-path parsing - syntax
//! `check` leans on rather than re-implementing - and, once Task 3 lands,
//! request building.

pub mod exec;

use crate::autorun::recipe::origin_of;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

fn ok_status() -> u16 {
    200
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Create,
    Edit,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct Antiforgery {
    pub page: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ParamType {
    String,
    Number,
    Boolean,
    Date,
    List,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct Param {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ParamType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: Option<String>,
    /// Guidance for the assistant only - the app never runs this.
    #[serde(default)]
    pub lookup: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    Get,
    Post,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    #[serde(default = "ok_status")]
    pub status: u16,
    #[serde(default)]
    pub json: Option<Value>,
}

impl Default for Expect {
    fn default() -> Self {
        Expect { status: 200, json: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub name: String,
    pub method: Method,
    pub path: String,
    #[serde(default)]
    pub query: BTreeMap<String, String>,
    #[serde(default)]
    pub json: Option<Value>,
    #[serde(default)]
    pub form: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub expect: Expect,
    #[serde(default)]
    pub capture: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct Proven {
    pub at: String,
    pub origin: String,
    pub account: String,
    pub outputs: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct ApiTemplate {
    pub id: String,
    pub title: String,
    pub module: String,
    pub effect: Effect,
    pub description: String,
    pub sources: Vec<String>,
    pub antiforgery: Antiforgery,
    pub params: Vec<Param>,
    pub steps: Vec<Step>,
    pub outputs: Vec<String>,
    /// Written by the app from a successful proving run; a draft that
    /// carries one is refused by `check`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proven: Option<Proven>,
}

/// Template `id`: a filename component. Windows filenames are
/// case-insensitive, so this tightens the spec's ASCII rule to lowercase
/// only - `Pms-X` and `pms-x` would otherwise be the same file - and
/// matches `autorun::store::safe_run_id`'s character rule, shorter (100
/// rather than 200).
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Decodes only well-formed `%XX` escapes; anything else (a lone `%`, a
/// non-hex pair) passes through unchanged. Used solely to catch a `..`
/// segment smuggled in as `%2e%2e` - not a general URL decoder.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A step's `path`: exactly one leading `/` (not `//`, which a browser
/// reads as protocol-relative), no backslash anywhere, no `..` segment
/// even percent-encoded, and - belt and braces - still the same origin
/// once laid after a placeholder origin, using the same rule the sign-in
/// recipe uses (`recipe::origin_of`).
fn is_safe_relative_path(path: &str) -> bool {
    if !path.starts_with('/') || path.starts_with("//") {
        return false;
    }
    if path.contains('\\') {
        return false;
    }
    if path.split('/').any(|seg| percent_decode(seg) == "..") {
        return false;
    }
    origin_of(&format!("https://x.invalid{path}")).as_deref() == Some("https://x.invalid")
}

fn json_placeholder_names(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.extend(exec::placeholders(s)),
        Value::Array(items) => items.iter().for_each(|item| json_placeholder_names(item, out)),
        Value::Object(map) => {
            for (k, val) in map {
                out.extend(exec::placeholders(k));
                json_placeholder_names(val, out);
            }
        }
        _ => {}
    }
}

/// Every placeholder name this step's request could use: path, query
/// values, json (keys and leaf strings, recursively), form values.
fn step_placeholder_names(step: &Step) -> Vec<String> {
    let mut out = Vec::new();
    out.extend(exec::placeholders(&step.path));
    for v in step.query.values() {
        out.extend(exec::placeholders(v));
    }
    if let Some(j) = &step.json {
        json_placeholder_names(j, &mut out);
    }
    if let Some(f) = &step.form {
        for v in f.values() {
            out.extend(exec::placeholders(v));
        }
    }
    out
}

/// A real calendar day in `YYYY-MM-DD` form, leap years included.
fn is_valid_date(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    let [y, m, d] = parts.as_slice() else { return false };
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return false;
    }
    if !(y.chars().all(|c| c.is_ascii_digit())
        && m.chars().all(|c| c.is_ascii_digit())
        && d.chars().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    let year: u32 = y.parse().expect("checked all-digit");
    let month: u32 = m.parse().expect("checked all-digit");
    let day: u32 = d.parse().expect("checked all-digit");
    if !(1..=12).contains(&month) {
        return false;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => unreachable!("month already checked to be 1-12"),
    };
    (1..=days_in_month).contains(&day)
}

/// Every problem with this template, in template order (top-level `id`
/// first, then each step in order, then `outputs`, then `proven`). Empty
/// means the draft is fit to run.
pub fn check(t: &ApiTemplate) -> Vec<String> {
    let mut problems = Vec::new();

    if !valid_id(&t.id) {
        problems.push(format!(
            "id '{}' must be lowercase ASCII letters, digits, '-' or '_', 1-100 characters long",
            t.id
        ));
    }

    // Names visible to a placeholder: every declared param, plus every
    // capture from a step strictly earlier than the one being checked -
    // captures accumulate as the loop walks forward, so a step can never
    // see its own capture or one from a step after it.
    let mut known: HashSet<&str> = t.params.iter().map(|p| p.name.as_str()).collect();

    for step in &t.steps {
        if !is_safe_relative_path(&step.path) {
            problems.push(format!(
                "step '{}' has a path that is not a safe relative path on this origin: '{}'",
                step.name, step.path
            ));
        }

        if step.json.is_some() && step.form.is_some() {
            problems.push(format!(
                "step '{}' has both a json body and a form body; a step may have only one",
                step.name
            ));
        }

        let mut reported = HashSet::new();
        for name in step_placeholder_names(step) {
            if !known.contains(name.as_str()) && reported.insert(name.clone()) {
                problems.push(format!(
                    "step '{}' uses placeholder {{{{{}}}}}, which is not a declared param or captured by an earlier step",
                    step.name, name
                ));
            }
        }

        for (key, path) in &step.capture {
            if let Err(e) = exec::parse_capture_path(path) {
                problems.push(format!("step '{}' capture '{}' is not a valid capture path: {}", step.name, key, e));
            }
        }

        for key in step.capture.keys() {
            known.insert(key.as_str());
        }
    }

    for name in &t.outputs {
        if !t.steps.iter().any(|s| s.capture.contains_key(name)) {
            problems.push(format!("output '{name}' is never captured by any step"));
        }
    }

    if t.proven.is_some() {
        problems.push("proven is written by the app - leave it out".to_string());
    }

    problems
}

fn value_matches(kind: ParamType, v: &Value) -> bool {
    match kind {
        ParamType::String => v.is_string(),
        ParamType::Number => v.is_number(),
        ParamType::Boolean => v.is_boolean(),
        ParamType::Date => v.as_str().is_some_and(is_valid_date),
        ParamType::List => v.is_array(),
    }
}

/// Every problem with `values` against `t`'s declared params: a missing
/// required param, an undeclared one, or one whose value does not match
/// its declared type (a number sent as the string `"33"`, an impossible
/// date, and so on). Each problem names its param.
pub fn check_values(t: &ApiTemplate, values: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut problems = Vec::new();

    for param in &t.params {
        match values.get(&param.name) {
            Some(v) => {
                if !value_matches(param.kind, v) {
                    let want = match param.kind {
                        ParamType::String => "a string",
                        ParamType::Number => "a number",
                        ParamType::Boolean => "a boolean",
                        ParamType::Date => "a date in YYYY-MM-DD form",
                        ParamType::List => "a list",
                    };
                    problems.push(format!("param '{}' must be {want}", param.name));
                }
            }
            None => {
                if param.required {
                    problems.push(format!("param '{}' is required but missing", param.name));
                }
            }
        }
    }

    let declared: HashSet<&str> = t.params.iter().map(|p| p.name.as_str()).collect();
    for key in values.keys() {
        if !declared.contains(key.as_str()) {
            problems.push(format!("param '{key}' is not declared by this template"));
        }
    }

    problems
}

/// Parses a draft into an `ApiTemplate` and runs `check` on it. A serde
/// error (an unknown field, a wrong type, a missing field) becomes one
/// sentence; a structurally valid draft that fails `check` - including one
/// that carries `proven`, which only the app may write - comes back as
/// every problem `check` found.
pub fn parse_draft(v: &Value) -> Result<ApiTemplate, Vec<String>> {
    let t: ApiTemplate = serde_json::from_value(v.clone()).map_err(|e| vec![e.to_string()])?;
    let problems = check(&t);
    if problems.is_empty() {
        Ok(t)
    } else {
        Err(problems)
    }
}
