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

// ---- C1: reconcile after a failed batch -------------------------------

use v2_lib::ado::endpoints::{created_since_wiql, wiql_datetime};
use v2_lib::ado::{AdoError, NET_TIMEOUT};
use v2_lib::commands::queue::{
    failed_batch_results, iso_utc, reconcile_with, resolve_failed_batch, ReconciledCase,
};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SINCE: &str = "2026-09-18T10:00:00Z";

#[test]
fn upload_start_is_written_the_way_wiql_reads_it() {
    assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
    assert_eq!(iso_utc(1_700_000_000), "2023-11-14T22:13:20Z");
}

/// The frontend sends `Date.toISOString()`; anything that is not exactly a
/// date-time is refused before it can reach a WIQL string.
#[test]
fn only_a_real_date_time_reaches_the_query() {
    assert_eq!(wiql_datetime("2026-09-18T10:00:00.000Z").as_deref(), Some(SINCE));
    assert_eq!(wiql_datetime("2026-09-18T10:00:00Z").as_deref(), Some(SINCE));
    assert_eq!(wiql_datetime("2026-09-18T10:00:00").as_deref(), Some(SINCE));
    assert_eq!(wiql_datetime("2026-09-18T10:00:00Z' OR ''='"), None);
    assert_eq!(wiql_datetime("2026-09-18' OR 1=1 --"), None);
    assert_eq!(wiql_datetime(""), None);
}

#[test]
fn the_lookup_asks_for_test_cases_the_pbi_is_tested_by_created_since_the_upload() {
    let q = created_since_wiql(42, "2026-09-18T10:00:00.000Z").unwrap();
    assert!(q.contains("FROM WorkItemLinks"), "{q}");
    assert!(q.contains("[Source].[System.Id] = 42"), "{q}");
    assert!(q.contains("'Microsoft.VSTS.Common.TestedBy-Forward'"), "{q}");
    assert!(q.contains("[Target].[System.WorkItemType] = 'Test Case'"), "{q}");
    assert!(q.contains("[Target].[System.CreatedDate] >= '2026-09-18T10:00:00Z'"), "{q}");
    assert!(q.contains("MODE (MustContain)"), "{q}");
    assert_eq!(created_since_wiql(42, "yesterday"), None);
}

/// Found creates are created, with their id; creates not found failed;
/// updates always failed (a PATCH is safe to retry). Repeated titles pair in order.
#[test]
fn a_lookup_that_answered_decides_each_create() {
    let queue = vec![case("A", None), case("B", None), case("C", Some(777)), case("A", None)];
    let found = vec![(902, "A".to_string()), (901, "A".to_string())];
    let r = failed_batch_results(&queue, &[0, 1, 2, 3], "http 500", Some(&found), false);
    let got: Vec<(u32, &str, Option<i32>)> = r.iter().map(|x| (x.index, x.action.as_str(), x.id)).collect();
    assert_eq!(
        got,
        vec![(0, "created", Some(901)), (1, "failed", None), (2, "failed", None), (3, "created", Some(902))]
    );
    assert_eq!(r[1].error.as_deref(), Some("http 500"));
}

#[test]
fn a_lookup_that_failed_leaves_the_creates_unknown() {
    let queue = vec![case("A", None), case("C", Some(777))];
    let r = failed_batch_results(&queue, &[0, 1], "http 500", None, false);
    assert_eq!(r[0].action, "unknown");
    assert!(r[0].error.as_deref().unwrap().contains("http 500"));
    assert_eq!(r[1].action, "failed", "an update is safe to retry, so it is never unknown");
}

/// After a timeout the server may still be working through the batch:
/// "not found yet" is not "not created".
#[test]
fn after_a_timeout_a_create_not_found_yet_is_unknown() {
    let queue = vec![case("A", None), case("B", None)];
    let found = vec![(901, "A".to_string())];
    let r = failed_batch_results(&queue, &[0, 1], NET_TIMEOUT, Some(&found), true);
    assert_eq!((r[0].action.as_str(), r[0].id), ("created", Some(901)));
    assert_eq!(r[1].action, "unknown");
}

async fn mount_lookup(server: &MockServer, links: &[i64], items: serde_json::Value) {
    let mut rels = vec![serde_json::json!({"rel": null, "source": null, "target": {"id": 42}})];
    for id in links {
        rels.push(serde_json::json!({
            "rel": "Microsoft.VSTS.Common.TestedBy-Forward", "source": {"id": 42}, "target": {"id": id}
        }));
    }
    Mock::given(method("POST"))
        .and(path("/acme/Web/_apis/wit/wiql"))
        .and(query_param("timePrecision", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "queryType": "oneHop", "workItemRelations": rels
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(items))
        .mount(server)
        .await;
}

fn mock_client(server: &MockServer) -> AdoClient {
    AdoClient::with_base_urls("tok".into(), server.uri(), server.uri())
}

#[tokio::test]
async fn the_lookup_returns_only_the_batch_titles_oldest_first() {
    let server = MockServer::start().await;
    mount_lookup(&server, &[903, 901, 902], serde_json::json!({"value": [
        {"id": 903, "fields": {"System.Title": "A"}},
        {"id": 901, "fields": {"System.Title": "A"}},
        {"id": 902, "fields": {"System.Title": "Someone else's case"}}
    ]}))
    .await;
    let found = mock_client(&server)
        .find_created_test_cases("acme", "Web", 42, SINCE, &["A".to_string(), "B".to_string()])
        .await
        .unwrap();
    assert_eq!(found, vec![(901, "A".to_string()), (903, "A".to_string())]);

    let sent = server.received_requests().await.unwrap();
    let wiql: serde_json::Value = serde_json::from_slice(&sent[0].body).unwrap();
    let q = wiql["query"].as_str().unwrap();
    assert!(q.contains("[Source].[System.Id] = 42") && q.contains(SINCE), "{q}");
}

#[tokio::test]
async fn a_failed_batch_reports_what_the_lookup_found() {
    let server = MockServer::start().await;
    mount_lookup(&server, &[901], serde_json::json!({"value": [{"id": 901, "fields": {"System.Title": "A"}}]})).await;
    let queue = vec![case("A", None), case("B", None), case("C", Some(777))];
    let err = AdoError::Http { status: 500, body: "boom".into() };
    let r =
        resolve_failed_batch(&mock_client(&server), "acme", "Web", 42, SINCE, &queue, &[0, 1, 2], &err, &[]).await;
    let got: Vec<(&str, Option<i32>)> = r.iter().map(|x| (x.action.as_str(), x.id)).collect();
    assert_eq!(got, vec![("created", Some(901)), ("failed", None), ("failed", None)]);
}

#[tokio::test]
async fn a_failed_lookup_holds_the_creates_as_unknown() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/acme/Web/_apis/wit/wiql"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let queue = vec![case("A", None), case("C", Some(777))];
    let err = AdoError::Http { status: 500, body: "boom".into() };
    let r = resolve_failed_batch(&mock_client(&server), "acme", "Web", 42, SINCE, &queue, &[0, 1], &err, &[]).await;
    assert_eq!(r[0].action, "unknown");
    assert_eq!(r[1].action, "failed");
}

#[tokio::test]
async fn a_failed_batch_of_updates_asks_nothing() {
    let server = MockServer::start().await;
    let queue = vec![case("C", Some(777))];
    let err = AdoError::Http { status: 500, body: "boom".into() };
    let r = resolve_failed_batch(&mock_client(&server), "acme", "Web", 42, SINCE, &queue, &[0], &err, &[]).await;
    assert_eq!(r[0].action, "failed");
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn check_later_pairs_titles_with_what_exists() {
    let server = MockServer::start().await;
    mount_lookup(&server, &[901, 903], serde_json::json!({"value": [
        {"id": 901, "fields": {"System.Title": "A"}},
        {"id": 903, "fields": {"System.Title": "A"}}
    ]}))
    .await;
    let titles = vec!["A".to_string(), "A".to_string(), "B".to_string()];
    let got = reconcile_with(&mock_client(&server), "acme", "Web", 42, "2026-09-18T10:00:00.000Z", &titles)
        .await
        .unwrap();
    assert_eq!(
        got,
        vec![ReconciledCase { title: "A".into(), id: 901 }, ReconciledCase { title: "A".into(), id: 903 }]
    );
}

#[tokio::test]
async fn check_later_refuses_a_bad_start_time_without_asking() {
    let server = MockServer::start().await;
    let err = reconcile_with(&mock_client(&server), "acme", "Web", 42, "not a date", &["A".to_string()])
        .await
        .unwrap_err();
    assert!(!err.contains("http"), "{err}");
    assert!(server.received_requests().await.unwrap().is_empty());
}

// ---- fix round 1: an id already reported cannot be reused, and an ------
// ---- ambiguous title is held rather than guessed -----------------------

/// A title repeated across chunks: chunk 1's own success must not be
/// handed to chunk 2's failed create of the same title just because the
/// lookup (which spans the whole upload) turns it up again.
#[test]
fn an_id_already_reported_is_not_reused_for_another_create() {
    let queue = vec![case("A", None)];
    let found = vec![(901, "A".to_string())];
    let r = failed_batch_results(&queue, &[0], "http 500", Some(&found), false);
    assert_eq!((r[0].action.as_str(), r[0].id), ("created", Some(901)), "sanity: unfiltered, it would match");

    // Same inputs, but #901 already belongs to an earlier chunk.
    let creates = vec![(0usize, "A".to_string())];
    let still_available: Vec<(i32, String)> =
        found.into_iter().filter(|(id, _)| ![901].contains(id)).collect();
    assert!(match_reconciled(&creates, &still_available).is_empty());
}

/// `resolve_failed_batch` itself must do that filtering: `already_claimed`
/// removes chunk 1's id from what the lookup returns before matching, so
/// chunk 2's failed "A" is reported failed, not a duplicate "created".
#[tokio::test]
async fn resolve_failed_batch_excludes_ids_this_upload_already_reported() {
    let server = MockServer::start().await;
    mount_lookup(&server, &[901], serde_json::json!({"value": [{"id": 901, "fields": {"System.Title": "A"}}]})).await;
    let queue = vec![case("A", None)];
    let err = AdoError::Http { status: 500, body: "boom".into() };
    let r = resolve_failed_batch(&mock_client(&server), "acme", "Web", 42, SINCE, &queue, &[0], &err, &[901]).await;
    assert_eq!((r[0].action.as_str(), r[0].id), ("failed", None), "#901 is already someone else's - not this row's");
}

/// More found items than creates for a title: which one is genuinely ours
/// cannot be told apart, so none are claimed - the row is held `unknown`
/// (something with that title exists), not guessed and not plain `failed`.
#[test]
fn more_found_items_than_creates_for_a_title_are_held_unknown_not_guessed() {
    let queue = vec![case("A", None)];
    let found = vec![(901, "A".to_string()), (902, "A".to_string())];
    let r = failed_batch_results(&queue, &[0], "http 500", Some(&found), false);
    assert_eq!((r[0].action.as_str(), r[0].id), ("unknown", None));
}

/// The same ambiguity at the `match_reconciled` level: two found for one
/// create claims neither, where the old first-match behaviour would have
/// picked the lower id and called it done.
#[test]
fn match_reconciled_claims_nothing_when_found_outnumbers_creates_for_a_title() {
    let creates = vec![(0, "A".to_string())];
    let found = vec![(901, "A".to_string()), (902, "A".to_string())];
    assert!(match_reconciled(&creates, &found).is_empty());
}

/// Exactly one found for one create is unambiguous: still claimed.
#[test]
fn exactly_one_found_for_one_create_is_claimed() {
    let queue = vec![case("A", None)];
    let found = vec![(901, "A".to_string())];
    let r = failed_batch_results(&queue, &[0], "http 500", Some(&found), false);
    assert_eq!((r[0].action.as_str(), r[0].id), ("created", Some(901)));
}
