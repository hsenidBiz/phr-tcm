//! The pure parts of the upload: what each queued case is sent as, what each
//! answer in a `$batch` means for its case, and (C1) how a lookup after a
//! failed batch is paired back to the creates in it.

use v2_lib::ado::wit_batch::{create_uri, temp_id_op, update_uri, BatchItem};
use v2_lib::ado::AdoClient;
use v2_lib::commands::queue::{map_batch_results, match_reconciled, queue_item_request};
use v2_lib::model::TestCase;
use v2_lib::steps_xml::Step;

fn client() -> AdoClient {
    AdoClient::with_base_urls(
        "tok".into(),
        "https://dev.azure.com".into(),
        "https://app.vssps.visualstudio.com".into(),
    )
}

fn case(title: &str, update_id: Option<i32>) -> TestCase {
    TestCase {
        title: title.into(),
        steps: vec![Step { action: "Open the page".into(), expected: "It opens".into(), ..Default::default() }],
        automation_status: "Not Automated".into(),
        update_id,
        ..Default::default()
    }
}

fn paths(body: &serde_json::Value) -> Vec<String> {
    body.as_array()
        .unwrap()
        .iter()
        .filter_map(|o| o["path"].as_str().map(str::to_string))
        .collect()
}

fn tag_ops(body: &serde_json::Value) -> Vec<String> {
    body.as_array()
        .unwrap()
        .iter()
        .filter(|o| o["path"] == "/fields/System.Tags")
        .map(|o| o["op"].as_str().unwrap().to_string())
        .collect()
}

/// A create is one request: the type's URL, the batch-local temporary id
/// FIRST, the fields, and the link to the PBI folded into the same document.
#[test]
fn a_create_is_one_document_that_links_the_pbi() {
    let req = queue_item_request(
        &client(), "acme", "Web", 42, &case("New one", None), None, None,
        "Web\\Team", "Web\\Sprint 1", None, None, 3,
    )
    .unwrap();
    assert_eq!(req.uri, create_uri("Web"));
    let ops = req.body.as_array().unwrap();
    assert_eq!(ops[0], temp_id_op(3), "the temporary id leads the document");
    assert!(ops.iter().any(|o| o["path"] == "/fields/System.Title" && o["value"] == "New one"));
    assert!(ops.iter().any(|o| o["path"] == "/fields/System.AreaPath" && o["value"] == "Web\\Team"));
    let link = ops.iter().find(|o| o["path"] == "/relations/-").expect("a create links the PBI");
    assert_eq!(link["value"]["rel"], "Microsoft.VSTS.Common.TestedBy-Reverse");
    assert!(link["value"]["url"].as_str().unwrap().ends_with("/workitems/42"));
}

/// An update PATCHes its own work item where it lives: no temporary id, no
/// re-link, no area/iteration move.
#[test]
fn an_update_patches_its_own_work_item_and_links_nothing() {
    let req = queue_item_request(
        &client(), "acme", "Web", 42, &case("Old one", Some(777)), None, None,
        "Web\\Team", "Web\\Sprint 1", None, None, 1,
    )
    .unwrap();
    assert_eq!(req.uri, update_uri(777));
    let p = paths(&req.body);
    assert!(p.contains(&"/fields/System.Title".to_string()));
    assert!(!p.contains(&"/id".to_string()), "no temporary id on an update");
    assert!(!p.contains(&"/relations/-".to_string()), "an update never re-links");
    assert!(!p.contains(&"/fields/System.AreaPath".to_string()), "an update stays where it lives");
}

/// The steps/tags baselines read from Azure DevOps describe an EXISTING case.
/// A create has none, and must come out the same whatever is passed.
/// On an update the tag baseline decides the op (see `tags_write_ops`).
#[test]
fn baselines_shape_updates_only() {
    let mut tc = case("Tagged", None);
    tc.tags = "smoke".into();
    let plain = queue_item_request(&client(), "acme", "Web", 42, &tc, None, None, "", "", None, None, 1).unwrap();
    let with = queue_item_request(
        &client(), "acme", "Web", 42, &tc, None, None, "", "",
        Some("<steps id=\"0\" last=\"2\"></steps>"), Some("legacy; old"), 1,
    )
    .unwrap();
    assert_eq!(plain.body, with.body, "a create ignores what an existing case held");

    let mut up = tc.clone();
    up.update_id = Some(777);
    let no_base = queue_item_request(&client(), "acme", "Web", 42, &up, None, None, "", "", None, None, 1).unwrap();
    let same = queue_item_request(&client(), "acme", "Web", 42, &up, None, None, "", "", None, Some("smoke"), 1).unwrap();
    assert_eq!(tag_ops(&no_base.body), vec!["add", "replace"]);
    assert!(tag_ops(&same.body).is_empty(), "tags already as wanted - nothing to write");
}

#[test]
fn an_invalid_case_never_becomes_a_request() {
    let err = queue_item_request(&client(), "acme", "Web", 42, &case("", None), None, None, "", "", None, None, 1)
        .unwrap_err();
    assert!(err.contains("Title is required"), "{err}");
}

/// A case that failed validation is never sent, so answer k belongs to
/// `sent_idx[k]`, not to row k. Row 1 here was refused before the call; the
/// answers must still land on rows 0, 2 and 3.
#[test]
fn a_refused_row_mid_chunk_does_not_shift_the_answers() {
    let queue = vec![case("A", None), case("", None), case("C", Some(777)), case("D", None)];
    let items = vec![
        BatchItem { code: 200, body: serde_json::json!({"id": 901}) },
        BatchItem { code: 200, body: serde_json::json!({"id": 777}) },
        BatchItem { code: 400, body: serde_json::json!({"message": "TF401320: Rule Error for field Title"}) },
    ];
    let r = map_batch_results(&queue, &[0, 2, 3], &items);
    assert_eq!(r.len(), 3);
    assert_eq!((r[0].index, r[0].title.as_str(), r[0].action.as_str(), r[0].id), (0, "A", "created", Some(901)));
    assert_eq!((r[1].index, r[1].title.as_str(), r[1].action.as_str(), r[1].id), (2, "C", "updated", Some(777)));
    assert_eq!((r[2].index, r[2].title.as_str(), r[2].action.as_str(), r[2].id), (3, "D", "failed", None));
    assert_eq!(r[2].error.as_deref(), Some("TF401320: Rule Error for field Title"));
    assert!(r.iter().all(|x| x.index != 1), "the refused row gets no batch answer");
}

/// A 2xx with no id is not a create - it is reported, never called success.
#[test]
fn a_create_answered_without_an_id_is_a_failure() {
    let queue = vec![case("A", None)];
    let r = map_batch_results(&queue, &[0], &[BatchItem { code: 200, body: serde_json::json!({}) }]);
    assert_eq!(r[0].action, "failed");
    assert!(r[0].error.as_deref().unwrap().contains("no work item id"));
}

#[test]
fn reconciling_matches_the_exact_title_only() {
    let creates = vec![(0, "Login works".to_string())];
    assert!(match_reconciled(&creates, &[(901, "login works".into())]).is_empty(), "case matters");
    assert!(match_reconciled(&creates, &[(901, "Login works fine".into())]).is_empty());
    assert_eq!(match_reconciled(&creates, &[(901, "Login works ".into())]), vec![(0, 901)], "outer spaces do not");
}

#[test]
fn each_found_case_is_claimed_once() {
    let creates = vec![(0, "A".to_string()), (1, "A".to_string())];
    assert_eq!(match_reconciled(&creates, &[(901, "A".into())]), vec![(0, 901)]);
}

/// The server runs a batch in order, so the lower id is the earlier create:
/// repeated titles pair up in queue order with ids ascending.
#[test]
fn repeated_titles_in_a_batch_pair_in_order() {
    let creates = vec![(3, "A".to_string()), (5, "B".to_string()), (7, "A".to_string())];
    let found = vec![(912, "A".to_string()), (911, "B".to_string()), (910, "A".to_string())];
    assert_eq!(match_reconciled(&creates, &found), vec![(3, 910), (5, 911), (7, 912)]);
}
