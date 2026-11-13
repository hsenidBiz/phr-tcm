//! The fields a work-item rule rejection names.
//!
//! Azure DevOps answers a create that breaks the process rules with a 400
//! whose `customProperties.RuleValidationErrors` lists EVERY field that
//! failed, while the top-level message names only the first. A tester filing
//! a bug needs the whole list at once, not one field per attempt.

use std::sync::LazyLock;

/// One field a rule rejected, with a sentence fit to show a person.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct FieldViolation {
    pub reference_name: String,
    pub message: String,
}

/// A plain dotted reference name (`Custom.BugType`) and nothing else, so a
/// value, a path or a URL can never ride along as a "field".
pub fn is_plain_ref(r: &str) -> bool {
    static REF: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"^[A-Za-z][A-Za-z0-9_]*(\.[A-Za-z0-9_]+)+$").unwrap()
    });
    REF.is_match(r)
}

/// The sentence for a set of status flags, used when ADO's own message is
/// not fit to show.
fn flag_sentence(flags: &str) -> &'static str {
    let f = flags.to_ascii_lowercase();
    if f.contains("required") || f.contains("invalidempty") {
        "This field is required."
    } else if f.contains("limitedtovalues") || f.contains("invalidlistvalue") {
        "Pick one of the allowed values."
    } else {
        "Azure DevOps did not accept this value."
    }
}

fn message_for(raw: &str, flags: &str) -> String {
    static CODE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^\s*TF\d+:\s*").unwrap());
    let stripped = CODE.replace(raw, "");
    let text = stripped.trim();
    let lower = text.to_ascii_lowercase();
    if text.is_empty() || lower.contains("://") || lower.contains("http") {
        return flag_sentence(flags).to_string();
    }
    text.chars().take(200).collect()
}

/// Every field a TF401320 / RuleValidationException body names, in the
/// order ADO lists them. Empty when the body is not a rule error.
pub fn rule_violations(body: &str) -> Vec<FieldViolation> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let props = &v["customProperties"];
    let mut out = Vec::new();
    for e in props["RuleValidationErrors"].as_array().into_iter().flatten() {
        let Some(r) = e["fieldReferenceName"].as_str().filter(|r| is_plain_ref(r)) else {
            continue;
        };
        let flags = e["fieldStatusFlags"].as_str().unwrap_or_default();
        out.push(FieldViolation {
            reference_name: r.to_string(),
            message: message_for(e["errorMessage"].as_str().unwrap_or_default(), flags),
        });
    }
    if out.is_empty() {
        if let Some(r) = props["FieldReferenceName"].as_str().filter(|r| is_plain_ref(r)) {
            let flags = props["FieldStatusFlags"].as_str().unwrap_or_default();
            out.push(FieldViolation {
                reference_name: r.to_string(),
                message: message_for(props["ErrorMessage"].as_str().unwrap_or_default(), flags),
            });
        }
    }
    out
}
