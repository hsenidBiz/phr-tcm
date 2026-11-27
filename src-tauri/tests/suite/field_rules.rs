//! Field rules for a Test Case: the sentence an Azure DevOps refusal becomes
//! (after sending), and the check a queue gets before sending.

use v2_lib::ado::field_rules::{check_case, check_queue, FieldRule};
use v2_lib::ado::rule_errors::{plain_reason, strip_urls};
use v2_lib::ado::wit_batch::BatchItem;
use v2_lib::ado::{AdoClient, AdoError};
use v2_lib::model::TestCase;
use v2_lib::steps_xml::Step;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The body the owner's log showed for each of 64 refused cases.
fn owners_body() -> String {
    serde_json::json!({
        "errorMessage": "TF401320: Rule Error for field Module. Error code: Required, HasValues, AllowsOldValue, InvalidEmpty.",
        "ruleValidationErrors": [{
            "fieldReferenceName": "Custom.Module",
            "fieldStatusFlags": "required, hasValues, allowsOldValue, invalidEmpty",
            "errorMessage": "TF401320: Rule Error for field Module. Error code: Required, HasValues, AllowsOldValue, InvalidEmpty.",
        }],
    })
    .to_string()
}

fn none() -> v2_lib::ado::rule_errors::FieldNames {
    Default::default()
}

#[test]
fn the_owners_body_reads_module_is_required() {
    assert_eq!(plain_reason(&owners_body(), &none()).as_deref(), Some("Module is required"));
}

#[test]
fn a_rule_error_without_its_own_message_takes_the_top_level_one() {
    let body = serde_json::json!({
        "errorMessage": "TF401320: Rule Error for field Module. Error code: Required, InvalidEmpty.",
        "ruleValidationErrors": [{"fieldReferenceName": "Custom.Module", "fieldStatusFlags": "required, invalidEmpty"}],
    })
    .to_string();
    assert_eq!(plain_reason(&body, &none()).as_deref(), Some("Module is required"));
}

#[test]
fn the_project_field_list_names_the_field_when_given() {
    let mut names = none();
    names.insert("Custom.Module".into(), "Product Module".into());
    let body = serde_json::json!({
        "ruleValidationErrors": [{"fieldReferenceName": "Custom.Module", "fieldStatusFlags": "required"}],
    })
    .to_string();
    assert_eq!(plain_reason(&body, &names).as_deref(), Some("Product Module is required"));
    // Without a list or a message, the reference name is made readable.
    assert_eq!(plain_reason(&body, &none()).as_deref(), Some("Module is required"));
}

#[test]
fn several_rule_errors_each_get_a_sentence() {
    let body = serde_json::json!({
        "errorMessage": "TF401320: Rule Error for field Module.",
        "ruleValidationErrors": [
            {"fieldReferenceName": "Custom.Module", "fieldStatusFlags": "required, invalidEmpty",
             "errorMessage": "TF401320: Rule Error for field Module. Error code: Required, InvalidEmpty."},
            {"fieldReferenceName": "Custom.TestType", "fieldStatusFlags": "hasValues, limitedToValues, invalidListValue",
             "errorMessage": "TF401320: Rule Error for field Test Type. Error code: InvalidListValue."},
            {"fieldReferenceName": "Custom.Risk", "fieldStatusFlags": "none",
             "errorMessage": "TF401320: Rule Error for field Risk. The value is too long. See https://dev.azure.com/acme/x?secret=1"},
        ],
    })
    .to_string();
    let said = plain_reason(&body, &none()).unwrap();
    assert_eq!(
        said,
        "Module is required; Test Type must be one of the allowed values; \
         Azure DevOps refused Risk: The value is too long. See"
    );
    assert!(!said.contains("://") && !said.contains("dev.azure.com"), "{said}");
}

#[test]
fn the_single_item_endpoints_spelling_parses_too() {
    let single_item_endpoint = serde_json::json!({
        "message": "TF401320: Rule Error for field Module.",
        "customProperties": {"RuleValidationErrors": [
            {"fieldReferenceName": "Custom.Module", "fieldStatusFlags": "required", "errorMessage": "TF401320: Rule Error for field Module. Error code: Required."}
        ]},
    })
    .to_string();
    assert_eq!(plain_reason(&single_item_endpoint, &none()).as_deref(), Some("Module is required"));
}

#[test]
fn no_rule_errors_falls_back_to_the_top_level_message_without_urls() {
    let body = serde_json::json!({
        "errorMessage": "VS402323: The request is not valid, see https://dev.azure.com/acme/_apis/x?y=1 or www.example.com now",
    })
    .to_string();
    let said = plain_reason(&body, &none()).unwrap();
    assert_eq!(said, "VS402323: The request is not valid, see or now");
    assert_eq!(strip_urls("a  https://x.y/z?q=1   b"), "a b");
}

#[test]
fn an_unknown_field_is_named() {
    let body = serde_json::json!({
        "message": "TF26027: A field definition Custom.Gone in the work item type definition file does not exist.",
    })
    .to_string();
    assert_eq!(plain_reason(&body, &none()).as_deref(), Some("Gone is not a field in this project"));
}

#[test]
fn a_body_that_is_not_json_gets_no_sentence_and_the_generic_one_stays() {
    assert_eq!(plain_reason("<html>Bad gateway</html>", &none()), None);
    assert_eq!(plain_reason("", &none()), None);
    assert_eq!(plain_reason("[1,2]", &none()), None);
    let item = BatchItem { code: 400, body: serde_json::Value::String("<html>nope</html>".into()) };
    assert_eq!(item.message(), "Azure DevOps returned HTTP 400");
    let item = BatchItem { code: 400, body: serde_json::Value::Null };
    assert_eq!(item.message(), "Azure DevOps returned HTTP 400");
}

#[test]
fn a_batch_item_with_the_owners_body_reads_module_is_required() {
    let item = BatchItem { code: 400, body: serde_json::from_str(&owners_body()).unwrap() };
    assert_eq!(item.message(), "Module is required");
}

#[test]
fn a_single_save_shows_the_same_sentence() {
    let e = AdoError::Http { status: 400, body: owners_body() };
    assert_eq!(e.plain_text(), "Module is required");
    // Anything else reads as it always did.
    assert_eq!(AdoError::Forbidden.plain_text(), "forbidden");
    assert_eq!(AdoError::Http { status: 500, body: owners_body() }.plain_text(), "http 500");
    assert_eq!(AdoError::Http { status: 400, body: "<b>x</b>".into() }.plain_text(), "http 400");
}

#[tokio::test]
async fn a_single_create_refused_by_a_rule_says_which_field() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/org/Web/_apis/wit/workitems/$Test%20Case"))
        .respond_with(ResponseTemplate::new(400).set_body_string(owners_body()))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let err = client
        .create_test_case("org", "Web", &case("A", "x"), Some("Custom.Module"), "", "", None)
        .await
        .unwrap_err();
    assert_eq!(err.user_text(), "Module is required");
}

// ---- Before sending ---------------------------------------------------------

fn case(title: &str, module: &str) -> TestCase {
    TestCase {
        title: title.into(),
        steps: vec![Step { action: "Open".into(), expected: "Opens".into(), ..Default::default() }],
        automation_status: "Not Automated".into(),
        module_value: module.into(),
        ..Default::default()
    }
}

fn rules_listing() -> serde_json::Value {
    serde_json::json!({"value": [
        {"referenceName": "System.Title", "name": "Title", "alwaysRequired": true},
        {"referenceName": "Custom.Module", "name": "Module", "alwaysRequired": true,
         "allowedValues": ["Payroll", "Leave", "Claims"]},
        {"referenceName": "Custom.Prereq", "name": "Prerequisites", "alwaysRequired": false},
        {"referenceName": "Custom.Other", "name": "Other Required", "alwaysRequired": true},
        {"referenceName": "System.Tags", "name": "Tags", "alwaysRequired": false},
    ]})
}

async fn rules_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/Web/_apis/wit/workitemtypes/Test%20Case/fields"))
        .respond_with(ResponseTemplate::new(200).set_body_json(rules_listing()))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn an_empty_required_field_is_found_and_a_value_outside_a_list_is_not_blocked() {
    let server = rules_server().await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    // "Billing" is outside the listing's allowedValues, but the listing does
    // not say whether that list is closed or only suggested - so it is sent.
    let queue = vec![case("A", ""), case("B", "Leave"), case("C", "  claims  "), case("D", "Billing")];
    let check = check_queue(&client, "org", "Web", &queue, Some("Custom.Module"), Some("Custom.Prereq")).await;
    assert!(check.checked);
    assert_eq!(check.problems.len(), 1, "{:?}", check.problems);
    let a = &check.problems[0];
    assert_eq!((a.index, a.kind.as_str(), a.field.as_str()), (0, "required", "Module"));
    assert_eq!(a.message, "Module is required for a Test Case in this project - pick one.");
}

#[tokio::test]
async fn a_required_field_the_app_never_writes_is_not_a_block() {
    let server = rules_server().await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let check = check_queue(&client, "org", "Web", &[case("A", "Payroll")], Some("Custom.Module"), None).await;
    assert!(check.checked && check.problems.is_empty(), "{:?}", check.problems);
}

#[test]
fn a_field_with_a_default_is_not_required() {
    let rules = vec![FieldRule {
        reference_name: "Custom.Module".into(),
        name: "Module".into(),
        required: false,
    }];
    assert!(check_case(0, &case("A", ""), &rules, Some("Custom.Module"), None).is_empty());
}

#[tokio::test]
async fn the_rules_are_read_once_for_many_checks() {
    let server = rules_server().await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    for _ in 0..3 {
        check_queue(&client, "org", "Web", &[case("A", "")], Some("Custom.Module"), None).await;
    }
    let gets = server.received_requests().await.unwrap().iter().filter(|r| r.method.as_str() == "GET").count();
    assert_eq!(gets, 1, "one read, then the session cache");
}

#[tokio::test]
async fn unreadable_rules_fail_open() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403).set_body_string("no"))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let check = check_queue(&client, "org", "Web", &[case("A", "")], Some("Custom.Module"), None).await;
    assert!(!check.checked, "the screen says the rules could not be checked");
    assert!(check.problems.is_empty(), "and nothing is blocked");
}

#[test]
fn an_update_row_is_never_blocked() {
    let rules = vec![FieldRule { reference_name: "Custom.Module".into(), name: "Module".into(), required: true }];
    let mut update = case("A", "");
    update.update_id = Some(77);
    assert!(check_case(0, &update, &rules, Some("Custom.Module"), None).is_empty());
    // A value outside what the field lists is not held against an update
    // either: Azure DevOps keeps a stored value that has left the list.
    update.module_value = "Billing".into();
    assert!(check_case(0, &update, &rules, Some("Custom.Module"), None).is_empty());
}

#[test]
fn a_suggested_list_never_blocks() {
    let rules = vec![FieldRule { reference_name: "Custom.Module".into(), name: "Module".into(), required: true }];
    assert!(check_case(0, &case("A", "Typed by hand"), &rules, Some("Custom.Module"), None).is_empty());
}

#[test]
fn a_required_field_given_a_wrong_value_reads_as_a_wrong_value() {
    let body = serde_json::json!({
        "ruleValidationErrors": [
            {"fieldReferenceName": "Custom.Module", "fieldStatusFlags": "required, hasValues, limitedToValues, invalidListValue"},
            {"fieldReferenceName": "Custom.Note", "fieldStatusFlags": "required, invalidTooLong"},
        ],
    })
    .to_string();
    assert_eq!(
        plain_reason(&body, &none()).as_deref(),
        Some("Module must be one of the allowed values; Note is too long")
    );
}

#[tokio::test]
async fn a_relinked_case_refused_by_a_rule_shows_the_plain_sentence() {
    let e = AdoError::Http { status: 400, body: owners_body() }.with_plain_body();
    assert_eq!(e.user_text(), "Module is required");
    assert_eq!(AdoError::Forbidden.with_plain_body().user_text(), "forbidden");
}
