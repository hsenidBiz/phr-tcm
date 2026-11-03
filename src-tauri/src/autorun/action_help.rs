//! What a discovery or a try does for an action that is almost right: a
//! refusal that shows what the action should look like, a wait that has no
//! `timeout_ms` given the discovery's own, a wait that is absurdly long
//! lowered to a ceiling, and the longest an action may take before the call
//! answers that it did not.
//!
//! Only discovery and try use this. A saved script keeps the rule that a
//! `wait_for` needs its `timeout_ms` (`autorun::guide`): nothing here is
//! applied to a save or to the `Action` type.

use crate::browser::actions::Action;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// How long after an action's own wait a discovery or try call lets it run
/// before it answers that the action did not.
pub const WAIT_MARGIN_MS: u64 = 5_000;

/// The longest wait a discovery or try action may ask for: a `timeout_ms` or
/// `within_ms` above it is lowered to it.
pub const MAX_DISCOVERY_WAIT_MS: u32 = 120_000;

/// The most one discovery or try call may take, below the proxy's own limit
/// on a call (300 s): the call answers by then whatever the browser is doing.
/// An action's budget never goes past it, and a batch shares it.
pub const CALL_BUDGET_MS: u64 = 280_000;

tokio::task_local! {
    /// When the discovery or try call being served began, so every action of
    /// it can be given what is left of `CALL_BUDGET_MS`.
    pub static CALL_STARTED: std::time::Instant;
}

/// One compact valid example of every action kind, taken from the guide's
/// vocabulary. A test parses each as an `Action`, so the table cannot drift
/// from the type.
const EXAMPLES: &[(&str, &str)] = &[
    ("navigate", r#"{"kind":"navigate","url":"https://example.test/"}"#),
    ("use_component", r#"{"kind":"use_component","component":"pick-date","inputs":{"day":"5"}}"#),
    ("click", r##"{"kind":"click","selector":{"css":"#save"}}"##),
    ("fill", r##"{"kind":"fill","selector":{"css":"#name"},"value":"Ann"}"##),
    ("select_option", r##"{"kind":"select_option","selector":{"css":"#type"},"label":"Numeric"}"##),
    ("wait_for", r##"{"kind":"wait_for","selector":{"css":"#done"},"timeout_ms":5000}"##),
    ("check_text", r#"{"kind":"check_text","value":"Saved"}"#),
    ("check_url", r#"{"kind":"check_url","contains":"/hr/home"}"#),
    ("remember_url", r#"{"kind":"remember_url","as":"before"}"#),
    ("remember_text", r##"{"kind":"remember_text","selector":{"css":"#pills"},"as":"counts"}"##),
    ("expect_visible", r##"{"kind":"expect_visible","selector":{"css":"#panel"}}"##),
    ("expect_hidden", r##"{"kind":"expect_hidden","selector":{"css":"#panel"}}"##),
    ("expect_text", r##"{"kind":"expect_text","selector":{"css":"#title"},"equals":"Leave"}"##),
    ("expect_text_moved", r##"{"kind":"expect_text_moved","selector":{"css":"#count"},"same_as":"before","by":1}"##),
    ("expect_contains_text", r##"{"kind":"expect_contains_text","selector":{"css":"#title"},"value":"Lea"}"##),
    ("expect_count", r##"{"kind":"expect_count","selector":{"css":"li"},"equals":3}"##),
    (
        "expect_attribute",
        r##"{"kind":"expect_attribute","selector":{"css":"#box"},"name":"aria-checked","equals":"true"}"##,
    ),
    (
        "expect_style",
        r##"{"kind":"expect_style","selector":{"css":"#box"},"property":"color","equals":"red"}"##,
    ),
    ("expect_enabled", r##"{"kind":"expect_enabled","selector":{"css":"#save"}}"##),
    ("expect_disabled", r##"{"kind":"expect_disabled","selector":{"css":"#save"}}"##),
    ("expect_checked", r##"{"kind":"expect_checked","selector":{"css":"#agree"},"checked":true}"##),
    ("expect_no_change", r##"{"kind":"expect_no_change","selector":{"css":"#save"},"within_ms":1000}"##),
    ("hover", r##"{"kind":"hover","selector":{"css":"#help"}}"##),
    ("sign_in", r#"{"kind":"sign_in","account":"hr.supervisor"}"#),
    ("upload", r##"{"kind":"upload","selector":{"css":"input[type=file]","visible":false},"file":"form.pdf"}"##),
    (
        "expect_response",
        r#"{"kind":"expect_response","method":"POST","url_contains":"/Cycle/Save","status":200}"#,
    ),
    ("api_request", r#"{"kind":"api_request","path":"/api/cycles/42","expect":{"status":200}}"#),
    (
        "when_visible",
        r##"{"kind":"when_visible","selector":{"css":"#banner"},"within_ms":2000,"then":[{"kind":"click","selector":{"css":"#close"}}]}"##,
    ),
    (
        "when_hidden",
        r##"{"kind":"when_hidden","selector":{"css":"#financial"},"within_ms":2000,"then":[{"kind":"click","selector":{"css":"#apply"}}]}"##,
    ),
    (
        "repeat_while_visible",
        r##"{"kind":"repeat_while_visible","selector":{"css":".row"},"max":20,"then":[{"kind":"click","selector":{"css":".remove"}}]}"##,
    ),
    ("reload", r#"{"kind":"reload"}"#),
    ("expire_session", r#"{"kind":"expire_session"}"#),
    ("return_to_area", r#"{"kind":"return_to_area"}"#),
    ("press_key", r#"{"kind":"press_key","key":"Tab"}"#),
    (
        "drag",
        r##"{"kind":"drag","from":{"css":"#a"},"to":{"css":"#b"},"position":"before"}"##,
    ),
    ("expect_focused", r##"{"kind":"expect_focused","selector":{"css":"#name"}}"##),
    ("expect_download", r#"{"kind":"expect_download","name":"Template*.xlsx"}"#),
    ("expect_tab", r#"{"kind":"expect_tab","name":"report"}"#),
    ("open_tab", r#"{"kind":"open_tab","name":"second","url":"/hr/employee/42"}"#),
    ("switch_tab", r#"{"kind":"switch_tab","name":"report"}"#),
    ("close_tab", r#"{"kind":"close_tab","name":"report"}"#),
    ("expect_tab_closed", r#"{"kind":"expect_tab_closed","name":"preview"}"#),
    ("expect_dialog", r#"{"kind":"expect_dialog","contains":"delete","answer":"dismiss"}"#),
    (
        "expect_row",
        r#"{"kind":"expect_row","table":{"role":"grid","name":"Employees"},"cells":{"Name":"Ann"}}"#,
    ),
    (
        "expect_no_row",
        r#"{"kind":"expect_no_row","table":{"role":"grid","name":"Employees"},"cells":{"Name":"Ann"}}"#,
    ),
    (
        "expect_sorted",
        r#"{"kind":"expect_sorted","table":{"role":"grid","name":"Employees"},"column":"Joined","order":"descending","as":"date"}"#,
    ),
    (
        "expect_row_count",
        r#"{"kind":"expect_row_count","table":{"role":"grid","name":"Employees"},"at_least":1}"#,
    ),
    (
        "expect_each",
        r##"{"kind":"expect_each","selector":{"css":"li"},"child":{"css":"h3"},"matches":"^AUTOTEST "}"##,
    ),
    (
        "expect_order",
        r#"{"kind":"expect_order","selector":{"role":"option"},"texts":["All","Draft"]}"#,
    ),
    (
        "expect_position",
        r##"{"kind":"expect_position","selector":{"css":"#a"},"relative_to":{"css":"#b"},"relation":"right_of"}"##,
    ),
    (
        "page_until",
        r##"{"kind":"page_until","next":{"css":"#next"},"until":{"text":"Annual Review"},"max_pages":40}"##,
    ),
];

/// One compact valid example of the action kind `kind`.
pub fn example_for(kind: &str) -> Option<&'static str> {
    EXAMPLES.iter().find(|(k, _)| *k == kind).map(|(_, e)| *e)
}

/// Every action kind, in the order the guide lists them.
pub fn kinds() -> Vec<&'static str> {
    EXAMPLES.iter().map(|(k, _)| *k).collect()
}

/// The refusal for an action that did not parse (`serde_error` is why). A
/// known kind says what went wrong and shows an example of that kind; a
/// missing or unknown kind lists the kinds. Always points at the guide.
pub fn refusal_for(action: &Value, serde_error: &str) -> String {
    let kind = action.get("kind").and_then(Value::as_str);
    if kind == Some(SCREENSHOT_KIND) {
        return SAVED_SCREENSHOT.to_string();
    }
    let why = serde_error.trim().trim_end_matches('.');
    let body = match kind.and_then(|k| example_for(k).map(|example| (k, example))) {
        Some((k, example)) => format!("that is not a {k} action: {why}. A {k} action looks like {example}"),
        None => {
            let named = match kind {
                Some(k) => format!("\"{k}\" is not an action kind"),
                None => "an action needs a \"kind\"".to_string(),
            };
            format!("{named}; the kinds are {}, and {SCREENSHOT_KIND} in a discovery or a try", kinds().join(", "))
        }
    };
    format!("{body} - call get_guide (topic autorun) for the vocabulary.")
}

/// Gives a `wait_for` that has no `timeout_ms` (inside a `when_visible` or
/// `repeat_while_visible`'s `then` too) `default_ms`. Returns whether it
/// gave one. For a discovery or a try only: a saved script is never filled.
pub fn fill_wait_defaults(action: &mut Value, default_ms: u32) -> bool {
    let Some(o) = action.as_object_mut() else {
        return false;
    };
    let mut filled = false;
    if o.get("kind").and_then(Value::as_str) == Some("wait_for") && o.get("timeout_ms").is_none_or(Value::is_null) {
        o.insert("timeout_ms".to_string(), Value::from(default_ms));
        filled = true;
    }
    if let Some(Value::Array(then)) = o.get_mut("then") {
        for inner in then {
            filled |= fill_wait_defaults(inner, default_ms);
        }
    }
    filled
}

/// Lowers every `timeout_ms` and `within_ms` above `MAX_DISCOVERY_WAIT_MS`
/// to it (inside a `then` too). Returns the keys it lowered, each once.
pub fn clamp_wait(action: &mut Value) -> Vec<&'static str> {
    let mut lowered = Vec::new();
    clamp_into(action, &mut lowered);
    lowered
}

fn clamp_into(action: &mut Value, lowered: &mut Vec<&'static str>) {
    let Some(o) = action.as_object_mut() else {
        return;
    };
    for key in ["timeout_ms", "within_ms"] {
        if o.get(key).and_then(Value::as_u64).is_some_and(|ms| ms > u64::from(MAX_DISCOVERY_WAIT_MS)) {
            o.insert(key.to_string(), Value::from(MAX_DISCOVERY_WAIT_MS));
            if !lowered.contains(&key) {
                lowered.push(key);
            }
        }
    }
    if let Some(Value::Array(then)) = o.get_mut("then") {
        for inner in then {
            clamp_into(inner, lowered);
        }
    }
}

/// What a discovery or try did to one action's waits before running it, as
/// sentences for the answer. Empty when it changed nothing.
pub fn prepare_waits(action: &mut Value, default_ms: u32) -> Vec<String> {
    let mut notes = Vec::new();
    if fill_wait_defaults(action, default_ms) {
        notes.push(format!("no timeout_ms given, used {default_ms} (a saved script needs one)"));
    }
    let lowered = clamp_wait(action);
    if !lowered.is_empty() {
        notes.push(format!("{} lowered to {MAX_DISCOVERY_WAIT_MS}", lowered.join(" and ")));
    }
    notes
}

/// The waits an action names (`timeout_ms`, `within_ms`), added up, inside a
/// `then` too.
fn named_waits(v: &Value) -> u64 {
    let Some(o) = v.as_object() else {
        return 0;
    };
    let own: u64 = ["timeout_ms", "within_ms"].iter().filter_map(|k| o.get(*k).and_then(Value::as_u64)).sum();
    let inner: u64 = o.get("then").and_then(Value::as_array).map_or(0, |then| then.iter().map(named_waits).sum());
    own + inner
}

/// The most a discovery or try action may take before the call stops
/// waiting: the waits it names (or `nav_ms` when they are shorter), the
/// longest a loading indicator may be waited out before it
/// (`autorun::loading::MAX_MAX_MS`), and `WAIT_MARGIN_MS`, never past
/// `CALL_BUDGET_MS`. An action that is several (a tried component, a page
/// walk, a list reset) gets the whole `CALL_BUDGET_MS`.
pub fn wait_budget_ms(action: &crate::browser::actions::Action, nav_ms: u64) -> u64 {
    let v = serde_json::to_value(action).unwrap_or(Value::Null);
    let composite = matches!(
        v.get("kind").and_then(Value::as_str),
        Some("use_component" | "page_until" | "repeat_while_visible")
    );
    if composite {
        return CALL_BUDGET_MS;
    }
    let own = named_waits(&v).max(nav_ms);
    (own + u64::from(super::loading::MAX_MAX_MS) + WAIT_MARGIN_MS).min(CALL_BUDGET_MS)
}

/// What an action may take given its own budget and how long the call has
/// run: the smaller of the budget and what is left of `CALL_BUDGET_MS`.
/// `None` when the call has no time left.
pub fn action_budget(own_ms: u64, call_elapsed_ms: u64) -> Option<u64> {
    let left = CALL_BUDGET_MS.checked_sub(call_elapsed_ms).filter(|left| *left > 0)?;
    Some(own_ms.min(CALL_BUDGET_MS).min(left))
}

/// How long an action may run: its budget, or the failed detail it gets
/// without being started when the call has no time left.
pub fn time_to_run(budget: Option<u64>) -> Result<u64, String> {
    budget.ok_or_else(|| did_not_answer(CALL_BUDGET_MS / 1000))
}

/// How long the call being served has run, when it is one (`CALL_STARTED`).
pub fn call_elapsed_ms() -> u64 {
    CALL_STARTED.try_with(|started| started.elapsed().as_millis() as u64).unwrap_or(0)
}

/// The detail of an action that did not answer in time.
pub fn did_not_answer(seconds: u64) -> String {
    format!("{DID_NOT_ANSWER} {seconds} s")
}

/// How `did_not_answer` begins: a batch that meets it stops there.
pub const DID_NOT_ANSWER: &str = "did not answer within";

/// What a call says when action `n` did not answer in time; `seconds` is
/// the time it was given.
pub fn no_answer(n: usize, seconds: u64) -> String {
    format!("action {n} did not answer within {seconds} s; the batch stopped there")
}

/// Runs `work`, giving up after `budget_ms`. `Err` carries the seconds the
/// work was given.
pub async fn within_budget<T>(budget_ms: u64, work: impl std::future::Future<Output = T>) -> Result<T, u64> {
    tokio::time::timeout(std::time::Duration::from_millis(budget_ms), work)
        .await
        .map_err(|_| budget_ms.div_ceil(1000))
}

// ---- the discovery's own `screenshot` (backlog 86) ---------------------------

/// The `kind` of the picture a discovery or a try takes. Not an `Action`
/// kind: a saved script never holds one (it takes its pictures when a step
/// fails), so it stays out of the script model and its exhaustive matches.
pub const SCREENSHOT_KIND: &str = "screenshot";

/// Said to a saved script that holds a `screenshot`.
pub const SAVED_SCREENSHOT: &str =
    "screenshot is a discovery action: a saved script takes its pictures when a step fails";

/// How a screenshot travels through the discovery batch, which carries
/// `Action`s: as a `check_text` whose value is one of these. Nothing parses
/// to it (`screenshot_from` is the one way in), `describe_action` words it as
/// a picture, and `try_action` takes the picture instead of checking text.
const SHOT_MARK: &str = "\u{1}screenshot";
const SHOT_MARK_FULL: &str = "\u{1}screenshot:full_page";

/// The marked action standing for a screenshot of the window (or, with
/// `full_page`, of the whole page).
pub fn screenshot_action(full_page: bool) -> Action {
    Action::CheckText { value: (if full_page { SHOT_MARK_FULL } else { SHOT_MARK }).to_string() }
}

/// `Some(full_page)` when `action` is the marked stand-in for a screenshot.
pub fn screenshot_ask(action: &Action) -> Option<bool> {
    match action {
        Action::CheckText { value } if value == SHOT_MARK => Some(false),
        Action::CheckText { value } if value == SHOT_MARK_FULL => Some(true),
        _ => None,
    }
}

/// A raw discovery or try action that is a `screenshot`: `None` when it is
/// any other kind, else the marked action, or why it was refused (a key but
/// `full_page`, a `full_page` that is not true or false).
pub fn screenshot_from(raw: &Value) -> Option<Result<Action, String>> {
    let o = raw.as_object()?;
    if o.get("kind").and_then(Value::as_str) != Some(SCREENSHOT_KIND) {
        return None;
    }
    let stray: Vec<&str> = o.keys().map(String::as_str).filter(|k| !matches!(*k, "kind" | "full_page")).collect();
    if !stray.is_empty() {
        return Some(Err(format!("{SCREENSHOT_KIND} does not take {} - it takes: full_page", stray.join(", "))));
    }
    Some(match o.get("full_page") {
        None | Some(Value::Null) => Ok(screenshot_action(false)),
        Some(Value::Bool(b)) => Ok(screenshot_action(*b)),
        Some(_) => Err("screenshot's full_page is true or false".to_string()),
    })
}

// ---- keys an action does not have (backlog 87) -------------------------------

/// Each action kind's own field names, in the order the type declares them,
/// from the very definition the TypeScript bindings are generated from, so
/// the list can never drift from `Action`. Empty when the definition cannot be
/// read, which turns the unknown-key refusal off rather than refusing
/// everything.
fn kind_fields() -> &'static BTreeMap<String, Vec<String>> {
    static FIELDS: OnceLock<BTreeMap<String, Vec<String>>> = OnceLock::new();
    FIELDS.get_or_init(|| {
        use specta::datatype::{DataType, Fields, Reference};
        use specta::Format as _;
        let mut types = specta::Types::default();
        let dt = <Action as specta::Type>::definition(&mut types);
        // The phases format, as the bindings are made: the type has conditional
        // omissions (`skip_serializing_if`) the one-shape format cannot hold.
        let Ok(mapped) = specta_serde::PhasesFormat.map_types(&types) else { return BTreeMap::new() };
        // What is read (the keys a script may carry), not what is written.
        let dt = specta_serde::select_phase_datatype(&dt, &mapped, specta_serde::Phase::Deserialize);
        let ty = match &dt {
            DataType::Reference(Reference::Named(r)) => mapped.get(r).and_then(|n| n.ty.clone()),
            other => Some(other.clone()),
        };
        let Some(DataType::Enum(e)) = ty else { return BTreeMap::new() };
        let mut out = BTreeMap::new();
        for (name, variant) in &e.variants {
            if variant.skip {
                continue;
            }
            // A kind that gathers every other key itself (`Stray`, flattened)
            // refuses them in its own words when it is validated.
            if matches!(&variant.fields, Fields::Named(n)
                if n.fields.iter().any(|(_, f)| f.attributes.contains_key("serde:field:flatten")))
            {
                continue;
            }
            let fields = match &variant.fields {
                Fields::Named(n) => n
                    .fields
                    .iter()
                    .filter(|(k, f)| f.ty.is_some() && k.as_ref() != "kind")
                    .map(|(k, _)| k.to_string())
                    .collect(),
                _ => Vec::new(),
            };
            out.insert(name.to_string(), fields);
        }
        out
    })
}

/// The field names of action kind `kind` (`"press_key"` is `key`, `times`),
/// or `None` for a kind that is not one, or that reads any other key itself
/// (`expect_response`, `api_request`, `expect_download`).
pub fn fields_of(kind: &str) -> Option<Vec<String>> {
    kind_fields().get(kind).cloned()
}

/// Keys of the nested action lists an action holds.
const NESTED_ACTIONS: &[&str] = &["then"];

/// The keys of a raw action that its kind does not have, nested actions
/// included, each once, in the order met. Nothing for a value that is not an
/// object, or has a kind `fields_of` has no list for.
pub fn unknown_keys(raw: &Value) -> Vec<String> {
    let mut found = Vec::new();
    collect_unknown(raw, &mut found);
    found
}

fn collect_unknown(raw: &Value, found: &mut Vec<String>) {
    let Some(o) = raw.as_object() else { return };
    let Some(kind) = o.get("kind").and_then(Value::as_str) else { return };
    if let Some(fields) = fields_of(kind) {
        for key in o.keys() {
            if key != "kind" && !fields.contains(key) && !found.contains(key) {
                found.push(key.clone());
            }
        }
    }
    for key in NESTED_ACTIONS {
        for inner in o.get(*key).and_then(Value::as_array).into_iter().flatten() {
            collect_unknown(inner, found);
        }
    }
}

/// Refuses a raw action holding a key its kind does not have, naming the
/// kind, the keys and what the kind takes: `"press_key does not take repeat
/// - it takes: key, times"`. A nested action is named by its own kind.
pub fn refuse_unknown_keys(raw: &Value) -> Result<(), String> {
    let Some(o) = raw.as_object() else { return Ok(()) };
    if let Some(kind) = o.get("kind").and_then(Value::as_str) {
        if let Some(fields) = fields_of(kind) {
            let stray: Vec<&str> =
                o.keys().map(String::as_str).filter(|k| *k != "kind" && !fields.iter().any(|f| f == k)).collect();
            if !stray.is_empty() {
                let takes = if fields.is_empty() { "no fields".to_string() } else { fields.join(", ") };
                return Err(format!("{kind} does not take {} - it takes: {takes}", stray.join(", ")));
            }
        }
    }
    for key in NESTED_ACTIONS {
        for inner in o.get(*key).and_then(Value::as_array).into_iter().flatten() {
            refuse_unknown_keys(inner)?;
        }
    }
    Ok(())
}

/// What a script sent to be saved is held to before it is read as one: no
/// `screenshot` (`SAVED_SCREENSHOT`), and no key an action does not have
/// (`refuse_unknown_keys`). Applied to what arrives, never to a script read
/// from disk.
pub fn refuse_raw_saved_action(raw: &Value) -> Result<(), String> {
    fn no_screenshot(raw: &Value) -> Result<(), String> {
        let Some(o) = raw.as_object() else { return Ok(()) };
        if o.get("kind").and_then(Value::as_str) == Some(SCREENSHOT_KIND) {
            return Err(SAVED_SCREENSHOT.to_string());
        }
        // The stand-in a discovery batch carries a screenshot as is never saved.
        if o.get("kind").and_then(Value::as_str) == Some("check_text")
            && o.get("value").and_then(Value::as_str).is_some_and(|v| v == SHOT_MARK || v == SHOT_MARK_FULL)
        {
            return Err("check_text cannot carry that value".to_string());
        }
        for key in NESTED_ACTIONS {
            for inner in o.get(*key).and_then(Value::as_array).into_iter().flatten() {
                no_screenshot(inner)?;
            }
        }
        Ok(())
    }
    no_screenshot(raw)?;
    refuse_unknown_keys(raw)
}

/// `repeat` on a `press_key` read as `times` (what an assistant writes for
/// "press it that many times"), inside a `then` too. Both given is refused.
/// For a discovery or a try only. The notes say what was read.
pub fn prepare_repeat(action: &mut Value) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    repeat_into(action, &mut notes)?;
    Ok(notes)
}

fn repeat_into(action: &mut Value, notes: &mut Vec<String>) -> Result<(), String> {
    let Some(o) = action.as_object_mut() else { return Ok(()) };
    if o.get("kind").and_then(Value::as_str) == Some("press_key") && o.contains_key("repeat") {
        if o.contains_key("times") {
            return Err("press_key takes times or repeat, not both".to_string());
        }
        if let Some(n) = o.remove("repeat") {
            o.insert("times".to_string(), n);
            if !notes.iter().any(|n| n == "repeat read as times") {
                notes.push("repeat read as times".to_string());
            }
        }
    }
    for key in NESTED_ACTIONS {
        if let Some(Value::Array(list)) = o.get_mut(*key) {
            for inner in list {
                repeat_into(inner, notes)?;
            }
        }
    }
    Ok(())
}
