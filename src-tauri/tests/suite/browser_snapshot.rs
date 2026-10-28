//! Seeing the page: the accessibility tree as text, and what a locator
//! matches right now.

use crate::common;

use common::ScriptedDriver;
use serde_json::json;
use v2_lib::browser::cdp::CdpError;
use v2_lib::browser::locator::{Target, VISIBLE_JS};
use v2_lib::browser::snapshot::{parse_nodes, probe, render, render_frames, render_frames_read, render_frames_with_lines, snapshot, snapshot_read, AxNode, FrameTree, DEFAULT_LIMIT, PROBE_SUMMARY_JS};

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
        backend: None,
        required: false,
        protected: false,
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
        node("2", "generic", "", &["3"]),
        // The button's own words: its name already says them.
        node("3", "button", "Add Method", &["4"]),
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
         2 more lines - call get_autorun_page with a larger limit or within",
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
    assert_eq!(render(&nodes, 0), "1 more lines - call get_autorun_page with a larger limit or within");
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
            "Accessibility.getPartialAXTree" => Ok(serde_json::json!({ "nodes": [] })),
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
            "Accessibility.getPartialAXTree" => Ok(serde_json::json!({ "nodes": [] })),
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
         nth 0: button \"Save\" visible at 10,20 80x24\n\
         nth 1: button \"Save\" hidden at 10,60 80x24\n\
         nth 2: a \"Save draft\" visible at 10,100 80x24\n\
         narrow the locator (add \"name\", \"exact\": true, a scope, or \"nth\")"
    );
}

/// Twelve matches: the probe counts every one, lists the first ten each with
/// the 0-based `nth` that picks it, and says so.
#[tokio::test]
async fn probe_counts_every_match_and_labels_the_listed_ones_by_nth() {
    let mut d = ScriptedDriver::new(|method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        match method {
            "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
            "Runtime.callFunctionOn" if f == VISIBLE_JS => Ok(json!({ "result": { "value": true } })),
            "Accessibility.getPartialAXTree" => Ok(json!({ "nodes": [] })),
            "Runtime.callFunctionOn" if f == PROBE_SUMMARY_JS => {
                Ok(json!({ "result": { "value": { "tag": "li", "text": "Row", "rect": [0.0, 0.0, 10.0, 10.0] } } }))
            }
            "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
            "Runtime.getProperties" => Ok(json!({ "result": (0..12)
                .map(|i| json!({ "name": i.to_string(), "value": { "objectId": format!("el-{i}") } }))
                .collect::<Vec<_>>() })),
            other => panic!("unexpected {other}"),
        }
    });
    let out = probe(&mut d, &Target::from("li")).await.unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "matches: 12");
    assert_eq!(lines[1], "nth 0: li \"Row\" visible at 0,0 10x10");
    assert_eq!(lines[10], "nth 9: li \"Row\" visible at 0,0 10x10");
    assert_eq!(lines[11], "(the first 10 are listed, nth 0 to 9; 12 matched in all)");
    assert!(lines.last().unwrap().contains("narrow the locator"), "{out}");
    assert!(!out.contains("nth 10"), "{out}");
}

/// A page name with spaces round it is said, and an `exact` name is compared
/// trimmed, so the spaces never stop it matching.
#[tokio::test]
async fn probe_says_when_the_pages_name_has_leading_or_trailing_spaces() {
    let mut d = ScriptedDriver::new(|method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        match method {
            "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
            "Runtime.callFunctionOn" if f == VISIBLE_JS => Ok(json!({ "result": { "value": true } })),
            "Accessibility.getPartialAXTree" => Ok(json!({ "nodes": [
                { "nodeId": "1", "role": { "value": "button" }, "name": { "value": "  Save " }, "ignored": false }
            ] })),
            "Runtime.callFunctionOn" if f == PROBE_SUMMARY_JS => {
                Ok(json!({ "result": { "value": { "tag": "button", "text": "Save", "rect": [1.0, 2.0, 3.0, 4.0] } } }))
            }
            "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
            "Runtime.getProperties" => Ok(json!({ "result": [ { "name": "0", "value": { "objectId": "el-0" } } ] })),
            other => panic!("unexpected {other}"),
        }
    });
    let out = probe(&mut d, &Target::from("#save")).await.unwrap();
    assert!(out.starts_with("matches: 1\nbutton \"Save\" visible at 1,2 3x4"), "{out}");
    assert!(out.contains("the page's name has leading or trailing spaces"), "{out}");
    assert!(out.contains("\"exact\": true still matches it"), "{out}");
    // The comparison the note promises.
    assert!(v2_lib::browser::locator::name_matches("  Save ", "Save", true));
    assert!(v2_lib::browser::locator::name_matches("Save", " Save  ", true));
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
            "Accessibility.getPartialAXTree" => Ok(serde_json::json!({ "nodes": [] })),
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
            "Accessibility.getPartialAXTree" => Ok(serde_json::json!({ "nodes": [] })),
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

// --- frames ---------------------------------------------------------------

/// A page with one iframe ("Employee Search", AX id `f1`) and the frame's
/// own tree: a root and `n` buttons named "B1".."Bn".
fn page_with_frame(n: usize) -> (Vec<AxNode>, FrameTree) {
    let parent = vec![node("1", "RootWebArea", "Frames", &["f1"]), node("f1", "Iframe", "Employee Search", &[])];
    let ids: Vec<String> = (1..=n).map(|i| format!("b{i}")).collect();
    let mut nodes = vec![node("1", "RootWebArea", "", &ids.iter().map(String::as_str).collect::<Vec<_>>())];
    nodes.extend(ids.iter().enumerate().map(|(i, id)| node(id, "button", &format!("B{}", i + 1), &[])));
    let frame = FrameTree {
        iframe_id: "f1".to_string(),
        step: json!({ "css": "iframe[title='Employee Search']" }),
        nodes,
        unreadable: None,
        cross_origin: false,
        frames: vec![],
    };
    (parent, frame)
}

#[test]
fn a_frame_tree_prints_under_its_iframe_with_chained_locators() {
    let (parent, frame) = page_with_frame(1);
    let out = render_frames(&parent, &[frame], DEFAULT_LIMIT);
    let lines: Vec<&str> = out.lines().collect();
    let iframe_line = lines.iter().position(|l| l.trim_start().starts_with("Iframe")).expect("no iframe line");
    let button_line = lines.iter().position(|l| l.contains("\"B1\"")).expect("no frame button line");
    assert!(button_line > iframe_line, "{out}");
    let indent = |l: &str| l.len() - l.trim_start().len();
    assert!(indent(lines[button_line]) > indent(lines[iframe_line]), "{out}");
    let chain = json!([{ "css": "iframe[title='Employee Search']" }, { "role": "button", "name": "B1" }]);
    assert!(lines[button_line].ends_with(&format!(" -> {chain}")), "{out}");
}

#[test]
fn an_unreadable_frame_prints_one_line_saying_so() {
    let (parent, mut frame) = page_with_frame(0);
    frame.nodes.clear();
    frame.unreadable = Some("no frame id".to_string());
    let out = render_frames(&parent, &[frame], DEFAULT_LIMIT);
    assert!(out.contains("(frame contents could not be read: no frame id)"), "{out}");
}

#[test]
fn the_line_limit_counts_frame_lines() {
    let (parent, frame) = page_with_frame(5);
    // 2 parent lines (root, iframe) + the frame's root + 5 buttons = 8; a
    // limit of 4 keeps 4 and says 4 more.
    let out = render_frames(&parent, &[frame], 4);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 5, "{out}");
    assert!(lines[4].starts_with("4 more lines - "), "{out}");
}

#[test]
fn render_without_frames_is_unchanged() {
    let (parent, _) = page_with_frame(1);
    assert_eq!(render(&parent, DEFAULT_LIMIT), render_frames(&parent, &[], DEFAULT_LIMIT));
}

/// A frame tree holding one iframe (AX id `inner`) and nothing else, its
/// own tree `frames` inside it.
fn nested(id: &str, title: &str, frames: Vec<FrameTree>) -> FrameTree {
    FrameTree {
        iframe_id: id.to_string(),
        step: json!({ "css": format!("iframe[title='{title}']") }),
        nodes: vec![node("r", "RootWebArea", "", &["inner"]), node("inner", "Iframe", &format!("{title} child"), &[])],
        unreadable: None,
        cross_origin: false,
        frames,
    }
}

/// Spec 9: frames are followed three deep. A frame inside the third one is
/// not silently missing - one line under its iframe says it is not shown.
#[test]
fn a_frame_deeper_than_three_says_it_is_not_shown() {
    let parent = vec![node("1", "RootWebArea", "Frames", &["f1"]), node("f1", "Iframe", "One", &[])];
    let three = nested("inner", "Three", vec![]);
    let two = nested("inner", "Two", vec![three]);
    let one = nested("f1", "One", vec![two]);
    let out = render_frames(&parent, &[one], DEFAULT_LIMIT);
    let lines: Vec<&str> = out.lines().collect();
    let deep = lines.iter().position(|l| l.contains("\"Three child\"")).unwrap_or_else(|| panic!("{out}"));
    let note = lines.get(deep + 1).copied().unwrap_or("");
    assert_eq!(note.trim(), "(frames nested deeper than 3 are not shown)", "{out}");
    let indent = |l: &str| l.len() - l.trim_start().len();
    assert!(indent(note) > indent(lines[deep]), "the note sits under the frame's line: {out}");
    assert_eq!(out.matches("nested deeper than 3").count(), 1, "{out}");
}

/// A frame whose contents were not shown (unreadable, or past the depth
/// the snapshot follows) makes the read not the whole page, as the line
/// limit does: `cut` is set. A read whose every frame was shown is whole.
#[test]
fn a_read_missing_a_frame_is_cut() {
    let (parent, frame) = page_with_frame(1);
    assert!(!render_frames_read(&parent, &[frame], DEFAULT_LIMIT).cut, "a whole read with its frame was cut");

    let (parent, mut unreadable) = page_with_frame(0);
    unreadable.nodes.clear();
    unreadable.unreadable = Some("no frame id".to_string());
    let read = render_frames_read(&parent, &[unreadable.clone()], DEFAULT_LIMIT);
    assert!(read.cut, "{}", read.text);
    assert!(!read.text.contains("... and"), "{}", read.text);

    // An unreadable frame inside a readable one counts too.
    let outer = nested("f1", "One", vec![FrameTree { iframe_id: "inner".into(), ..unreadable }]);
    let parent = vec![node("1", "RootWebArea", "Frames", &["f1"]), node("f1", "Iframe", "One", &[])];
    let read = render_frames_read(&parent, &[outer], DEFAULT_LIMIT);
    assert!(read.text.contains("frame contents could not be read"), "{}", read.text);
    assert!(read.cut, "{}", read.text);

    let three = nested("inner", "Three", vec![]);
    let deep = nested("f1", "One", vec![nested("inner", "Two", vec![three])]);
    let read = render_frames_read(&parent, &[deep], DEFAULT_LIMIT);
    assert!(read.text.contains("nested deeper than 3"), "{}", read.text);
    assert!(read.cut, "{}", read.text);
}

/// A frame the page cannot reach (another site) holds nothing that could
/// ever be recorded or scripted: its note line stays, but it alone leaves
/// the read whole. A frame unreadable for any other reason still cuts it.
#[test]
fn only_a_frame_from_another_site_leaves_the_read_whole() {
    let parent = vec![
        node("1", "RootWebArea", "Frames", &["f1", "f2"]),
        node("f1", "Iframe", "Badge", &[]),
        node("f2", "Iframe", "Report", &[]),
    ];
    let unread = |id: &str, why: &str, cross_origin: bool| FrameTree {
        iframe_id: id.into(),
        step: json!({ "css": "iframe" }),
        nodes: vec![],
        unreadable: Some(why.into()),
        cross_origin,
        frames: vec![],
    };
    let badge = unread("f1", "another site", true);
    let report = unread("f2", "the frame tree did not come back", false);

    let read = render_frames_read(&parent, &[badge.clone()], DEFAULT_LIMIT);
    assert!(read.text.contains("(frame contents could not be read: another site)"), "{}", read.text);
    assert!(!read.cut, "a frame from another site cut the read: {}", read.text);

    let read = render_frames_read(&parent, &[badge, report], DEFAULT_LIMIT);
    assert!(read.cut, "a frame that failed to read did not cut the read: {}", read.text);
}

/// An iframe Chrome gave no DOM node for is not followed: a line says so,
/// and the read is not the whole page.
#[tokio::test]
async fn an_iframe_with_no_node_is_named_and_cuts_the_read() {
    let mut d = ScriptedDriver::new(|method, _| {
        Ok(match method {
            "Accessibility.getFullAXTree" => json!({ "nodes": [
                { "nodeId": "1", "ignored": false, "role": { "value": "RootWebArea" }, "name": { "value": "Page" },
                  "childIds": ["2", "3"] },
                { "nodeId": "2", "ignored": false, "role": { "value": "button" }, "name": { "value": "Save" }, "childIds": [] },
                { "nodeId": "3", "ignored": false, "role": { "value": "Iframe" }, "name": { "value": "Help" }, "childIds": [] }
            ] }),
            _ => json!({}),
        })
    });
    let read = snapshot_read(&mut d, DEFAULT_LIMIT).await.unwrap();
    assert!(read.text.contains("(frame contents could not be read: Chrome gave no node for it)"), "{}", read.text);
    assert!(read.cut, "{}", read.text);
}

/// Shallower frames that were not followed (no tree for them) print no note.
#[test]
fn a_shallow_frame_with_no_tree_prints_no_deep_note() {
    let parent = vec![node("1", "RootWebArea", "Frames", &["f1"]), node("f1", "Iframe", "One", &[])];
    let out = render_frames(&parent, &[nested("f1", "One", vec![])], DEFAULT_LIMIT);
    assert!(!out.contains("nested deeper"), "{out}");
    assert!(out.contains("\"One child\""), "{out}");
}

// --- lines as data ------------------------------------------------------

/// The text is the same bytes `render_frames` gives, and every line that
/// carries a locator has one `SnapLine`, whose locator is the one printed.
#[test]
fn render_with_lines_matches_the_text_render() {
    let nodes = vec![
        node("1", "dialog", "Add Rating Method", &["2", "3"]),
        node("2", "button", "Add Method", &[]),
        node("3", "textbox", "", &[]),
    ];
    let frames = vec![];
    let (text, lines) = render_frames_with_lines(&nodes, &frames, DEFAULT_LIMIT);
    assert_eq!(text, render_frames(&nodes, &frames, DEFAULT_LIMIT));
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[1].role, "button");
    assert_eq!(lines[1].name, "Add Method");
    assert_eq!(lines[1].locator, serde_json::from_value::<Target>(json!({ "role": "button", "name": "Add Method" })).unwrap());
    assert_eq!(lines[2].locator, serde_json::from_value::<Target>(json!({ "role": "textbox" })).unwrap());
}

/// A line inside an iframe carries the frame's step in front of its own.
#[test]
fn a_line_inside_a_frame_has_a_chain_locator() {
    let nodes = vec![node("1", "main", "", &["2"]), node("2", "Iframe", "Pay", &[])];
    let frames = vec![FrameTree {
        iframe_id: "2".into(),
        step: json!({ "role": "Iframe", "name": "Pay", "exact": true }),
        nodes: vec![node("1", "button", "Go", &[])],
        unreadable: None,
        cross_origin: false,
        frames: vec![],
    }];
    let (text, lines) = render_frames_with_lines(&nodes, &frames, DEFAULT_LIMIT);
    assert_eq!(text, render_frames(&nodes, &frames, DEFAULT_LIMIT));
    let want: Target = serde_json::from_value(json!([{ "role": "Iframe", "name": "Pay", "exact": true }, { "role": "button", "name": "Go" }])).unwrap();
    assert_eq!(lines.last().unwrap().locator, want);
}

#[test]
fn a_required_textbox_is_marked() {
    let raw = json!({ "nodes": [
        { "nodeId": "1", "role": { "value": "textbox" }, "name": { "value": "Email" },
          "properties": [{ "name": "required", "value": { "type": "boolean", "value": true } }] },
        { "nodeId": "2", "role": { "value": "textbox" }, "name": { "value": "Nick" } }
    ] });
    let nodes = parse_nodes(&raw);
    assert!(nodes[0].required);
    assert!(!nodes[1].required);
    let flagged = vec![AxNode { required: true, ..node("1", "textbox", "Email", &[]) }];
    let (_, lines) = render_frames_with_lines(&flagged, &[], DEFAULT_LIMIT);
    assert!(lines[0].required);
}

#[test]
fn lines_past_the_limit_are_not_returned() {
    let nodes = vec![
        node("1", "form", "", &["2", "3", "4"]),
        node("2", "button", "A", &[]),
        node("3", "button", "B", &[]),
        node("4", "button", "C", &[]),
    ];
    let (text, lines) = render_frames_with_lines(&nodes, &[], 2);
    assert_eq!(lines.len(), 2);
    assert_eq!(text, render_frames(&nodes, &[], 2));
    assert_eq!(lines[1].name, "A");
}

// --- lines no locator can act on, and reads of one element ---------------

/// The page itself, a list bullet and a closed native list (with what it
/// holds) print as lines, for what they say, but with no locator: the
/// document and a pseudo-element are not elements, and a native list's
/// options are picked with `fill` on the list. Each role in
/// `NO_LOCATOR_ROLES` is held here; every other role keeps its locator.
#[test]
fn lines_nothing_can_act_on_print_without_a_locator() {
    use v2_lib::browser::snapshot::NO_LOCATOR_ROLES;
    let nodes = vec![
        node("1", "RootWebArea", "Page", &["2", "3", "6"]),
        node("2", "ListMarker", "1. ", &[]),
        node("3", "combobox", "Colour", &["4"]),
        node("4", "MenuListPopup", "", &["5"]),
        node("5", "option", "Red", &[]),
        node("6", "switch", "Email alerts", &[]),
    ];
    let read = render_frames_read(&nodes, &[], DEFAULT_LIMIT);
    let expected = format!(
        "RootWebArea \"Page\"\n ListMarker \"1. \"\n combobox \"Colour\"{}\n  MenuListPopup \"\"\n   option \"Red\"\n switch \"Email alerts\"{}",
        locator("combobox", "Colour"),
        locator("switch", "Email alerts"),
    );
    assert_eq!(read.text, expected);
    let roles: Vec<&str> = read.lines.iter().map(|l| l.role.as_str()).collect();
    assert_eq!(roles, vec!["combobox", "switch"], "only lines with a locator are data");
    for role in NO_LOCATOR_ROLES {
        assert!(nodes.iter().any(|n| n.role == role), "{role} is not held by this test");
    }
}

/// A read cut short by its limit ends with how many lines it left out and
/// how to see them, and still knows every line of the page, so what it
/// lists can be counted against the whole page.
#[test]
fn a_cut_read_says_how_many_more_lines_and_keeps_the_page_lines() {
    let nodes = vec![
        node("1", "main", "", &["2", "3", "4"]),
        node("2", "button", "One", &[]),
        node("3", "button", "Two", &[]),
        node("4", "button", "Three", &[]),
    ];
    let read = render_frames_read(&nodes, &[], 2);
    assert!(read.cut);
    assert!(
        read.text.ends_with("\n2 more lines - call get_autorun_page with a larger limit or within"),
        "{}",
        read.text
    );
    assert_eq!(read.lines.len(), 2, "the two lines listed");
    assert_eq!(read.page_lines.as_ref().map(Vec::len), Some(4), "every line of the page is known");
}

/// `within` lists the subtree of the one element behind a DOM node, from
/// its own line down, at depth 0, with locators written as the whole read
/// writes them; it is always cut (never the whole page) and still knows
/// the page's lines. A folded element (a plain `div`) lists its children.
/// An element the tree does not list says it holds nothing.
#[test]
fn a_within_read_lists_one_elements_subtree() {
    use v2_lib::browser::snapshot::render_frames_read_within;
    let mut dialog = node("2", "dialog", "Edit", &["3"]);
    dialog.backend = Some(20);
    let mut div = node("4", "generic", "", &["5"]);
    div.backend = Some(40);
    let nodes = vec![
        node("1", "RootWebArea", "Page", &["2", "4", "6"]),
        dialog,
        node("3", "button", "Save", &[]),
        div,
        node("5", "button", "Save", &[]),
        node("6", "link", "Home", &[]),
    ];
    let read = render_frames_read_within(&nodes, &[], DEFAULT_LIMIT, 20);
    assert_eq!(read.text, format!("dialog \"Edit\"{}\n button \"Save\"{}", locator("dialog", "Edit"), locator("button", "Save")));
    assert!(read.cut, "a within read is never the whole page");
    assert_eq!(read.page_lines.as_ref().map(Vec::len), Some(4), "{:?}", read.page_lines);
    let folded = render_frames_read_within(&nodes, &[], DEFAULT_LIMIT, 40);
    assert_eq!(folded.text, format!("button \"Save\"{}", locator("button", "Save")));
    let none = render_frames_read_within(&nodes, &[], DEFAULT_LIMIT, 99);
    assert_eq!(none.text, "that element holds nothing a locator could name");
    assert!(none.lines.is_empty());
}


// --- render: text lines -------------------------------------------------

fn text_locator(text: &str) -> String {
    format!(" -> {}", json!({ "text": text }))
}

/// Visible text Chrome folds away (a span, a div, a paragraph's words)
/// prints as a `text` line with a text locator, unless the nearest printed
/// line's name already says it (a button's own words), at the depth the
/// folded node would have used. Blank text and its InlineTextBox children
/// print nothing.
#[test]
fn folded_text_prints_as_a_text_line_unless_its_parent_says_it() {
    let nodes = vec![
        node("1", "RootWebArea", "Cycles", &["2"]),
        node("2", "generic", "", &["3", "4", "5", "7", "9"]),
        node("3", "StaticText", "Name", &[]),
        node("4", "StaticText", "  AUTOTEST   Preview 2027 ", &["40"]),
        node("40", "InlineTextBox", "AUTOTEST Preview 2027", &[]),
        node("5", "paragraph", "", &["6"]),
        node("6", "StaticText", "Self 20%", &[]),
        node("7", "button", "Edit AUTOTEST Preview 2027", &["8"]),
        node("8", "StaticText", "edit autotest preview 2027", &[]),
        node("9", "StaticText", "   ", &[]),
    ];
    let expected = [
        "RootWebArea \"Cycles\"".to_string(),
        format!(" text \"Name\"{}", text_locator("Name")),
        format!(" text \"AUTOTEST Preview 2027\"{}", text_locator("AUTOTEST Preview 2027")),
        format!(" paragraph \"\"{}", locator_role_only("paragraph")),
        format!("  text \"Self 20%\"{}", text_locator("Self 20%")),
        format!(" button \"Edit AUTOTEST Preview 2027\"{}", locator("button", "Edit AUTOTEST Preview 2027")),
    ]
    .join("
");
    assert_eq!(render(&nodes, DEFAULT_LIMIT), expected);
    let read = render_frames_read(&nodes, &[], DEFAULT_LIMIT);
    let texts: Vec<&str> = read.lines.iter().filter(|l| l.role == "text").map(|l| l.name.as_str()).collect();
    assert_eq!(texts, vec!["Name", "AUTOTEST Preview 2027", "Self 20%"]);
    let t: Target = serde_json::from_value(json!({ "text": "Self 20%" })).unwrap();
    assert!(read.lines.iter().any(|l| l.locator == t));
}

/// Text in a frame is reached through the frame, as every line in it is.
#[test]
fn a_text_line_in_a_frame_carries_the_frame_step() {
    let nodes = vec![node("1", "RootWebArea", "", &["2"]), AxNode { backend: Some(9), ..node("2", "Iframe", "", &[]) }];
    let frame = FrameTree {
        iframe_id: "2".into(),
        step: json!({ "css": "iframe", "nth": 0 }),
        nodes: vec![node("f1", "RootWebArea", "", &["f2"]), node("f2", "StaticText", "Inside the frame", &[])],
        unreadable: None,
        cross_origin: false,
        frames: vec![],
    };
    let text = render_frames(&nodes, &[frame], DEFAULT_LIMIT);
    let chain = json!([{ "css": "iframe", "nth": 0 }, { "text": "Inside the frame" }]);
    assert!(text.contains(&format!("text \"Inside the frame\" -> {chain}")), "{text}");
}

/// The line cap keeps every control before any text line: with more lines
/// than the limit, text lines are what is left out, and the cut is said.
#[test]
fn the_line_cap_drops_text_lines_before_controls() {
    let mut nodes = vec![node("1", "RootWebArea", "", &[])];
    let mut kids = Vec::new();
    for i in 0..4 {
        let (b, t) = (format!("b{i}"), format!("t{i}"));
        nodes.push(node(&t, "StaticText", &format!("Value {i}"), &[]));
        nodes.push(node(&b, "button", &format!("Button {i}"), &[]));
        kids.push(t);
        kids.push(b);
    }
    nodes[0].children = kids;
    let read = render_frames_read(&nodes, &[], 6);
    let roles: Vec<(&str, &str)> = read.lines.iter().map(|l| (l.role.as_str(), l.name.as_str())).collect();
    assert_eq!(
        roles,
        vec![("text", "Value 0"), ("button", "Button 0"), ("button", "Button 1"), ("button", "Button 2"), ("button", "Button 3")]
    );
    assert!(read.cut);
    assert!(read.text.ends_with("3 more lines - call get_autorun_page with a larger limit or within"), "{}", read.text);
    // More controls (the page line among them) than the limit: the first
    // of them, and no text at all.
    let read = render_frames_read(&nodes, &[], 3);
    let names: Vec<&str> = read.lines.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["Button 0", "Button 1"]);
}

/// The probe says the name the page read gives the element, where its own
/// text is not that name: an icon button named by its aria-label.
#[tokio::test]
async fn probe_says_the_name_the_page_read_gives() {
    let mut d = ScriptedDriver::new(|method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        match method {
            "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
            "Runtime.callFunctionOn" if f == VISIBLE_JS => Ok(json!({ "result": { "value": true } })),
            "Accessibility.getPartialAXTree" => {
                assert_eq!(params["objectId"], "el-0");
                Ok(json!({ "nodes": [{ "nodeId": "5", "role": { "value": "button" }, "name": { "value": "Edit AUTOTEST Preview 2027" } }] }))
            }
            "Runtime.callFunctionOn" if f == PROBE_SUMMARY_JS => Ok(json!({
                "result": { "value": { "tag": "button", "text": "", "rect": [1.0, 2.0, 24.0, 24.0] } }
            })),
            "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
            "Runtime.getProperties" => Ok(json!({ "result": [ { "name": "0", "value": { "objectId": "el-0" } } ] })),
            other => panic!("unexpected {other}"),
        }
    });
    let out = probe(&mut d, &Target::from("button.edit")).await.unwrap();
    assert_eq!(out, "matches: 1
button \"\" visible at 1,2 24x24 - page read: button \"Edit AUTOTEST Preview 2027\"");
}

/// The name the page read gives an element, after its probe line, is not
/// read as the line's visibility: a visible icon button still counts.
#[test]
fn a_probe_line_with_the_page_read_name_still_counts_as_visible() {
    use v2_lib::ai_bridge::visible_matches;
    let with_name = "matches: 1\nbutton \"\" visible at 1,2 24x24 - page read: button \"Edit AUTOTEST Preview 2027\"";
    assert_eq!(visible_matches(with_name), Some(1));
    let hidden = "matches: 1\nbutton \"\" hidden at 1,2 24x24 - page read: button \"Edit\"";
    assert_eq!(visible_matches(hidden), None);
    let two = "matches: 2\nbutton \"Save\" visible at 1,2 3x4\nbutton \"\" visible at 5,6 7x8 - page read: button \"Save draft\"";
    assert_eq!(visible_matches(two), Some(2));
}

/// Text inside a field is its value, never a text line: a textbox's typed
/// words, a combobox's pick, and above all a password's, protected or not.
#[test]
fn text_inside_a_field_never_prints_as_a_text_line() {
    let nodes = vec![
        node("1", "RootWebArea", "", &["2", "4", "6", "8", "10"]),
        AxNode { value: Some("typed words".into()), ..node("2", "textbox", "Notes", &["3"]) },
        node("3", "StaticText", "typed words", &[]),
        AxNode { protected: true, ..node("4", "textbox", "Secret", &["5"]) },
        node("5", "StaticText", "s3cret", &[]),
        node("6", "combobox", "Colour", &["7"]),
        node("7", "StaticText", "Teal", &[]),
        node("8", "spinbutton", "Count", &["9"]),
        node("9", "generic", "", &["90"]),
        node("90", "StaticText", "42", &[]),
        node("10", "StaticText", "Outside", &[]),
    ];
    let read = render_frames_read(&nodes, &[], DEFAULT_LIMIT);
    let texts: Vec<&str> = read.lines.iter().filter(|l| l.role == "text").map(|l| l.name.as_str()).collect();
    assert_eq!(texts, vec!["Outside"]);
    for never in ["typed words\" ->", "s3cret", "Teal\" ->", "\"42\""] {
        assert!(!read.text.contains(&format!("text \"{never}")) && !read.text.contains("s3cret"), "{never}: {}", read.text);
    }
}

/// A page with two nested `<main>` elements and two more, reached by a css
/// `within` (`CSS_JS` hands back every match) or by role (`queryAXTree`):
/// `el-0` is `main#app.layout` named "Shell", `el-1` `main.content` with no
/// name, and so on.
fn four_mains() -> ScriptedDriver {
    use v2_lib::browser::snapshot::WITHIN_MATCH_JS;
    ScriptedDriver::new(|method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        let on = params["objectId"].as_str().unwrap_or("");
        match method {
            "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
            "Runtime.callFunctionOn" if f == VISIBLE_JS => Ok(json!({ "result": { "value": true } })),
            "Runtime.callFunctionOn" if f == WITHIN_MATCH_JS => {
                let said = match on {
                    "el-0" => "main#app.layout",
                    "el-1" => "main.content",
                    "el-2" => "main#side",
                    _ => "main",
                };
                Ok(json!({ "result": { "value": said } }))
            }
            "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
            "Runtime.getProperties" => Ok(json!({ "result": [
                { "name": "0", "value": { "objectId": "el-0" } },
                { "name": "1", "value": { "objectId": "el-1" } },
                { "name": "2", "value": { "objectId": "el-2" } },
                { "name": "3", "value": { "objectId": "el-3" } }
            ] })),
            "Accessibility.queryAXTree" => Ok(json!({ "nodes": [
                { "nodeId": "1", "role": { "value": "main" }, "name": { "value": "Shell" }, "backendDOMNodeId": 10 },
                { "nodeId": "2", "role": { "value": "main" }, "name": { "value": "" }, "backendDOMNodeId": 11 }
            ] })),
            "DOM.resolveNode" => {
                let backend = params["backendNodeId"].as_i64().unwrap();
                Ok(json!({ "object": { "objectId": format!("el-{}", backend - 10) } }))
            }
            "DOM.describeNode" => {
                let n: i64 = on.trim_start_matches("el-").parse().unwrap_or(0);
                Ok(json!({ "node": { "backendNodeId": 10 + n } }))
            }
            "Accessibility.getPartialAXTree" => Ok(match on {
                "el-0" => json!({ "nodes": [{ "nodeId": "1", "role": { "value": "main" }, "name": { "value": "Shell" } }] }),
                _ => json!({ "nodes": [{ "nodeId": "2", "role": { "value": "main" }, "name": { "value": "" } }] }),
            }),
            other => panic!("unexpected {other}"),
        }
    })
}

/// A `within` that matches several names up to three of them, the way a
/// page read lists a line: a css match as tag#id.class, a role match as
/// its role and name (tag#id.class when the read gives it no name).
#[tokio::test]
async fn a_within_matching_several_names_the_first_three() {
    use v2_lib::ai_bridge::within_target;
    use v2_lib::browser::snapshot::within_node;
    let mut d = four_mains();
    let refused = within_node(&mut d, &within_target("main").unwrap()).await.unwrap().unwrap_err();
    assert!(refused.starts_with("within matched 4 elements: main must match exactly one"), "{refused}");
    assert!(refused.ends_with(" The first 3 of 4: main#app.layout; main.content; main#side."), "{refused}");

    let mut d = four_mains();
    let by_role = within_target(r#"{"role":"main"}"#).unwrap();
    let refused = within_node(&mut d, &by_role).await.unwrap().unwrap_err();
    assert!(refused.starts_with("within matched 2 elements: main must match exactly one"), "{refused}");
    assert!(refused.ends_with(" They are: main \"Shell\"; main.content."), "{refused}");
}

/// `nth` picks one of the elements a `within` matches, counting from 0 as a
/// locator's `nth` does: on the last link, so a chain picks among its final
/// matches. Without `within`, given twice, or not a count, it is refused.
#[tokio::test]
async fn a_within_nth_picks_one_of_its_matches() {
    use v2_lib::ai_bridge::within_target_nth;
    use v2_lib::browser::snapshot::within_node;
    let picked = within_target_nth(Some("main"), Some("1")).unwrap().unwrap();
    assert_eq!(picked.links().last().unwrap().nth, Some(1));
    let mut d = four_mains();
    assert_eq!(within_node(&mut d, &picked).await.unwrap(), Ok(11), "nth 1 is the second match");

    let chain = within_target_nth(Some(r#"[{"role":"dialog"},{"css":"main"}]"#), Some("0")).unwrap().unwrap();
    let links = chain.links();
    assert_eq!((links[0].nth, links[1].nth), (None, Some(0)));

    assert_eq!(within_target_nth(None, None), Ok(None));
    assert!(within_target_nth(None, Some("1")).unwrap_err().contains("within"));
    assert!(within_target_nth(Some(r#"{"css":"main","nth":0}"#), Some("1")).unwrap_err().contains("nth"));
    assert!(within_target_nth(Some("main"), Some("-1")).unwrap_err().contains("from 0"));
    assert!(within_target_nth(Some("main"), Some("x")).unwrap_err().contains("from 0"));
}

/// An `nth` past the end of what `within` matches says so, with how many
/// it matches, rather than reading as if nothing on the page answered.
#[tokio::test]
async fn a_within_nth_past_the_end_says_how_many_there_are() {
    use v2_lib::ai_bridge::within_target_nth;
    use v2_lib::browser::snapshot::within_node;
    let mut d = four_mains();
    let past = within_target_nth(Some("main"), Some("5")).unwrap().unwrap();
    let refused = within_node(&mut d, &past).await.unwrap().unwrap_err();
    assert_eq!(refused, "nth 5 is past the end: within matches 4 elements (nth 0 to 3)");
    let mut d = four_mains();
    let past = within_target_nth(Some(r#"{"role":"main","name":"Shell"}"#), Some("1")).unwrap().unwrap();
    let refused = within_node(&mut d, &past).await.unwrap().unwrap_err();
    assert_eq!(refused, "nth 1 is past the end: within matches 1 element (nth 0)");
}

/// A match the browser cannot name while the refusal is written (it went
/// from the page) is "an element": the refusal is still said.
#[tokio::test]
async fn a_within_match_that_cannot_be_named_is_an_element() {
    use v2_lib::ai_bridge::within_target;
    use v2_lib::browser::snapshot::{within_node, WITHIN_MATCH_JS};
    let mut d = ScriptedDriver::new(|method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        match method {
            "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
            "Runtime.callFunctionOn" if f == WITHIN_MATCH_JS => {
                Err(CdpError::Protocol { method: "Runtime.callFunctionOn".into(), message: "No node with given id".into() })
            }
            "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
            "Runtime.getProperties" => Ok(json!({ "result": [
                { "name": "0", "value": { "objectId": "el-0" } },
                { "name": "1", "value": { "objectId": "el-1" } }
            ] })),
            other => panic!("unexpected {other}"),
        }
    });
    let refused = within_node(&mut d, &within_target("section").unwrap()).await.unwrap().unwrap_err();
    assert!(refused.starts_with("within matched 2 elements: section must match exactly one"), "{refused}");
    assert!(refused.ends_with(" They are: an element; an element."), "{refused}");
}

/// A plain css string is a step's first match only (`LEGACY_JS`); the probe
/// lists it in its css form, so every match shows by `nth`, and says a step
/// using the plain string acts on the first. The fake models both.
#[tokio::test]
async fn probe_of_a_plain_css_string_lists_every_match_and_says_a_step_takes_the_first() {
    use v2_lib::browser::locator::{CSS_JS, LEGACY_JS};
    let build = || {
        ScriptedDriver::new(|method, params| {
            let f = params["functionDeclaration"].as_str().unwrap_or("");
            let on = params["objectId"].as_str().unwrap_or("");
            match method {
                "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
                "Runtime.callFunctionOn" if f == LEGACY_JS => Ok(json!({ "result": { "objectId": "one" } })),
                "Runtime.callFunctionOn" if f == CSS_JS => Ok(json!({ "result": { "objectId": "four" } })),
                "Runtime.callFunctionOn" if f == VISIBLE_JS => Ok(json!({ "result": { "value": true } })),
                "Accessibility.getPartialAXTree" => Ok(json!({ "nodes": [] })),
                "Runtime.callFunctionOn" if f == PROBE_SUMMARY_JS => {
                    Ok(json!({ "result": { "value": { "tag": "i", "text": "x", "rect": [0.0, 0.0, 1.0, 1.0] } } }))
                }
                "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
                "Runtime.getProperties" => {
                    let n = if on == "one" { 1 } else { 4 };
                    Ok(json!({ "result": (0..n).map(|i| json!({ "name": i.to_string(), "value": { "objectId": format!("el-{i}") } })).collect::<Vec<_>>() }))
                }
                other => panic!("unexpected {other}"),
            }
        })
    };
    let out = probe(&mut build(), &Target::from("main i")).await.unwrap();
    assert!(out.starts_with("matches: 4\nnth 0: i "), "{out}");
    assert!(out.contains("nth 3: i "), "{out}");
    assert!(out.contains("acts on the first match (nth 0)"), "{out}");
    // As a run takes it, the same string is one match.
    let exact = v2_lib::browser::snapshot::probe_exact(&mut build(), &Target::from("main i")).await.unwrap();
    assert!(exact.starts_with("matches: 1"), "{exact}");
}
