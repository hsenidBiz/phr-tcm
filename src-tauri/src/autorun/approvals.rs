//! A person's approval of a script's setup: the design doc's "Approval"
//! (section 2).
//!
//! A setup writes data in the application under test on every run of its
//! case, so it runs only once a person has approved it in the script
//! editor. What they approved is a fingerprint: SHA-256 over the setup
//! together with its fixture's `steps`, `outputs`, `creates` and
//! `account`, and each step's template body (without its `proven` block,
//! which a re-prove changes and which says nothing about what the template
//! sends). When any of these changes, the fingerprint no longer matches
//! and the approval no longer counts.
//!
//! Approvals live in the Auto Run store at `approvals/<case id>.json`,
//! never in the script file, so no script save can carry one. Only the
//! webview's three setup commands (`commands::autorun_setup`) approve or
//! withdraw: no bridge route and no MCP tool reaches them, so an assistant
//! can never approve its own setup.

use super::Setup;
use crate::api_templates::fixture::Fixture;
use crate::api_templates::ApiTemplate;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// One approval as it is kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stored {
    pub fingerprint: String,
    /// When the person approved it, local time `YYYY-MM-DD HH:MM:SS`.
    pub at: String,
}

/// Where a case's setup stands against what is saved now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Approval {
    /// Approved at `at`, and nothing has changed since.
    Approved { at: String },
    /// Approved once, but the setup, its fixture or a template changed.
    Changed,
    /// Never approved, or withdrawn.
    None,
}

impl Approval {
    /// How the script editor names it: `approved`, `changed` or `none`.
    pub fn word(&self) -> &'static str {
        match self {
            Approval::Approved { .. } => "approved",
            Approval::Changed => "changed",
            Approval::None => "none",
        }
    }
}

/// `v` with every object's keys in sorted order, all the way down: the
/// crate keeps JSON keys in insertion order, and a fingerprint must not
/// depend on the order a file happened to list them in.
fn canonical(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut out = serde_json::Map::with_capacity(map.len());
            for k in keys {
                out.insert(k.clone(), canonical(&map[k]));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

/// A template's body as the fingerprint takes it: everything but `proven`.
/// A template that is no longer saved is `null`.
fn body(t: Option<&ApiTemplate>) -> Value {
    let Some(t) = t else { return Value::Null };
    let mut v = serde_json::to_value(t).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut v {
        map.remove("proven");
    }
    v
}

/// The fingerprint of a setup: SHA-256, as lowercase hex, over the
/// canonical JSON (sorted keys) of `{ setup, fixture: { steps, outputs,
/// creates, account }, templates: [each step's template body without its
/// proven block, in step order] }`. `templates` holds one entry per
/// fixture step, `None` for a template that is not saved.
pub fn fingerprint(setup: &Setup, fixture: &Fixture, templates: &[Option<ApiTemplate>]) -> String {
    let doc = json!({
        "setup": setup,
        "fixture": {
            "steps": fixture.steps,
            "outputs": fixture.outputs,
            "creates": fixture.creates,
            "account": fixture.account,
        },
        "templates": templates.iter().map(Option::as_ref).map(body).collect::<Vec<_>>(),
    });
    let text = serde_json::to_string(&canonical(&doc)).unwrap_or_default();
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn dir(root: &Path) -> PathBuf {
    root.join("approvals")
}

fn path(root: &Path, case_id: i32) -> PathBuf {
    dir(root).join(format!("{case_id}.json"))
}

/// The approval kept for `case_id`, if any. One that does not read is
/// logged and counts as none.
fn load(root: &Path, case_id: i32) -> Option<Stored> {
    match std::fs::read_to_string(path(root, case_id)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            match serde_json::from_str(s) {
                Ok(a) => Some(a),
                Err(e) => {
                    crate::applog::warn(format!("the setup approval for case {case_id} does not read: {e}"));
                    None
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            crate::applog::warn(format!("the setup approval for case {case_id} could not be read: {e}"));
            None
        }
    }
}

/// Where case `case_id`'s setup stands against `fp`, its fingerprint now.
pub fn state(root: &Path, case_id: i32, fp: &str) -> Approval {
    match load(root, case_id) {
        None => Approval::None,
        Some(a) if a.fingerprint == fp => Approval::Approved { at: a.at },
        Some(_) => Approval::Changed,
    }
}

/// Records the person's approval of `fp` for `case_id`, replacing any
/// earlier one, and gives back when. Only the webview's
/// `auto_run_approve_setup` calls this.
pub fn approve(root: &Path, case_id: i32, fp: &str) -> Result<String, String> {
    let at = crate::applog::stamp();
    let stored = Stored { fingerprint: fp.to_string(), at: at.clone() };
    std::fs::create_dir_all(dir(root)).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&stored).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&path(root, case_id), &text)?;
    Ok(at)
}

/// Withdraws case `case_id`'s approval. One that is not there is already
/// withdrawn. Only the webview's `auto_run_withdraw_setup` calls this.
pub fn withdraw(root: &Path, case_id: i32) -> Result<(), String> {
    match std::fs::remove_file(path(root, case_id)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
