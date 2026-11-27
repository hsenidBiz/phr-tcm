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

// ---- One parser for every create or update of a Test Case ------------------
//
// A queue submit (`$batch`), an editor save and a single create all end in
// the same Azure DevOps 400, and each used to show its own version of "HTTP
// 400" while the reason sat in the log. `plain_reason` is the one place that
// turns the body into a sentence, so the three cannot drift apart.

/// A key of a JSON object, whatever its capitalisation: a `$batch` item
/// spells it `ruleValidationErrors`, the single-item endpoint
/// `customProperties.RuleValidationErrors`.
fn key_ci<'a>(v: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    v.as_object()
        .and_then(|o| o.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v))
        .unwrap_or(&serde_json::Value::Null)
}

/// `text` with every URL and anything naming an Azure DevOps host taken
/// out, and its whitespace folded: a reason is shown to a person and
/// pasted into bug reports, and a URL names an organization.
pub fn strip_urls(text: &str) -> String {
    static URL: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)\S*(?:://|www\.|dev\.azure\.com|visualstudio\.com)\S*").unwrap()
    });
    let cleaned = URL.replace_all(text, "");
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "Custom.TestType" -> "Test Type": the name to show when neither the
/// project's field list nor Azure DevOps' own message gave one.
fn humanise(reference_name: &str) -> String {
    static GLUED: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"([a-z0-9])([A-Z])").unwrap());
    let last = reference_name.rsplit('.').next().unwrap_or(reference_name);
    GLUED.replace_all(last, "$1 $2").replace('_', " ")
}

/// The field name in "Rule Error for field Module. Error code: ..." - the
/// display name, as the project spells it.
fn named_in_message(message: &str) -> Option<String> {
    static NAMED: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?i)rule error for field ([^.]+?)\.(?:\s|$)").unwrap());
    NAMED
        .captures(message)
        .map(|c| c[1].trim().to_string())
        .filter(|n| !n.is_empty() && n.chars().count() <= 80)
}

/// The words after "Error code:" - what Azure DevOps lists for the field.
fn codes_in_message(message: &str) -> String {
    message.split_once("Error code:").map(|(_, rest)| rest.to_string()).unwrap_or_default()
}

/// Display names by reference name, from the project's Test Case field list.
pub type FieldNames = std::collections::HashMap<String, String>;

/// One sentence per field Azure DevOps refused in `body`, in its order.
/// Empty when the body names no field. `names` gives each field's display
/// name when the caller has the project's list; without one the name comes
/// from Azure DevOps' own message, then from the reference name.
pub fn field_reasons(body: &str, names: &FieldNames) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    if !v.is_object() {
        return Vec::new();
    }
    let props = key_ci(&v, "customProperties");
    let listed = [key_ci(&v, "ruleValidationErrors"), key_ci(props, "ruleValidationErrors")]
        .into_iter()
        .find_map(|l| l.as_array().filter(|a| !a.is_empty()));
    let top = ["errorMessage", "message"]
        .iter()
        .find_map(|k| key_ci(&v, k).as_str().filter(|m| !m.trim().is_empty()))
        .unwrap_or_default();
    let mut out = Vec::new();
    for e in listed.into_iter().flatten() {
        let Some(r) = key_ci(e, "fieldReferenceName").as_str().filter(|r| is_plain_ref(r)) else {
            continue;
        };
        let own = ["errorMessage", "message"]
            .iter()
            .find_map(|k| key_ci(e, k).as_str().filter(|m| !m.trim().is_empty()))
            .unwrap_or_default();
        // A lone error may be described by the top-level message; with
        // several, only an error's own message belongs to it.
        let said = if own.is_empty() && listed.is_some_and(|l| l.len() == 1) { top } else { own };
        let field = names
            .get(r)
            .cloned()
            .or_else(|| named_in_message(said))
            .unwrap_or_else(|| humanise(r));
        let flags = format!("{} {}", key_ci(e, "fieldStatusFlags").as_str().unwrap_or_default(), codes_in_message(said))
            .to_ascii_lowercase();
        out.push(if flags.contains("required") || flags.contains("invalidempty") {
            format!("{field} is required")
        } else if flags.contains("invalidlistvalue") || flags.contains("limitedtovalues") {
            format!("{field} must be one of the allowed values")
        } else {
            let why = strip_urls(RULE_PREFIX.replace(&TF_CODE.replace(said, ""), "").trim());
            let why: String = why.chars().take(160).collect();
            let why = if why.is_empty() { "it did not accept the value".to_string() } else { why };
            format!("Azure DevOps refused {field}: {why}")
        });
    }
    out
}

static RULE_PREFIX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)^\s*rule error for field [^.]+\.\s*").unwrap());

static TF_CODE: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"^\s*(?:TF|VS)\d+:\s*").unwrap());

/// The sentence for a refused create or update, or `None` when `body`
/// is not a JSON error Azure DevOps explained: the caller then keeps its own
/// generic wording. Several rule errors read as one sentence each, joined
/// with "; ". A body that names no field falls back to its top-level
/// message with URLs removed; one that points at a field this project does
/// not have says so.
pub fn plain_reason(body: &str, names: &FieldNames) -> Option<String> {
    let reasons = field_reasons(body, names);
    if !reasons.is_empty() {
        return Some(reasons.join("; "));
    }
    let v = serde_json::from_str::<serde_json::Value>(body).ok().filter(|v| v.is_object())?;
    let said = ["errorMessage", "message"]
        .iter()
        .find_map(|k| key_ci(&v, k).as_str().filter(|m| !m.trim().is_empty()))?;
    static UNKNOWN_FIELD: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^\s*(?:TF26027|TF51005)\b").unwrap());
    if UNKNOWN_FIELD.is_match(said) {
        static NAMED: LazyLock<regex::Regex> =
            LazyLock::new(|| regex::Regex::new(r"[A-Za-z][A-Za-z0-9_]*(?:\.[A-Za-z0-9_]+)+").unwrap());
        return Some(match NAMED.find(said) {
            Some(m) => format!("{} is not a field in this project", humanise(m.as_str())),
            None => "A field this test case sets does not exist in this project".to_string(),
        });
    }
    let text: String = strip_urls(said).chars().take(300).collect();
    (!text.is_empty()).then_some(text)
}
