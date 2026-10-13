//! Sync: the export record, each case's state, and Update, which is the
//! export again for the cases asked for only. The fake clone and Auto Run
//! store are Export to Playwright's own fixture (`pw_export_write`).

use crate::pw_export_write::{doc, docs, write, Fx, MD_REL, ORG, PROJECT};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use v2_lib::autorun::components::{self, Component};
use v2_lib::autorun::{store, CaseScript, StepScript};
use v2_lib::pw_export::export::{write_recorded, ExportResult};
use v2_lib::pw_export::test_case::CaseDoc;
use v2_lib::pw_repo::sync::{self, ExportRecord, SyncRow, SyncState};

struct Sx {
    fx: Fx,
    dir: tempfile::TempDir,
}

impl Sx {
    fn new() -> Sx {
        Sx { fx: Fx::new(), dir: tempfile::tempdir().unwrap() }
    }

    fn root(&self) -> &Path {
        self.fx.root.path()
    }

    fn export(&self, ids: &[i32], d: &BTreeMap<i32, CaseDoc>) -> Result<ExportResult, String> {
        write_recorded(self.root(), self.dir.path(), ORG, PROJECT, ids, &self.fx.clone_path(), &self.fx.modules.borrow(), d)
    }

    fn rows(&self) -> BTreeMap<i32, SyncRow> {
        sync::states(self.dir.path(), self.root(), &self.fx.clone_path(), ORG, PROJECT)
            .unwrap()
            .into_iter()
            .map(|r| (r.case_id, r))
            .collect()
    }

    fn state(&self, id: i32) -> SyncState {
        self.rows()[&id].state.clone()
    }

    fn record_path(&self) -> std::path::PathBuf {
        sync::record_path(self.dir.path(), &self.fx.clone_path(), ORG, PROJECT)
    }

    fn script(&self, id: i32) -> CaseScript {
        store::load_script(self.root(), id).unwrap().unwrap()
    }

    /// Saves `id`'s script stamped with this project, as the app saves it.
    fn stamp(&self, id: i32) {
        let mut s = self.script(id);
        s.organization = Some(ORG.into());
        s.project = Some(PROJECT.into());
        store::save_script(self.root(), &s).unwrap();
    }

    /// A repair: the step's selector changes.
    fn repair(&self, id: i32, selector: &str) {
        let mut s = self.script(id);
        s.steps[0].actions = serde_json::from_value(json!([{ "kind": "click", "selector": selector }])).unwrap();
        s.repairs += 1;
        store::save_script(self.root(), &s).unwrap();
    }

    fn raw(&self, id: i32) -> String {
        let index: serde_json::Value = serde_json::from_str(&self.fx.read_clone("suites/_generated/index.json")).unwrap();
        format!("suites/_generated/{}", index[id.to_string()].as_str().unwrap())
    }
}

/// Every file under `dir`, by path relative to it, with its bytes.
fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                out.insert(rel, fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

#[test]
fn a_repair_makes_a_case_out_of_date_and_an_unchanged_one_stays_current() {
    let sx = Sx::new();
    sx.fx.good(20, "Twenty");
    sx.fx.good(21, "Twenty one");
    let r = sx.export(&[20, 21], &docs(&[(20, "Twenty"), (21, "Twenty one")])).unwrap();
    assert_eq!(r.record_error, None);
    assert_eq!(sx.state(20), SyncState::Current);
    assert_eq!(sx.state(21), SyncState::Current);

    // 21 is read and saved again through the app's own save: unchanged.
    let again = sx.script(21);
    store::save_scripts_atomically(sx.root(), &[again]).unwrap();
    assert!(sx.script(21).saved_at.is_some());
    assert_eq!(sx.state(21), SyncState::Current);

    // 20 is repaired.
    sx.repair(20, "#went");
    match sx.state(20) {
        SyncState::OutOfDate { why } => assert!(why.contains("steps") && !why.contains("area"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(sx.state(21), SyncState::Current);

    let rows = sx.rows();
    assert!(rows[&20].exported_at.is_some() && rows[&20].has_script);
    assert_eq!(rows[&20].title, "Twenty");
}

#[test]
fn the_fingerprint_ignores_key_order_and_names_each_part() {
    let steps = |v: serde_json::Value| -> serde_json::Value {
        let s: Vec<StepScript> = serde_json::from_value(v).unwrap();
        serde_json::to_value(&s).unwrap()
    };
    let input = |steps: serde_json::Value| sync::FingerprintInput {
        title: "t".into(),
        steps,
        area: "Definition Wizard".into(),
        area_clicks: json!(["a.dw"]),
        account: "admin".into(),
        user_key: Some("REPO_KEY".into()),
    };
    let base = json!([{ "step_number": 1, "actions": [
        { "kind": "expect_response", "url_contains": "x", "json": { "a": 1, "b": { "c": 2, "d": 3 } } }
    ] }]);
    let first = sync::fingerprint(&input(steps(base.clone())));

    // The same steps with their keys in another order.
    let reordered = json!([{ "actions": [
        { "json": { "b": { "d": 3, "c": 2 }, "a": 1 }, "url_contains": "x", "kind": "expect_response" }
    ], "step_number": 1 }]);
    assert_eq!(sync::fingerprint(&input(steps(reordered))), first);

    // Each part changes its own hash and the whole.
    let mut i = input(steps(base.clone()));
    i.title = "renamed".into();
    let t = sync::fingerprint(&i);
    assert!(t.hash != first.hash && t.parts.title != first.parts.title && t.parts.steps == first.parts.steps);
    let mut i = input(steps(base.clone()));
    i.area_clicks = json!(["a.other"]);
    assert_ne!(sync::fingerprint(&i).parts.area, first.parts.area);
    let mut i = input(steps(base.clone()));
    i.user_key = Some("OTHER_KEY".into());
    assert_ne!(sync::fingerprint(&i).parts.account, first.parts.account);
    let mut unchecked = base;
    unchecked[0]["unchecked"] = json!("not visible");
    assert_ne!(sync::fingerprint(&input(steps(unchecked))).parts.steps, first.parts.steps);
}

#[test]
fn what_the_export_reads_decides_the_state() {
    let sx = Sx::new();
    let press = |selector: &str| -> Component {
        serde_json::from_value(json!({
            "name": "Press", "description": "d", "version": 1, "inputs": [],
            "actions": [{ "kind": "click", "selector": selector }]
        }))
        .unwrap()
    };
    components::put(sx.root(), ORG, PROJECT, press("#go")).unwrap();
    sx.fx.script(40, "Forty", "admin", "Definition Wizard", json!([{ "kind": "use_component", "component": "Press", "inputs": {} }]));
    sx.fx.run("r40", "5", 40, "Passed");
    sx.fx.good(41, "Forty one");
    sx.export(&[40, 41], &docs(&[(40, "Forty"), (41, "Forty one")])).unwrap();
    assert_eq!(sx.state(40), SyncState::Current);

    // Marks are not read by the export: an edit of them alone changes nothing.
    let mut s = sx.script(41);
    s.changes = vec!["cycle published".into()];
    s.needs_unchanged = vec!["draft".into()];
    store::save_script(sx.root(), &s).unwrap();
    assert_eq!(sx.state(41), SyncState::Current);

    // The title is.
    let mut s = sx.script(41);
    s.title = "Forty one, renamed".into();
    store::save_script(sx.root(), &s).unwrap();
    assert_eq!(sx.state(41), SyncState::OutOfDate { why: "changed since the export: title".into() });

    // A shared component the case uses changes: its expanded steps do.
    components::put(sx.root(), ORG, PROJECT, press("#gone")).unwrap();
    assert_eq!(sx.state(40), SyncState::OutOfDate { why: "changed since the export: steps".into() });
}

#[test]
fn a_record_that_cannot_be_read_is_kept_aside_and_sync_starts_again() {
    let sx = Sx::new();
    sx.fx.good(20, "Twenty");
    sx.stamp(20);
    sx.export(&[20], &docs(&[(20, "Twenty")])).unwrap();
    let path = sx.record_path();
    fs::write(&path, "{ not json").unwrap();

    assert_eq!(sx.state(20), SyncState::NeverExported);
    let names: Vec<String> = fs::read_dir(sx.dir.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    let stem = path.file_stem().unwrap().to_string_lossy().to_string();
    let backup = names.iter().find(|n| n.starts_with(&format!("{stem}.corrupt-")) && n.ends_with(".json")).expect("kept aside");
    assert_eq!(fs::read_to_string(sx.dir.path().join(backup)).unwrap(), "{ not json");
    assert!(!path.exists());

    // The next export records as usual.
    sx.export(&[20], &docs(&[(20, "Twenty")])).unwrap();
    assert_eq!(sx.state(20), SyncState::Current);
}

#[test]
fn a_case_whose_section_or_raw_spec_is_gone_is_not_in_the_repo() {
    let sx = Sx::new();
    sx.fx.good(20, "Twenty");
    sx.fx.good(21, "Twenty one");
    sx.export(&[20, 21], &docs(&[(20, "Twenty"), (21, "Twenty one")])).unwrap();

    // 20's raw spec is deleted in the clone.
    fs::remove_file(sx.fx.clone.path().join(sx.raw(20))).unwrap();
    assert_eq!(sx.state(20), SyncState::NotInRepo);

    // 21's section is removed from its test-case file.
    let md = sx.fx.read_clone(MD_REL);
    let start = md.find("## 21 ").unwrap();
    write(sx.fx.clone.path().join(MD_REL), &md[..start]);
    assert_eq!(sx.state(21), SyncState::NotInRepo);
}

#[test]
fn a_script_of_the_project_never_exported_says_so() {
    let sx = Sx::new();
    sx.fx.good(30, "Thirty");
    sx.stamp(30);
    // Another project's script is not this project's to list.
    sx.fx.good(31, "Thirty one");
    let mut other = sx.script(31);
    other.project = Some("another".into());
    store::save_script(sx.root(), &other).unwrap();

    let rows = sx.rows();
    assert_eq!(rows[&30].state, SyncState::NeverExported);
    assert_eq!(rows[&30].exported_at, None);
    assert!(rows[&30].has_script);
    assert!(!rows.contains_key(&31));

    // Exported, it is current.
    sx.export(&[30], &docs(&[(30, "Thirty")])).unwrap();
    assert_eq!(sx.state(30), SyncState::Current);
}

#[test]
fn update_re_exports_only_the_given_cases() {
    let sx = Sx::new();
    sx.fx.good(20, "Twenty");
    sx.fx.good(21, "Twenty one");
    sx.export(&[20, 21], &docs(&[(20, "Twenty"), (21, "Twenty one")])).unwrap();
    let record_21 = sync::load(&sx.record_path()).unwrap().cases[&21].clone();
    sx.repair(20, "#went");
    sx.repair(21, "#gone");
    let raw_20 = sx.fx.read_clone(&sx.raw(20));
    let raw_21 = sx.fx.read_clone(&sx.raw(21));

    let r = sx.export(&[20], &docs(&[(20, "Twenty")])).unwrap();
    assert_eq!(r.cases.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![20]);
    assert_eq!(r.sections, vec![(20, MD_REL.to_string())]);

    assert_ne!(sx.fx.read_clone(&sx.raw(20)), raw_20);
    assert!(sx.fx.read_clone(&sx.raw(20)).contains("#went"));
    assert_eq!(sx.fx.read_clone(&sx.raw(21)), raw_21, "21 was not asked for");
    assert_eq!(sx.state(20), SyncState::Current);
    assert!(matches!(sx.state(21), SyncState::OutOfDate { .. }));
    let after = sync::load(&sx.record_path()).unwrap();
    assert_eq!(after.cases[&21], record_21);
    assert_eq!(
        after.cases[&20].files,
        vec![MD_REL.to_string(), sx.raw(20)],
        "20 was already in the index, so the index was not written"
    );
}

#[test]
fn update_leaves_the_repos_own_files_alone() {
    let sx = Sx::new();
    let c = sx.fx.clone.path();
    sx.fx.good(20, "Twenty");
    sx.export(&[20], &docs(&[(20, "Twenty")])).unwrap();

    // The repo makes the file its own: an Also line and an engineer's steps
    // in 20's section, a case of its own, and an Amendment with its floor.
    let also = "**Also:** \"delete what step 1 made\" \u{2014} added by qa@example.test on 2026-10-01\n\n";
    let engineer = "**Added by engineer (not from ADO):**\n\n| # | Step | Expected |\n|---|------|----------|\n| 2 | Delete it | It is gone, so it can run again |\n\nADO's own numbering runs 2 to 3.\n\n";
    let floor = "**Assertion floor:** 7 (raw sum 9), set by `npm run spec:floor` on 2026-09-02\n";
    let own = "## 999 \u{2014} The repo's own case\n\n### Test Steps\n\n| Step | Action | Expected Result |\n|------|--------|-----------------|\n| 1 | Look | Seen |\n\n";
    let amendment = format!("## Amendment \u{2014} 2026-09-02\n\nQA asked for one chain.\n\n{floor}");
    let md = sx.fx.read_clone(MD_REL);
    let heading_end = md.find("## 20 ").map(|i| i + md[i..].find("\n\n").unwrap() + 2).unwrap();
    let cleanup = md.find("### Cleanup").unwrap();
    let md = format!("{}{also}{}{engineer}{}{own}{amendment}", &md[..heading_end], &md[heading_end..cleanup], &md[cleanup..]);
    write(c.join(MD_REL), &md);

    // And files TCM never writes.
    let feature = "suites/sl/admin/performance/proficiency-levels";
    write(c.join(format!("{feature}/data/proficiency-levels.json")), "{ \"levels\": [1, 2] }\r\n");
    write(c.join(format!("{feature}/pages/proficiency-levels.page.ts")), "export class P {}\n");
    write(c.join(format!("{feature}/specs/proficiency-levels.spec.ts")), "// refactored 20\r\ntest('x', () => {});\n");
    write(c.join("suites/_generated/seed.spec.ts"), "// seed\n");
    // The repo's index in its own layout (CRLF, its own spacing).
    let raw_name = sx.raw(20).trim_start_matches("suites/_generated/").to_string();
    write(
        c.join("suites/_generated/index.json"),
        &format!("{{\r\n    \"1\" : \"old.spec.ts\",\r\n    \"20\" : \"{raw_name}\"\r\n}}\r\n"),
    );
    let before = snapshot(c);
    let raw = sx.raw(20);

    sx.repair(20, "#went");
    let r = sx.export(&[20], &docs(&[(20, "Twenty renamed in ADO")])).unwrap();
    assert_eq!(r.record_error, None);
    let after = snapshot(c);

    // Nothing but 20's section and its raw spec was written, and nothing
    // was added or removed. The index already maps 20 to its file, so it is
    // left exactly as the repo wrote it.
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>());
    for (rel, bytes) in &before {
        if rel == MD_REL || *rel == raw {
            continue;
        }
        assert_eq!(&after[rel], bytes, "{rel} changed");
    }
    assert_ne!(after[&raw], before[&raw]);
    assert!(!r.files.iter().any(|f| f.ends_with("index.json")), "{:?}", r.files);

    // In the test-case file: everything before 20's section and everything
    // after it byte for byte, and the repo's own blocks inside it verbatim.
    let new_md = String::from_utf8(after[MD_REL].clone()).unwrap();
    let start = md.find("## 20 ").unwrap();
    assert_eq!(&new_md[..start], &md[..start]);
    let tail = &md[md.find("## 999 ").unwrap()..];
    assert!(new_md.ends_with(tail), "the repo's case and the Amendment are kept");
    let section = &new_md[start..new_md.len() - tail.len()];
    assert!(section.starts_with("## 20 \u{2014} Twenty renamed in ADO\n\n"), "{section}");
    assert!(section.contains(also), "{section}");
    assert!(section.contains(engineer), "{section}");
    assert!(section.find(engineer).unwrap() < section.find("### Cleanup").unwrap());
    assert!(section.find(also).unwrap() < section.find("### Metadata").unwrap());
    assert_eq!(new_md.matches(floor).count(), 1);
    assert_eq!(sx.state(20), SyncState::Current);
}

#[test]
fn a_refused_update_writes_nothing_and_keeps_the_record() {
    let sx = Sx::new();
    sx.fx.good(20, "Twenty");
    sx.export(&[20], &docs(&[(20, "Twenty")])).unwrap();
    sx.repair(20, "#went");
    // 22 cannot be exported: its newest run failed.
    sx.fx.click_script(22, "Twenty two", "admin", "Definition Wizard");
    sx.fx.run("z", "9", 22, "Failed");
    let clone_before = snapshot(sx.fx.clone.path());
    let record_before = fs::read(sx.record_path()).unwrap();

    let err = sx.export(&[20, 22], &docs(&[(20, "Twenty"), (22, "Twenty two")])).unwrap_err();
    assert!(err.contains("Nothing was written"), "{err}");
    assert_eq!(snapshot(sx.fx.clone.path()), clone_before);
    assert_eq!(fs::read(sx.record_path()).unwrap(), record_before);
    assert!(matches!(sx.state(20), SyncState::OutOfDate { .. }));
}

#[test]
fn the_record_is_written_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let path = sync::record_path(dir.path(), "C:\\clone\\", ORG, PROJECT);
    // The clone's spelling does not matter, its folder does.
    assert_eq!(path, sync::record_path(dir.path(), " C:/clone ", ORG, PROJECT));
    if cfg!(windows) {
        assert_eq!(path, sync::record_path(dir.path(), "c:\\CLONE", ORG, PROJECT));
    }
    assert_ne!(path, sync::record_path(dir.path(), "C:\\other", ORG, PROJECT));
    assert_eq!(sync::load(&path).unwrap(), ExportRecord::default(), "none yet is empty");

    let entry = |title: &str| sync::Exported {
        title: title.into(),
        fingerprint: Default::default(),
        exported_at: "2026-10-13T00:00:00Z".into(),
        files: vec!["suites/_generated/a.spec.ts".into()],
        module: String::new(),
    };
    sync::record(&path, vec![(1, entry("one"))]).unwrap();
    let good = fs::read(&path).unwrap();

    // A save whose final rename fails leaves the old record whole, and no
    // temp file behind.
    let mut r = sync::load(&path).unwrap();
    r.cases.insert(2, entry("two"));
    let err = sync::save_with(&path, &r, |p, s| {
        v2_lib::ai_tools::atomic_write_with(p, s, |p, s| fs::write(p, s), |_, _| Err(std::io::Error::other("disk full")), |_| {})
    })
    .unwrap_err();
    assert!(err.contains("disk full"), "{err}");
    assert_eq!(fs::read(&path).unwrap(), good);
    let names: Vec<String> = fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    assert_eq!(names.len(), 1, "{names:?}");

    // A record keeps every other case's entry.
    sync::record(&path, vec![(2, entry("two"))]).unwrap();
    let both = sync::load(&path).unwrap();
    assert_eq!(both.cases.keys().copied().collect::<Vec<_>>(), vec![1, 2]);
    let names: Vec<String> = fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    assert_eq!(names.len(), 1, "{names:?}");
}

#[test]
fn a_re_export_keeps_the_repos_blocks_in_a_section() {
    use v2_lib::pw_export::test_case::{section, splice};
    let mut d = doc(7, "Seven");
    d.side = "admin".into();
    d.feature_title = "F".into();
    let ours = section(&d);
    let engineer = "**Added by engineer (not from ADO):**\r\n\r\n| # | Step | Expected |\r\n| 2 | More | Done |\r\n\r\n";
    let cleanup = ours.find("### Cleanup").unwrap();
    let file = format!("# Set\n\n{}{engineer}{}## 8 \u{2014} Eight\n\nkept\n", &ours[..cleanup], &ours[cleanup..]);
    let mut d2 = d.clone();
    d2.title = "Seven again".into();
    let out = splice(&file, 7, &section(&d2));
    assert!(out.contains(engineer));
    assert!(out.contains("## 7 \u{2014} Seven again"));
    assert!(out.ends_with("## 8 \u{2014} Eight\n\nkept\n"));
    // A section without blocks of the repo's is replaced exactly as before.
    let plain = format!("# Set\n\n{ours}");
    assert_eq!(splice(&plain, 7, &section(&d2)), format!("# Set\n\n{}", section(&d2)));
}

/// 135271 as the repo has it: its section rewritten by `/gen-test`, an
/// engineer's step 11 and its note before Cleanup, and the cases around it.
const ENGINEER_135271: &str = concat!(
    "**Added by engineer (not from ADO):**\n",
    "\n",
    "| # | Step | Expected |\n",
    "|---|------|----------|\n",
    "| 11 | Delete the \"Core Competencies\" group created by step 8 | The group is removed, so the template is free for the next run |\n",
    "\n",
    "ADO's own step numbering runs 2 to 11, skipping id 1.\n",
    "The table above renumbers sequentially; step\n",
    "11 (deletion) has no ADO counterpart.\n",
    "\n",
);

fn doc_135271() -> CaseDoc {
    let mut d = doc(135271, "Competencies - Applying a template twice is blocked");
    d.state = "Design".into();
    d.area_path = "HRM\\Gamma Guardians".into();
    d.iteration_path = "HRM\\Sprint 4".into();
    d.project = "HRM".into();
    d.tags = String::new();
    d.preconditions = "Signed in as an HR admin".into();
    d.steps = vec![
        ("Click the Core Competencies template card.".into(), "A new group appears".into()),
        ("Click it a second time.".into(), "A warning is shown".into()),
    ];
    d.side = "admin".into();
    d.navigation_captured = true;
    d.feature_title = "Competencies".into();
    d
}

/// The old 135271 section with `block` where the engineer's block is.
fn file_135271(before_cleanup: &str, after_cleanup: &str) -> String {
    format!(
        concat!(
            "# Test Case Set: Competencies\n\n",
            "## 135270 \u{2014} Quick create adds a group\n\nkept\n\n",
            "## 135271 \u{2014} Competencies - Apply template twice\n\n",
            "### Metadata\n\n| ID | 135271 |\n\n",
            "### Test Steps\n\n| 1 | a | b |\n\n",
            "{}",
            "### Cleanup / Postconditions\n\n| # | Action |\n|---|--------|\n| 1 | Deleted by step 11 |\n| 2 | Log out all actors |\n\n",
            "{}",
            "## 135283 \u{2014} Import Excel\n\nkept\n",
        ),
        before_cleanup, after_cleanup
    )
}

#[test]
fn the_135271_section_is_rewritten_with_the_engineers_block_before_cleanup() {
    use v2_lib::pw_export::test_case::{section, splice};
    let out = splice(&file_135271(ENGINEER_135271, ""), 135271, &section(&doc_135271()));
    let expected = [
        "# Test Case Set: Competencies\n\n",
        "## 135270 \u{2014} Quick create adds a group\n\nkept\n\n",
        "## 135271 \u{2014} Competencies - Applying a template twice is blocked\n\n",
        "### Metadata\n\n",
        "| Field | Value |\n",
        "|-------|-------|\n",
        "| ID | 135271 |\n",
        "| Title | Competencies - Applying a template twice is blocked |\n",
        "| State | Design |\n",
        "| Area Path | HRM\\Gamma Guardians |\n",
        "| Iteration | HRM\\Sprint 4 |\n",
        "| Project | HRM |\n",
        "| Module | Performance |\n",
        "| Tags |  |\n",
        "\n**Side:** admin \u{2014} mapped in TCM.\n\n",
        "**Navigation:** captured in src/navigation.json.\n\n",
        "### Preconditions\n\n| # | Condition |\n|---|-----------|\n| 1 | Signed in as an HR admin |\n",
        "\n### Test Data & Validation\n\n",
        "| Name | Value / Rule | Source | Expected After Save | Notes |\n",
        "|------|--------------|--------|---------------------|-------|\n",
        "\n### Navigation Path\n\n```text\nCompetencies\n```\n",
        "\n### Test Steps\n\n",
        "| Step | Action | Expected Result |\n",
        "|------|--------|-----------------|\n",
        "| 1 | Click the Core Competencies template card. | A new group appears |\n",
        "| 2 | Click it a second time. | A warning is shown |\n",
        "\n",
        ENGINEER_135271,
        "### Cleanup / Postconditions\n\n| # | Action |\n|---|--------|\n\n",
        "## 135283 \u{2014} Import Excel\n\nkept\n",
    ]
    .concat();
    assert_eq!(out, expected);
}

#[test]
fn an_engineers_block_after_cleanup_stays_after_it() {
    use v2_lib::pw_export::test_case::{section, splice};
    let out = splice(&file_135271("", ENGINEER_135271), 135271, &section(&doc_135271()));
    let ours = section(&doc_135271());
    let expected_section = format!("{ours}{ENGINEER_135271}");
    assert!(out.contains(&format!("{expected_section}## 135283 ")), "{out}");
}

#[test]
fn a_floor_inside_a_case_section_is_kept_at_its_end() {
    use v2_lib::pw_export::test_case::{section, splice};
    let floor = "**Assertion floor:** 12 (raw sum 14), set by `npm run spec:floor` on 2026-09-02\n\n";
    let out = splice(&file_135271(ENGINEER_135271, floor), 135271, &section(&doc_135271()));
    let ours = section(&doc_135271());
    let cleanup = ours.find("### Cleanup").unwrap();
    let expected_section = format!("{}{ENGINEER_135271}{}{floor}", &ours[..cleanup], &ours[cleanup..]);
    assert!(out.contains(&format!("{expected_section}## 135283 ")), "{out}");
    assert_eq!(out.matches(floor).count(), 1);
}

#[test]
fn a_hash_line_in_an_engineers_note_does_not_end_the_block() {
    use v2_lib::pw_export::test_case::{section, splice};
    let block = concat!(
        "**Added by engineer (not from ADO):**\n",
        "\n",
        "| # | Step | Expected |\n",
        "|---|------|----------|\n",
        "| 11 | Delete the group | It is gone |\n",
        "\n",
        "#11 deletes what step 8 made; see #135270 for the same gap.\n",
        "```bash\n",
        "# a comment, not a heading\n",
        "npm run qa\n",
        "```\n",
        "\n",
    );
    let out = splice(&file_135271(block, ""), 135271, &section(&doc_135271()));
    let ours = section(&doc_135271());
    let cleanup = ours.find("### Cleanup").unwrap();
    let expected_section = format!("{}{block}{}", &ours[..cleanup], &ours[cleanup..]);
    assert!(out.contains(&format!("{expected_section}## 135283 ")), "{out}");
}
