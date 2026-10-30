//! The parameter-prompt surface — what a front end needs to prompt a user for the report's
//! parameters before a render (the saved-data preview's `params` and the live preview's
//! parameter values both key on these).
//!
//! Pure extraction from the already-decoded model: the MAIN report's declared parameters
//! (a stored-proc report's `@`-named inputs), their kinds (`Number`/`String`/… — the value
//! kinds [`crate::preview::coerce_params`] parses), prompt text, and pick-list/current
//! values. Subreport `Pm-` parameters are excluded: they are bound by subreport links, not
//! prompted.

use rpt_reader::model::Report;
use serde::{Deserialize, Serialize};

/// One promptable value in a pick list: the value's text plus its pick-list description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterValueInfo {
    /// The value in the form the parameter system stores (e.g. `"42"`).
    pub value: String,
    /// The pick-list description shown beside it, when one is stored.
    pub description: Option<String>,
}

/// One declared parameter, shaped for a prompt form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterInfo {
    /// The parameter's name — the key `coerce_params`/the live preview match on (with or
    /// without the `@`, case-insensitively).
    pub name: String,
    /// The value kind: `Number`, `String`, `Currency`, `Boolean`, `Date`, `Time`, `DateTime`.
    pub value_kind: String,
    /// The prompt text the designer authored for it.
    pub prompt_text: Option<String>,
    /// The prompt may be skipped.
    pub optional_prompt: bool,
    /// More than one value may be supplied.
    pub allow_multiple_values: bool,
    /// A null (empty) selection is allowed.
    pub allow_null_value: bool,
    /// A saved current value exists (its contents are in `current_values`).
    pub has_current_value: bool,
    /// The authored pick list, in stored order.
    pub default_values: Vec<ParameterValueInfo>,
    /// The saved last-used values, when the report was saved with them.
    pub current_values: Vec<ParameterValueInfo>,
}

/// Extract the main report's declared parameters, in declaration order.
pub fn report_parameters(report: &Report) -> Vec<ParameterInfo> {
    report
        .data_definition
        .parameter_fields()
        .map(|(f, p)| ParameterInfo {
            name: f.name.clone(),
            //  yields the variant name (); the prompt form
            // wants the bare kind, matching the value kind coerce_params parses.
            value_kind: p
                .value_kind
                .sdk_name()
                .trim_end_matches("Parameter")
                .to_string(),
            prompt_text: p.prompt_text.clone(),
            optional_prompt: p.optional_prompt,
            allow_multiple_values: p.allow_multiple_values,
            allow_null_value: p.allow_null_value,
            has_current_value: p.has_current_value,
            default_values: values_of(&p.default_values),
            current_values: values_of(&p.current_values),
        })
        .collect()
}

fn values_of(vs: &[rpt_reader::model::ParameterValue]) -> Vec<ParameterValueInfo> {
    vs.iter()
        .map(|v| ParameterValueInfo { value: v.value.clone(), description: v.description.clone() })
        .collect()
}
