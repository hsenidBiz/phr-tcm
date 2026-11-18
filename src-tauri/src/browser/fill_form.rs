//! `fill_form`: match one label to one visible form control and choose the
//! action that sets it. Discovery only - the actions it plans are what a
//! script would keep, never the step itself.

use super::actions::Action;
use super::cdp::{CdpError, Driver};
use super::locator::{resolve, LocatorStep, Target};
use super::page;
use crate::autorun::action_help::FieldValue;
use serde_json::json;

/// The roles a label may name.
pub const FORM_ROLES: [&str; 8] =
    ["textbox", "searchbox", "spinbutton", "combobox", "listbox", "checkbox", "radio", "switch"];

/// What one field comes to.
#[derive(Debug, Clone, PartialEq)]
pub enum Planned {
    /// The action to run, the locator it was written with, and how many
    /// controls the label matched (always 1 here).
    Act { action: Action, locator: Target, matched: usize },
    /// The control already holds what was asked.
    AlreadySet,
    Failed(String),
}

/// `this` is the control: what it is, as the plan needs it.
const KIND_JS: &str = r#"function() {
  const tag = this.tagName.toLowerCase();
  return {
    tag: tag,
    type: (this.getAttribute('type') || '').toLowerCase(),
    editable: this.isContentEditable === true,
    checked: this.checked === true || this.getAttribute('aria-checked') === 'true',
  };
}"#;

const TEXT_TYPES: [&str; 11] =
    ["", "text", "email", "number", "tel", "url", "search", "date", "time", "datetime-local", "month"];

fn step(role: &str, label: &str, exact: bool) -> LocatorStep {
    LocatorStep { role: Some(role.to_string()), name: Some(label.to_string()), exact, ..LocatorStep::default() }
}

fn chained(within: Option<&Target>, last: LocatorStep) -> Target {
    match within {
        None => Target::One(last),
        Some(w) => {
            let mut links = w.links();
            links.push(last);
            Target::Chain(links)
        }
    }
}

/// The `within` as it will be written, and how many visible elements it
/// matched: one is kept as given, several are narrowed to the first
/// (`nth: 0` on the last link). The inner `Err` says why it cannot be used.
pub async fn within_target<D: Driver>(
    d: &mut D,
    within: &Target,
) -> Result<Result<(Target, usize), String>, CdpError> {
    if within.is_legacy() {
        return Ok(Err("within must be a role/name locator".to_string()));
    }
    let count = resolve(d, within).await?.len();
    if count == 0 {
        return Ok(Err(format!("within matches no visible element ({})", within.describe())));
    }
    if count == 1 {
        return Ok(Ok((within.clone(), 1)));
    }
    let first = |s: &LocatorStep| LocatorStep { nth: Some(0), ..s.clone() };
    let narrowed = match within {
        Target::One(s) => Target::One(first(s)),
        Target::Chain(v) => {
            let mut v = v.clone();
            if let Some(last) = v.last_mut() {
                *last = first(last);
            }
            Target::Chain(v)
        }
        Target::Legacy(_) => unreachable!("refused above"),
    };
    Ok(Ok((narrowed, count)))
}

/// Visible matches of the label across every form role, as (role, handle).
async fn matches<D: Driver>(
    d: &mut D,
    within: Option<&Target>,
    label: &str,
    exact: bool,
) -> Result<Vec<(&'static str, page::Handle)>, CdpError> {
    let mut found = vec![];
    for role in FORM_ROLES {
        for handle in resolve(d, &chained(within, step(role, label, exact))).await? {
            found.push((role, handle));
        }
    }
    Ok(found)
}

/// Pick the action for one label and value.
pub async fn plan_field<D: Driver>(
    d: &mut D,
    within: Option<&Target>,
    label: &str,
    value: &FieldValue,
) -> Result<Planned, CdpError> {
    let mut exact = true;
    let mut found = matches(d, within, label, true).await?;
    if found.is_empty() {
        exact = false;
        found = matches(d, within, label, false).await?;
    }
    let (role, handle) = match found.len() {
        0 => return Ok(Planned::Failed(format!("no form control is named \"{label}\""))),
        1 => found.remove(0),
        n => {
            let mut roles: Vec<&str> = found.iter().map(|(r, _)| *r).collect();
            roles.dedup();
            return Ok(Planned::Failed(format!(
                "{n} form controls match \"{label}\" ({}): name one more exactly",
                roles.join(", ")
            )));
        }
    };
    let info = page::call_value(d, &handle, KIND_JS, &[]).await?;
    let tag = info["tag"].as_str().unwrap_or("");
    let kind = info["type"].as_str().unwrap_or("");
    let editable = info["editable"].as_bool().unwrap_or(false);
    let checked = info["checked"].as_bool().unwrap_or(false);
    let locator = chained(within, step(role, label, exact));
    let selector = locator.clone();
    let act = |action: Action| Planned::Act { action, locator: selector.clone(), matched: 1 };
    let takes = |what: &str| Planned::Failed(format!("\"{label}\" is a {role}: it takes {what}"));

    if tag == "input" && kind == "password" {
        return Ok(Planned::Failed("passwords are never typed by an assistant".to_string()));
    }
    if tag == "select" {
        return Ok(match value {
            FieldValue::Text(t) => act(Action::SelectOption {
                selector: locator.clone(),
                label: Some(t.clone()),
                value: None,
                within_ms: None,
            }),
            FieldValue::Bool(_) => takes("an option's text, not true or false"),
        });
    }
    if matches!(role, "combobox" | "listbox") {
        return Ok(Planned::Failed(format!(
            "\"{label}\" is a {role} that is not a native select: use a saved component or explicit actions"
        )));
    }
    if matches!(role, "checkbox" | "switch" | "radio") {
        return Ok(match value {
            FieldValue::Text(_) => takes("true or false, not text"),
            FieldValue::Bool(false) if role == "radio" => {
                Planned::Failed(format!("\"{label}\": a radio is turned off by choosing another"))
            }
            FieldValue::Bool(want) if checked == *want => Planned::AlreadySet,
            FieldValue::Bool(_) => act(Action::Click { selector: locator.clone() }),
        });
    }
    let text_like = editable || tag == "textarea" || (tag == "input" && TEXT_TYPES.contains(&kind));
    if text_like {
        return Ok(match value {
            FieldValue::Text(t) => act(Action::Fill { selector: locator.clone(), value: t.clone() }),
            FieldValue::Bool(_) => takes("text, not true or false"),
        });
    }
    Ok(Planned::Failed(format!(
        "\"{label}\" is a {role} this step cannot fill: use a saved component or explicit actions"
    )))
}
