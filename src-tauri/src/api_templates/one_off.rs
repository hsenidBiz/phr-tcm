//! A one-off request an explore session sends from its own page
//! (`send_api_request`): reading the call, the one-step template a run of it
//! uses, and the sentences it refuses with. Nothing here sends anything or
//! touches a file; `ai_bridge::send_in` does the sending.

use super::{ApiTemplate, Antiforgery, Effect, Method, Param, Step};
use serde_json::Value;
use std::collections::BTreeMap;

/// A one-off run's template id, before the call is saved as a template.
pub const ONE_OFF_ID: &str = "one-off";
/// Said to a delete the person has not said yes to in the chat.
pub const DELETE_NEEDS_YES: &str = "Tell the person what will be deleted and wait for their yes in the chat first.";
/// Said to a POST that does not say what it does.
pub const EFFECT_NEEDED: &str = "a POST needs an \"effect\": read, create, edit or delete";
/// Said to a GET that names an effect other than read.
pub const GET_ONLY_READS: &str = "a GET only reads: leave effect out, or send read";
/// Said to a create that does not say what it made.
pub const CREATED_NEEDED: &str = "effect create needs \"created\": { \"kind\", \"id\", \"name\" }, with id the name of one of the step's captures, so the change list holds what it made";
/// Said when there is no discovery to send from.
pub const NO_SESSION_FOR_SEND: &str = "send_api_request runs in your own explore session's browser - start one with start_autorun_discovery and explore: true";
/// Said to a POST from a discovery that is not an explore session.
pub const POST_NEEDS_EXPLORE: &str = "a POST from send_api_request needs an explore session, so what it changes goes on a change list - start the discovery again with explore: true";
/// Said when the session's browser no longer holds the session's account.
pub const SESSION_NOT_SIGNED_IN: &str = "your explore session's browser no longer holds its account - end the session and start it again";

const SHAPE: &str = "{ \"step\": { \"method\": \"GET\" | \"POST\", \"path\": ..., \"query\"?, \"json\"? | \"form\"?, \"capture\"?, \"expect\"? }, \"effect\"?: \"read\" | \"create\" | \"edit\" | \"delete\", \"created\"?: { \"kind\", \"id\", \"name\" }, \"person_said_yes\"?: true, \"params\"?, \"values\"?, \"antiforgery_page\"? }";

/// A `created.id` that names no capture of the step.
pub fn not_a_capture(name: &str) -> String {
    format!("created.id is \"{name}\", which the step does not capture")
}

/// What to do after an edit that was sent.
pub fn edit_reminder(request: &str) -> String {
    format!(
        "Record the value it had before with record_explore_change (change \"changed\", with before, after and request \"{request}\"), if you have not already."
    )
}

/// What to do after a delete that was sent.
pub fn delete_reminder(request: &str) -> String {
    format!(
        "Record what you deleted with record_explore_change (change \"deleted\", with what it held and request \"{request}\")."
    )
}

/// What a create made, as the call says it: `id` is the NAME of one of the
/// step's captures, whose value the response gave.
#[derive(Debug, Clone, PartialEq)]
pub struct Created {
    pub kind: String,
    pub id: String,
    pub name: String,
}

/// One call, read.
#[derive(Debug, Clone, PartialEq)]
pub struct OneOff {
    pub step: Step,
    pub effect: Effect,
    pub params: Vec<Param>,
    pub values: serde_json::Map<String, Value>,
    /// Only for a create.
    pub created: Option<Created>,
    pub person_said_yes: bool,
    pub antiforgery_page: Option<String>,
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub module: Option<String>,
}

fn bad<T>(why: impl Into<String>) -> Result<T, (u16, String)> {
    Err((400, why.into()))
}

/// A text field of the body, trimmed; blank or not text is none.
fn text(obj: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    obj.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// Reads a `send_api_request` body: `{ step (the template step format, its
/// "name" defaulting to "Request"), effect?, created?, person_said_yes?,
/// params?, values?, antiforgery_page?, id?, name?, description?, module? }`.
/// Every refusal that needs nothing but the body is here, so none of them
/// waits for the session.
pub fn parse(body: &str) -> Result<OneOff, (u16, String)> {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return bad(format!("that is not readable JSON: {e}. Expected {SHAPE}.")),
    };
    let Some(obj) = v.as_object() else {
        return bad(format!("that is not an object. Expected {SHAPE}."));
    };
    let mut raw = match obj.get("step") {
        Some(Value::Object(m)) => m.clone(),
        _ => return bad(format!("this call needs a \"step\". Expected {SHAPE}.")),
    };
    if raw.get("name").is_none_or(Value::is_null) {
        raw.insert("name".into(), Value::String("Request".into()));
    }
    if let Some(Value::String(m)) = raw.get("method") {
        let upper = m.trim().to_ascii_uppercase();
        raw.insert("method".into(), Value::String(upper));
    }
    let step: Step = match serde_json::from_value(Value::Object(raw)) {
        Ok(s) => s,
        Err(e) => return bad(format!("that is not a step: {e}. Expected {SHAPE}.")),
    };

    let said = match obj.get("effect") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => match serde_json::from_value::<Effect>(Value::String(s.trim().to_ascii_lowercase())) {
            Ok(e) => Some(e),
            Err(_) => return bad(format!("effect is read, create, edit or delete, not \"{}\"", s.trim())),
        },
        Some(_) => return bad("effect is read, create, edit or delete"),
    };
    let effect = match (step.method, said) {
        (Method::Get, None) => Effect::Read,
        (Method::Get, Some(Effect::Read)) => Effect::Read,
        (Method::Get, Some(_)) => return bad(GET_ONLY_READS),
        (Method::Post, None) => return bad(EFFECT_NEEDED),
        (Method::Post, Some(e)) => e,
    };

    let person_said_yes = obj.get("person_said_yes").and_then(Value::as_bool) == Some(true);
    if effect == Effect::Delete && !person_said_yes {
        return bad(DELETE_NEEDS_YES);
    }

    let created = if effect == Effect::Create {
        let made = obj.get("created").and_then(Value::as_object).and_then(|c| {
            Some(Created { kind: text(c, "kind")?, id: text(c, "id")?, name: text(c, "name")? })
        });
        let Some(made) = made else { return bad(CREATED_NEEDED) };
        if !step.capture.contains_key(&made.id) {
            return bad(not_a_capture(&made.id));
        }
        Some(made)
    } else {
        None
    };

    let params: Vec<Param> = match obj.get("params") {
        None | Some(Value::Null) => Vec::new(),
        Some(p) => match serde_json::from_value(p.clone()) {
            Ok(p) => p,
            Err(e) => return bad(format!("those are not params: {e}. Expected {SHAPE}.")),
        },
    };
    let values = match obj.get("values") {
        None | Some(Value::Null) => serde_json::Map::new(),
        Some(Value::Object(m)) => m.clone(),
        Some(_) => return bad(format!("values is an object of param names to values. Expected {SHAPE}.")),
    };

    Ok(OneOff {
        step,
        effect,
        params,
        values,
        created,
        person_said_yes,
        antiforgery_page: text(obj, "antiforgery_page"),
        id: text(obj, "id"),
        name: text(obj, "name"),
        description: text(obj, "description"),
        module: text(obj, "module"),
    })
}

/// The one-step template a run of `o` uses: `id`, the call's effect,
/// params and step, its capture names as outputs, and `token_page` as the
/// page the anti-forgery token is read on.
pub fn as_template(o: &OneOff, id: &str, token_page: &str) -> ApiTemplate {
    ApiTemplate {
        id: id.to_string(),
        title: o.name.clone().unwrap_or_else(|| "A one-off request".to_string()),
        module: o.module.clone().unwrap_or_default(),
        effect: o.effect,
        description: o.description.clone().unwrap_or_else(|| "A call made while exploring.".to_string()),
        sources: Vec::new(),
        antiforgery: Antiforgery { page: token_page.to_string() },
        params: o.params.clone(),
        steps: vec![o.step.clone()],
        outputs: o.step.capture.keys().cloned().collect(),
        stage: None,
        deletes_kind: None,
        proven: None,
    }
}

/// The step's path with the call's values filled in (its params' defaults
/// too); a placeholder naming a capture stays as written.
pub fn filled_path(o: &OneOff) -> String {
    let t = as_template(o, ONE_OFF_ID, "/");
    let vars: BTreeMap<String, Value> = super::with_defaults(&t, &o.values).into_iter().collect();
    super::exec::substitute_str(&o.step.path, &vars)
}

/// The call as the change list names a request: `POST /hr/x`.
pub fn request_of(o: &OneOff) -> String {
    let method = match o.step.method {
        Method::Get => "GET",
        Method::Post => "POST",
    };
    crate::autorun::explore::request_line(method, &filled_path(o))
}
