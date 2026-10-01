//! The API template format - the JSON shape written to
//! `<Auto Run data>/templates/<org-project slug>/<template-id>.json` - and
//! every check a draft must pass before it may be proven. See the design
//! doc, "API templates" §4, for the format and its rules; the fixture in
//! `tests/suite/api_templates.rs` mirrors that section's example.
//!
//! `exec` holds placeholder scanning and capture-path parsing - syntax
//! `check` leans on rather than re-implementing - and, once Task 3 lands,
//! request building.

pub mod cookies;
pub mod exec;
pub mod flow;
pub mod flow_page;
pub mod flow_store;
pub mod gate;
pub mod guide;
pub mod runner;
pub mod share;
pub mod store;

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
    /// What an optional param stands for when a run does not give it: what
    /// the application's own UI sends when the person leaves it empty
    /// (usually `[]` or `""`). Only an optional param has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(type = Option<specta_typescript::Unknown>)]
    pub default: Option<Value>,
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
    // `serde_json::Value`'s specta mapping pulls in `serde_json::Number`'s
    // i64/u64 variants, which the TypeScript exporter refuses to emit
    // (precision loss) - see the same note on `Step::json` below. This
    // only changes what TypeScript type the field is declared as
    // (`unknown | null`, cast before use); the wire format is still real
    // JSON.
    #[specta(type = Option<specta_typescript::Unknown>)]
    pub json: Option<Value>,
}

impl Default for Expect {
    fn default() -> Self {
        Expect { status: 200, json: None }
    }
}

/// One step of a template. Everything in this crate calls it `Step`.
pub type Step = ApiTemplateStep;

// `steps_xml::Step` (Azure DevOps' step XML) already owns the plain name in
// the generated bindings, and specta refuses two types of the same name in
// one export - so this one is NAMED `ApiTemplateStep` (`Step` above is an
// alias). It used to be a `Step` with `serde(rename = "ApiTemplateStep")`,
// which only renamed the exported type; but `files`' `skip_serializing_if`
// splits a type into `_Serialize` and `_Deserialize` shapes, and both of
// those took the rename whole and collided. The JSON shape is the same
// either way: a struct's container name is not in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct ApiTemplateStep {
    pub name: String,
    pub method: Method,
    pub path: String,
    #[serde(default)]
    pub query: BTreeMap<String, String>,
    #[serde(default)]
    // See `Expect::json`'s comment: specta's built-in `serde_json::Value`
    // mapping is unexportable as-is (it includes i64/u64), so this field
    // is declared to TypeScript as `unknown | null` rather than through it.
    #[specta(type = Option<specta_typescript::Unknown>)]
    pub json: Option<Value>,
    #[serde(default)]
    pub form: Option<BTreeMap<String, String>>,
    /// Files the step sends with its `form`: a form field name to the NAME
    /// of a file in the project's Test files (`crate::test_files`) - one
    /// file per field, a name and never a path, no placeholders. Left out
    /// when empty, so a template written before files existed reads and
    /// writes back exactly as it was.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, String>,
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
    // See `Expect::json`'s comment on why the value side is `unknown`.
    #[specta(type = BTreeMap<String, specta_typescript::Unknown>)]
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
    /// The flow stage this template performs, if it belongs to a flow.
    /// Checked where a flow can be loaded (`flow::check_stage_ref`), not by
    /// `check`, so a template saved before flows existed keeps loading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<flow::StageRef>,
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
/// non-hex pair) passes through unchanged. Not a general URL decoder - see
/// `fully_decode`, which is what `is_safe_relative_path` actually uses.
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

/// Applies `percent_decode` until it reaches a genuine fixed point - not a
/// fixed pass count - so a `..` percent-encoded any number of times over
/// (`%2e%2e`, `%252e%252e`, `%25252e%25252e`, ...: each extra layer wraps
/// the previous one's `%` as `%25`, which only one more decode pass peels
/// off) is always eventually caught, however many layers deep.
///
/// This terminates without an arbitrary cap: `percent_decode` only ever
/// turns a 3-byte `%XX` into 1 decoded byte, so every pass that changes
/// anything strictly shortens the string, and a string of length `path.len()`
/// cannot shorten more than `path.len()` times. The loop bound below is
/// that argument made explicit, purely as a backstop against a bug in it -
/// it is never expected to bind (a genuine fixed point is always reached
/// first, and the caller's `has_percent_encoding` check catches it if that
/// argument is ever wrong).
fn fully_decode(path: &str) -> String {
    let mut current = path.to_string();
    for _ in 0..=path.len() {
        let next = percent_decode(&current);
        if next == current {
            return current;
        }
        current = next;
    }
    current
}

/// True if `s` still contains a `%` followed by two hex digits - i.e. a
/// span `percent_decode` would still turn into a byte. Only ever true if
/// `fully_decode` stopped before reaching a genuine fixed point (it
/// shouldn't, see its own doc comment) - the second, independent belt
/// `is_safe_relative_path` relies on to refuse rather than silently treat
/// a not-fully-decoded path as safe.
fn has_percent_encoding(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && std::str::from_utf8(&bytes[i + 1..i + 3]).is_ok_and(|hex| u8::from_str_radix(hex, 16).is_ok())
        {
            return true;
        }
        i += 1;
    }
    false
}

/// A step's `path`: exactly one leading `/` (not `//`, which a browser
/// reads as protocol-relative) - checked on the raw text, since decoding
/// never changes where that leading slash falls - then, decoded (fully,
/// so `..%2f`, `%2e%2e%2f` and any depth of double (or more) encoding are
/// all caught the same as a literal `..`, with `has_percent_encoding` as a
/// second belt in case decoding somehow didn't reach a fixed point): no
/// backslash anywhere, and no `..` segment, treating `\` as a segment
/// separator too since an encoded backslash only appears after decoding.
pub(crate) fn is_safe_relative_path(path: &str) -> bool {
    if !path.starts_with('/') || path.starts_with("//") {
        return false;
    }
    let decoded = fully_decode(path);
    if has_percent_encoding(&decoded) {
        return false;
    }
    if decoded.contains('\\') {
        return false;
    }
    if decoded.split(['/', '\\']).any(|seg| seg == "..") {
        return false;
    }
    // Defense in depth, not load-bearing given the checks above: confirms
    // that laying the raw `path` after a placeholder origin still can't
    // move the request off that origin, using the exact rule the sign-in
    // recipe uses.
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

/// Every placeholder name this step's request could use: path, query keys
/// and values, json (keys and leaf strings, recursively), form keys and
/// values.
fn step_placeholder_names(step: &Step) -> Vec<String> {
    let mut out = Vec::new();
    out.extend(exec::placeholders(&step.path));
    // Keys as well as values: `build_request` fills in both.
    for (k, v) in &step.query {
        out.extend(exec::placeholders(k));
        out.extend(exec::placeholders(v));
    }
    if let Some(j) = &step.json {
        json_placeholder_names(j, &mut out);
    }
    if let Some(f) = &step.form {
        for (k, v) in f {
            out.extend(exec::placeholders(k));
            out.extend(exec::placeholders(v));
        }
    }
    out
}

/// Every problem with a step's `files`. They ride on a `form` body - one
/// that may be empty (`"form": {}`) when the step sends nothing but files -
/// so `files` on a step with no `form` (a `json` step, or no body at all)
/// is refused. Each field is non-empty, takes no placeholder, is not also a
/// `form` field, and names a file by a name `test_files` accepts; a file
/// name is written out in full, never through a placeholder.
fn file_problems(step: &Step) -> Vec<String> {
    let mut problems = Vec::new();
    if step.files.is_empty() {
        return problems;
    }
    if step.form.is_none() {
        problems.push(format!(
            "step '{}' sends files but has no form body - files go with a form (use \"form\": {{}} when there are no other fields)",
            step.name
        ));
    }
    for (field, name) in &step.files {
        if field.trim().is_empty() {
            problems.push(format!("step '{}' has a file field with an empty name", step.name));
            continue;
        }
        if !exec::placeholders(field).is_empty() || !exec::placeholders(name).is_empty() {
            problems.push(format!(
                "step '{}' file field '{field}' uses a placeholder - write the field and the test file's name out in full",
                step.name
            ));
            continue;
        }
        if step.form.as_ref().is_some_and(|f| f.contains_key(field)) {
            problems.push(format!(
                "step '{}' has '{field}' in both form and files - a field is one or the other",
                step.name
            ));
        }
        if !crate::test_files::valid_test_file_name(name) {
            problems.push(format!("step '{}' file field '{field}': {}", step.name, crate::test_files::bad_name(name)));
        }
    }
    problems
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
    // A default stands in for an optional param a run leaves out, so it is
    // a value of the param's type - and a required param, which every run
    // gives, has no use for one.
    for p in &t.params {
        match (&p.default, p.required) {
            (Some(_), true) => problems.push(format!(
                "param '{}' is required, so it has no use for a default - drop the default, or make it optional",
                p.name
            )),
            (Some(d), false) if !value_matches(p.kind, d) => problems.push(format!(
                "param '{}' has a default that is not {}",
                p.name,
                wanted(p.kind)
            )),
            _ => {}
        }
    }

    let mut known: HashSet<&str> = t.params.iter().map(|p| p.name.as_str()).collect();

    for step in &t.steps {
        if !is_safe_relative_path(&step.path) {
            problems.push(format!(
                "step '{}' has a path that is not a safe relative path on this origin: '{}'",
                step.name, step.path
            ));
        }
        // `build_request` adds `query` after a `?` of its own, so a path
        // with one (or a fragment) would send a second - raw or encoded.
        if step.path.contains(['?', '#']) || fully_decode(&step.path).contains(['?', '#']) {
            problems.push(format!(
                "step '{}' has a '?' or '#' in its path '{}' - the path is only the address; query parameters go in query",
                step.name, step.path
            ));
        }

        if step.json.is_some() && step.form.is_some() {
            problems.push(format!(
                "step '{}' has both a json body and a form body; a step may have only one",
                step.name
            ));
        }

        problems.extend(file_problems(step));

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

/// What a value of `kind` must be, for a sentence.
fn wanted(kind: ParamType) -> &'static str {
    match kind {
        ParamType::String => "a string",
        ParamType::Number => "a number",
        ParamType::Boolean => "a boolean",
        ParamType::Date => "a date in YYYY-MM-DD form",
        ParamType::List => "a list",
    }
}

/// `values` with every optional param a run left out filled in from its
/// `default` - the values a run's placeholders are read from. A param with
/// no default is never filled in: `check_values` has already refused a run
/// that leaves one out.
pub fn with_defaults(t: &ApiTemplate, values: &serde_json::Map<String, Value>) -> serde_json::Map<String, Value> {
    let mut out = values.clone();
    for p in &t.params {
        if let (false, Some(d)) = (out.contains_key(&p.name), &p.default) {
            out.insert(p.name.clone(), d.clone());
        }
    }
    out
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
                    problems.push(format!("param '{}' must be {}", param.name, wanted(param.kind)));
                }
            }
            None => {
                if param.required {
                    problems.push(format!("param '{}' is required but missing", param.name));
                } else if param.default.is_none() {
                    // Left out with nothing to stand in for it, its placeholder
                    // would go to the application as the text `{{name}}`.
                    problems.push(format!(
                        "param '{}' is optional but has no default, so a run must give it - or give the template a default: what the application's UI sends when it is left empty",
                        param.name
                    ));
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
