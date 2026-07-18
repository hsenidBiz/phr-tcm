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
    let board = client.fetch_board("org", "proj", None, None).await.unwrap();
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
        .fetch_board("org", "proj", Some("HRM\\Gamma Guardians"), Some(4242))
        .await
        .unwrap();
    assert!(board.items.is_empty());

    let me_only = client.fetch_board("org", "proj", None, None).await;
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
        .fetch_board("org", "proj", Some("HRM\\Gamma's Guardians"), None)
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
