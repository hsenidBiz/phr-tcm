//! Seeing the page: the accessibility tree as text, and what a locator
//! matches right now.

use crate::common;

use common::ScriptedDriver;
use serde_json::json;
use v2_lib::browser::cdp::CdpError;
use v2_lib::browser::locator::{Target, VISIBLE_JS};
use v2_lib::browser::snapshot::{parse_nodes, probe, render, snapshot, AxNode, DEFAULT_LIMIT, PROBE_SUMMARY_JS};

/// A plain, printable node with no value/ignored/disabled/focusable
/// wrinkles - tests override the fields they care about with `..`.
fn node(id: &str, role: &str, name: &str, children: &[&str]) -> AxNode {
    AxNode {
        id: id.to_string(),
        role: role.to_string(),
        name: name.to_string(),
        value: None,
        ignored: false,
        children: children.iter().map(|s| s.to_string()).collect(),
        focusable: false,
        disabled: false,
    }
}

/// The locator suffix for a role and non-empty name, the same compact
/// JSON `locator_suffix` produces - one place to build the expected
/// string instead of repeating the punctuation in every test.
fn locator(role: &str, name: &str) -> String {
    format!(" -> {}", json!({ "role": role, "name": name }))
}

fn locator_role_only(role: &str) -> String {
    format!(" -> {}", json!({ "role": role }))
}

// --- render: rule 2, folding -------------------------------------------

/// A `generic` wrapper and an explicitly `ignored` node are both skipped,
/// but their own children still print - at the depth the folded node
/// would have used, since it left no line to indent under.
#[test]
fn folded_nodes_disappear_but_their_children_print_at_the_parents_depth() {
    let nodes = vec![
        node("1", "dialog", "Add Rating Method", &["2", "5"]),
        node("2", "generic", "", &["3", "4"]),
        node("3", "button", "Add Method", &[]),
        node("4", "StaticText", "Add Method", &[]),
        AxNode { ignored: true, ..node("5", "group", "", &["6"]) },
        node("6", "heading", "Section", &[]),
    ];
    let expected = format!(
        "dialog \"Add Rating Method\"{}\n \
         button \"Add Method\"{}\n \
         heading \"Section\"{}",
        locator("dialog", "Add Rating Method"),
        locator("button", "Add Method"),
        locator("heading", "Section"),
    );
    assert_eq!(render(&nodes, DEFAULT_LIMIT), expected);
}

// --- render: rule 3, value/disabled/password ----------------------------

/// A textbox's value is shown; a password's never is, however it is
/// named, and `disabled` is called out in words.
#[test]
fn a_value_prints_except_for_a_password_and_disabled_is_named() {
    let nodes = vec![
        node("1", "form", "", &["2", "3", "4"]),
        AxNode { value: Some("kim".into()), ..node("2", "textbox", "Username", &[]) },
        AxNode { value: Some("hunter2".into()), ..node("3", "textbox", "Password", &[]) },
        AxNode { disabled: true, ..node("4", "button", "Submit", &[]) },
    ];
    let expected = format!(
        "form \"\"{}\n \
         textbox \"Username\" = \"kim\"{}\n \
         textbox \"Password\"{}\n \
         button \"Submit\" (disabled){}",
        locator_role_only("form"),
        locator("textbox", "Username"),
        locator("textbox", "Password"),
        locator("button", "Submit"),
    );
    assert_eq!(render(&nodes, DEFAULT_LIMIT), expected);
}

/// A role that never shows a value (e.g. a heading) is unaffected even
/// when a `value` happens to be present on the node.
#[test]
fn only_value_roles_ever_show_a_value() {
    let nodes = vec![AxNode { value: Some("ignored".into()), ..node("1", "heading", "Title", &[]) }];
    assert_eq!(render(&nodes, DEFAULT_LIMIT), format!("heading \"Title\"{}", locator("heading", "Title")));
}

/// I3: a nameless textbox has nothing a locator could match the value
/// against, so the value never prints - `protected` or not.
#[test]
fn a_nameless_value_role_never_shows_its_value() {
    let nodes = vec![AxNode { value: Some("something".into()), ..node("1", "textbox", "", &[]) }];
    assert_eq!(render(&nodes, DEFAULT_LIMIT), format!("textbox \"\"{}", locator_role_only("textbox")));
}

// --- render: rule 4, the locator suffix ---------------------------------

/// A nameless node gets a role-only locator; focusable is tracked but
/// never printed.
#[test]
fn a_nameless_node_gets_a_role_only_locator_and_focusable_stays_silent() {
    let nodes = vec![AxNode { focusable: true, ..node("1", "region", "", &[]) }];
    assert_eq!(render(&nodes, DEFAULT_LIMIT), format!("region \"\"{}", locator_role_only("region")));
}

/// C2: the locator is built as real JSON, not hand-formatted string
/// concatenation, so a name carrying a quote or a backslash still comes
/// back as something `Target`'s own deserializer accepts.
#[test]
fn a_name_with_quotes_and_a_backslash_still_yields_a_parseable_locator() {
    let raw_name = "Say \"Hello\" \\ now";
    let nodes = vec![node("1", "button", raw_name, &[])];
    let out = render(&nodes, DEFAULT_LIMIT);
    let suffix = out.splitn(2, " -> ").nth(1).expect("a locator suffix");
    let target: Target = serde_json::from_str(suffix).expect("valid JSON a Target can be read back from");
    assert!(target.describe().contains(raw_name), "{}", target.describe());
}

/// The printed name is truncated at 80 characters with `...`, but the
/// locator on the end of the line is built from the FULL sanitized name -
/// otherwise a locator copied off a snapshot line for a long-named
/// control would only ever match the truncated prefix, not the control's
/// real accessible name.
#[test]
fn a_long_name_is_truncated_on_the_line_but_the_locator_keeps_it_whole() {
    let long_name = "x".repeat(100);
    let nodes = vec![node("1", "button", &long_name, &[])];
    let out = render(&nodes, DEFAULT_LIMIT);
    assert!(out.contains("..."), "the printed name must still be truncated: {out}");
    let suffix = out.splitn(2, " -> ").nth(1).expect("a locator suffix");
    let target: Target = serde_json::from_str(suffix).expect("valid JSON a Target can be read back from");
    assert_eq!(target.describe(), format!("button \"{long_name}\""));
}

// --- render: rule 5, the line cap ----------------------------------------

#[test]
fn the_output_is_capped_and_says_how_many_more() {
    let nodes = vec![
        node("0", "generic", "", &["1", "2", "3", "4", "5"]),
        node("1", "listitem", "Item 1", &[]),
        node("2", "listitem", "Item 2", &[]),
        node("3", "listitem", "Item 3", &[]),
        node("4", "listitem", "Item 4", &[]),
        node("5", "listitem", "Item 5", &[]),
    ];
    let expected = format!(
        "listitem \"Item 1\"{}\n\
         listitem \"Item 2\"{}\n\
         listitem \"Item 3\"{}\n\
         ... and 2 more (raise the limit, or scope the probe)",
        locator("listitem", "Item 1"),
        locator("listitem", "Item 2"),
        locator("listitem", "Item 3"),
    );
    assert_eq!(render(&nodes, 3), expected);
}

/// M6: a zero limit shows no lines at all, only the "more" count.
#[test]
fn a_zero_limit_shows_only_the_count() {
    let nodes = vec![node("1", "listitem", "Item 1", &[])];
    assert_eq!(render(&nodes, 0), "... and 1 more (raise the limit, or scope the probe)");
}

/// M6: exactly at the limit, there is nothing more to report.
#[test]
fn when_total_equals_the_limit_no_more_message_is_added() {
    let nodes = vec![
        node("0", "generic", "", &["1", "2"]),
        node("1", "listitem", "Item 1", &[]),
        node("2", "listitem", "Item 2", &[]),
    ];
    let expected =
        format!("listitem \"Item 1\"{}\nlistitem \"Item 2\"{}", locator("listitem", "Item 1"), locator("listitem", "Item 2"));
    assert_eq!(render(&nodes, 2), expected);
}

// --- render: rule 6, depth cap -------------------------------------------

/// Fifteen nodes nested one inside the next: depth 12 and everything
/// deeper all indent the same twelve spaces.
#[test]
fn indent_stops_growing_past_twelve_spaces() {
    let mut nodes = vec![];
    for i in 0..15 {
        let id = i.to_string();
        let child = if i + 1 < 15 { vec![(i + 1).to_string()] } else { vec![] };
        let children: Vec<&str> = child.iter().map(String::as_str).collect();
        nodes.push(node(&id, "group", &format!("L{i}"), &children));
    }
    let out = render(&nodes, DEFAULT_LIMIT);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 15);
    let indent_of = |line: &str| line.len() - line.trim_start_matches(' ').len();
    assert_eq!(indent_of(lines[11]), 11);
    assert_eq!(indent_of(lines[12]), 12);
    assert_eq!(indent_of(lines[14]), 12, "depth beyond 12 stays at 12, it never keeps growing");
}

// --- render: rule 7, nothing to show -------------------------------------

#[test]
fn an_empty_tree_says_so() {
    assert_eq!(render(&[], DEFAULT_LIMIT), "the page has nothing a locator could name");
}

/// Every node present is folded away, so nothing ever gets a line -
/// the same sentence as a genuinely empty tree, not a blank string.
#[test]
fn a_tree_that_folds_away_entirely_says_so_too() {
    let nodes = vec![node("1", "generic", "", &[])];
    assert_eq!(render(&nodes, DEFAULT_LIMIT), "the page has nothing a locator could name");
}

// --- render: I4, a cyclic tree --------------------------------------------

/// A two-node `childIds` cycle: each node prints exactly once and `walk`
/// returns instead of recursing forever.
#[test]
fn a_childids_cycle_does_not_loop_forever() {
    let nodes = vec![node("1", "group", "A", &["2"]), node("2", "group", "B", &["1"])];
    let expected = format!("group \"A\"{}\n group \"B\"{}", locator("group", "A"), locator("group", "B"));
    assert_eq!(render(&nodes, DEFAULT_LIMIT), expected);
}

/// The same node reachable through two different parents is printed once,
/// under whichever parent's turn came first.
#[test]
fn the_same_child_reached_two_ways_prints_once() {
    let nodes = vec![
        node("1", "generic", "", &["2", "3"]),
        node("2", "group", "First", &["4"]),
        node("3", "group", "Second", &["4"]),
        node("4", "button", "Shared", &[]),
    ];
    let out = render(&nodes, DEFAULT_LIMIT);
    // Each line legitimately says the name twice (the printed quote and
    // the JSON locator), so it is LINES containing "Shared" that must be
    // one, not the substring count.
    assert_eq!(out.lines().filter(|l| l.contains("Shared")).count(), 1, "{out}");
}

// --- render: I5, control characters ---------------------------------------

/// A newline inside an accessible name would otherwise split one printed
/// node across two lines; it becomes a space instead, before truncation.
#[test]
fn a_control_character_in_a_name_becomes_a_space() {
    let nodes = vec![node("1", "heading", "Line one\nLine two", &[])];
    assert_eq!(
        render(&nodes, DEFAULT_LIMIT),
        format!("heading \"Line one Line two\"{}", locator("heading", "Line one Line two"))
    );
}

/// The same sanitising applies to a shown value.
#[test]
fn a_control_character_in_a_value_becomes_a_space() {
    let nodes = vec![AxNode { value: Some("line1\r\nline2".into()), ..node("1", "textbox", "Notes", &[]) }];
    assert_eq!(
        render(&nodes, DEFAULT_LIMIT),
        format!("textbox \"Notes\" = \"line1  line2\"{}", locator("textbox", "Notes"))
    );
}

// --- parse_nodes ----------------------------------------------------------

/// A small captured `Accessibility.getFullAXTree` answer: a root, a
/// `generic` wrapper, a button with a folded `StaticText` child, a
/// password field found through the `protected` property, a second
/// password field found only by its name, and an ordinary username field.
#[test]
fn parse_nodes_reads_a_captured_ax_tree() {
    let fixture = json!({ "nodes": [
        { "nodeId": "1", "ignored": false, "role": { "value": "RootWebArea" }, "name": { "value": "Sign in" },
          "childIds": ["2", "5", "7", "6"] },
        { "nodeId": "2", "ignored": false, "role": { "value": "generic" }, "name": { "value": "" },
          "childIds": ["3"] },
        { "nodeId": "3", "ignored": false, "role": { "value": "button" }, "name": { "value": "Save" },
          "properties": [
              { "name": "focusable", "value": { "value": true } },
              { "name": "disabled", "value": { "value": true } }
          ],
          "childIds": ["4"] },
        { "nodeId": "4", "ignored": false, "role": { "value": "StaticText" }, "name": { "value": "Save" },
          "childIds": [] },
        { "nodeId": "5", "ignored": false, "role": { "value": "textbox" }, "name": { "value": "Password" },
          "value": { "value": "hunter2" },
          "properties": [ { "name": "protected", "value": { "value": true } } ],
          "childIds": [] },
        { "nodeId": "6", "ignored": false, "role": { "value": "textbox" }, "name": { "value": "Username" },
          "value": { "value": "kim" }, "childIds": [] },
        { "nodeId": "7", "ignored": false, "role": { "value": "textbox" }, "name": { "value": "New Password" },
          "value": { "value": "abc" }, "childIds": [] }
    ] });
    let nodes = parse_nodes(&fixture);
    assert_eq!(nodes.len(), 7);
    let by_id = |id: &str| nodes.iter().find(|n| n.id == id).unwrap();

    let root = by_id("1");
    assert_eq!(root.role, "RootWebArea");
    assert_eq!(root.name, "Sign in");
    assert_eq!(root.children, vec!["2", "5", "7", "6"]);

    let button = by_id("3");
    assert_eq!(button.role, "button");
    assert!(button.focusable);
    assert!(button.disabled);
    assert_eq!(button.children, vec!["4"]);

    // Protected property: the value never makes it into the node at all.
    let password = by_id("5");
    assert_eq!(password.name, "Password");
    assert_eq!(password.value, None);

    // No `protected` property, but the name alone is enough.
    let new_password = by_id("7");
    assert_eq!(new_password.name, "New Password");
    assert_eq!(new_password.value, None);

    let username = by_id("6");
    assert_eq!(username.value, Some("kim".to_string()));
}

// --- snapshot --------------------------------------------------------------

#[tokio::test]
async fn snapshot_calls_get_full_ax_tree_with_no_params() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Accessibility.getFullAXTree" => Ok(json!({ "nodes": [
            { "nodeId": "1", "ignored": false, "role": { "value": "button" }, "name": { "value": "Go" },
              "childIds": [] }
        ] })),
        other => panic!("unexpected {other}"),
    });
    let out = snapshot(&mut d, DEFAULT_LIMIT).await.unwrap();
    assert_eq!(out, format!("button \"Go\"{}", locator("button", "Go")));
    assert_eq!(d.calls_to("Accessibility.getFullAXTree"), vec![json!({})]);
}

/// The first call fails because the domain was never switched on; the
/// snapshot enables it once and retries, rather than surfacing the
/// refusal.
#[tokio::test]
async fn snapshot_enables_accessibility_and_retries_when_not_enabled() {
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls2 = calls.clone();
    let mut d = ScriptedDriver::new(move |method, _| match method {
        "Accessibility.getFullAXTree" => {
            let n = calls2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n == 0 {
                Err(CdpError::Protocol {
                    method: "Accessibility.getFullAXTree".into(),
                    message: "Accessibility domain is not enabled".into(),
                })
            } else {
                Ok(json!({ "nodes": [] }))
            }
        }
        "Accessibility.enable" => Ok(json!({})),
        other => panic!("unexpected {other}"),
    });
    let out = snapshot(&mut d, DEFAULT_LIMIT).await.unwrap();
    assert_eq!(out, "the page has nothing a locator could name");
    assert_eq!(
        d.methods(),
        vec!["Accessibility.getFullAXTree", "Accessibility.enable", "Accessibility.getFullAXTree"]
    );
}

/// A refusal unrelated to the domain being disabled is not retried.
#[tokio::test]
async fn snapshot_does_not_retry_an_unrelated_refusal() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Accessibility.getFullAXTree" => {
            Err(CdpError::Protocol { method: method.to_string(), message: "the target has gone away".into() })
        }
        other => panic!("unexpected {other}"),
    });
    let err = snapshot(&mut d, DEFAULT_LIMIT).await.unwrap_err();
    assert!(matches!(err, CdpError::Protocol { .. }));
    assert_eq!(d.methods(), vec!["Accessibility.getFullAXTree"]);
}

// --- probe -------------------------------------------------------------

#[tokio::test]
async fn probe_of_nothing_names_the_locator() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
        "Runtime.getProperties" => Ok(json!({ "result": [] })),
        other => panic!("unexpected {other}"),
    });
    let out = probe(&mut d, &Target::from("#missing")).await.unwrap();
    assert_eq!(out, "matches: 0 - nothing on the page answers to #missing");
}

#[tokio::test]
async fn probe_of_one_match_describes_it_in_one_line() {
    let mut d = ScriptedDriver::new(|method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        match method {
            "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
            "Runtime.callFunctionOn" if f == VISIBLE_JS => Ok(json!({ "result": { "value": true } })),
            "Runtime.callFunctionOn" if f == PROBE_SUMMARY_JS => Ok(json!({
                "result": { "value": { "tag": "button", "text": "Save changes", "rect": [120.0, 340.0, 200.0, 32.0] } }
            })),
            "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
            "Runtime.getProperties" => {
                Ok(json!({ "result": [ { "name": "0", "value": { "objectId": "el-0" } } ] }))
            }
            other => panic!("unexpected {other}"),
        }
    });
    let out = probe(&mut d, &Target::from("#save")).await.unwrap();
    assert_eq!(out, "matches: 1\nbutton \"Save changes\" visible at 120,340 200x32");
}

/// Three matches: one hidden, and (since N > 1) a last line asking the
/// caller to narrow the locator.
#[tokio::test]
async fn probe_of_several_matches_lists_each_and_asks_to_narrow() {
    let mut d = ScriptedDriver::new(|method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        let on = params["objectId"].as_str().unwrap_or("");
        match method {
            "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
            "Runtime.callFunctionOn" if f == VISIBLE_JS => Ok(json!({ "result": { "value": on != "el-1" } })),
            "Runtime.callFunctionOn" if f == PROBE_SUMMARY_JS => {
                let (tag, text, rect) = match on {
                    "el-0" => ("button", "Save", vec![10.0, 20.0, 80.0, 24.0]),
                    "el-1" => ("button", "Save", vec![10.0, 60.0, 80.0, 24.0]),
                    _ => ("a", "Save draft", vec![10.0, 100.0, 80.0, 24.0]),
                };
                Ok(json!({ "result": { "value": { "tag": tag, "text": text, "rect": rect } } }))
            }
            "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
            "Runtime.getProperties" => Ok(json!({ "result": [
                { "name": "0", "value": { "objectId": "el-0" } },
                { "name": "1", "value": { "objectId": "el-1" } },
                { "name": "2", "value": { "objectId": "el-2" } }
            ] })),
            other => panic!("unexpected {other}"),
        }
    });
    let out = probe(&mut d, &Target::from("text=Save")).await.unwrap();
    assert_eq!(
        out,
        "matches: 3\n\
         button \"Save\" visible at 10,20 80x24\n\
         button \"Save\" hidden at 10,60 80x24\n\
         a \"Save draft\" visible at 10,100 80x24\n\
         narrow the locator (add \"name\", \"exact\": true, a scope, or \"nth\")"
    );
}

/// C1: a password input's typed value is never printed by the probe -
/// the JS itself substitutes `[password]` before the value ever crosses
/// into the protocol reply this test scripts.
#[tokio::test]
async fn probe_of_a_password_field_never_shows_the_typed_value() {
    let mut d = ScriptedDriver::new(|method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        match method {
            "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
            "Runtime.callFunctionOn" if f == VISIBLE_JS => Ok(json!({ "result": { "value": true } })),
            "Runtime.callFunctionOn" if f == PROBE_SUMMARY_JS => Ok(json!({
                "result": { "value": { "tag": "input", "text": "[password]", "rect": [5.0, 5.0, 100.0, 20.0] } }
            })),
            "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
            "Runtime.getProperties" => {
                Ok(json!({ "result": [ { "name": "0", "value": { "objectId": "el-0" } } ] }))
            }
            other => panic!("unexpected {other}"),
        }
    });
    let out = probe(&mut d, &Target::from("#pw")).await.unwrap();
    assert!(out.contains("[password]"), "{out}");
    assert!(!out.contains("hunter2"), "{out}");
}

/// C1's drift guard: whatever else changes in `PROBE_SUMMARY_JS`, it must
/// keep checking `this.type` before ever touching `.value`.
#[test]
fn probe_summary_js_source_guards_password_inputs() {
    assert!(PROBE_SUMMARY_JS.contains("type === 'password'"), "{PROBE_SUMMARY_JS}");
}

/// I5: a control character in the page's own text is sanitised the same
/// way a name or value would be.
#[tokio::test]
async fn probe_text_with_a_control_character_is_sanitized() {
    let mut d = ScriptedDriver::new(|method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        match method {
            "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
            "Runtime.callFunctionOn" if f == VISIBLE_JS => Ok(json!({ "result": { "value": true } })),
            "Runtime.callFunctionOn" if f == PROBE_SUMMARY_JS => Ok(json!({
                "result": { "value": { "tag": "div", "text": "line one\nline two", "rect": [0.0, 0.0, 10.0, 10.0] } }
            })),
            "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
            "Runtime.getProperties" => {
                Ok(json!({ "result": [ { "name": "0", "value": { "objectId": "el-0" } } ] }))
            }
            other => panic!("unexpected {other}"),
        }
    });
    let out = probe(&mut d, &Target::from("#msg")).await.unwrap();
    assert_eq!(out, "matches: 1\ndiv \"line one line two\" visible at 0,0 10x10");
}

/// A `CdpError` from resolving the target propagates as-is.
#[tokio::test]
async fn a_resolve_failure_propagates() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.evaluate" => {
            Err(CdpError::Protocol { method: method.to_string(), message: "no document yet".into() })
        }
        other => panic!("unexpected {other}"),
    });
    let err = probe(&mut d, &Target::from("#go")).await.unwrap_err();
    assert!(matches!(err, CdpError::Protocol { .. }));
}
