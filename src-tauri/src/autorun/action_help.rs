//! What a discovery or a try does for an action that is almost right: a
//! refusal that shows what the action should look like, a wait that has no
//! `timeout_ms` given the discovery's own, a wait that is absurdly long
//! lowered to a ceiling, and the longest an action may take before the call
//! answers that it did not.
//!
//! Only discovery and try use this. A saved script keeps the rule that a
//! `wait_for` needs its `timeout_ms` (`autorun::guide`): nothing here is
//! applied to a save or to the `Action` type.

use serde_json::Value;

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
    ("wait_for", r##"{"kind":"wait_for","selector":{"css":"#done"},"timeout_ms":5000}"##),
    ("check_text", r#"{"kind":"check_text","value":"Saved"}"#),
    ("check_url", r#"{"kind":"check_url","contains":"/hr/home"}"#),
    ("remember_url", r#"{"kind":"remember_url","as":"before"}"#),
    ("expect_visible", r##"{"kind":"expect_visible","selector":{"css":"#panel"}}"##),
    ("expect_hidden", r##"{"kind":"expect_hidden","selector":{"css":"#panel"}}"##),
    ("expect_text", r##"{"kind":"expect_text","selector":{"css":"#title"},"equals":"Leave"}"##),
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
    let why = serde_error.trim().trim_end_matches('.');
    let body = match kind.and_then(|k| example_for(k).map(|example| (k, example))) {
        Some((k, example)) => format!("that is not a {k} action: {why}. A {k} action looks like {example}"),
        None => {
            let named = match kind {
                Some(k) => format!("\"{k}\" is not an action kind"),
                None => "an action needs a \"kind\"".to_string(),
            };
            format!("{named}; the kinds are {}", kinds().join(", "))
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
