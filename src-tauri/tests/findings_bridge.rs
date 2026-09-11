//! The two bridge routes behind record_finding and list_findings. Local
//! data only - neither needs a signed-in client.

use v2_lib::ai_bridge::{route, BridgeContext};
use v2_lib::findings::set_root;

struct TempDir(std::path::PathBuf);
impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir()
            .join(format!("tcm-findings-bridge-{nanos}-{}", N.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The root is process-wide and cargo runs tests in parallel.
static ROOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn ctx() -> BridgeContext {
    BridgeContext { org: "acme".into(), project: "Web".into(), ..BridgeContext::default() }
}

#[tokio::test]
async fn a_finding_is_recorded_for_the_open_org_and_project_and_listed_back() {
    let dir = TempDir::new();
    let _root = ROOT_LOCK.lock().unwrap();
    set_root(dir.path().to_path_buf());

    let body = serde_json::json!({
        "kind": "spec",
        "subject": "Step10-ManagePerformanceCycle.md 7.7",
        "title": "AC-3 contradicts the table above it",
        "detail": "The table says **closed**; AC-3 says open."
    })
    .to_string();
    let (status, resp) = route(&ctx(), None, "POST", "/findings", &body, "1.0.0").await;
    assert_eq!(status, 200, "{resp}");
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["id"].as_str().is_some());
    assert_eq!(v["open"], 1);

    let (status, resp) = route(&ctx(), None, "GET", "/findings", "", "1.0.0").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["total"], 1);
    assert_eq!(v["findings"][0]["kind"], "spec");
    assert_eq!(v["findings"][0]["org"], "acme");
    assert_eq!(v["findings"][0]["status"], "open");

    let other = BridgeContext { project: "Mobile".into(), ..ctx() };
    let (_, resp) = route(&other, None, "GET", "/findings", "", "1.0.0").await;
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["total"], 0, "another project's findings are not this project's");
}

#[tokio::test]
async fn a_bad_finding_is_refused_with_the_reason() {
    let dir = TempDir::new();
    let _root = ROOT_LOCK.lock().unwrap();
    set_root(dir.path().to_path_buf());
    let (status, resp) =
        route(&ctx(), None, "POST", "/findings", r#"{"kind":"vibes","title":"x"}"#, "1.0.0").await;
    assert_eq!(status, 400);
    assert!(resp.contains("test_case, spec or code"), "{resp}");
    let (status, resp) = route(&ctx(), None, "POST", "/findings", "not json", "1.0.0").await;
    assert_eq!(status, 400);
    assert!(resp.contains("kind"), "says what the body should carry: {resp}");
}

#[tokio::test]
async fn listing_filters_by_status() {
    let dir = TempDir::new();
    let _root = ROOT_LOCK.lock().unwrap();
    set_root(dir.path().to_path_buf());
    let body = r#"{"kind":"code","subject":"IndexModel.cs","title":"Null check missing","detail":""}"#;
    let (_, resp) = route(&ctx(), None, "POST", "/findings", body, "1.0.0").await;
    let id = serde_json::from_str::<serde_json::Value>(&resp).unwrap()["id"].as_str().unwrap().to_string();
    v2_lib::findings::set_status(dir.path(), &id, "resolved").unwrap();

    let (_, resp) = route(&ctx(), None, "GET", "/findings", "", "1.0.0").await;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&resp).unwrap()["total"], 0, "open by default");
    let (_, resp) = route(&ctx(), None, "GET", "/findings?status=resolved", "", "1.0.0").await;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&resp).unwrap()["total"], 1);
    let (_, resp) = route(&ctx(), None, "GET", "/findings?status=all", "", "1.0.0").await;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&resp).unwrap()["total"], 1);
}
