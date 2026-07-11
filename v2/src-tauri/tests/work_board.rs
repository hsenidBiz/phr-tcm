//! Column/state rules ported from v1 work_item.py + mywork_screen semantics
//! (the "Later"->Done rule, name heuristics, exact-column-name preference).

use std::collections::HashMap;
use v2_lib::ado::AdoClient;
use v2_lib::work_board::{column_for_state, state_for_column, StateInfo};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
    let board = client.fetch_board("org", "proj").await.unwrap();
    assert_eq!(board.items.len(), 2);
    // WIQL order preserved: 11 first.
    assert_eq!(board.items[0].id, 11);
    assert_eq!(board.items[0].column, Some("Done".into())); // Later override
    assert_eq!(board.items[1].column, Some("In Progress".into()));
    assert_eq!(board.items[1].state_color, "007acc");
    assert_eq!(board.states_by_type["Bug"].len(), 2);
}
