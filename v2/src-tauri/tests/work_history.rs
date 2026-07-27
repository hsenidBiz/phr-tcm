//! Work-item history: reducing ADO's raw revision feed to readable changes.

use v2_lib::ado::AdoClient;
use v2_lib::work_board::history::{de_camel, parse_revision};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn a_state_change_reads_as_old_to_new_with_the_state_first() {
    let rev = serde_json::json!({
        "rev": 12,
        "revisedBy": { "displayName": "Avin Alwis", "imageUrl": "https://x/av" },
        "fields": {
            "System.ChangedDate": { "oldValue": "2026-07-24T02:28:00Z", "newValue": "2026-07-24T03:58:00Z" },
            "System.Rev": { "oldValue": 11, "newValue": 12 },
            "Microsoft.VSTS.Scheduling.RemainingWork": { "oldValue": 6.8, "newValue": 0 },
            "System.State": { "oldValue": "Resolved", "newValue": "QA Ready" },
            "System.Reason": { "oldValue": "Moved out of state Resolved", "newValue": "Moved out of state In Progress" }
        }
    });
    let r = parse_revision(&rev).expect("a real change");

    assert_eq!(r.by, "Avin Alwis");
    assert_eq!(r.avatar_url, "https://x/av");
    // The date is System.ChangedDate's NEW value, not revisedDate.
    assert_eq!(r.at, "2026-07-24T03:58:00Z");
    assert_eq!((r.state_from.as_str(), r.state_to.as_str()), ("Resolved", "QA Ready"));

    // Bookkeeping is gone; State leads, then Reason, then the rest.
    assert_eq!(
        r.fields.iter().map(|f| f.label.as_str()).collect::<Vec<_>>(),
        vec!["State", "Reason", "Remaining Work"]
    );
    let work = &r.fields[2];
    assert_eq!((work.old.as_str(), work.new.as_str()), ("6.8", "0"));
}

/// ADO emits revisions where nothing but the rev counter moved. Rendering
/// those would pad the timeline with rows that say nothing.
#[test]
fn a_bookkeeping_only_revision_is_dropped() {
    let rev = serde_json::json!({
        "rev": 3,
        "revisedBy": { "displayName": "Someone" },
        "fields": {
            "System.Rev": { "oldValue": 2, "newValue": 3 },
            "System.AuthorizedDate": { "oldValue": "a", "newValue": "b" },
            "System.ChangedDate": { "oldValue": "a", "newValue": "b" },
            "System.Watermark": { "oldValue": 1, "newValue": 2 }
        }
    });
    assert!(parse_revision(&rev).is_none());
}

/// A field whose value is unchanged (ADO does emit these) is not a change.
#[test]
fn an_unchanged_value_is_not_reported() {
    let rev = serde_json::json!({
        "rev": 4,
        "revisedBy": { "displayName": "Someone" },
        "fields": {
            "System.Title": { "oldValue": "Same", "newValue": "Same" },
            "System.Tags": { "oldValue": "", "newValue": "smoke" }
        }
    });
    let r = parse_revision(&rev).unwrap();
    assert_eq!(r.fields.len(), 1);
    assert_eq!(r.fields[0].label, "Tags");
}

#[test]
fn identity_fields_show_the_person_not_the_json() {
    let rev = serde_json::json!({
        "rev": 5,
        "revisedBy": { "displayName": "Someone" },
        "fields": {
            "System.AssignedTo": {
                "oldValue": { "displayName": "Dilshan Kaviratne", "uniqueName": "d@x" },
                "newValue": { "displayName": "Avin Alwis", "uniqueName": "a@x" }
            }
        }
    });
    let f = &parse_revision(&rev).unwrap().fields[0];
    assert_eq!(f.label, "Assigned To");
    assert_eq!((f.old.as_str(), f.new.as_str()), ("Dilshan Kaviratne", "Avin Alwis"));
}

#[test]
fn long_html_is_flattened_and_truncated_not_dumped() {
    let body = format!("<p>{}</p>", "word ".repeat(200));
    let rev = serde_json::json!({
        "rev": 6,
        "revisedBy": { "displayName": "Someone" },
        "fields": { "System.Description": { "oldValue": "", "newValue": body } }
    });
    let f = &parse_revision(&rev).unwrap().fields[0];
    assert!(!f.new.contains('<'), "no markup reaches the timeline");
    assert!(f.new.ends_with('…'), "truncated: {}", f.new);
    assert!(f.new.chars().count() <= 241);
}

#[test]
fn links_are_named_readably() {
    let rev = serde_json::json!({
        "rev": 7,
        "revisedBy": { "displayName": "Dilshan Kaviratne" },
        "fields": {},
        "relations": {
            "added": [
                { "rel": "ArtifactLink", "attributes": { "name": "Commit" } },
                { "rel": "System.LinkTypes.Related" }
            ],
            "removed": [ { "rel": "AttachedFile" } ]
        }
    });
    let r = parse_revision(&rev).unwrap();
    assert_eq!(r.links_added, vec!["Commit link", "Related link"]);
    assert_eq!(r.links_removed, vec!["Attachment"]);
}

/// A comment is rendered by the Discussion tab; history only notes it,
/// and never repeats the body as a field row.
#[test]
fn a_comment_is_flagged_but_its_body_is_not_a_field_row() {
    let rev = serde_json::json!({
        "rev": 8,
        "revisedBy": { "displayName": "Someone" },
        "fields": {
            "System.History": { "newValue": "<div>Looks good to me</div>" },
            "System.CommentCount": { "oldValue": 1, "newValue": 2 }
        }
    });
    let r = parse_revision(&rev).unwrap();
    assert!(r.comment_added);
    assert!(r.fields.is_empty(), "the body belongs to Discussion");
}

/// The newest revision carries ADO's year-9999 "not yet superseded"
/// sentinel in revisedDate - showing that would be absurd.
#[test]
fn the_never_superseded_sentinel_never_becomes_a_date() {
    let rev = serde_json::json!({
        "rev": 9,
        "revisedDate": "9999-01-01T00:00:00Z",
        "revisedBy": { "displayName": "Someone" },
        "fields": { "System.Tags": { "oldValue": "", "newValue": "smoke" } }
    });
    assert_eq!(parse_revision(&rev).unwrap().at, "");
}

#[test]
fn custom_field_names_are_de_camel_cased() {
    assert_eq!(de_camel("RemainingWork"), "Remaining Work");
    assert_eq!(de_camel("Already Spaced"), "Already Spaced");
    assert_eq!(de_camel("QAReady"), "QA Ready");
    assert_eq!(de_camel("Module"), "Module");
}

#[tokio::test]
async fn history_comes_back_newest_first() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/acme/Web/_apis/wit/workItems/77/updates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                // ADO's own order: oldest first.
                { "rev": 1, "revisedBy": { "displayName": "A" },
                  "fields": { "System.ChangedDate": { "newValue": "2026-07-01T00:00:00Z" },
                              "System.State": { "newValue": "New" } } },
                { "rev": 2, "revisedBy": { "displayName": "B" },
                  "fields": { "System.Rev": { "oldValue": 1, "newValue": 2 } } },
                { "rev": 3, "revisedBy": { "displayName": "C" },
                  "fields": { "System.ChangedDate": { "newValue": "2026-07-03T00:00:00Z" },
                              "System.State": { "oldValue": "New", "newValue": "In Progress" } } }
            ]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let hist = client.get_work_item_history("acme", "Web", 77).await.unwrap();

    assert_eq!(hist.len(), 2, "the empty rev-bump revision is not shown");
    assert_eq!(hist[0].rev, 3, "newest first");
    assert_eq!(hist[0].state_to, "In Progress");
    assert_eq!(hist[1].rev, 1);
}
