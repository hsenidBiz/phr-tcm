//! The test case Priority field (`Microsoft.VSTS.Common.Priority`): a
//! create always writes one (the case's, else 2), an update writes it only
//! when the case sets one, the read path parses it, the JSON importer
//! accepts 1-4 and refuses anything else, the exporter leaves it out when
//! unset, and every transform keeps it.

use v2_lib::ado::endpoints::parse_priority;
use v2_lib::ado::{AdoClient, BlankPolicy};
use v2_lib::import_parser::{merge_cases_into_draft, parse_json_text, queue_to_json_string};
use v2_lib::model::TestCase;
use v2_lib::steps_xml::Step;
use v2_lib::transform::{apply, parse_ops, SUPPORTED_OPS};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PRIORITY: &str = "/fields/Microsoft.VSTS.Common.Priority";

fn case(title: &str, priority: Option<i32>) -> TestCase {
    TestCase {
        title: title.into(),
        steps: vec![Step { action: "Open the page.".into(), expected: "It opens.".into(), shared: None }],
        automation_status: "Not Automated".into(),
        priority,
        ..Default::default()
    }
}

fn priority_op(ops: &[serde_json::Value]) -> Option<&serde_json::Value> {
    ops.iter().find(|op| op["path"] == PRIORITY)
}

fn client() -> AdoClient {
    AdoClient::with_base_url("tok".into(), "http://127.0.0.1:9".into())
}

#[test]
fn a_create_writes_2_when_the_case_sets_no_priority() {
    let doc = client().create_test_case_doc("org", "Web", &case("A", None), None, "", "", None, None);
    let op = priority_op(&doc).expect("a create always writes a priority");
    assert_eq!(op["op"], "add");
    assert_eq!(op["value"], 2);
}

#[test]
fn a_create_writes_the_priority_the_case_sets() {
    for p in 1..=4 {
        let doc = client().create_test_case_doc("org", "Web", &case("A", Some(p)), None, "", "", None, None);
        assert_eq!(priority_op(&doc).expect("priority op")["value"], p);
    }
}

#[test]
fn an_update_leaves_priority_out_when_the_case_sets_none_whatever_the_blank_policy() {
    for blanks in [BlankPolicy::Skip, BlankPolicy::Clear] {
        let ops = client().update_test_case_doc(&case("A", None), None, None, None, None, blanks);
        assert!(priority_op(&ops).is_none(), "{blanks:?}: {ops:?}");
    }
}

#[test]
fn an_update_writes_the_priority_when_the_case_sets_one() {
    for blanks in [BlankPolicy::Skip, BlankPolicy::Clear] {
        let ops = client().update_test_case_doc(&case("A", Some(3)), None, None, None, None, blanks);
        let op = priority_op(&ops).expect("priority op");
        assert_eq!(op["op"], "add");
        assert_eq!(op["value"], 3);
    }
}

#[test]
fn the_read_path_parses_priority_as_a_number_or_a_numeric_string() {
    assert_eq!(parse_priority(&serde_json::json!(1)), Some(1));
    assert_eq!(parse_priority(&serde_json::json!(4)), Some(4));
    assert_eq!(parse_priority(&serde_json::json!("3")), Some(3));
    assert_eq!(parse_priority(&serde_json::Value::Null), None);
    assert_eq!(parse_priority(&serde_json::json!(0)), None);
    assert_eq!(parse_priority(&serde_json::json!(9)), None);
    assert_eq!(parse_priority(&serde_json::json!("high")), None);
}

#[tokio::test]
async fn full_cases_read_priority_and_ask_for_the_field() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 201, "fields": {"System.Title": "With", "Microsoft.VSTS.Common.Priority": 1}},
                {"id": 202, "fields": {"System.Title": "Without"}}
            ]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let cases = client.get_test_cases_by_ids("org", &[201, 202], None, None).await.unwrap();
    assert_eq!(cases[0].priority, Some(1));
    assert_eq!(cases[1].priority, None);

    let asked = server.received_requests().await.unwrap();
    let fields = asked[0].url.query_pairs().find(|(k, _)| k == "fields").unwrap().1.to_string();
    assert!(fields.split(',').any(|f| f == "Microsoft.VSTS.Common.Priority"), "{fields}");
}

fn import_one(priority: serde_json::Value) -> (Vec<TestCase>, Vec<String>) {
    let text = serde_json::json!({ "test_cases": [{
        "title": "Login works",
        "priority": priority,
        "steps": [{ "action": "Open", "expected": "Shown" }]
    }]})
    .to_string();
    let parsed = parse_json_text(&text).unwrap();
    (parsed.cases, parsed.warnings)
}

#[test]
fn import_accepts_priority_1_to_4() {
    for p in 1..=4 {
        let (cases, warnings) = import_one(serde_json::json!(p));
        assert_eq!(cases.len(), 1, "{warnings:?}");
        assert_eq!(cases[0].priority, Some(p));
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}

#[test]
fn import_without_priority_leaves_it_unset() {
    let text = r#"{"test_cases":[{"title":"A","steps":[{"action":"Open","expected":""}]}]}"#;
    let cases = parse_json_text(text).unwrap().cases;
    assert_eq!(cases[0].priority, None);
}

#[test]
fn import_refuses_0_5_and_a_word_with_a_plain_sentence() {
    for bad in [serde_json::json!(0), serde_json::json!(5), serde_json::json!("high")] {
        let (cases, warnings) = import_one(bad.clone());
        assert!(cases.is_empty(), "{bad}: case should be refused");
        assert!(
            warnings.iter().any(|w| w.contains("Priority must be 1, 2, 3 or 4")),
            "{bad}: {warnings:?}"
        );
    }
}

#[test]
fn validation_refuses_a_priority_outside_1_to_4() {
    assert!(case("A", Some(2)).is_valid().is_ok());
    assert!(case("A", None).is_valid().is_ok());
    for bad in [0, 5, -1] {
        let err = case("A", Some(bad)).is_valid().unwrap_err();
        assert!(err.contains("Priority must be 1, 2, 3 or 4"), "{err}");
    }
}

#[test]
fn export_omits_priority_when_unset_and_writes_it_when_set() {
    let text = queue_to_json_string(&[case("Unset", None), case("Set", Some(4))]).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    let cases = doc["test_cases"].as_array().unwrap();
    assert!(cases[0].get("priority").is_none(), "{}", cases[0]);
    assert_eq!(cases[1]["priority"], 4);
    // And it round-trips.
    let back = parse_json_text(&text).unwrap().cases;
    assert_eq!(back[0].priority, None);
    assert_eq!(back[1].priority, Some(4));
}

#[test]
fn the_case_serialization_omits_priority_when_unset() {
    let v = serde_json::to_value(case("A", None)).unwrap();
    assert!(v.get("priority").is_none(), "{v}");
    let v = serde_json::to_value(case("A", Some(1))).unwrap();
    assert_eq!(v["priority"], 1);
}

/// One op of every kind `parse_ops` accepts, each run alone against a draft
/// whose cases carry priorities. Whatever the op does, every case that
/// survives it keeps its priority, both in memory and in the patched file.
#[test]
fn every_transform_keeps_priority() {
    let draft = serde_json::json!({ "test_cases": [
        { "title": "Login works", "priority": 1, "tags": "smoke",
          "reviewer_notes": "Spec: Login\n> quoted",
          "steps": [{ "action": "Open the login page", "expected": "Shown" },
                    { "action": "Sign in", "expected": "Home" }] },
        { "title": "Logout works", "priority": 3, "tags": "smoke",
          "steps": [{ "action": "Sign out", "expected": "Signed out" }] },
        { "title": "Logout works", "priority": 4,
          "steps": [{ "action": "Sign out again", "expected": "Signed out" }] }
    ]})
    .to_string();
    let original = parse_json_text(&draft).unwrap().cases;
    let want: Vec<(String, Option<i32>)> = vec![
        ("Login works".into(), Some(1)),
        ("Logout works".into(), Some(3)),
        ("Logout works".into(), Some(4)),
    ];

    let ops = serde_json::json!([
        { "op": "set_tags", "value": "a" },
        { "op": "add_tags", "value": "b" },
        { "op": "remove_tags", "value": "smoke" },
        { "op": "set_module", "value": "Auth" },
        { "op": "set_automation_status", "value": "Planned" },
        { "op": "set_preconditions", "value": "Signed out" },
        { "op": "set_area", "value": "Login / Form" },
        { "op": "set_reviewer_notes", "value": "Why" },
        { "op": "set_findings", "value": [{ "kind": "spec", "title": "Gap" }] },
        { "op": "prefix_title", "value": "P " },
        { "op": "suffix_title", "value": " S" },
        { "op": "replace_in_title", "find": "works", "replace": "runs" },
        { "op": "replace_in_steps", "find": "Sign", "replace": "Log" },
        { "op": "replace_in_notes", "find": "Spec", "replace": "Source" },
        { "op": "replace_in_preconditions", "find": "x", "replace": "y" },
        { "op": "normalise_citations" },
        { "op": "prepend_step", "action": "Start", "expected": "Started" },
        { "op": "append_step", "action": "End", "expected": "Ended" },
        { "op": "remove_step_matching", "value": "Open" },
        { "op": "split_step", "find": "Sign in", "into": [{ "action": "Type", "expected": "" }, { "action": "Submit", "expected": "Home" }] },
        { "op": "sort_by", "value": "title" },
        { "op": "group_by", "value": "tags" },
        { "op": "dedupe" },
        { "op": "remove_cases", "where": { "at_index": 2 } },
        { "op": "insert_cases", "cases": [{ "title": "New", "priority": 2, "steps": [{ "action": "Go", "expected": "" }] }] }
    ]);
    let list = ops.as_array().unwrap();
    // The list above covers the whole vocabulary, so a new op cannot be
    // added without this test seeing it.
    let named: Vec<&str> = list.iter().map(|o| o["op"].as_str().unwrap()).collect();
    for op in SUPPORTED_OPS {
        assert!(named.contains(&op), "no priority check for op {op}");
    }

    for raw in list {
        let parsed = parse_ops(&serde_json::json!([raw])).unwrap_or_else(|e| panic!("{raw}: {e}"));
        let (out, _) = apply(original.clone(), &parsed);
        assert!(!out.is_empty(), "{raw}");
        // Each surviving case still has a priority from the draft (the
        // inserted one carries its own 2).
        for c in &out {
            let p = c.priority.unwrap_or_else(|| panic!("{raw}: '{}' lost its priority", c.title));
            assert!([1, 2, 3, 4].contains(&p), "{raw}: {p}");
        }
        if raw["op"] == "insert_cases" {
            assert!(out.iter().any(|c| c.title == "New" && c.priority == Some(2)), "{raw}");
        }
        // Order-free ops keep each case's own value.
        if ["set_tags", "set_module", "set_automation_status", "append_step"].contains(&raw["op"].as_str().unwrap()) {
            let got: Vec<(String, Option<i32>)> = out.iter().map(|c| (c.title.clone(), c.priority)).collect();
            assert_eq!(got, want, "{raw}");
        }

        // And the file written back keeps the key on every case.
        let merged = merge_cases_into_draft(&draft, &out).unwrap();
        let back = parse_json_text(&merged).unwrap().cases;
        assert_eq!(
            back.iter().map(|c| c.priority).collect::<Vec<_>>(),
            out.iter().map(|c| c.priority).collect::<Vec<_>>(),
            "{raw}"
        );
    }
}

#[test]
fn optimize_keeps_priority() {
    let cases = vec![case("Login works", Some(1)), case("Logout works", Some(4)), case("Other", None)];
    let (out, _) = v2_lib::optimize::optimize(cases, None);
    let by_title = |t: &str| out.iter().find(|c| c.title == t).map(|c| c.priority);
    assert_eq!(by_title("Login works"), Some(Some(1)));
    assert_eq!(by_title("Logout works"), Some(Some(4)));
    assert_eq!(by_title("Other"), Some(None));
}

#[test]
fn a_patched_draft_gains_and_loses_the_key_with_the_case() {
    let draft = r#"{"test_cases":[{"title":"A","steps":[{"action":"Open","expected":""}]}]}"#;
    let mut cases = parse_json_text(draft).unwrap().cases;
    cases[0].priority = Some(3);
    let with = merge_cases_into_draft(draft, &cases).unwrap();
    let v: serde_json::Value = serde_json::from_str(&with).unwrap();
    assert_eq!(v["test_cases"][0]["priority"], 3);

    let mut again = parse_json_text(&with).unwrap().cases;
    again[0].priority = None;
    let without = merge_cases_into_draft(&with, &again).unwrap();
    let v: serde_json::Value = serde_json::from_str(&without).unwrap();
    assert!(v["test_cases"][0].get("priority").is_none(), "{v}");
}
