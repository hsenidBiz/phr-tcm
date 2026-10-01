//! Sharing API templates and flows as one file: export strips the proof and
//! never reads run history; import takes each entry on its own merits,
//! replaces a same-id one (which arrives unproven) and keeps that id's run
//! history here.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use v2_lib::api_templates::flow::{Flow, FlowSaved};
use v2_lib::api_templates::share::{self, TemplatesExportResult};
use v2_lib::api_templates::store::{self, RunRecord};
use v2_lib::api_templates::{flow_store, ApiTemplate, Proven};
use v2_lib::commands::api_templates::{export_at, import_at};

const ORG: &str = "Org";
const FROM: &str = "Sender";
const TO: &str = "Receiver";

fn template_json() -> Value {
    json!({
        "id": "pms-create-draft-cycle",
        "title": "Create a draft performance cycle",
        "module": "PMS / Performance Cycle",
        "effect": "create",
        "description": "Cycle setup; leaves the cycle in Draft.",
        "sources": ["Pages/PerformanceCycle/Index.CycleSetup.cshtml.cs:95"],
        "antiforgery": { "page": "/hr/pmsv10/performancecycle?mode=create" },
        "params": [{ "name": "cycleName", "type": "string", "required": true }],
        "steps": [
            { "name": "Cycle setup", "method": "POST",
              "path": "/hr/pmsv10/performancecycle", "query": { "handler": "SaveProgress" },
              "form": { "CycleName": "{{cycleName}}" },
              "expect": { "status": 200, "json": { "success": true } },
              "capture": { "cycleId": "$.cycleId" } }
        ],
        "outputs": ["cycleId"],
        "stage": { "flow": "pms-performance-cycle", "id": "setup" }
    })
}

fn template() -> ApiTemplate {
    serde_json::from_value(template_json()).unwrap()
}

fn flow_json() -> Value {
    json!({
        "id": "pms-performance-cycle",
        "title": "Performance cycle wizard",
        "module": "PMS / Performance Cycle",
        "subject": { "name": "cycleId", "type": "number" },
        "stages": [
            { "id": "setup", "title": "Cycle setup", "creates": true,
              "check": "SELECT 1 FROM perf_cycle WHERE cycle_id = {{cycleId}}" },
            { "id": "publish", "title": "Publish", "requires": ["setup"],
              "check": "SELECT 1 FROM perf_cycle WHERE cycle_id = {{cycleId}} AND status = 'Published'" }
        ]
    })
}

fn flow() -> Flow {
    serde_json::from_value(flow_json()).unwrap()
}

fn proven() -> Proven {
    Proven {
        at: "2026-09-30 10:00:00".into(),
        origin: "https://sender-site.example".into(),
        account: "sender.admin".into(),
        outputs: [("cycleId".to_string(), json!(272))].into(),
    }
}

fn run(account: &str) -> RunRecord {
    serde_json::from_value(json!({ "at": "2026-09-30 11:00:00", "account": account, "ok": true,
                                   "outputs": { "cycleId": 301 } }))
    .unwrap()
}

/// Saves a proven template (with a run) and a saved flow under `project`.
fn seed(root: &Path, project: &str) {
    store::save(root, ORG, project, &ApiTemplate { proven: Some(proven()), ..template() }).unwrap();
    store::append_run(root, ORG, project, &template().id, run("sender.runner")).unwrap();
    let saved = FlowSaved { at: "2026-09-30 09:00:00".into(), sample: json!({ "cycleId": 274 }) };
    flow_store::save(root, ORG, project, &Flow { saved: Some(saved), ..flow() }).unwrap();
}

fn write_file(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

fn share_file(dir: &Path, templates: Vec<Value>, flows: Vec<Value>) -> PathBuf {
    let doc = json!({ "kind": "tcm-api-templates", "version": 1, "exported_at": "2026-10-01T00:00:00Z",
                      "templates": templates, "flows": flows });
    write_file(dir, "share.json", &serde_json::to_string_pretty(&doc).unwrap())
}

#[test]
fn export_strips_proof_and_never_includes_run_history() {
    let root = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    seed(root.path(), FROM);
    let path = out.path().join("api-templates-sender.json");

    let result = export_at(true, root.path(), ORG, FROM, &path).unwrap();
    assert_eq!(result, TemplatesExportResult { templates: 1, flows: 1, skipped: 0 });

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\n  \"kind\""), "the file is pretty-printed");
    let doc: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(doc["kind"], "tcm-api-templates");
    assert_eq!(doc["version"], 1);
    let at = doc["exported_at"].as_str().unwrap();
    assert!(at.len() == 20 && at.ends_with('Z') && at.contains('T'), "ISO UTC: {at}");
    assert_eq!(doc["templates"].as_array().unwrap().len(), 1);
    assert_eq!(doc["flows"].as_array().unwrap().len(), 1);
    assert!(doc["templates"][0].get("proven").is_none());
    assert!(doc["flows"][0].get("saved").is_none());
    // Nothing captured on the sender's site, and nothing saying where it is.
    for leak in ["sender.admin", "sender.runner", "sender-site", "Sender", "runs", "274", "272", "301"] {
        assert!(!text.contains(leak), "{leak} must not be in the file");
    }
    let keys: Vec<&String> = doc.as_object().unwrap().keys().collect();
    assert_eq!(keys.len(), 5, "{keys:?}");

    // The template is exactly the saved one, less its proof.
    let back: ApiTemplate = serde_json::from_value(doc["templates"][0].clone()).unwrap();
    assert_eq!(back, template());
}

#[test]
fn export_leaves_out_and_counts_a_saved_file_that_does_not_parse() {
    let root = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    seed(root.path(), FROM);
    std::fs::write(store::templates_dir(root.path(), ORG, FROM).join("broken.json"), "{").unwrap();
    std::fs::write(flow_store::flows_dir(root.path(), ORG, FROM).join("broken-flow.json"), "[]").unwrap();

    let result = export_at(true, root.path(), ORG, FROM, &out.path().join("x.json")).unwrap();
    assert_eq!(result, TemplatesExportResult { templates: 1, flows: 1, skipped: 2 });
}

#[test]
fn an_empty_project_has_nothing_to_export() {
    let root = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("x.json");
    assert_eq!(export_at(true, root.path(), ORG, FROM, &path).unwrap_err(), share::NOTHING_TO_EXPORT);
    assert!(!path.exists());
}

#[test]
fn a_round_trip_reproduces_the_templates_and_flows_without_their_proof() {
    let root = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    seed(root.path(), FROM);
    let path = out.path().join("share.json");
    export_at(true, root.path(), ORG, FROM, &path).unwrap();

    let result = import_at(true, root.path(), ORG, TO, &path).unwrap();
    assert_eq!(result.added, vec!["Performance cycle wizard (flow)", "Create a draft performance cycle"]);
    assert!(result.replaced.is_empty());
    assert!(result.skipped.is_empty(), "{:?}", result.skipped);
    assert!(result.notes.is_empty(), "{:?}", result.notes);

    let listed = store::list(root.path(), ORG, TO).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].template, template(), "unproven, otherwise the same");
    assert!(listed[0].runs.is_empty(), "run history never travels");
    assert_eq!(flow_store::list(root.path(), ORG, TO).unwrap(), vec![flow()]);
}

#[test]
fn a_same_id_import_replaces_the_saved_one_unproven_and_keeps_its_run_history() {
    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    seed(root.path(), TO);
    store::append_run(root.path(), ORG, TO, &template().id, run("receiver.runner")).unwrap();

    let mut incoming = template_json();
    incoming["title"] = json!("Create a draft cycle, revised");
    let mut incoming_flow = flow_json();
    incoming_flow["title"] = json!("Cycle wizard, revised");
    let path = share_file(dir.path(), vec![incoming], vec![incoming_flow]);

    let result = import_at(true, root.path(), ORG, TO, &path).unwrap();
    assert!(result.added.is_empty());
    assert_eq!(result.replaced, vec!["Cycle wizard, revised (flow)", "Create a draft cycle, revised"]);

    let saved = store::load(root.path(), ORG, TO, &template().id).unwrap().unwrap();
    assert_eq!(saved.title, "Create a draft cycle, revised");
    assert_eq!(saved.proven, None, "a replaced template is unproven");
    let on_disk = std::fs::read_to_string(store::templates_dir(root.path(), ORG, TO).join("pms-create-draft-cycle.json")).unwrap();
    assert!(!on_disk.contains("\"proven\""), "the proof proven here is gone from the file: {on_disk}");
    let saved_flow = flow_store::load(root.path(), ORG, TO, &flow().id).unwrap().unwrap();
    assert_eq!(saved_flow.title, "Cycle wizard, revised");
    assert_eq!(saved_flow.saved, None);
    let runs = &store::list(root.path(), ORG, TO).unwrap()[0].runs;
    assert_eq!(runs.len(), 2, "the run history here is kept");
    assert_eq!(runs[0].account, "receiver.runner");
}

#[test]
fn invalid_entries_are_skipped_with_their_reasons_and_the_rest_import() {
    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();

    let mut bad_id = template_json();
    bad_id["id"] = json!("Bad Id");
    let mut unknown_field = template_json();
    unknown_field["id"] = json!("has-an-extra");
    unknown_field["colour"] = json!("blue");
    let mut bad_check = template_json();
    bad_check["id"] = json!("bad-placeholder");
    bad_check["steps"][0]["form"]["Other"] = json!("{{nobody}}");
    // A file that carries proof anyway imports without it.
    let mut with_proof = template_json();
    with_proof["id"] = json!("carries-proof");
    with_proof["title"] = json!("Carries proof");
    with_proof["proven"] = serde_json::to_value(proven()).unwrap();
    // Two of one id: the later wins.
    let mut first = template_json();
    first["title"] = json!("First of two");
    let mut second = template_json();
    second["title"] = json!("Second of two");

    let mut no_creator = flow_json();
    no_creator["id"] = json!("no-creator");
    no_creator["stages"][0]["creates"] = json!(false);
    let mut flow_with_saved = flow_json();
    flow_with_saved["saved"] = json!({ "at": "2026-09-30 09:00:00", "sample": { "cycleId": 274 } });

    let path = share_file(
        dir.path(),
        vec![bad_id, unknown_field, bad_check, with_proof, first, second],
        vec![no_creator, flow_with_saved],
    );
    let result = import_at(true, root.path(), ORG, TO, &path).unwrap();

    assert_eq!(result.added, vec!["Performance cycle wizard (flow)", "Carries proof", "Second of two"]);
    let reason = |id: &str| -> String {
        result.skipped.iter().filter(|s| s.id == id).map(|s| s.reason.clone()).collect::<Vec<_>>().join(" | ")
    };
    assert_eq!(result.skipped.len(), 5, "{:?}", result.skipped);
    assert!(reason("no-creator").contains("creates: true"), "{}", reason("no-creator"));
    assert!(reason("no-creator").starts_with("this flow is not valid"));
    assert!(reason("Bad Id").contains("id is not valid"), "{}", reason("Bad Id"));
    assert!(reason("has-an-extra").contains("colour"), "{}", reason("has-an-extra"));
    assert!(reason("bad-placeholder").contains("nobody"), "{}", reason("bad-placeholder"));
    assert!(reason("pms-create-draft-cycle").contains("later template in the file has the same id"));

    assert_eq!(store::load(root.path(), ORG, TO, "carries-proof").unwrap().unwrap().proven, None);
    assert_eq!(store::load(root.path(), ORG, TO, "pms-create-draft-cycle").unwrap().unwrap().title, "Second of two");
    assert_eq!(flow_store::load(root.path(), ORG, TO, &flow().id).unwrap().unwrap().saved, None);
    for id in ["has-an-extra", "bad-placeholder"] {
        assert_eq!(store::load(root.path(), ORG, TO, id).unwrap(), None, "{id}");
    }
    assert_eq!(flow_store::load(root.path(), ORG, TO, "no-creator").unwrap(), None);
}

#[test]
fn a_file_that_is_not_an_export_this_app_reads_is_refused_whole() {
    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let refused = |name: &str, body: &str| {
        let p = write_file(dir.path(), name, body);
        import_at(true, root.path(), ORG, TO, &p).unwrap_err()
    };
    let one = serde_json::to_string(&vec![template_json()]).unwrap();

    assert_eq!(
        refused("kind.json", &format!(r#"{{"kind":"tcm-backup","version":1,"templates":{one}}}"#)),
        share::WRONG_KIND
    );
    assert_eq!(refused("nokind.json", &format!(r#"{{"version":1,"templates":{one}}}"#)), share::WRONG_KIND);
    assert_eq!(
        refused("newer.json", &format!(r#"{{"kind":"tcm-api-templates","version":3,"templates":{one}}}"#)),
        share::NEWER
    );
    assert_eq!(refused("text.json", "not json at all"), share::NOT_JSON);
    assert_eq!(
        refused("empty.json", r#"{"kind":"tcm-api-templates","version":1,"templates":[],"flows":[]}"#),
        share::NOTHING_IN_IT
    );
    let big = format!(
        r#"{{"kind":"tcm-api-templates","version":1,"templates":{one},"pad":"{}"}}"#,
        "x".repeat(share::MAX_FILE_BYTES as usize)
    );
    assert_eq!(refused("big.json", &big), share::TOO_BIG);
    assert!(store::list(root.path(), ORG, TO).unwrap().is_empty(), "nothing was written");

    // A file that is not there names only itself.
    let missing = import_at(true, root.path(), ORG, TO, &dir.path().join("gone.json")).unwrap_err();
    assert_eq!(missing, "gone.json could not be read - see Settings, Logs");
}

#[test]
fn a_template_whose_flow_is_nowhere_still_imports_with_a_note() {
    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut lost = template_json();
    lost["stage"] = json!({ "flow": "missing-flow", "id": "setup" });
    let path = share_file(dir.path(), vec![lost], vec![]);

    let result = import_at(true, root.path(), ORG, TO, &path).unwrap();
    assert_eq!(result.added, vec!["Create a draft performance cycle"]);
    assert_eq!(result.notes.len(), 1);
    assert_eq!(result.notes[0].id, "pms-create-draft-cycle");
    assert!(result.notes[0].note.contains("missing-flow"), "{}", result.notes[0].note);
    assert!(store::load(root.path(), ORG, TO, "pms-create-draft-cycle").unwrap().is_some());

    // A flow already saved here resolves it: no note.
    flow_store::save(root.path(), ORG, TO, &Flow { id: "missing-flow".into(), ..flow() }).unwrap();
    let again = import_at(true, root.path(), ORG, TO, &path).unwrap();
    assert!(again.notes.is_empty(), "{:?}", again.notes);
    assert_eq!(again.replaced, vec!["Create a draft performance cycle"]);
}

#[test]
fn export_and_import_are_refused_where_auto_run_is_not_offered() {
    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    seed(root.path(), FROM);
    let out = dir.path().join("out.json");
    assert_eq!(export_at(false, root.path(), ORG, FROM, &out).unwrap_err(), "not available in this build");
    assert!(!out.exists());

    let path = share_file(dir.path(), vec![template_json()], vec![flow_json()]);
    assert_eq!(import_at(false, root.path(), ORG, TO, &path).unwrap_err(), "not available in this build");
    assert!(store::list(root.path(), ORG, TO).unwrap().is_empty());
    assert!(flow_store::list(root.path(), ORG, TO).unwrap().is_empty());
}

/// Every file under `dir`, as paths relative to it with `/` separators.
fn files_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(d) = pending.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                pending.push(p);
            } else {
                out.push(p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn hostile_ids_and_step_paths_are_skipped_and_nothing_lands_outside_the_project_folders() {
    let outer = tempfile::tempdir().unwrap();
    let root = outer.path().join("root");
    std::fs::create_dir(&root).unwrap();
    let dir = tempfile::tempdir().unwrap();

    let with_id = |id: &str| {
        let mut t = template_json();
        t["id"] = json!(id);
        t
    };
    let with_path = |id: &str, path: &str| {
        let mut t = with_id(id);
        t["steps"][0]["path"] = json!(path);
        t
    };
    let mut flow_escape = flow_json();
    flow_escape["id"] = json!("../evil-flow");
    let path = share_file(
        dir.path(),
        vec![
            with_id("../evil"),
            with_id("a\\b"),
            with_id("a/b"),
            with_path("protocol-relative", "//evil.example/x"),
            with_path("encoded-dotdot", "/a/%2e%2e/b"),
            template_json(),
        ],
        vec![flow_escape, flow_json()],
    );

    let result = import_at(true, root.as_path(), ORG, TO, &path).unwrap();
    let skipped: Vec<&str> = result.skipped.iter().map(|s| s.id.as_str()).collect();
    for id in ["../evil", "a\\b", "a/b", "protocol-relative", "encoded-dotdot", "../evil-flow"] {
        assert!(skipped.contains(&id), "{id} must be skipped: {:?}", result.skipped);
    }
    let reason = |id: &str| result.skipped.iter().find(|s| s.id == id).unwrap().reason.clone();
    assert!(reason("protocol-relative").contains("safe relative path"), "{}", reason("protocol-relative"));
    assert!(reason("encoded-dotdot").contains("safe relative path"), "{}", reason("encoded-dotdot"));
    assert_eq!(result.added, vec!["Performance cycle wizard (flow)", "Create a draft performance cycle"]);

    // Only the root went into the outer folder, and inside it only the two
    // files the valid entries make, in the project's own folders.
    let outer_names: Vec<String> =
        std::fs::read_dir(outer.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(outer_names, vec!["root"]);
    let templates = store::templates_dir(&root, ORG, TO);
    let flows = flow_store::flows_dir(&root, ORG, TO);
    let rel = |p: &Path, f: &str| p.join(f).strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
    let mut want = vec![rel(&flows, "pms-performance-cycle.json"), rel(&templates, "pms-create-draft-cycle.json")];
    want.sort();
    assert_eq!(files_under(&root), want);
}

#[test]
fn a_flow_saved_here_that_does_not_read_gets_its_own_note() {
    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let flows = flow_store::flows_dir(root.path(), ORG, TO);
    std::fs::create_dir_all(&flows).unwrap();
    std::fs::write(flows.join("pms-performance-cycle.json"), "{").unwrap();
    let path = share_file(dir.path(), vec![template_json()], vec![]);

    let result = import_at(true, root.path(), ORG, TO, &path).unwrap();
    assert_eq!(result.added, vec!["Create a draft performance cycle"]);
    assert_eq!(result.notes.len(), 1, "{:?}", result.notes);
    assert!(result.notes[0].note.contains("could not be read"), "{}", result.notes[0].note);
    assert!(!result.notes[0].note.contains("not in the file"), "{}", result.notes[0].note);
}

#[test]
fn replacing_a_flow_names_the_saved_templates_it_no_longer_fits() {
    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    flow_store::save(root.path(), ORG, TO, &flow()).unwrap();
    // Saved here, not in the file: performs the "publish" stage.
    let mut publish = template_json();
    publish["id"] = json!("pms-publish-cycle");
    publish["title"] = json!("Publish a cycle");
    publish["params"] = json!([{ "name": "cycleId", "type": "number", "required": true }]);
    publish["steps"] = json!([{ "name": "Publish", "method": "POST", "path": "/hr/pmsv10/publish",
                                "form": { "CycleId": "{{cycleId}}" } }]);
    publish["outputs"] = json!([]);
    publish["stage"] = json!({ "flow": "pms-performance-cycle", "id": "publish" });
    let publish: ApiTemplate = serde_json::from_value(publish).unwrap();
    store::save(root.path(), ORG, TO, &publish).unwrap();

    // The incoming flow has no "publish" stage.
    let mut incoming = flow_json();
    incoming["stages"] = json!([flow_json()["stages"][0].clone()]);
    let path = share_file(dir.path(), vec![], vec![incoming]);

    let result = import_at(true, root.path(), ORG, TO, &path).unwrap();
    assert_eq!(result.replaced, vec!["Performance cycle wizard (flow)"]);
    assert_eq!(result.notes.len(), 1, "{:?}", result.notes);
    let note = &result.notes[0];
    assert_eq!((note.id.as_str(), note.title.as_str()), ("pms-publish-cycle", "Publish a cycle"));
    assert!(note.note.contains("replaced its flow pms-performance-cycle"), "{}", note.note);
    assert!(note.note.contains("publish"), "{}", note.note);
}
