//! Sharing a project's API templates and flows as one JSON file - export on
//! one machine, import on another. Pure: building the file, reading one
//! back, and deciding what an import writes. The disk is the command's
//! (`commands::api_templates::{export_at, import_at}`).
//!
//! ```json
//! { "kind": "tcm-api-templates", "version": 1, "exported_at": "<ISO UTC>",
//!   "templates": [ApiTemplate, ...], "flows": [Flow, ...] }
//! ```
//!
//! Proof never travels: a template goes without `proven` and a flow without
//! `saved`, since both carry what a real record held on the sender's site
//! (the account, captured outputs, a sample subject). Run history
//! (`<id>.runs.json`) is never read at all. No org, project, origin or
//! account is written either - the file says what the operations are, not
//! where they were proven.

use super::flow::{check_flow, check_stage_ref, Flow};
use super::{check, valid_id, ApiTemplate};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

/// What `kind` says in every file this module writes.
pub const KIND: &str = "tcm-api-templates";
/// The newest format this copy of the app reads.
pub const VERSION: u64 = 1;
/// The largest file an import will read: far more than any real project's
/// templates, and small enough that a wrong pick is refused before it is
/// read into memory.
pub const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

pub const TOO_BIG: &str = "that file is larger than 5 MB, so it is not an API templates export";
pub const NOT_JSON: &str = "that file is not readable JSON, so it is not an API templates export";
pub const WRONG_KIND: &str = "that file is not an API templates export";
pub const NEWER: &str = "that file was exported by a newer version of the app - update this copy first";
pub const DAMAGED: &str = "that API templates export is damaged: it has no list of templates and flows";
pub const NOTHING_IN_IT: &str = "that file has no templates or flows in it";
pub const NOTHING_TO_EXPORT: &str = "there are no templates or flows to export in this project";

/// The file as it is written.
#[derive(Debug, Serialize)]
pub struct ShareDoc {
    pub kind: &'static str,
    pub version: u64,
    pub exported_at: String,
    pub templates: Vec<ApiTemplate>,
    pub flows: Vec<Flow>,
}

/// What an export wrote, and how many saved files it could not read (each
/// is in the log, as the tab's own listing logs it).
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct TemplatesExportResult {
    pub templates: u32,
    pub flows: u32,
    pub skipped: u32,
}

/// One entry of the file that was not imported, and why.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct TemplatesImportSkip {
    pub id: String,
    pub reason: String,
}

/// One template that was imported but cannot run as it is - its flow is
/// not saved here, or that flow does not have its stage.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct TemplatesImportNote {
    pub id: String,
    pub title: String,
    pub note: String,
}

/// What an import did. `added` and `replaced` are titles - a flow's marked
/// " (flow)" - flows first, each list in file order. `skipped` names the
/// entry by its id, which is all an entry that does not parse is sure to
/// have.
#[derive(Debug, Clone, Default, PartialEq, Serialize, specta::Type)]
pub struct TemplatesImportResult {
    pub added: Vec<String>,
    pub replaced: Vec<String>,
    pub skipped: Vec<TemplatesImportSkip>,
    pub notes: Vec<TemplatesImportNote>,
}

/// The file for these templates and flows, proof stripped from each.
pub fn build_doc(templates: Vec<ApiTemplate>, flows: Vec<Flow>, exported_at: &str) -> ShareDoc {
    ShareDoc {
        kind: KIND,
        version: VERSION,
        exported_at: exported_at.to_string(),
        templates: templates.into_iter().map(|t| ApiTemplate { proven: None, ..t }).collect(),
        flows: flows.into_iter().map(|f| Flow { saved: None, ..f }).collect(),
    }
}

/// `doc` as the pretty-printed text the file holds.
pub fn to_json(doc: &ShareDoc) -> Result<String, String> {
    serde_json::to_string_pretty(doc).map_err(|e| e.to_string())
}

/// A file's entries, each still raw JSON, so one that does not parse is
/// skipped on its own rather than refusing the whole file.
#[derive(Debug, Clone, Default)]
pub struct ReadDoc {
    pub templates: Vec<Value>,
    pub flows: Vec<Value>,
}

/// Reads an export's text. Refused, each with its own sentence: over
/// `MAX_FILE_BYTES`, not JSON, a different `kind`, a `version` this copy
/// does not know, no lists, or nothing in them.
pub fn read_doc(text: &str) -> Result<ReadDoc, String> {
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err(TOO_BIG.to_string());
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let v: Value = serde_json::from_str(text).map_err(|_| NOT_JSON.to_string())?;
    if v.get("kind").and_then(Value::as_str) != Some(KIND) {
        return Err(WRONG_KIND.to_string());
    }
    match v.get("version").and_then(Value::as_u64) {
        Some(n) if (1..=VERSION).contains(&n) => {}
        Some(n) if n > VERSION => return Err(NEWER.to_string()),
        _ => return Err("that API templates export is damaged: its version is not one this app writes".to_string()),
    }
    let list = |key: &str| -> Result<Vec<Value>, String> {
        match v.get(key) {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::Array(items)) => Ok(items.clone()),
            Some(_) => Err(DAMAGED.to_string()),
        }
    };
    let doc = ReadDoc { templates: list("templates")?, flows: list("flows")? };
    if doc.templates.is_empty() && doc.flows.is_empty() {
        return Err(NOTHING_IN_IT.to_string());
    }
    Ok(doc)
}

/// The id an entry says it has, for a skip line - or a stand-in when it
/// has none.
fn entry_id(v: &Value) -> String {
    match v.get("id").and_then(Value::as_str) {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => "(no id)".to_string(),
    }
}

/// `v` without the app-written `key` (`proven`, `saved`): a file that
/// carries one anyway imports without it rather than being refused.
fn without(v: &Value, key: &str) -> Value {
    let mut v = v.clone();
    if let Value::Object(map) = &mut v {
        map.remove(key);
    }
    v
}

/// The entries of one kind that are fit to write, in file order, with a
/// skip line for every one that is not. `parse` turns a stripped entry into
/// the type or the sentences saying why not; `what` is "flow" or
/// "template", for those sentences. Of several valid entries with one id
/// the last wins, and each earlier one is a skip line.
fn sort_entries<T>(
    entries: &[Value],
    strip: &str,
    what: &str,
    id_of: impl Fn(&T) -> &str,
    parse: impl Fn(&Value) -> Result<T, Vec<String>>,
    skipped: &mut Vec<TemplatesImportSkip>,
) -> Vec<T> {
    let mut valid: Vec<(usize, T)> = Vec::new();
    for (i, raw) in entries.iter().enumerate() {
        let id = entry_id(raw);
        if !raw.get("id").and_then(Value::as_str).is_some_and(valid_id) {
            skipped.push(TemplatesImportSkip {
                id,
                reason: format!(
                    "this {what}'s id is not valid: it must be lowercase ASCII letters, digits, '-' or '_', 1-100 characters long"
                ),
            });
            continue;
        }
        match parse(&without(raw, strip)) {
            Ok(t) => valid.push((i, t)),
            Err(problems) => skipped.push(TemplatesImportSkip {
                id,
                reason: format!("this {what} is not valid: {}", problems.join("; ")),
            }),
        }
    }
    let mut last: HashMap<String, usize> = HashMap::new();
    for (i, t) in &valid {
        last.insert(id_of(t).to_string(), *i);
    }
    let mut out = Vec::new();
    for (i, t) in valid {
        if last.get(id_of(&t)) == Some(&i) {
            out.push(t);
        } else {
            skipped.push(TemplatesImportSkip {
                id: id_of(&t).to_string(),
                reason: format!("a later {what} in the file has the same id, and that one was imported instead"),
            });
        }
    }
    out
}

/// What an import will write: the valid flows and templates, and a skip
/// line for every entry that is not. Never fatal to the rest: one bad entry
/// costs only itself.
#[derive(Debug, Clone, Default)]
pub struct ImportPlan {
    pub flows: Vec<Flow>,
    pub templates: Vec<ApiTemplate>,
    pub skipped: Vec<TemplatesImportSkip>,
}

pub fn plan_import(doc: &ReadDoc) -> ImportPlan {
    let mut skipped = Vec::new();
    let flows = sort_entries(
        &doc.flows,
        "saved",
        "flow",
        |f: &Flow| f.id.as_str(),
        |v| {
            let f: Flow = serde_json::from_value(v.clone()).map_err(|e| vec![e.to_string()])?;
            let problems = check_flow(&f);
            if problems.is_empty() {
                Ok(f)
            } else {
                Err(problems)
            }
        },
        &mut skipped,
    );
    let templates = sort_entries(
        &doc.templates,
        "proven",
        "template",
        |t: &ApiTemplate| t.id.as_str(),
        |v| {
            let t: ApiTemplate = serde_json::from_value(v.clone()).map_err(|e| vec![e.to_string()])?;
            let problems = check(&t);
            if problems.is_empty() {
                Ok(t)
            } else {
                Err(problems)
            }
        },
        &mut skipped,
    );
    ImportPlan { flows, templates, skipped }
}

/// Why an imported template on a flow cannot run as it is, given the flow
/// its `stage` names as it stands once the import's flows are written
/// (`None`: neither in the file nor saved here). Empty when it can - or
/// when it is on no flow. It is imported either way: the run's own check
/// refuses it until the flow is there.
pub fn stage_note(t: &ApiTemplate, flow: Option<&Flow>) -> Option<String> {
    let r = t.stage.as_ref()?;
    match flow {
        None => Some(format!(
            "its flow {} is not in the file or saved here, so it cannot run until that flow is saved",
            r.flow
        )),
        Some(f) => {
            let problems = check_stage_ref(t, Some(f));
            (!problems.is_empty()).then(|| problems.join("; "))
        }
    }
}

/// How a flow is listed in `added` / `replaced`.
pub fn flow_label(f: &Flow) -> String {
    format!("{} (flow)", f.title)
}
