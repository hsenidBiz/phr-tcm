//! A project's Test files (`v2_lib::test_files`) and the two things that
//! upload them: an API template step's `files`, and Auto Run's `upload`
//! action. The run-level checks for templates (what reaches the page, the
//! report and the activity log) are in `api_templates_runner.rs`, beside
//! the fake browser they need.

use crate::common::{quick, ready_probe, FakePage, ScriptedDriver};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use v2_lib::api_templates::exec::{build_request, Body};
use v2_lib::api_templates::runner::body_text;
use v2_lib::api_templates::share::missing_files_note;
use v2_lib::api_templates::{check, ApiTemplate, Step};
use v2_lib::autorun::recipe::SignInRecipe;
use v2_lib::autorun::runner::run_step;
use v2_lib::autorun::{store, CaseScript, StepScript};
use v2_lib::browser::actions::{execute_with, no_chooser, upload_in, Action, FILE_INPUT_JS};
use v2_lib::browser::cdp::Event;
use v2_lib::commands::test_files::{add_at, ensure_folder_at, list_at, remove_at};
use v2_lib::test_files::{
    self, add, check_for_run, content_type, folder, human_size, list, read_for_run, remove, valid_test_file_name,
    MAX_BYTES,
};

const ORG: &str = "acme";
const PROJECT: &str = "PMS";

fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

/// A file of exactly `len` bytes, written without holding them in memory.
fn sized(dir: &Path, name: &str, len: u64) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::File::create(&p).unwrap().set_len(len).unwrap();
    p
}

// --- names ---

#[test]
fn ordinary_file_names_are_test_file_names() {
    for good in [
        "cv.pdf",
        "Appraisal Form v2.docx",
        "a",
        "photo.JPEG",
        "x.tar.gz",
        "report (final).xlsx",
        "consolidated.txt",
        "console.log",
        "com10.txt",
        "lpt.txt",
        "naïve résumé.pdf",
        &"n".repeat(120),
    ] {
        assert!(valid_test_file_name(good), "{good:?} should be a test file name");
    }
}

#[test]
fn every_refused_class_of_name_is_refused() {
    for bad in [
        "",
        &"n".repeat(121),
        "..\\x",
        "a/b",
        "a\\b",
        "C:x",
        "a*b",
        "what?.pdf",
        "say\"hi\".txt",
        "a<b",
        "a>b",
        "a|b",
        "tab\there",
        "line\nbreak",
        "nul\0char",
        ".",
        "..",
        ".hidden",
        "trailing.",
        "trailing ",
        " leading",
        "CON",
        "con",
        "con.txt",
        "Con.tar.gz",
        "PRN",
        "aux.pdf",
        "NUL.docx",
        "COM1",
        "com9.txt",
        "LPT1",
        "lpt9.csv",
        "COM\u{b9}",
        "com\u{b2}.txt",
        "LPT\u{b3}.pdf",
        "CONIN$",
        "conout$.log",
        "a.txt:b",
        "\\\\server\\share\\x",
    ] {
        assert!(!valid_test_file_name(bad), "{bad:?} should be refused");
    }
}

#[test]
fn a_bad_name_never_resolves_to_a_path() {
    let dir = tempfile::tempdir().unwrap();
    let err = test_files::resolve(dir.path(), "..\\secret.txt").unwrap_err();
    assert!(err.contains("..\\secret.txt") && err.contains("cannot be a test file name"), "{err}");
    assert_eq!(test_files::resolve(dir.path(), "cv.pdf").unwrap(), dir.path().join("cv.pdf"));
}

#[test]
fn the_folder_is_per_project_under_the_auto_run_root() {
    let root = Path::new("C:/data/autorun");
    let a = folder(root, ORG, PROJECT);
    assert!(a.starts_with(root.join("test-files")), "{a:?}");
    assert_eq!(a.file_name().unwrap().to_str().unwrap(), v2_lib::autorun::recipe::project_slug(ORG, PROJECT));
    assert_ne!(a, folder(root, ORG, "Other"));
}

#[test]
fn content_types_come_from_a_fixed_table() {
    assert_eq!(content_type("a.pdf"), "application/pdf");
    assert_eq!(content_type("a.PNG"), "image/png");
    assert_eq!(content_type("a.jpg"), "image/jpeg");
    assert_eq!(content_type("a.jpeg"), "image/jpeg");
    assert_eq!(content_type("a.csv"), "text/csv");
    assert_eq!(
        content_type("a.docx"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    );
    assert_eq!(content_type("a.zip"), "application/zip");
    assert_eq!(content_type("a.exe"), "application/octet-stream");
    assert_eq!(content_type("README"), "application/octet-stream");
}

#[test]
fn sizes_read_as_a_person_says_them() {
    assert_eq!(human_size(0), "0 bytes");
    assert_eq!(human_size(1), "1 byte");
    assert_eq!(human_size(512), "512 bytes");
    assert_eq!(human_size(1536), "1.5 KB");
    assert_eq!(human_size(25 * 1024 * 1024), "25.0 MB");
}

// --- the folder ---

#[test]
fn add_list_and_remove_round_trip() {
    let root = tempfile::tempdir().unwrap();
    let picked = tempfile::tempdir().unwrap();
    let dir = folder(root.path(), ORG, PROJECT);
    assert!(list(&dir).unwrap().is_empty(), "a missing folder is an empty list");

    let b = add(&dir, &write(picked.path(), "b.pdf", b"%PDF-1"), false).unwrap();
    assert_eq!((b.name.as_str(), b.size), ("b.pdf", 6));
    assert!(!b.modified.is_empty());
    add(&dir, &write(picked.path(), "A.txt", b"hello"), false).unwrap();

    // Something that is not a test file a script could name is not listed.
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join(".b.pdf.part"), b"half").unwrap();

    let names: Vec<String> = list(&dir).unwrap().into_iter().map(|f| f.name).collect();
    assert_eq!(names, ["A.txt", "b.pdf"], "by name, ignoring case");
    assert_eq!(std::fs::read(dir.join("b.pdf")).unwrap(), b"%PDF-1");

    remove(&dir, "b.pdf").unwrap();
    let names: Vec<String> = list(&dir).unwrap().into_iter().map(|f| f.name).collect();
    assert_eq!(names, ["A.txt"]);
    let err = remove(&dir, "b.pdf").unwrap_err();
    assert!(err.contains("\"b.pdf\"") && !err.contains(&dir.display().to_string()), "{err}");
    assert!(remove(&dir, "../A.txt").unwrap_err().contains("cannot be a test file name"));
}

#[test]
fn a_same_name_is_refused_unless_replace_is_sent() {
    let root = tempfile::tempdir().unwrap();
    let picked = tempfile::tempdir().unwrap();
    let dir = folder(root.path(), ORG, PROJECT);
    add(&dir, &write(picked.path(), "cv.pdf", b"one"), false).unwrap();

    let again = write(picked.path(), "cv.pdf", b"two!");
    let err = add(&dir, &again, false).unwrap_err();
    assert!(err.contains("\"cv.pdf\" is already in Test files"), "{err}");
    assert!(!err.contains(&picked.path().display().to_string()), "a sentence names no path: {err}");
    assert_eq!(std::fs::read(dir.join("cv.pdf")).unwrap(), b"one", "nothing was written");

    let replaced = add(&dir, &again, true).unwrap();
    assert_eq!(replaced.size, 4);
    assert_eq!(std::fs::read(dir.join("cv.pdf")).unwrap(), b"two!");
    assert_eq!(list(&dir).unwrap().len(), 1);
}

#[test]
fn a_picked_file_with_an_unusable_name_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let picked = tempfile::tempdir().unwrap();
    let dir = folder(root.path(), ORG, PROJECT);
    // Names a disk allows but a test file may not have.
    for name in [".hidden", " leading.pdf"] {
        let err = add(&dir, &write(picked.path(), name, b"x"), false).unwrap_err();
        assert!(err.contains("cannot be a test file name"), "{name:?}: {err}");
    }
    assert!(list(&dir).unwrap().is_empty());
}

#[test]
fn the_size_cap_holds_on_add_and_on_read() {
    let root = tempfile::tempdir().unwrap();
    let picked = tempfile::tempdir().unwrap();
    let dir = folder(root.path(), ORG, PROJECT);

    let at_cap = sized(picked.path(), "at-cap.bin", MAX_BYTES);
    assert_eq!(add(&dir, &at_cap, false).unwrap().size as u64, MAX_BYTES);

    let over = sized(picked.path(), "over.bin", MAX_BYTES + 1);
    let err = add(&dir, &over, false).unwrap_err();
    assert!(err.contains("\"over.bin\" is larger than 25 MB"), "{err}");
    assert!(!dir.join("over.bin").exists());

    // A file put in the folder by hand is held to the same cap for a run.
    sized(&dir, "grown.bin", MAX_BYTES + 1);
    let err = check_for_run(&dir, "grown.bin", "this step").unwrap_err();
    assert!(err.contains("larger than 25 MB"), "{err}");
    let err = read_for_run(&dir, "grown.bin", "this step").unwrap_err();
    assert!(err.contains("larger than 25 MB"), "{err}");
    assert_eq!(read_for_run(&dir, "at-cap.bin", "this step").unwrap().len() as u64, MAX_BYTES);
}

#[test]
fn a_missing_file_says_where_to_add_it() {
    let dir = tempfile::tempdir().unwrap();
    let err = check_for_run(dir.path(), "cv.pdf", "the step \"Attach\"").unwrap_err();
    assert_eq!(err, "add \"cv.pdf\" to Test files (Auto Run or API Templates) - the step \"Attach\" uploads it");
}

// --- the commands' gate ---

#[test]
fn every_command_is_refused_where_auto_run_is_not_offered() {
    let root = tempfile::tempdir().unwrap();
    let picked = tempfile::tempdir().unwrap();
    let file = write(picked.path(), "cv.pdf", b"x");
    let refused = "not available in this build";
    assert_eq!(list_at(false, root.path(), ORG, PROJECT).unwrap_err(), refused);
    assert_eq!(add_at(false, root.path(), ORG, PROJECT, &file, false).unwrap_err(), refused);
    assert_eq!(remove_at(false, root.path(), ORG, PROJECT, "cv.pdf").unwrap_err(), refused);
    assert_eq!(ensure_folder_at(false, root.path(), ORG, PROJECT).unwrap_err(), refused);
    assert!(!root.path().join("test-files").exists(), "nothing was touched");
}

#[test]
fn where_offered_the_commands_work_on_the_projects_folder() {
    let root = tempfile::tempdir().unwrap();
    let picked = tempfile::tempdir().unwrap();
    let file = write(picked.path(), "cv.pdf", b"x");
    assert!(list_at(true, root.path(), " ", PROJECT).unwrap_err().contains("choose an organization and a project"));
    assert_eq!(
        add_at(true, root.path(), ORG, PROJECT, Path::new("cv.pdf"), false).unwrap_err(),
        v2_lib::commands::test_files::NOT_A_FULL_PATH
    );
    assert!(!folder(root.path(), ORG, PROJECT).exists(), "nothing was copied");
    add_at(true, root.path(), ORG, PROJECT, &file, false).unwrap();
    assert_eq!(list_at(true, root.path(), ORG, PROJECT).unwrap()[0].name, "cv.pdf");
    assert!(list_at(true, root.path(), ORG, "Other").unwrap().is_empty(), "another project has its own");
    let opened = ensure_folder_at(true, root.path(), ORG, PROJECT).unwrap();
    assert_eq!(opened, folder(root.path(), ORG, PROJECT));
    remove_at(true, root.path(), ORG, PROJECT, "cv.pdf").unwrap();
    assert!(list_at(true, root.path(), ORG, PROJECT).unwrap().is_empty());
}

// --- API templates: Step.files ---

fn template_json() -> Value {
    json!({
        "id": "pms-attach",
        "title": "Attach a form",
        "module": "PMS",
        "effect": "edit",
        "description": "Uploads the appraisal form.",
        "sources": ["Pages/Attach.cshtml.cs:10"],
        "antiforgery": { "page": "/hr/pmsv10/attach" },
        "params": [ { "name": "recordId", "type": "number", "required": true } ],
        "steps": [
            { "name": "Attach", "method": "POST",
              "path": "/hr/pmsv10/attach", "query": { "handler": "Upload" },
              "form": { "RecordId": "{{recordId}}" },
              "files": { "Document": "appraisal form.pdf" } }
        ],
        "outputs": []
    })
}

fn template() -> ApiTemplate {
    serde_json::from_value(template_json()).unwrap()
}

fn without_files() -> Value {
    let mut v = template_json();
    v["steps"][0].as_object_mut().unwrap().remove("files");
    v
}

#[test]
fn a_template_without_files_reads_and_writes_back_as_it_was() {
    let t: ApiTemplate = serde_json::from_value(without_files()).unwrap();
    assert!(t.steps[0].files.is_empty());
    let once = serde_json::to_string_pretty(&t).unwrap();
    assert!(!once.contains("\"files\""), "an empty files is left out: {once}");
    let again: ApiTemplate = serde_json::from_str(&once).unwrap();
    assert_eq!(serde_json::to_string_pretty(&again).unwrap(), once, "byte for byte");
    assert!(check(&t).is_empty(), "{:?}", check(&t));
}

#[test]
fn a_template_with_files_round_trips_them() {
    let t = template();
    assert_eq!(t.steps[0].files.get("Document").map(String::as_str), Some("appraisal form.pdf"));
    assert!(check(&t).is_empty(), "{:?}", check(&t));
    let v = serde_json::to_value(&t).unwrap();
    assert_eq!(v["steps"][0]["files"], json!({ "Document": "appraisal form.pdf" }));
}

#[test]
fn check_refuses_files_it_cannot_send() {
    let problems_of = |edit: &dyn Fn(&mut Value)| {
        let mut v = template_json();
        edit(&mut v);
        check(&serde_json::from_value::<ApiTemplate>(v).unwrap())
    };
    let has = |problems: &[String], text: &str| problems.iter().any(|p| p.contains(text));

    // Files ride on a form: none at all, or a json body, is refused.
    let p = problems_of(&|v| {
        v["steps"][0].as_object_mut().unwrap().remove("form");
    });
    assert!(has(&p, "sends files but has no form body"), "{p:?}");
    let p = problems_of(&|v| {
        v["steps"][0].as_object_mut().unwrap().remove("form");
        v["steps"][0]["json"] = json!({ "a": 1 });
    });
    assert!(has(&p, "sends files but has no form body"), "{p:?}");
    // An empty form is how a step sends only files.
    let p = problems_of(&|v| {
        v["steps"][0]["form"] = json!({});
        v["params"] = json!([]);
    });
    assert!(p.is_empty(), "{p:?}");

    let p = problems_of(&|v| v["steps"][0]["files"] = json!({ "": "cv.pdf" }));
    assert!(has(&p, "file field with an empty name"), "{p:?}");
    let p = problems_of(&|v| v["steps"][0]["files"] = json!({ "RecordId": "cv.pdf" }));
    assert!(has(&p, "'RecordId' in both form and files"), "{p:?}");
    let p = problems_of(&|v| v["steps"][0]["files"] = json!({ "Document": "../cv.pdf" }));
    assert!(has(&p, "cannot be a test file name"), "{p:?}");
    let p = problems_of(&|v| v["steps"][0]["files"] = json!({ "Document": "CON.pdf" }));
    assert!(has(&p, "cannot be a test file name"), "{p:?}");
    let p = problems_of(&|v| v["steps"][0]["files"] = json!({ "Document": "{{recordId}}.pdf" }));
    assert!(has(&p, "uses a placeholder"), "{p:?}");
    let p = problems_of(&|v| v["steps"][0]["files"] = json!({ "{{recordId}}": "cv.pdf" }));
    assert!(has(&p, "uses a placeholder"), "{p:?}");
}

fn vars() -> BTreeMap<String, Value> {
    BTreeMap::from([("recordId".to_string(), json!(7))])
}

#[test]
fn a_built_request_carries_each_files_name_type_and_bytes() {
    let t = template();
    let bytes = b"%PDF-1.7 fake".to_vec();
    let files = BTreeMap::from([("appraisal form.pdf".to_string(), bytes.clone())]);
    let built = build_request(&t.steps[0], &vars(), &files).unwrap();
    let Body::Form { fields, files: sent } = &built.body else { panic!("a form body: {:?}", built.body) };
    assert_eq!(fields.get("RecordId").map(String::as_str), Some("7"));
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].field, "Document");
    assert_eq!(sent[0].name, "appraisal form.pdf");
    assert_eq!(sent[0].content_type, "application/pdf");
    assert_eq!(sent[0].size, bytes.len() as u64);
    use base64::Engine;
    assert_eq!(base64::engine::general_purpose::STANDARD.decode(&sent[0].base64).unwrap(), bytes);

    // On the wire to the page: camelCase, beside the text fields.
    let wire = serde_json::to_value(&built).unwrap();
    assert_eq!(wire["body"]["files"][0]["contentType"], "application/pdf");
    assert_eq!(wire["body"]["files"][0]["field"], "Document");
    assert_eq!(wire["body"]["fields"]["RecordId"], "7");

    // A record, and Debug, name the file and its size - never the bytes.
    let shown = body_text(&built.body);
    assert!(shown.contains("<file appraisal form.pdf, 13 bytes>"), "{shown}");
    assert!(!shown.contains(&sent[0].base64), "{shown}");
    let debug = format!("{built:?}");
    assert!(!debug.contains(&sent[0].base64), "{debug}");
}

#[test]
fn a_form_with_no_files_goes_out_as_it_always_has() {
    let t: ApiTemplate = serde_json::from_value(without_files()).unwrap();
    let built = build_request(&t.steps[0], &vars(), &BTreeMap::new()).unwrap();
    let wire = serde_json::to_value(&built).unwrap();
    assert_eq!(wire["body"], json!({ "kind": "form", "fields": { "RecordId": "7" } }));
}

#[test]
fn a_built_request_without_the_files_bytes_is_refused_with_the_sentence() {
    let err = build_request(&template().steps[0], &vars(), &BTreeMap::new()).unwrap_err();
    assert_eq!(
        err,
        "add \"appraisal form.pdf\" to Test files (Auto Run or API Templates) - the step \"Attach\" uploads it"
    );
}

#[test]
fn the_import_note_names_test_files_this_machine_does_not_have() {
    let t = template();
    let note = missing_files_note(&t, &[]).unwrap();
    assert!(note.contains("\"appraisal form.pdf\"") && note.contains("Test files"), "{note}");
    assert_eq!(missing_files_note(&t, &["APPRAISAL FORM.pdf".to_string()]), None, "matched ignoring case");
    let plain: ApiTemplate = serde_json::from_value(without_files()).unwrap();
    assert_eq!(missing_files_note(&plain, &[]), None);
}

#[test]
fn an_import_notes_a_template_whose_test_file_is_not_here_and_carries_no_bytes() {
    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut second = template_json();
    second["id"] = json!("pms-attach-two");
    second["title"] = json!("Attach two");
    second["steps"][0]["files"] = json!({ "Document": "here.pdf" });
    let file = dir.path().join("share.json");
    std::fs::write(
        &file,
        json!({ "kind": "tcm-api-templates", "version": 1, "exported_at": "2026-10-01T00:00:00Z",
                "templates": [template_json(), second], "flows": [] })
        .to_string(),
    )
    .unwrap();
    write(&folder(root.path(), ORG, PROJECT), "here.pdf", b"x");

    let result = v2_lib::commands::api_templates::import_at(true, root.path(), ORG, PROJECT, &file).unwrap();
    assert_eq!(result.added.len(), 2, "{result:?}");
    assert_eq!(result.notes.len(), 1, "{:?}", result.notes);
    assert_eq!(result.notes[0].id, "pms-attach");
    assert!(result.notes[0].note.contains("\"appraisal form.pdf\""), "{:?}", result.notes[0]);

    // Exported again, the file travels as its name alone.
    let out = dir.path().join("out.json");
    v2_lib::commands::api_templates::export_at(true, root.path(), ORG, PROJECT, &out).unwrap();
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains("\"appraisal form.pdf\""), "{text}");
    assert!(!text.contains("base64"), "{text}");
    // A file with uploads in it says version 2, and this app reads it back.
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["version"], 2);
    let again = v2_lib::commands::api_templates::import_at(true, root.path(), ORG, PROJECT, &out).unwrap();
    assert_eq!(again.replaced.len(), 2, "{again:?}");
}

#[test]
fn an_export_with_no_uploads_stays_version_1_and_both_versions_read() {
    use v2_lib::api_templates::share::{build_doc, read_doc, to_json, NEWER};
    let plain: ApiTemplate = serde_json::from_value(without_files()).unwrap();
    let v1 = to_json(&build_doc(vec![plain], vec![], "2026-10-01T00:00:00Z")).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&v1).unwrap()["version"], 1, "older apps still import it");
    let v2 = to_json(&build_doc(vec![template()], vec![], "2026-10-01T00:00:00Z")).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&v2).unwrap()["version"], 2);
    assert_eq!(read_doc(&v1).unwrap().templates.len(), 1);
    assert_eq!(read_doc(&v2).unwrap().templates.len(), 1);
    let v3 = v2.replacen("\"version\": 2", "\"version\": 3", 1);
    assert_eq!(read_doc(&v3).unwrap_err(), NEWER);
}

// --- Auto Run: the upload action ---

#[test]
fn upload_reads_and_writes_like_the_other_actions() {
    let v = json!({ "kind": "upload", "selector": { "css": "#cv" }, "file": "cv.pdf" });
    let a: Action = serde_json::from_value(v.clone()).unwrap();
    assert!(matches!(&a, Action::Upload { file, .. } if file == "cv.pdf"));
    assert_eq!(serde_json::to_value(&a).unwrap(), v);
    assert!(a.validate().is_ok());
    assert!(!a.is_check());

    let bad = |file: &str| {
        serde_json::from_value::<Action>(json!({ "kind": "upload", "selector": "#cv", "file": file }))
            .unwrap()
            .validate()
            .unwrap_err()
    };
    assert!(bad("..\\cv.pdf").contains("cannot be a test file name"));
    assert!(bad("C:\\cv.pdf").contains("cannot be a test file name"));
    assert!(bad("").contains("cannot be a test file name"));
    let no_selector = serde_json::from_value::<Action>(json!({ "kind": "upload", "selector": {}, "file": "cv.pdf" }));
    assert!(no_selector.unwrap().validate().is_err());
}

fn script_with(action: Value) -> CaseScript {
    CaseScript {
        case_id: 501,
        title: "Attach a form".into(),
        account: None,
        area: None,
        steps: vec![StepScript { step_number: 1, actions: vec![serde_json::from_value(action).unwrap()], unchecked: None }],
        repairs: 0,
        last_repair: None,
        suspected_defect: None,
        no_save: false,
        preconditions: vec![],
        setup: None,
        changes: vec![],
        needs_unchanged: vec![],
        saved_at: None,
    }
}

#[test]
fn a_script_with_upload_saves_and_a_bad_name_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let good = script_with(json!({ "kind": "upload", "selector": { "css": "#cv" }, "file": "cv.pdf" }));
    v2_lib::commands::autorun::save_script_from_editor(root.path(), ORG, PROJECT, good.clone()).unwrap();
    let back = store::load_script(root.path(), 501).unwrap().unwrap();
    // Every save stamps when it happened; everything else is as sent.
    assert!(back.saved_at.is_some(), "the save was not stamped");
    assert_eq!(v2_lib::autorun::CaseScript { saved_at: None, ..back }, good);

    let bad = script_with(json!({ "kind": "upload", "selector": { "css": "#cv" }, "file": "a/b.pdf" }));
    let err = store::save_scripts_atomically(root.path(), &[bad]).unwrap_err().to_string();
    assert!(err.contains("case 501 step 1 action 1") && err.contains("cannot be a test file name"), "{err}");
}

fn recipe_with(steps: Value, after: Value) -> SignInRecipe {
    serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/",
        "steps": steps,
        "after_sign_in": after,
        "signed_in": { "css": "#home" }
    }))
    .unwrap()
}

#[test]
fn a_sign_in_recipe_cannot_upload() {
    let fill = json!({ "kind": "fill", "selector": { "css": "#user" }, "value": "{{username}}" });
    let upload = json!({ "kind": "upload", "selector": { "css": "#cv" }, "file": "cv.pdf" });
    assert!(recipe_with(json!([fill.clone()]), json!([])).validate().is_ok());

    let err = recipe_with(json!([fill.clone(), upload.clone()]), json!([])).validate().unwrap_err();
    assert!(err.starts_with("step 2:") && err.contains("cannot contain upload"), "{err}");
    let err = recipe_with(json!([fill.clone()]), json!([upload.clone()])).validate().unwrap_err();
    assert!(err.starts_with("after_sign_in step 1:") && err.contains("cannot contain upload"), "{err}");
    let inside = json!([{ "kind": "when_visible", "selector": { "css": "#x" }, "within_ms": 100, "then": [upload] }]);
    assert!(recipe_with(json!([fill]), inside).validate().unwrap_err().contains("cannot contain upload"));
}

/// A page whose one element is (`file: true`) or is not a file input; the
/// file input's backend node is 42.
fn page_where(file_input: bool) -> ScriptedDriver {
    let page = FakePage::default();
    ScriptedDriver::new(move |method, params| {
        if params["functionDeclaration"] == FILE_INPUT_JS {
            return Ok(json!({ "result": { "value": { "file": file_input, "disabled": false } } }));
        }
        if method == "DOM.describeNode" {
            return Ok(json!({ "node": { "backendNodeId": 42 } }));
        }
        page.answer(method, params)
    })
}

fn interceptions(d: &ScriptedDriver) -> Vec<Value> {
    d.calls_to("Page.setInterceptFileChooserDialog")
}

fn target(css: &str) -> v2_lib::browser::locator::Target {
    serde_json::from_value(json!({ "css": css })).unwrap()
}

#[tokio::test]
async fn a_file_input_gets_the_file_directly() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "cv.pdf", b"hello");
    let mut d = page_where(true);
    let out = upload_in(&mut d, &target("#cv"), &path, "\"cv.pdf\" (5 bytes)", &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "uploaded \"cv.pdf\" (5 bytes) to #cv");
    let set = d.calls_to("DOM.setFileInputFiles");
    assert_eq!(set.len(), 1);
    assert_eq!(set[0]["backendNodeId"], 42);
    let sent = std::path::PathBuf::from(set[0]["files"][0].as_str().unwrap());
    assert!(sent.is_absolute(), "{sent:?}");
    assert_eq!(std::fs::read(&sent).unwrap(), b"hello");
    assert!(d.calls_to("Input.dispatchMouseEvent").is_empty(), "nothing was clicked");
    assert!(interceptions(&d).is_empty(), "no chooser was needed");
    assert!(d.deadline_was_cleared());
}

#[tokio::test]
async fn a_button_is_clicked_and_the_chooser_it_opens_gets_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "cv.pdf", b"hello");
    let mut d = page_where(false);
    d.on_call_events.push((
        "Input.dispatchMouseEvent".to_string(),
        Event { method: "Page.fileChooserOpened".into(), params: json!({ "frameId": "F", "mode": "selectSingle", "backendNodeId": 77 }) },
    ));
    let out = upload_in(&mut d, &target("#attach"), &path, "\"cv.pdf\" (5 bytes)", &quick()).await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "uploaded \"cv.pdf\" (5 bytes) to #attach");
    assert_eq!(d.calls_to("Input.dispatchMouseEvent").len(), 3, "a real click");
    let set = d.calls_to("DOM.setFileInputFiles");
    assert_eq!(set.len(), 1);
    assert_eq!(set[0]["backendNodeId"], 77, "the chooser's own input");
    assert_eq!(interceptions(&d), vec![json!({ "enabled": true }), json!({ "enabled": false })]);
    let methods = d.methods();
    let on = methods.iter().position(|m| m == "Page.setInterceptFileChooserDialog").unwrap();
    let click = methods.iter().position(|m| m == "Input.dispatchMouseEvent").unwrap();
    let off = methods.iter().rposition(|m| m == "Page.setInterceptFileChooserDialog").unwrap();
    assert!(on < click && click < off, "{methods:?}");
}

#[tokio::test]
async fn a_click_that_opens_no_chooser_fails_with_the_sentence_and_stops_intercepting() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "cv.pdf", b"hello");
    let mut d = page_where(false);
    let out = upload_in(&mut d, &target("#nothing"), &path, "\"cv.pdf\" (5 bytes)", &quick()).await;
    assert!(!out.ok);
    assert_eq!(out.detail, no_chooser("#nothing"));
    assert_eq!(
        out.detail,
        "clicking #nothing did not open a file chooser - point upload at the page's file input or the button that opens it"
    );
    assert!(d.calls_to("DOM.setFileInputFiles").is_empty());
    assert_eq!(interceptions(&d), vec![json!({ "enabled": true }), json!({ "enabled": false })]);
}

#[tokio::test]
async fn a_button_that_cannot_be_clicked_still_stops_intercepting() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "cv.pdf", b"hello");
    let mut covered = ready_probe();
    covered["hit"] = json!(false);
    covered["covered_by"] = json!("div.modal-backdrop");
    let page = FakePage { probes: vec![covered], ..FakePage::default() };
    let mut d = ScriptedDriver::new(move |method, params| {
        if params["functionDeclaration"] == FILE_INPUT_JS {
            return Ok(json!({ "result": { "value": { "file": false, "disabled": false } } }));
        }
        page.answer(method, params)
    });
    let out = upload_in(&mut d, &target("#attach"), &path, "\"cv.pdf\" (5 bytes)", &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("covered by div.modal-backdrop"), "{}", out.detail);
    assert!(d.calls_to("Input.dispatchMouseEvent").is_empty());
    assert_eq!(interceptions(&d), vec![json!({ "enabled": true }), json!({ "enabled": false })]);
    assert!(d.deadline_was_cleared());
}

/// `page_where(false)` - a button, not a file input - whose browser refuses
/// whatever `refuse` picks out, and whose click opens a chooser described
/// by `chooser`.
fn chooser_page(
    refuse: impl Fn(&str, &Value) -> bool + Send + 'static,
    chooser: Value,
) -> ScriptedDriver {
    let page = FakePage::default();
    let mut d = ScriptedDriver::new(move |method, params| {
        if refuse(method, params) {
            return Err(v2_lib::browser::cdp::CdpError::Protocol { method: method.into(), message: "refused".into() });
        }
        if params["functionDeclaration"] == FILE_INPUT_JS {
            return Ok(json!({ "result": { "value": { "file": false, "disabled": false } } }));
        }
        page.answer(method, params)
    });
    d.on_call_events.push((
        "Input.dispatchMouseEvent".to_string(),
        Event { method: "Page.fileChooserOpened".into(), params: chooser },
    ));
    d
}

fn opened() -> Value {
    json!({ "frameId": "F", "mode": "selectSingle", "backendNodeId": 77 })
}

#[tokio::test]
async fn interception_is_switched_off_when_switching_it_on_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "cv.pdf", b"hello");
    let mut d = chooser_page(
        |m, p| m == "Page.setInterceptFileChooserDialog" && p["enabled"] == json!(true),
        opened(),
    );
    let out = upload_in(&mut d, &target("#attach"), &path, "\"cv.pdf\" (5 bytes)", &quick()).await;
    assert!(!out.ok);
    assert!(d.calls_to("Input.dispatchMouseEvent").is_empty(), "nothing was clicked");
    assert_eq!(interceptions(&d), vec![json!({ "enabled": true }), json!({ "enabled": false })]);
}

#[tokio::test]
async fn interception_is_switched_off_when_the_chooser_will_not_take_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "cv.pdf", b"hello");
    let mut d = chooser_page(|m, _| m == "DOM.setFileInputFiles", opened());
    let out = upload_in(&mut d, &target("#attach"), &path, "\"cv.pdf\" (5 bytes)", &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("refused"), "{}", out.detail);
    assert_eq!(interceptions(&d), vec![json!({ "enabled": true }), json!({ "enabled": false })]);
}

#[tokio::test]
async fn a_chooser_tied_to_no_input_fails_and_interception_is_switched_off() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "cv.pdf", b"hello");
    let mut d = chooser_page(|_, _| false, json!({ "frameId": "F", "mode": "selectSingle" }));
    let out = upload_in(&mut d, &target("#attach"), &path, "\"cv.pdf\" (5 bytes)", &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("did not tie to a file input"), "{}", out.detail);
    assert!(d.calls_to("DOM.setFileInputFiles").is_empty());
    assert_eq!(interceptions(&d), vec![json!({ "enabled": true }), json!({ "enabled": false })]);
}

#[tokio::test]
async fn interception_that_will_not_switch_off_is_said_in_the_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "cv.pdf", b"hello");
    let mut d = chooser_page(
        |m, p| m == "Page.setInterceptFileChooserDialog" && p["enabled"] == json!(false),
        opened(),
    );
    let out = upload_in(&mut d, &target("#attach"), &path, "\"cv.pdf\" (5 bytes)", &quick()).await;
    assert!(out.ok, "the file still went in: {}", out.detail);
    assert!(out.detail.starts_with("uploaded \"cv.pdf\" (5 bytes) to #attach"), "{}", out.detail);
    assert!(out.detail.ends_with(v2_lib::browser::actions::CHOOSER_STILL_HELD), "{}", out.detail);
}

#[tokio::test]
async fn nothing_to_upload_to_fails_without_touching_the_chooser() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "cv.pdf", b"hello");
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let out = upload_in(&mut d, &target("#cv"), &path, "\"cv.pdf\" (5 bytes)", &quick()).await;
    assert!(!out.ok);
    assert!(out.detail.contains("#cv not found"), "{}", out.detail);
    assert!(interceptions(&d).is_empty());
    assert!(d.deadline_was_cleared());

    let mut d = FakePage { found: 2, ..FakePage::default() }.driver();
    let out = upload_in(&mut d, &target("#cv"), &path, "\"cv.pdf\" (5 bytes)", &quick()).await;
    assert!(out.detail.contains("matched 2 elements"), "{}", out.detail);
}

#[tokio::test]
async fn the_executor_alone_does_not_carry_out_an_upload() {
    let mut d = FakePage::default().driver();
    let a: Action = serde_json::from_value(json!({ "kind": "upload", "selector": "#cv", "file": "cv.pdf" })).unwrap();
    let out = execute_with(&mut d, &a, &quick()).await;
    assert!(!out.ok);
    assert_eq!(out.detail, "upload is carried out by the runner");
}

fn upload_step(file: &str) -> StepScript {
    StepScript {
        step_number: 1,
        actions: vec![serde_json::from_value(json!({ "kind": "upload", "selector": { "css": "#cv" }, "file": file })).unwrap()],
        unchecked: None,
    }
}

#[tokio::test]
async fn the_runner_uploads_from_the_projects_test_files() {
    let root = tempfile::tempdir().unwrap();
    write(&folder(root.path(), ORG, PROJECT), "cv.pdf", b"hello");
    let mut d = page_where(true);
    let mut account = None;
    let out = run_step(&mut d, root.path(), ORG, PROJECT, &upload_step("cv.pdf"), &quick(), &mut account).await.unwrap();
    assert!(out[0].ok, "{}", out[0].detail);
    assert_eq!(out[0].detail, "uploaded \"cv.pdf\" (5 bytes) to #cv");
    let sent = d.calls_to("DOM.setFileInputFiles")[0]["files"][0].as_str().unwrap().to_string();
    assert!(sent.ends_with("cv.pdf") && sent.contains("test-files"), "{sent}");
}

#[tokio::test]
async fn a_file_missing_from_test_files_fails_the_step_before_the_page_is_touched() {
    let root = tempfile::tempdir().unwrap();
    // In another project's Test files, which is not this one's.
    write(&folder(root.path(), ORG, "Other"), "cv.pdf", b"hello");
    let mut d = page_where(true);
    let mut account = None;
    let out = run_step(&mut d, root.path(), ORG, PROJECT, &upload_step("cv.pdf"), &quick(), &mut account).await.unwrap();
    assert!(!out[0].ok);
    assert!(
        out[0].detail.starts_with("add \"cv.pdf\" to Test files (Auto Run or API Templates) - this step uploads it"),
        "{}",
        out[0].detail
    );
    let touched: Vec<String> = d.methods().into_iter().filter(|m| m != "Page.captureScreenshot").collect();
    assert!(touched.is_empty(), "the page was touched: {touched:?}");
}

#[tokio::test]
async fn a_test_file_over_the_cap_fails_the_step_before_the_page_is_touched() {
    let root = tempfile::tempdir().unwrap();
    sized(&folder(root.path(), ORG, PROJECT), "big.bin", MAX_BYTES + 1);
    let mut d = page_where(true);
    let mut account = None;
    let out = run_step(&mut d, root.path(), ORG, PROJECT, &upload_step("big.bin"), &quick(), &mut account).await.unwrap();
    assert!(!out[0].ok);
    assert!(out[0].detail.contains("larger than 25 MB"), "{}", out[0].detail);
    assert!(d.calls_to("DOM.setFileInputFiles").is_empty());
}

#[test]
fn the_guides_live_section_lists_names_and_sizes() {
    let files = vec![test_files::TestFile { name: "cv.pdf".into(), size: 2048, modified: "1".into() }];
    let s = test_files::guide_section(&files);
    assert!(s.contains("`cv.pdf` (2.0 KB)"), "{s}");
    assert_eq!(test_files::guide_section(&[]), "", "none adds nothing - the guides say what that means");
}

#[test]
fn the_guides_document_files_and_upload() {
    let api = v2_lib::api_templates::guide::text(&[], None);
    assert!(api.contains("`files`") && api.contains("Test files") && api.contains("never invent a name"), "{api}");
    assert!(!api.contains("File fields are not supported"));
    let autorun = v2_lib::autorun::guide::autorun_guide();
    assert!(autorun.contains("\"kind\": \"upload\"") && autorun.contains("never invent a name"));
}

#[test]
fn a_step_value_never_needs_the_struct_literal_to_know_about_files() {
    // `Step` built from JSON with no files is the same as one with an
    // explicitly empty map.
    let a: Step = serde_json::from_value(json!({ "name": "s", "method": "GET", "path": "/x" })).unwrap();
    let b: Step = serde_json::from_value(json!({ "name": "s", "method": "GET", "path": "/x", "files": {} })).unwrap();
    assert_eq!(a, b);
}
