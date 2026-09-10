//! Column/state rules ported from v1 work_item.py + mywork_screen semantics
//! (the "Later"->Done rule, name heuristics, exact-column-name preference).

use std::collections::HashMap;
use v2_lib::ado::AdoClient;
use v2_lib::work_board::{
    column_for_state, state_for_column, team_area_clause, wiql_str, StateInfo,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn wiql_clause_builders_match_v1() {
    assert_eq!(wiql_str("it's"), "'it''s'");
    // Tree field + includeChildren -> UNDER; flat value -> '='.
    let clause = team_area_clause(
        "System.AreaPath",
        &[
            ("Proj\\Team".to_string(), true),
            ("Proj\\Other".to_string(), false),
            (String::new(), true), // empty values dropped
        ],
    );
    assert_eq!(
        clause,
        "[System.AreaPath] UNDER 'Proj\\Team' OR [System.AreaPath] = 'Proj\\Other'"
    );
    // Non-tree fields never use UNDER.
    let clause = team_area_clause("Custom.Squad", &[("Alpha".to_string(), true)]);
    assert_eq!(clause, "[Custom.Squad] = 'Alpha'");
}

#[tokio::test]
async fn comments_parse_with_avatar_fallback_chain() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workItems/11/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "comments": [
                {"id": 1, "text": "<div>Looks <b>good</b></div>",
                 "createdBy": {"displayName": "Ada",
                     "_links": {"avatar": {"href": "https://x/avatar/ada"}}},
                 "createdDate": "2026-07-12T01:00:00Z"},
                {"id": 2, "text": "plain",
                 "createdBy": {"displayName": "Bob", "imageUrl": "https://x/img/bob"},
                 "createdDate": "2026-07-12T00:00:00Z"},
                {"id": 3, "text": "x", "createdBy": {"displayName": "Cy"},
                 "createdDate": "2026-07-11T00:00:00Z"}
            ]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let comments = client.get_work_item_comments("org", "proj", 11).await.unwrap();
    assert_eq!(comments.len(), 3);
    assert_eq!(comments[0].text, "Looks good"); // html flattened
    assert_eq!(comments[0].avatar_url, "https://x/avatar/ada"); // _links first
    assert_eq!(comments[1].avatar_url, "https://x/img/bob"); // imageUrl fallback
    assert_eq!(comments[2].avatar_url, ""); // initials disc in the UI
}

/// The editor needs the stored HTML, ownership needs the author's id, and
/// "edited" needs the modified stamp - none of which the flattened text
/// carries.
#[tokio::test]
async fn comments_carry_html_author_id_and_edit_stamp() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workItems/11/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "comments": [{
                "id": 7, "text": "<div>Looks <b>good</b></div>",
                "createdBy": {"displayName": "Ada", "id": "u-ada"},
                "createdDate": "2026-07-12T01:00:00Z",
                "modifiedDate": "2026-07-12T02:00:00Z"
            }]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let c = &client.get_work_item_comments("org", "proj", 11).await.unwrap()[0];
    assert_eq!(c.text_html, "<div>Looks <b>good</b></div>");
    assert_eq!(c.created_by_id, "u-ada");
    assert_eq!(c.modified_date, "2026-07-12T02:00:00Z");
}

/// Editing is a plain-JSON PATCH on the comment itself - the same call
/// ADO's own Update button makes - and never anything else.
#[tokio::test]
async fn updating_a_comment_patches_its_text_as_plain_json() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/org/proj/_apis/wit/workItems/11/comments/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 7})))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    client
        .update_work_item_comment("org", "proj", 11, 7, "<div>Looks <b>great</b></div>")
        .await
        .unwrap();
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);
    let sent: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(sent, serde_json::json!({"text": "<div>Looks <b>great</b></div>"}));
    let ct = reqs[0].headers.get("content-type").unwrap().to_str().unwrap();
    assert!(ct.starts_with("application/json"), "{ct}");
}

#[tokio::test]
async fn connected_user_is_the_token_owner_by_identity_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/connectionData"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "authenticatedUser": {"id": "u-ada", "providerDisplayName": "Ada Lovelace"}
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let me = client.connected_user("org").await.unwrap();
    assert_eq!(me.id, "u-ada");
    assert_eq!(me.display_name, "Ada Lovelace");
}

#[tokio::test]
async fn team_members_dedupe_and_sort() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/projects/proj/teams"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": "t1", "name": "Alpha"}, {"id": "t2", "name": "Beta"}]
        })))
        .mount(&server)
        .await;
    for (tid, members) in [
        ("t1", serde_json::json!([
            {"identity": {"displayName": "Zoe", "uniqueName": "z@x.com"}},
            {"identity": {"displayName": "Avin", "uniqueName": "a@x.com"}}
        ])),
        ("t2", serde_json::json!([
            {"identity": {"displayName": "Avin", "uniqueName": "a@x.com"}}
        ])),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/org/_apis/projects/proj/teams/{tid}/members")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"value": members})),
            )
            .mount(&server)
            .await;
    }
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let members = client.list_team_members("org", "proj").await.unwrap();
    assert_eq!(members.len(), 2); // Avin deduped across teams
    assert_eq!(members[0].display_name, "Avin");
    assert_eq!(members[1].display_name, "Zoe");
}

#[tokio::test]
async fn detail_uses_reprosteps_for_bugs() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitems/12"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 12,
            "fields": {
                "System.Title": "Crash", "System.WorkItemType": "Bug",
                "System.State": "Active",
                "Microsoft.VSTS.TCM.ReproSteps": "<div>Click &amp; boom</div>",
                "Microsoft.VSTS.Scheduling.RemainingWork": 2.5
            }
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let d = client.get_work_item_detail("org", "proj", 12).await.unwrap();
    assert_eq!(d.description_field, "Microsoft.VSTS.TCM.ReproSteps");
    assert_eq!(d.description_text, "Click & boom");
    assert_eq!(d.remaining_work, Some(2.5));
}

fn s(name: &str, category: &str) -> StateInfo {
    StateInfo {
        name: name.to_string(),
        color: "007acc".to_string(),
        category: category.to_string(),
    }
}

fn task_states() -> HashMap<String, Vec<StateInfo>> {
    let mut m = HashMap::new();
    m.insert(
        "Task".to_string(),
        vec![
            s("New", "Proposed"),
            s("To Do", "Proposed"),
            s("Active", "InProgress"),
            s("In Progress", "InProgress"),
            s("Later", "InProgress"), // process says InProgress; v1 overrides
            s("Resolved", "Resolved"),
            s("Done", "Completed"),
            s("Removed", "Removed"),
        ],
    );
    m
}

#[test]
fn later_named_state_always_lands_in_done() {
    let states = task_states();
    assert_eq!(column_for_state("Task", "Later", &states), Some("Done".into()));
    assert_eq!(column_for_state("Task", " later ", &states), Some("Done".into()));
}

#[test]
fn categories_drive_columns() {
    let states = task_states();
    assert_eq!(column_for_state("Task", "New", &states), Some("To Do".into()));
    assert_eq!(column_for_state("Task", "Active", &states), Some("In Progress".into()));
    assert_eq!(column_for_state("Task", "Resolved", &states), Some("Done".into()));
    assert_eq!(column_for_state("Task", "Done", &states), Some("Done".into()));
    assert_eq!(column_for_state("Task", "Removed", &states), None);
}

#[test]
fn unknown_type_falls_back_to_name_heuristic() {
    let states = HashMap::new();
    assert_eq!(column_for_state("Widget", "Design", &states), Some("To Do".into()));
    assert_eq!(column_for_state("Widget", "Closed", &states), Some("Done".into()));
    assert_eq!(column_for_state("Widget", "Removed", &states), None);
    assert_eq!(column_for_state("Widget", "Kneading", &states), Some("In Progress".into()));
}

#[test]
fn drop_prefers_state_named_exactly_like_the_column() {
    let states = task_states();
    // "Active" is listed before "In Progress", but the exact name wins.
    assert_eq!(
        state_for_column("Task", "In Progress", &states),
        Some("In Progress".into())
    );
    // First state of the category wins when no exact name exists.
    assert_eq!(state_for_column("Task", "To Do", &states), Some("To Do".into()));
    // Done: Completed listed before Resolved in the column's categories.
    assert_eq!(state_for_column("Task", "Done", &states), Some("Done".into()));
    assert_eq!(state_for_column("Unknown", "Done", &states), None);
}

#[tokio::test]
async fn fetch_board_pipeline() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/wit/wiql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "workItems": [{"id": 11}, {"id": 12}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 12, "fields": {
                    "System.Title": "Fix bug", "System.WorkItemType": "Bug",
                    "System.State": "Active",
                    "System.AssignedTo": {"displayName": "Avin"},
                    "System.ChangedDate": "2026-07-11T00:00:00Z"
                }},
                {"id": 11, "fields": {
                    "System.Title": "Write docs", "System.WorkItemType": "Bug",
                    "System.State": "Later",
                    "System.AssignedTo": {"displayName": "Avin"},
                    "System.ChangedDate": "2026-07-11T01:00:00Z"
                }}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitemtypes/Bug/states"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"name": "Active", "color": "007acc", "category": "InProgress"},
                {"name": "Later", "color": "cccccc", "category": "InProgress"}
            ]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let board = client.fetch_board("org", "proj", None, None, false).await.unwrap();
    assert_eq!(board.items.len(), 2);
    // WIQL order preserved: 11 first.
    assert_eq!(board.items[0].id, 11);
    assert_eq!(board.items[0].column, Some("Done".into())); // Later override
    assert_eq!(board.items[1].column, Some("In Progress".into()));
    assert_eq!(board.items[1].state_color, "007acc");
    assert_eq!(board.states_by_type["Bug"].len(), 2);
}

#[tokio::test]
async fn fetch_board_pbi_scope_queries_parent_or_self() {
    let server = MockServer::start().await;
    // The WIQL must scope to the PBI's children plus the PBI itself, and
    // must NOT carry the @Me clause the default scope uses.
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/wit/wiql"))
        .and(wiremock::matchers::body_string_contains(
            "([System.Parent] = 4242 OR [System.Id] = 4242)",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "workItems": []
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    // Area AND pbi supplied: pbi wins (the mock only matches the pbi WIQL).
    let board = client
        .fetch_board("org", "proj", Some("HRM\\Gamma Guardians"), Some(4242), false)
        .await
        .unwrap();
    assert!(board.items.is_empty());

    let me_only = client.fetch_board("org", "proj", None, None, false).await;
    // Default scope generates @Me WIQL, which this mock does not match ->
    // 404 from wiremock, proving the pbi clause is really scope-dependent.
    assert!(me_only.is_err());
}

#[tokio::test]
async fn fetch_board_area_scope_queries_under_with_escaping() {
    let server = MockServer::start().await;
    // UNDER the picked area subtree, quotes escaped WIQL-style, and no @Me.
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/wit/wiql"))
        // The JSON body escapes the path's backslash, so assert around it:
        // the UNDER clause opens with the quoted root, and the single quote
        // in the name arrives WIQL-doubled.
        .and(wiremock::matchers::body_string_contains(
            "[System.AreaPath] UNDER 'HRM",
        ))
        .and(wiremock::matchers::body_string_contains("Gamma''s Guardians'"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "workItems": []
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let board = client
        .fetch_board("org", "proj", Some("HRM\\Gamma's Guardians"), None, false)
        .await
        .unwrap();
    assert!(board.items.is_empty());
}

#[tokio::test]
async fn bug_detail_builds_extra_pages_from_the_process_layout() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitems/13"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 13,
            "fields": {
                "System.Title": "Crash", "System.WorkItemType": "Bug",
                "System.State": "Active",
                "Custom.InitialFindings": "<div>Null ref in save path</div>",
                "Custom.RootCauseCategory": "Design/Requirement"
                // Custom.LessonsLearned intentionally unset
            }
        })))
        .mount(&server)
        .await;
    // Chain hop 1: project capabilities -> process id (works by name).
    Mock::given(method("GET"))
        .and(path("/org/_apis/projects/proj"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "proj-guid", "name": "proj",
            "capabilities": {"processTemplate": {
                "templateName": "Custom Agile", "templateTypeId": "proc-guid-1"
            }}
        })))
        .mount(&server)
        .await;
    // Chain hop 2: work item type -> reference name.
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitemtypes/Bug"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "Bug", "referenceName": "Microsoft.VSTS.WorkItemTypes.Bug"
        })))
        .mount(&server)
        .await;
    // Chain hop 3: the org-level process layout (the real tabs source).
    Mock::given(method("GET"))
        .and(path(
            "/org/_apis/work/processes/proc-guid-1/workItemTypes/Microsoft.VSTS.WorkItemTypes.Bug/layout",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "pages": [
                {"label": "Details", "pageType": "custom", "visible": true, "sections": []},
                {"label": "RCA", "pageType": "custom", "visible": true, "sections": [
                    {"groups": [
                        {"label": "Initial Findings", "controls": [
                            {"id": "Custom.InitialFindings", "label": "",
                             "controlType": "HtmlFieldControl", "visible": true}
                        ]}
                    ]},
                    {"groups": [
                        {"label": "Root Cause Identification", "controls": [
                            {"id": "Custom.RootCauseCategory", "label": "Root Cause Category",
                             "controlType": "FieldControl", "visible": true},
                            {"id": "System.History", "label": "Discussion",
                             "controlType": "WorkItemLogControl", "visible": true},
                            {"controlType": "LinksControl", "label": "Links", "visible": true}
                        ]}
                    ]},
                    {"groups": []}
                ]},
                {"label": "Preventive Measures", "pageType": "custom", "visible": true, "sections": [
                    {"groups": [
                        {"label": "Lessons Learned", "controls": [
                            {"id": "Custom.LessonsLearned", "label": "",
                             "controlType": "HtmlFieldControl", "visible": true}
                        ]}
                    ]}
                ]},
                {"label": "History", "pageType": "history", "visible": true, "sections": []},
                {"label": "Links", "pageType": "links", "visible": true, "sections": []},
                {"label": "Hidden Page", "pageType": "custom", "visible": false, "sections": []}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitemtypes/Bug/fields/Custom.RootCauseCategory"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "referenceName": "Custom.RootCauseCategory",
            "allowedValues": ["Code Defect", "Design/Requirement", "Environment"]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let d = client.get_work_item_detail("org", "proj", 13).await.unwrap();

    assert_eq!(d.extra_pages.len(), 2);
    let rca = &d.extra_pages[0];
    assert_eq!(rca.name, "RCA");
    // Non-field controls (history, links) are dropped; rich-text controls
    // take their group's label when their own is empty.
    assert_eq!(rca.fields.len(), 2);
    assert_eq!(rca.fields[0].label, "Initial Findings");
    assert_eq!(rca.fields[0].kind, "html");
    assert_eq!(rca.fields[0].value, "<div>Null ref in save path</div>");
    // Sections map to ADO's form columns (empty ones don't count).
    assert_eq!(rca.fields[0].section, 0);
    assert_eq!(rca.fields[1].section, 1);
    // FieldControl with allowedValues becomes a picklist.
    assert_eq!(rca.fields[1].kind, "pick");
    assert_eq!(rca.fields[1].allowed.len(), 3);
    assert_eq!(rca.fields[1].value, "Design/Requirement");

    // Empty fields still get an entry so the drawer can fill them in.
    let pm = &d.extra_pages[1];
    assert_eq!(pm.name, "Preventive Measures");
    assert_eq!(pm.fields[0].label, "Lessons Learned");
    assert_eq!(pm.fields[0].value, "");
}

#[tokio::test]
async fn detail_without_layout_access_has_no_extra_tabs() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitems/14"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 14,
            "fields": {
                "System.Title": "A task", "System.WorkItemType": "Task",
                "System.State": "To Do"
            }
        })))
        .mount(&server)
        .await;
    // Project endpoint unmocked -> 404 -> no tabs, but the detail still
    // loads and the failure reason is surfaced instead of silent.
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let d = client.get_work_item_detail("org", "proj", 14).await.unwrap();
    assert!(d.extra_pages.is_empty());
    let err = d.extra_pages_error.expect("failure reason should be surfaced");
    assert!(err.contains("project lookup failed"), "got: {err}");
}

#[tokio::test]
async fn rich_text_attachment_images_are_downloaded_for_preview() {
    let server = MockServer::start().await;
    let img_url = format!(
        "{}/org/proj/_apis/wit/attachments/att-1?fileName=shot.png&amp;download=true",
        server.uri()
    );
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitems/15"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 15,
            "fields": {
                "System.Title": "Crash", "System.WorkItemType": "Bug",
                "System.State": "Active",
                "Microsoft.VSTS.TCM.ReproSteps":
                    format!("<div>Boom <img src=\"{img_url}\"></div>")
            }
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/attachments/att-1"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "image/png")
                .set_body_bytes(vec![137u8, 80, 78, 71]),
        )
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let d = client.get_work_item_detail("org", "proj", 15).await.unwrap();

    // The field value keeps the original URL (edits must round-trip it)...
    assert!(d.description_html.contains("/_apis/wit/attachments/att-1"));
    assert!(!d.description_html.contains("data:image"));
    // ...while the preview map carries the authenticated download.
    assert_eq!(d.inline_images.len(), 1);
    assert!(d.inline_images[0].url.contains("fileName=shot.png&download=true"));
    assert!(d.inline_images[0].data.starts_with("data:image/png;base64,"));
}

#[tokio::test]
async fn fetch_board_current_sprint_runs_in_default_team_context() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/projects/proj"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "defaultTeam": {"id": "t1", "name": "Proj Team"}
        })))
        .mount(&server)
        .await;
    // The WIQL must POST to the TEAM-scoped route (the @CurrentIteration
    // macro has no meaning project-wide) and carry the macro clause.
    Mock::given(method("POST"))
        .and(path("/org/proj/Proj%20Team/_apis/wit/wiql"))
        .and(wiremock::matchers::body_string_contains(
            "[System.IterationPath] = @CurrentIteration",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "workItems": []
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let board = client
        .fetch_board("org", "proj", None, None, true)
        .await
        .unwrap();
    assert!(board.items.is_empty());
}

#[tokio::test]
async fn create_work_item_carries_fields_and_parent_relation() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/wit/workitems/$Task"))
        .and(wiremock::matchers::body_partial_json(serde_json::json!([
            {"op": "add", "path": "/fields/System.Title", "value": "Wire the login flow"}
        ])))
        .and(wiremock::matchers::body_string_contains("Microsoft.VSTS.Common.Priority"))
        // Parent nesting: Hierarchy-Reverse pointing at the PBI.
        .and(wiremock::matchers::body_string_contains("System.LinkTypes.Hierarchy-Reverse"))
        .and(wiremock::matchers::body_string_contains("/workitems/4242"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 9001,
            "_links": {"html": {"href": "https://example.invalid/wi/9001"}}
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let fields = vec![
        ("System.Title".to_string(), "Wire the login flow".to_string()),
        ("Microsoft.VSTS.Common.Priority".to_string(), "1".to_string()),
    ];
    let (id, url) = client
        .create_work_item("org", "proj", "Task", &fields, &[], Some(4242))
        .await
        .unwrap();
    assert_eq!(id, 9001);
    assert!(url.contains("/wi/9001"));
}

#[tokio::test]
async fn area_sprint_uses_that_teams_current_iteration() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/Gamma%20Guardians/_apis/work/teamsettings/iterations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"name": "SP10", "path": "proj\\26R1_SP10_Gamma_Guardians",
                 "attributes": {"startDate": "2026-07-06T00:00:00Z", "finishDate": "2026-07-17T00:00:00Z", "timeFrame": "past"}},
                {"name": "SP01", "path": "proj\\26R2_SP01_Gamma_Guardians",
                 "attributes": {"startDate": "2026-07-20T00:00:00Z", "finishDate": "2026-07-31T00:00:00Z", "timeFrame": "current"}}
            ]
        })))
        .mount(&server)
        .await;
    // Project-scoped WIQL (no team context needed once the path is explicit),
    // carrying the team's CURRENT sprint path. The backslash is asserted in
    // two parts - JSON escapes it in the request body.
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/wit/wiql"))
        .and(wiremock::matchers::body_string_contains("[System.IterationPath] UNDER 'proj"))
        .and(wiremock::matchers::body_string_contains("26R2_SP01_Gamma_Guardians'"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"workItems": []})))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let board = client
        .fetch_board("org", "proj", Some("HRM\\Gamma Guardians"), None, true)
        .await
        .unwrap();
    assert!(board.items.is_empty());
}

#[tokio::test]
async fn area_sprint_without_a_current_falls_back_to_latest_past() {
    // The next sprint wasn't created in time: show the previous one until
    // it exists (owner rule). Two past sprints - the LATER finish wins.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/Gamma%20Guardians/_apis/work/teamsettings/iterations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"name": "SP09", "path": "proj\\26R1_SP09_Gamma_Guardians",
                 "attributes": {"startDate": "2026-06-22T00:00:00Z", "finishDate": "2026-07-03T00:00:00Z", "timeFrame": "past"}},
                {"name": "SP10", "path": "proj\\26R1_SP10_Gamma_Guardians",
                 "attributes": {"startDate": "2026-07-06T00:00:00Z", "finishDate": "2026-07-17T00:00:00Z", "timeFrame": "past"}}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/wit/wiql"))
        .and(wiremock::matchers::body_string_contains("26R1_SP10_Gamma_Guardians'"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"workItems": []})))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let board = client
        .fetch_board("org", "proj", Some("HRM\\Gamma Guardians"), None, true)
        .await
        .unwrap();
    assert!(board.items.is_empty());
}

#[tokio::test]
async fn area_sprint_with_unknown_team_falls_back_to_default_team_macro() {
    // The area's last segment isn't a team name (404) - the old
    // default-team @CurrentIteration behavior takes over.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/projects/proj"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "defaultTeam": {"id": "t1", "name": "Proj Team"}
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/org/proj/Proj%20Team/_apis/wit/wiql"))
        .and(wiremock::matchers::body_string_contains(
            "[System.IterationPath] = @CurrentIteration",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"workItems": []})))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let board = client
        .fetch_board("org", "proj", Some("HRM\\Not A Team"), None, true)
        .await
        .unwrap();
    assert!(board.items.is_empty());
}

#[tokio::test]
async fn dated_iterations_carry_sprint_windows() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/classificationnodes/iterations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "proj",
            "children": [
                {"name": "Sprint 1", "attributes": {"startDate": "2026-07-20T00:00:00Z", "finishDate": "2026-07-31T00:00:00Z"}},
                {"name": "Future folder"}
            ]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let its = client.get_iterations_dated("org", "proj").await.unwrap();
    assert_eq!(its.len(), 3); // root + two children
    let sprint = its.iter().find(|i| i.path.ends_with("Sprint 1")).unwrap();
    assert_eq!(sprint.start_date.as_deref(), Some("2026-07-20T00:00:00Z"));
    assert_eq!(sprint.finish_date.as_deref(), Some("2026-07-31T00:00:00Z"));
    // Nodes without dates (root, folders) stay date-less rather than erroring.
    assert!(its.iter().find(|i| i.path == "proj").unwrap().start_date.is_none());
}

/// A screenshot pasted into a bug from a failed test run is a TEST RESULT
/// attachment, not a work item one. The old rule only recognised
/// `/_apis/wit/attachments/`, so nothing was ever fetched for it and the
/// image stayed broken forever with no request in the log to explain it.
#[test]
fn a_test_result_attachment_is_recognised_not_just_a_work_item_one() {
    use v2_lib::work_board::detail::attachment_download_url;
    let base = "https://dev.azure.com";
    for src in [
        "https://dev.azure.com/Acme/HRM/_apis/wit/attachments/GUID?fileName=a.png",
        "https://dev.azure.com/Acme/HRM/_apis/test/Runs/12/Results/1/attachments/9",
        "https://dev.azure.com/Acme/HRM/_apis/testresults/runs/12/results/1/attachments/9",
    ] {
        assert!(
            attachment_download_url(src, base).is_some(),
            "should be fetched: {src}"
        );
    }
}

/// The half that matters most: a work item's HTML is written by whoever
/// can edit the item, so the src is attacker-controlled. Sending the Azure
/// DevOps bearer token to a host of their choosing would hand it over.
#[test]
fn the_token_is_never_sent_to_a_host_we_do_not_trust() {
    use v2_lib::work_board::detail::attachment_download_url;
    let base = "https://dev.azure.com";
    // The same PATH that would otherwise qualify, on someone else's host.
    assert_eq!(
        attachment_download_url("https://evil.example/_apis/wit/attachments/GUID", base),
        None
    );
    assert_eq!(
        attachment_download_url("http://dev.azure.com.evil.example/_apis/wit/attachments/x", base),
        None
    );
    // Relative and data URLs have no host to check, so they are not fetched.
    assert_eq!(attachment_download_url("/_apis/wit/attachments/x", base), None);
    assert_eq!(attachment_download_url("data:image/png;base64,AAAA", base), None);

    // Microsoft's own Azure DevOps domains are trusted, plus whatever host
    // this client is already talking to (a mock server, in these tests).
    assert!(attachment_download_url(
        "https://acme.visualstudio.com/p/_apis/wit/attachments/x",
        base
    )
    .is_some());
    assert!(attachment_download_url(
        "http://127.0.0.1:9999/p/_apis/wit/attachments/x",
        "http://127.0.0.1:9999"
    )
    .is_some());
}

/// Avatars were fetched exactly the same way as attachments - bearer token
/// attached to whatever URL arrived over IPC - and never had the host check
/// the attachment path grew after the leak found while widening its filter.
/// Anything that could reach the command could name the host the token went
/// to, and a work item's HTML is written by whoever can edit the item.
#[test]
fn the_avatar_fetch_uses_the_same_host_rule_as_attachments() {
    use v2_lib::work_board::detail::token_may_be_sent_to;
    let base = "https://dev.azure.com";
    for bad in [
        "https://evil.example/avatar.png",
        "http://dev.azure.com.evil.example/a.png",
        // Userinfo: reads as an allowed host, connects to another one.
        "https://dev.azure.com@evil.example/a.png",
        "https://acme.visualstudio.com@evil.example/a.png",
        "/relative/avatar.png",
        "data:image/png;base64,AAAA",
        "",
    ] {
        assert!(!token_may_be_sent_to(bad, base), "token would go to {bad}");
    }

    for good in [
        "https://dev.azure.com/Acme/_apis/GraphProfile/MemberAvatars/abc",
        // Avatars and test results live on the subdomains, which the
        // attachment rule's exact-match on dev.azure.com never covered.
        "https://vssps.dev.azure.com/Acme/_apis/graph/Subjects/abc/avatars",
        "https://acme.visualstudio.com/_api/_common/identityImage?id=1",
    ] {
        assert!(token_may_be_sent_to(good, base), "should be fetched: {good}");
    }

    // An on-premises server: whatever host this client already talks to.
    assert!(token_may_be_sent_to("http://tfs.corp.local/a.png", "http://tfs.corp.local"));
    assert!(!token_may_be_sent_to("http://other.corp.local/a.png", "http://tfs.corp.local"));
}

/// An ADO URL that is not an attachment is left alone - the token has no
/// business going to it just because the host is right.
#[test]
fn only_attachment_endpoints_are_fetched() {
    use v2_lib::work_board::detail::attachment_download_url;
    assert_eq!(
        attachment_download_url(
            "https://dev.azure.com/Acme/HRM/_apis/wit/workitems/42",
            "https://dev.azure.com"
        ),
        None
    );
}

/// Azure DevOps embeds these without an api-version - the browser gets one
/// from its session, a bare request does not, and the service can answer
/// 400 rather than the bytes.
#[test]
fn an_attachment_url_is_given_an_api_version_when_it_has_none() {
    use v2_lib::work_board::detail::attachment_download_url;
    let base = "https://dev.azure.com";
    assert_eq!(
        attachment_download_url("https://dev.azure.com/o/p/_apis/wit/attachments/GUID", base),
        Some("https://dev.azure.com/o/p/_apis/wit/attachments/GUID?api-version=7.1".into())
    );
    assert_eq!(
        attachment_download_url(
            "https://dev.azure.com/o/p/_apis/wit/attachments/GUID?fileName=a.png",
            base
        ),
        Some(
            "https://dev.azure.com/o/p/_apis/wit/attachments/GUID?fileName=a.png&api-version=7.1"
                .into()
        )
    );
    // One that already says which version keeps it.
    let already = "https://dev.azure.com/o/p/_apis/wit/attachments/GUID?api-version=6.0";
    assert_eq!(attachment_download_url(already, base), Some(already.into()));
}

/// A related id of 0 means "no work item here" (a bug filed from a
/// suite-scoped runner session has no PBI). Linking /workitems/0 would
/// 400 the whole create, so non-positive ids are skipped, not sent.
#[tokio::test]
async fn create_work_item_skips_non_positive_related_ids() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/wit/workitems/$Bug"))
        .and(wiremock::matchers::body_string_contains("/workitems/77"))
        .respond_with(move |req: &wiremock::Request| {
            let body = String::from_utf8_lossy(&req.body).to_string();
            assert!(
                !body.contains("/workitems/0"),
                "a zero related id must not become a relation: {body}"
            );
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": 9002,
                "_links": {"html": {"href": "https://example.invalid/wi/9002"}}
            }))
        })
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let fields = vec![("System.Title".to_string(), "It broke".to_string())];
    let (id, _) = client
        .create_work_item("org", "proj", "Bug", &fields, &[77, 0], None)
        .await
        .unwrap();
    assert_eq!(id, 9002);
}
