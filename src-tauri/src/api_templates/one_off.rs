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
/// Said to a save sent as anything but a delete whose address (or `handler`)
/// says delete or remove.
pub const LOOKS_LIKE_A_DELETE: &str = "This call looks like a delete: send it with effect delete and the person's yes.";
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

    let one = OneOff {
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
    };
    // A save whose address says delete or remove is a delete, whatever label
    // it was sent with: the label is what asks for the person's yes. (A GET
    // cannot be sent as a delete, so only a POST is judged.)
    if one.step.method == Method::Post && one.effect != Effect::Delete {
        let address = request_path(&one).to_ascii_lowercase();
        if address.contains("delete") || address.contains("remove") {
            return bad(LOOKS_LIKE_A_DELETE);
        }
    }
    Ok(one)
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

/// The values the call sends: its own, and its params' defaults.
fn sent_vars(o: &OneOff) -> BTreeMap<String, Value> {
    let t = as_template(o, ONE_OFF_ID, "/");
    super::with_defaults(&t, &o.values).into_iter().collect()
}

/// The step's path with its values filled in, and the `handler` of its
/// query when it has one (`/hr/x?handler=Save`): the part of the address
/// that says which save a POST to the page is.
pub fn request_path(o: &OneOff) -> String {
    let path = filled_path(o);
    match handler_of(&o.step) {
        Some(handler) => format!("{path}?handler={}", super::exec::substitute_str(handler.trim(), &sent_vars(o))),
        None => path,
    }
}

/// The call as the change list names a request: `POST /hr/x`, with the
/// `handler` when the call names one (`POST /hr/x?handler=Save`).
pub fn request_of(o: &OneOff) -> String {
    let method = match o.step.method {
        Method::Get => "GET",
        Method::Post => "POST",
    };
    crate::autorun::explore::request_line(method, &request_path(o))
}

/// Whether the call itself sends `id`: in its address, its query, its body or
/// its values. A create cannot send the id of a record that does not exist
/// yet, so a call that does is an edit or a read, whatever it was called.
pub fn sends_id(o: &OneOff, id: &str) -> bool {
    use crate::autorun::explore::{id_in_json, id_in_text};
    let vars = sent_vars(o);
    let fill = |s: &str| super::exec::substitute_str(s, &vars);
    id_in_text(&fill(&o.step.path), id)
        || o.step.query.values().any(|v| id_in_text(&fill(v), id))
        || o.step.form.iter().flat_map(|f| f.values()).any(|v| id_in_text(&fill(v), id))
        || o.step.json.as_ref().is_some_and(|j| id_in_json(j, id))
        || vars.values().any(|v| id_in_json(v, id))
}

/// The module a call saved from an explore session is filed under unless the
/// assistant names one.
pub const EXPLORE_MODULE: &str = "Explore";
/// Said in place of a template id for a GET that worked in a discovery that
/// is not an explore session.
pub const PLAIN_DISCOVERY_NOT_SAVED: &str = "saved only from an explore session";
/// Said in place of a template id for a delete call that worked.
pub const DELETES_NOT_SAVED: &str = "a delete call is never saved as an API template";
/// Said for a call whose page had no anti-forgery token.
pub const NO_TOKEN_NOT_SAVED: &str =
    "not saved as a template: the page had no anti-forgery token, which a saved template needs to run later";
/// Said for a call that writes a secret out.
pub const SECRET_NOT_SAVED: &str = "not saved as a template: it sends a password, token or other secret as written (in its body, its query, or the query of its token page), and a template never holds one - send that value as a param";

/// The longest a derived id is before a "-2" is put on it.
const ID_BASE_MAX: usize = 90;

/// Whether `s` is wholly one `{{placeholder}}`.
fn is_placeholder(s: &str) -> bool {
    let t = s.trim();
    t.len() > 4 && t.starts_with("{{") && t.ends_with("}}") && !t[2..t.len() - 2].contains(['{', '}'])
}

/// Body field names: json object keys and one level down ("a", "a.b"), an
/// array looked into through its first element ("a[].b"); a form's keys plus
/// its file fields; none for no body.
pub fn body_field_names(step: &Step) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    if let Some(json) = &step.json {
        match json {
            Value::Object(m) => {
                for (k, v) in m {
                    names.insert(k.clone());
                    match v {
                        Value::Object(inner) => names.extend(inner.keys().map(|i| format!("{k}.{i}"))),
                        Value::Array(items) => {
                            if let Some(Value::Object(first)) = items.first() {
                                names.extend(first.keys().map(|i| format!("{k}[].{i}")));
                            }
                        }
                        _ => {}
                    }
                }
            }
            Value::Array(items) => {
                if let Some(Value::Object(first)) = items.first() {
                    names.extend(first.keys().map(|i| format!("[].{i}")));
                }
            }
            _ => {}
        }
    }
    if let Some(form) = &step.form {
        names.extend(form.keys().cloned());
    }
    names.extend(step.files.keys().cloned());
    names
}

#[derive(PartialEq)]
enum Body {
    None,
    Json,
    Form,
}

fn body_of(step: &Step) -> Body {
    if step.json.is_some() {
        Body::Json
    } else if step.form.is_some() || !step.files.is_empty() {
        Body::Form
    } else {
        Body::None
    }
}

/// The value of a step's `handler` query key (the key's case ignored).
fn handler_of(step: &Step) -> Option<&str> {
    step.query.iter().find(|(k, _)| k.eq_ignore_ascii_case("handler")).map(|(_, v)| v.as_str())
}

/// The same call: one step, not a delete template, the same method, the same
/// path segment by segment (ASCII case ignored; a segment that is wholly a
/// {{placeholder}} on either side matches any), the same query keys with the
/// same `handler` value (case ignored), json vs form alike, and equal
/// `body_field_names`.
pub fn same_call(saved: &ApiTemplate, step: &Step) -> bool {
    let [one] = saved.steps.as_slice() else { return false };
    if saved.effect == Effect::Delete || one.method != step.method {
        return false;
    }
    let (a, b): (Vec<&str>, Vec<&str>) = (one.path.split('/').collect(), step.path.split('/').collect());
    if a.len() != b.len()
        || !a.iter().zip(&b).all(|(x, y)| x.eq_ignore_ascii_case(y) || is_placeholder(x) || is_placeholder(y))
    {
        return false;
    }
    let keys = |s: &Step| s.query.keys().cloned().collect::<std::collections::BTreeSet<_>>();
    if keys(one) != keys(step) {
        return false;
    }
    let same_handler = match (handler_of(one), handler_of(step)) {
        (Some(x), Some(y)) => x.eq_ignore_ascii_case(y),
        (None, None) => true,
        _ => false,
    };
    same_handler && body_of(one) == body_of(step) && body_field_names(one) == body_field_names(step)
}

/// The saved template that is the same call as `step`, if there is one. Only
/// a template that has been proven (`proven` set) is ever matched: one that
/// never worked is not a reason to leave a call that just did unsaved.
pub fn matching<'a>(saved: &'a [super::store::SavedTemplate], step: &Step) -> Option<&'a ApiTemplate> {
    saved.iter().map(|s| &s.template).filter(|t| t.proven.is_some()).find(|t| same_call(t, step))
}

/// Whether `segment` is a record id: all digits, or GUID-shaped
/// (8-4-4-4-12 hex digits).
pub fn is_id_segment(segment: &str) -> bool {
    if !segment.is_empty() && segment.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    let groups: Vec<&str> = segment.split('-').collect();
    groups.len() == 5
        && groups.iter().zip([8usize, 4, 4, 4, 12]).all(|(g, n)| g.len() == n && g.chars().all(|c| c.is_ascii_hexdigit()))
}

/// The step with each path segment that is a record id (`is_id_segment`)
/// made a `{{id}}` param (`{{id2}}`, `{{id3}}`... for more, and never a name
/// the call's params already use), so one template serves every record. The
/// params are the call's own plus one required param per id; a number for an
/// all-digit segment, text for a GUID. A step with no id segment comes back
/// as it was.
pub fn with_id_params(step: &Step, params: &[Param]) -> (Step, Vec<Param>) {
    let mut out = step.clone();
    let mut all: Vec<Param> = params.to_vec();
    let mut n = 0;
    let segments: Vec<String> = step
        .path
        .split('/')
        .map(|segment| {
            if !is_id_segment(segment) {
                return segment.to_string();
            }
            let name = loop {
                n += 1;
                let name = if n == 1 { "id".to_string() } else { format!("id{n}") };
                if !all.iter().any(|p| p.name == name) {
                    break name;
                }
            };
            let kind = if segment.chars().all(|c| c.is_ascii_digit()) { super::ParamType::Number } else { super::ParamType::String };
            all.push(Param {
                name: name.clone(),
                kind,
                required: true,
                description: Some("The id of the record the call is for.".to_string()),
                lookup: None,
                default: None,
            });
            format!("{{{{{name}}}}}")
        })
        .collect();
    out.path = segments.join("/");
    (out, all)
}

/// `s` as lowercase `[a-z0-9]` words joined by single dashes.
fn slug(s: &str) -> String {
    s.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// "<method>-<path slug>[-<handler slug>]", lowercase, [a-z0-9-] only, at
/// most 90 chars before a "-2", "-3"... that `taken` refuses.
pub fn derived_id(step: &Step, taken: &dyn Fn(&str) -> bool) -> String {
    let method = match step.method {
        Method::Get => "get",
        Method::Post => "post",
    };
    let path = step.path.split('?').next().unwrap_or("");
    let parts = [method.to_string(), slug(path), handler_of(step).map(slug).unwrap_or_default()];
    let base = parts.iter().filter(|p| !p.is_empty()).cloned().collect::<Vec<_>>().join("-");
    unique_id(&base, taken)
}

/// `base` (cut to 90 characters) or, when `taken` refuses it, the first of
/// `base-2`, `base-3`... that it does not.
pub fn unique_id(base: &str, taken: &dyn Fn(&str) -> bool) -> String {
    let base: String = base.chars().take(ID_BASE_MAX).collect();
    let base = base.trim_end_matches('-');
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !taken(id))
        .expect("an unbounded count always reaches an id that is not taken")
}

/// "POST /hr/pmsv10/PerformanceCycle (Save)"; no "(..)" without a handler.
pub fn derived_title(step: &Step) -> String {
    let method = match step.method {
        Method::Get => "GET",
        Method::Post => "POST",
    };
    match handler_of(step).map(str::trim).filter(|h| !h.is_empty()) {
        Some(h) => format!("{method} {} ({h})", step.path),
        None => format!("{method} {}", step.path),
    }
}

/// Whether a value of `v`, under a name that is (`secret`) or is not a
/// secret's, is written out.
fn writes_out(v: &Value, secret: bool) -> bool {
    match v {
        Value::Object(m) => m.iter().any(|(k, v)| writes_out(v, secret || crate::autorun::capture_redact::secret_name(k))),
        Value::Array(items) => items.iter().any(|v| writes_out(v, secret)),
        Value::String(s) => secret && !s.trim().is_empty() && !is_placeholder(s),
        Value::Null => false,
        _ => secret,
    }
}

/// A query value written out that is a secret: its name `secret_name()`
/// matches, or the value alone is token-shaped (the redaction of a captured
/// request would hide it). A value wholly a {{placeholder}} is not written out.
fn query_secret(key: &str, value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || is_placeholder(value) {
        return false;
    }
    crate::autorun::capture_redact::secret_name(key) || crate::autorun::capture_redact::redact_field(key, value) != value
}

/// A json, form or query field whose name `secret_name()` matches and whose
/// value is written out (not wholly a {{placeholder}}), or a query value that
/// is token-shaped under any name.
pub fn sends_a_secret(step: &Step) -> bool {
    use crate::autorun::capture_redact::secret_name;
    let in_json = step.json.as_ref().is_some_and(|j| writes_out(j, false));
    let in_form = step
        .form
        .as_ref()
        .is_some_and(|f| f.iter().any(|(k, v)| secret_name(k) && !v.trim().is_empty() && !is_placeholder(v)));
    let in_query = step.query.iter().any(|(k, v)| query_secret(k, v));
    in_json || in_form || in_query
}

/// A token page whose address (`antiforgery.page`) carries a secret in its
/// query, by the same rules as `sends_a_secret`.
pub fn page_sends_a_secret(page: &str) -> bool {
    let Some((_, query)) = page.split_once('?') else { return false };
    let query = query.split('#').next().unwrap_or("");
    query.split('&').any(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        query_secret(key, value)
    }) || crate::autorun::capture_redact::redact_path_query(page) != page
}
