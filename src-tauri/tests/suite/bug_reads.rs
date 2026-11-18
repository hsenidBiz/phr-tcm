//! `GET /bugs` on the assistant bridge: bugs read from Azure DevOps by PBI,
//! by test case, or by their own ids. A wiremock host answers as a small
//! work item graph, so the links are what decides the result.

use std::collections::BTreeMap;
use v2_lib::ado::AdoClient;
use v2_lib::ai_bridge::{route, BridgeContext};
use wiremock::matchers::{method as wm_method, path as wm_path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn ctx() -> BridgeContext {
    BridgeContext {
        org: "acme".into(),
        project: "Web".into(),
        module_ref: None,
        preconditions_ref: None,
        disabled_tools: vec![],
        working_dir: None,
        db_id: None,
        db_secrets: None,
        db_writes: false,
        api_writes: false,
        network_tools: false,
        all_projects: false,
        db_no_ask: false,
        autorun_mode: None,
        app_simple_mode: None,
        gate: Default::default(),
        current_pbi: None,
    }
}

const TESTED_BY: &str = "Microsoft.VSTS.Common.TestedBy-Forward";
const RELATED: &str = "System.LinkTypes.Related";
const CHILD: &str = "System.LinkTypes.Hierarchy-Forward";
const PARENT: &str = "System.LinkTypes.Hierarchy-Reverse";

struct Item {
    kind: String,
    title: String,
    extra: serde_json::Value,
    links: Vec<(String, i32)>,
}

/// The mock Azure DevOps: work items by id, answered one at a time (with
/// relations) and in batches (filtered by `ids=`), plus the type list.
struct World {
    server: MockServer,
    items: BTreeMap<i32, Item>,
    types: Vec<&'static str>,
    hidden: Vec<i32>,
}

impl World {
    async fn new() -> World {
        World { server: MockServer::start().await, items: BTreeMap::new(), types: vec!["Bug", "Task"], hidden: vec![] }
    }

    fn add(&mut self, id: i32, kind: &str, title: &str, links: &[(&str, i32)]) {
        self.items.insert(
            id,
            Item {
                kind: kind.into(),
                title: title.into(),
                extra: serde_json::json!({}),
                links: links.iter().map(|(r, i)| (r.to_string(), *i)).collect(),
            },
        );
    }

    fn bug(&mut self, id: i32, links: &[(&str, i32)]) {
        self.add(id, "Bug", &format!("Bug {id}"), links);
    }

    fn json_of(base: &str, id: i32, it: &Item) -> serde_json::Value {
        let mut fields = serde_json::json!({
            "System.WorkItemType": it.kind,
            "System.Title": it.title,
            "System.State": "Active",
        });
        for (k, v) in it.extra.as_object().unwrap() {
            fields[k] = v.clone();
        }
        let relations: Vec<serde_json::Value> = it
            .links
            .iter()
            .map(|(rel, to)| serde_json::json!({"rel": rel, "url": format!("{base}/acme/Web/_apis/wit/workitems/{to}")}))
            .collect();
        serde_json::json!({"id": id, "fields": fields, "relations": relations})
    }

    async fn start(&self) -> AdoClient {
        let base = self.server.uri();
        let snapshot: BTreeMap<i32, serde_json::Value> =
            self.items.iter().map(|(id, it)| (*id, Self::json_of(&base, *id, it))).collect();
        for (id, v) in &snapshot {
            Mock::given(wm_method("GET"))
                .and(wm_path(format!("/acme/_apis/wit/workitems/{id}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(v.clone()))
                .mount(&self.server)
                .await;
        }
        let hidden = self.hidden.clone();
        Mock::given(wm_method("GET"))
            .and(wm_path("/acme/_apis/wit/workitems"))
            .respond_with(move |req: &Request| {
                let ids: Vec<i32> = req
                    .url
                    .query_pairs()
                    .find(|(k, _)| k == "ids")
                    .map(|(_, v)| v.split(',').filter_map(|s| s.parse().ok()).collect())
                    .unwrap_or_default();
                // As Azure DevOps does: an unreadable id fails the batch, unless
                // errorPolicy=omit, which answers null for it.
                let omit = req.url.query_pairs().any(|(k, v)| k == "errorPolicy" && v == "omit");
                if !omit && ids.iter().any(|i| hidden.contains(i)) {
                    return ResponseTemplate::new(404);
                }
                let value: Vec<serde_json::Value> = ids
                    .iter()
                    .filter_map(|i| {
                        if hidden.contains(i) {
                            Some(serde_json::Value::Null)
                        } else {
                            snapshot.get(i).cloned()
                        }
                    })
                    .collect();
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": value}))
            })
            .mount(&self.server)
            .await;
        let names: Vec<serde_json::Value> = self.types.iter().map(|n| serde_json::json!({"name": n})).collect();
        Mock::given(wm_method("GET"))
            .and(wm_path("/acme/Web/_apis/wit/workitemtypes"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": names})))
            .mount(&self.server)
            .await;
        AdoClient::with_base_urls("tok".into(), base.clone(), base)
    }
}

async fn get(client: &AdoClient, target: &str) -> (u16, serde_json::Value, String) {
    let (status, body) = route(&ctx(), Some(client), "GET", target, "", "1.10.3").await;
    let v = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
    (status, v, body)
}

fn ids_of(v: &serde_json::Value) -> Vec<i64> {
    v["bugs"].as_array().unwrap().iter().map(|b| b["id"].as_i64().unwrap()).collect()
}

#[tokio::test]
async fn pbi_bugs_include_direct_children_and_related() {
    let mut w = World::new().await;
    w.add(42, "Product Backlog Item", "PBI", &[(CHILD, 101), (RELATED, 102), (CHILD, 103), (PARENT, 9)]);
    w.bug(101, &[(PARENT, 42)]);
    w.bug(102, &[(RELATED, 42)]);
    w.add(103, "Task", "A task", &[(PARENT, 42)]);
    w.add(9, "Feature", "Parent feature", &[]);
    let client = w.start().await;
    let (status, v, body) = get(&client, "/bugs?pbi=42").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(ids_of(&v), vec![102, 101], "newest first; the task and the feature are not bugs");
    assert_eq!(v["total"], 2);
    assert!(v.get("not_a_bug").is_none(), "empty lists are left out: {body}");
}

#[tokio::test]
async fn pbi_bugs_include_bugs_on_its_cases_once() {
    let mut w = World::new().await;
    w.add(42, "Product Backlog Item", "PBI", &[(TESTED_BY, 201), (TESTED_BY, 202), (CHILD, 101)]);
    w.add(201, "Test Case", "Case 1", &[(RELATED, 301), (RELATED, 101)]);
    w.add(202, "Test Case", "Case 2", &[(RELATED, 301)]);
    w.bug(101, &[(PARENT, 42), (RELATED, 201)]);
    w.bug(301, &[(RELATED, 201), (RELATED, 202)]);
    let client = w.start().await;
    let (status, v, body) = get(&client, "/bugs?pbi=42").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(ids_of(&v), vec![301, 101], "each bug once, though two routes reach it");
    // How the bug links back is read off its own relations.
    let b101 = v["bugs"].as_array().unwrap().iter().find(|b| b["id"] == 101).unwrap();
    assert_eq!(b101["linked_pbis"], serde_json::json!([42]));
    assert_eq!(b101["linked_test_cases"], serde_json::json!([201]));
}

#[tokio::test]
async fn case_bugs_are_its_linked_bugs() {
    let mut w = World::new().await;
    w.add(201, "Test Case", "Case", &[(RELATED, 301), (RELATED, 55), (TESTED_BY, 7)]);
    w.bug(301, &[(RELATED, 201)]);
    w.add(55, "Product Backlog Item", "Other", &[]);
    w.add(7, "Product Backlog Item", "Tested", &[]);
    let client = w.start().await;
    let (status, v, body) = get(&client, "/bugs?case=201").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(ids_of(&v), vec![301]);
}

#[tokio::test]
async fn bug_ids_list_a_test_case_under_not_a_bug() {
    let mut w = World::new().await;
    w.bug(301, &[]);
    w.bug(302, &[]);
    w.add(201, "Test Case", "Case", &[]);
    let client = w.start().await;
    let (status, v, body) = get(&client, "/bugs?ids=302,201,301").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(ids_of(&v), vec![302, 301], "the order asked");
    assert_eq!(v["not_a_bug"], serde_json::json!([201]));
}

#[tokio::test]
async fn pbi_and_ids_list_outsiders_under_not_on_pbi() {
    let mut w = World::new().await;
    w.add(42, "Product Backlog Item", "PBI", &[(CHILD, 101)]);
    w.bug(101, &[]);
    w.bug(900, &[]);
    let client = w.start().await;
    let (status, v, body) = get(&client, "/bugs?pbi=42&ids=900,101").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(ids_of(&v), vec![101]);
    assert_eq!(v["not_on_pbi"], serde_json::json!([900]));
}

#[tokio::test]
async fn an_issue_project_reads_issues_as_bugs_with_their_description() {
    // A process without "Bug" files them as Issue (detect_bug_type), with
    // the repro steps in System.Description.
    let mut w = World::new().await;
    w.types = vec!["Issue", "Task"];
    w.add(42, "Product Backlog Item", "PBI", &[(CHILD, 101), (CHILD, 102)]);
    w.add(101, "Issue", "An issue", &[]);
    w.items.get_mut(&101).unwrap().extra = serde_json::json!({
        "System.Description": "<div>Click <b>Save</b></div>",
        "Microsoft.VSTS.TCM.ReproSteps": "wrong field",
    });
    w.add(102, "Bug", "Not this project's bug type", &[]);
    let client = w.start().await;
    let (status, v, body) = get(&client, "/bugs?pbi=42").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(ids_of(&v), vec![101]);
    assert_eq!(v["bugs"][0]["repro_steps"], "Click Save");
}

/// The bugs TCM itself files: Related to the case AND to the PBI, no parent.
#[tokio::test]
async fn a_bug_filed_by_tcm_sorts_its_case_and_its_pbi_by_type() {
    let mut w = World::new().await;
    w.add(42, "Product Backlog Item", "PBI", &[(TESTED_BY, 201), (RELATED, 301)]);
    w.add(201, "Test Case", "Case", &[(RELATED, 301)]);
    w.bug(301, &[(RELATED, 201), (RELATED, 42)]);
    let client = w.start().await;
    let (status, v, body) = get(&client, "/bugs?pbi=42").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(ids_of(&v), vec![301]);
    assert_eq!(v["bugs"][0]["linked_test_cases"], serde_json::json!([201]));
    assert_eq!(v["bugs"][0]["linked_pbis"], serde_json::json!([42]));
}

/// One linked item Azure DevOps cannot read does not fail the PBI lookup.
#[tokio::test]
async fn an_unreadable_linked_item_is_left_out_of_a_pbi_lookup() {
    let mut w = World::new().await;
    w.add(42, "Product Backlog Item", "PBI", &[(CHILD, 101), (RELATED, 777)]);
    w.bug(101, &[(PARENT, 42), (RELATED, 777)]);
    w.hidden = vec![777];
    let client = w.start().await;
    let (status, v, body) = get(&client, "/bugs?pbi=42").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(ids_of(&v), vec![101]);
    // Asking for the unreadable id itself is still reported.
    let (status, _, _) = get(&client, "/bugs?ids=777").await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn pbi_and_case_together_give_the_bugs_on_both() {
    let mut w = World::new().await;
    w.add(42, "Product Backlog Item", "PBI", &[(TESTED_BY, 201), (TESTED_BY, 202)]);
    w.add(201, "Test Case", "Case 1", &[(RELATED, 301), (RELATED, 302)]);
    w.add(202, "Test Case", "Case 2", &[(RELATED, 302)]);
    w.bug(301, &[]);
    w.bug(302, &[]);
    let client = w.start().await;
    let (status, v, body) = get(&client, "/bugs?pbi=42&case=202").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(ids_of(&v), vec![302]);
}

#[tokio::test]
async fn a_test_case_id_with_a_pbi_is_not_a_bug_rather_than_not_on_the_pbi() {
    let mut w = World::new().await;
    w.add(42, "Product Backlog Item", "PBI", &[(CHILD, 101)]);
    w.bug(101, &[]);
    w.add(201, "Test Case", "Case", &[]);
    let client = w.start().await;
    let (status, v, body) = get(&client, "/bugs?pbi=42&ids=201,101").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(v["not_a_bug"], serde_json::json!([201]));
    assert!(v.get("not_on_pbi").is_none(), "{body}");
}

#[tokio::test]
async fn repro_steps_arrive_as_plain_text() {
    let mut w = World::new().await;
    w.bug(301, &[]);
    w.items.get_mut(&301).unwrap().extra = serde_json::json!({
        "Microsoft.VSTS.TCM.ReproSteps": "<div>Open the page<br>Press <b>Save</b></div><ul><li>one</li></ul>",
        "Microsoft.VSTS.TCM.SystemInfo": "<p>Edge &amp; Windows</p>",
        "System.Tags": "ui; regression",
        "System.AssignedTo": {"displayName": "Sam Tester", "uniqueName": "sam@example.com"},
    });
    let client = w.start().await;
    let (status, v, body) = get(&client, "/bugs?ids=301").await;
    assert_eq!(status, 200, "{body}");
    let b = &v["bugs"][0];
    let repro = b["repro_steps"].as_str().unwrap();
    assert!(!repro.contains('<') && repro.contains("Open the page") && repro.contains("Save"), "{repro}");
    assert_eq!(b["system_info"], "Edge & Windows");
    assert_eq!(b["tags"], serde_json::json!(["ui", "regression"]));
    assert_eq!(b["assigned_to"], "Sam Tester");
}

#[tokio::test]
async fn titles_only_returns_id_title_state() {
    let mut w = World::new().await;
    w.bug(301, &[]);
    let client = w.start().await;
    let (status, v, body) = get(&client, "/bugs?ids=301&titles_only=true").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(v["bugs"][0], serde_json::json!({"id": 301, "title": "Bug 301", "state": "Active"}));
}

#[tokio::test]
async fn paging_caps_at_20_and_titles_only_at_200() {
    let mut w = World::new().await;
    let links: Vec<(String, i32)> = (1000..1205).map(|i| (CHILD.to_string(), i)).collect();
    let refs: Vec<(&str, i32)> = links.iter().map(|(r, i)| (r.as_str(), *i)).collect();
    w.add(42, "Product Backlog Item", "PBI", &refs);
    for i in 1000..1205 {
        w.bug(i, &[]);
    }
    let client = w.start().await;

    let (_, v, _) = get(&client, "/bugs?pbi=42").await;
    assert_eq!(ids_of(&v).len(), 5, "default limit 5");
    assert_eq!(v["total"], 205);
    let (_, v, _) = get(&client, "/bugs?pbi=42&limit=100").await;
    assert_eq!(ids_of(&v).len(), 20, "capped at 20");
    let (_, v, _) = get(&client, "/bugs?pbi=42&limit=5&offset=203").await;
    assert_eq!(ids_of(&v), vec![1001, 1000], "offset pages through, newest first");
    assert_eq!(v["offset"], 203);
    assert_eq!(v["offset"], 203);
    let (_, v, _) = get(&client, "/bugs?pbi=42&titles_only=true").await;
    assert_eq!(ids_of(&v).len(), 200, "titles_only is capped at 200");
    assert_eq!(v["total"], 205);
}

#[tokio::test]
async fn no_pbi_case_or_ids_is_refused_with_a_sentence() {
    let w = World::new().await;
    let client = w.start().await;
    let (status, _, body) = get(&client, "/bugs").await;
    assert_eq!(status, 400);
    assert!(body.contains("pbi") && body.contains("case") && body.contains("ids"), "{body}");
    let (status, _, body) = get(&client, "/bugs?ids=12,abc").await;
    assert_eq!(status, 400, "a malformed id is refused, not dropped: {body}");
    let (status, _) = route(&ctx(), None, "GET", "/bugs?pbi=1", "", "1.10.3").await;
    assert_eq!(status, 503, "signed out");
}

#[tokio::test]
async fn a_transport_failure_names_no_url() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let client = AdoClient::with_base_urls("tok".into(), base.clone(), base.clone());
    let (status, _, body) = get(&client, "/bugs?pbi=42").await;
    assert_eq!(status, 502, "{body}");
    assert!(!body.contains(&base) && !body.contains("127.0.0.1") && !body.contains("_apis"), "{body}");
    assert!(body.contains("Logs"), "points at the log: {body}");
}
