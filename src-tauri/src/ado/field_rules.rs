//! The project's Test Case field rules, and the check a queue gets against
//! them before anything is sent.
//!
//! Azure DevOps refuses a Test Case that leaves a required field empty (a
//! process with a required Module, say) with a 400 per case. Asking first
//! turns 64 identical refusals into one sentence above the queue that says
//! what to fill in. Read only: two GETs, remembered for the session.
//!
//! The check is deliberately limited to the fields this app WRITES (tags, and
//! the module and prerequisites fields it was told about). Another required
//! field is one Azure DevOps may fill by a rule this listing does not show,
//! and a check that blocked on it would stop an upload that would have
//! worked. Whatever it cannot judge is left to the refusal itself, which
//! `rule_errors` turns into a sentence.

use super::endpoints::percent_encode_segment;
use super::{AdoClient, AdoError};
use serde::{Deserialize, Serialize};

/// One field of the Test Case type, as the project defines it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct FieldRule {
    pub reference_name: String,
    /// The display name, as Azure DevOps shows it.
    pub name: String,
    /// Always required, and with no default Azure DevOps could fill in.
    pub required: bool,
    /// The fixed list, when the field has one. Empty means any value.
    pub allowed_values: Vec<String>,
}

/// One thing wrong with one queued case.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct RuleProblem {
    /// Position in the queue that was checked.
    pub index: u32,
    pub reference_name: String,
    /// The field's display name, never only its reference name.
    pub field: String,
    /// "required" or "not_allowed".
    pub kind: String,
    /// A sentence fit for the row.
    pub message: String,
}

/// What the check found. `checked` is false when the rules could not be
/// read: the queue is then sent as it always was and the screen says so once.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct RuleCheck {
    pub checked: bool,
    pub problems: Vec<RuleProblem>,
}

impl RuleCheck {
    pub fn unchecked() -> Self {
        RuleCheck { checked: false, problems: Vec::new() }
    }
}

/// How many allowed values a sentence lists before it says "...".
const LISTED_VALUES: usize = 6;

impl AdoClient {
    /// The Test Case type's fields with what each requires. `pick_refs` are
    /// fields whose own entry may omit its list (a custom picklist does until
    /// asked by name): their allowed values are read separately, and a read
    /// that fails just leaves that field without a list to check against.
    pub async fn test_case_field_rules(
        &self,
        organization: &str,
        project: &str,
        pick_refs: &[&str],
    ) -> Result<Vec<FieldRule>, AdoError> {
        let key = crate::cache::keys::test_case_rules(&self.base_url, organization, project, &pick_refs.join(","));
        if let Some(hit) = crate::cache::session_fresh::<Vec<FieldRule>>(&key, crate::cache::keys::TEST_CASE_RULES_TTL) {
            return Ok(hit);
        }
        let url = format!(
            "{}/{}/{}/_apis/wit/workitemtypes/Test%20Case/fields?$expand=all&api-version=7.1",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project)
        );
        let listing = self.get_json(url).await?;
        let has_default = |f: &serde_json::Value| match &f["defaultValue"] {
            serde_json::Value::Null => false,
            serde_json::Value::String(s) => !s.is_empty(),
            _ => true,
        };
        let mut rules: Vec<FieldRule> = Vec::new();
        for f in listing["value"].as_array().into_iter().flatten() {
            let Some(r) = f["referenceName"].as_str() else { continue };
            rules.push(FieldRule {
                reference_name: r.to_string(),
                name: f["name"].as_str().filter(|n| !n.is_empty()).unwrap_or(r).to_string(),
                required: f["alwaysRequired"].as_bool() == Some(true) && !has_default(f),
                allowed_values: f["allowedValues"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            });
        }
        for r in pick_refs {
            if let Some(rule) = rules.iter_mut().find(|x| x.reference_name == *r && x.allowed_values.is_empty()) {
                match self.get_field_allowed_values(organization, project, "Test Case", r).await {
                    Ok(list) => rule.allowed_values = list,
                    Err(e) => crate::applog::warn(format!("could not read the allowed values of {r}: {e}")),
                }
            }
        }
        crate::cache::session_put(&key, rules.clone());
        Ok(rules)
    }
}

/// What is wrong with one case against the rules. Pure. An UPDATE row is
/// only checked for a value outside a fixed list: its blank fields are not
/// written, so they cannot be refused. `module_ref` and
/// `preconditions_ref` are the fields the app writes the case's module and
/// prerequisites into; a case with a blank value writes nothing for them.
pub fn check_case(
    index: usize,
    tc: &crate::model::TestCase,
    rules: &[FieldRule],
    module_ref: Option<&str>,
    preconditions_ref: Option<&str>,
) -> Vec<RuleProblem> {
    let fields: [(Option<&str>, &str); 3] = [
        (Some("System.Tags"), tc.tags.as_str()),
        (module_ref, tc.module_value.as_str()),
        (preconditions_ref, tc.preconditions.as_str()),
    ];
    let mut out = Vec::new();
    for (reference, value) in fields {
        let Some(reference) = reference else { continue };
        let Some(rule) = rules.iter().find(|r| r.reference_name == reference) else { continue };
        let value = value.trim();
        let problem = |kind: &str, message: String| RuleProblem {
            index: index as u32,
            reference_name: rule.reference_name.clone(),
            field: rule.name.clone(),
            kind: kind.to_string(),
            message,
        };
        let pick = !rule.allowed_values.is_empty() || Some(reference) == module_ref;
        if value.is_empty() {
            // An update leaves a blank field as Azure DevOps holds it (an
            // import never erases), so only a create can send it empty.
            if rule.required && tc.update_id.is_none() {
                out.push(problem(
                    "required",
                    format!(
                        "{} is required for a Test Case in this project - {}.",
                        rule.name,
                        if pick { "pick one" } else { "fill it in" }
                    ),
                ));
            }
        } else if !rule.allowed_values.is_empty()
            && !rule.allowed_values.iter().any(|a| a.trim().eq_ignore_ascii_case(value))
        {
            let shown: Vec<&str> = rule.allowed_values.iter().take(LISTED_VALUES).map(String::as_str).collect();
            let more = if rule.allowed_values.len() > LISTED_VALUES { ", ..." } else { "" };
            out.push(problem(
                "not_allowed",
                format!(
                    "{} \"{}\" is not one of the allowed values for a Test Case in this project - pick one of: {}{}.",
                    rule.name,
                    value,
                    shown.join(", "),
                    more
                ),
            ));
        }
    }
    out
}

/// Check a whole queue. Fails OPEN: when the rules cannot be read (network,
/// permission, an unexpected answer) nothing is blocked - the result is
/// `unchecked`, and the log says why.
pub async fn check_queue(
    client: &AdoClient,
    organization: &str,
    project: &str,
    queue: &[crate::model::TestCase],
    module_ref: Option<&str>,
    preconditions_ref: Option<&str>,
) -> RuleCheck {
    let refs: Vec<&str> = [module_ref, preconditions_ref].into_iter().flatten().collect();
    let rules = match client.test_case_field_rules(organization, project, &refs).await {
        Ok(r) if !r.is_empty() => r,
        Ok(_) => return RuleCheck::unchecked(),
        Err(e) => {
            crate::applog::warn(format!("could not read the Test Case field rules, so nothing was checked ({e})"));
            return RuleCheck::unchecked();
        }
    };
    let problems = queue
        .iter()
        .enumerate()
        .flat_map(|(i, tc)| check_case(i, tc, &rules, module_ref, preconditions_ref))
        .collect();
    RuleCheck { checked: true, problems }
}
