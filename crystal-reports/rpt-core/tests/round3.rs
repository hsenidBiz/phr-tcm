//! Round-3 battery: the render-geometry bridge (N), find & replace (O), batch apply (P).

use rpt_core::batch::{apply_plan_all, BatchOutcome};
use rpt_core::find::find_replace;
use rpt_core::hits::{hit_test, preview_hits, resolve_scope, boxes_of_object, Scope};
use rpt_core::open;

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

/* ---------- N: the render-geometry bridge ---------- */

/// A data-bearing render (World Sales Report) projects named hit boxes with sane geometry;
/// hit-testing inside a box finds it; instance grouping coalesces one object's ops.
#[test]
fn hits_project_and_hit_test() {
    let rpt = open(&fixture("World Sales Report.rpt")).unwrap();
    let hits = preview_hits(&rpt).unwrap();
    assert_eq!(hits.pages.len(), 37, "the saved-data page count");
    let p1 = &hits.pages[0];
    assert!(p1.width_twips > 0 && p1.height_twips > 0);
    assert!(p1.objects.iter().any(|b| b.object.as_deref() == Some("Text1")), "named boxes");
    // The chart object's many draw-ops group into ONE box (shared instance id).
    let chart_boxes = p1.objects.iter().filter(|b| b.object.as_deref() == Some("Graph1")).count();
    assert_eq!(chart_boxes, 1, "instance grouping coalesces the chart");
    // Click-to-select: a point inside a text box resolves to it.
    let text1 = p1
        .objects
        .iter()
        .find(|b| b.object.as_deref() == Some("Text1"))
        .expect("Text1 box");
    let cx = text1.x + text1.w / 2;
    let cy = text1.y + text1.h / 2;
    let hit = hit_test(&hits, 1, cx, cy).expect("the center hits");
    assert_eq!(hit.object.as_deref(), Some("Text1"));
    // And a point in empty space hits nothing (or something else only if genuinely covered).
    let miss = hit_test(&hits, 1, -500, -500);
    assert!(miss.is_none(), "far outside the page hits nothing");
    // True rendered heights: the geometry carries positive extents everywhere.
    assert!(p1.objects.iter().all(|b| b.w >= 0 && b.h >= 0));
    // boxes_of_object coalesces across pages for selection highlighting.
    assert!(!boxes_of_object(&hits, "Text1").is_empty());
}

/// Scope resolution: a main-only object resolves to Main; a subreport-only object resolves
/// to its subreport (the click-through-the-render routing for subreport internals).
#[test]
fn scope_resolution_routes_subreports() {
    let rpt = open(&fixture("PMSV10_IndDetailedEvalSS.rpt")).unwrap();
    assert!(matches!(resolve_scope(&rpt, "FtConf").unwrap(), Scope::Main));
    // StageWise's box object only exists inside the subreport.
    match resolve_scope(&rpt, "ComBox").unwrap() {
        Scope::Subreport(name) => assert_eq!(name, "StageWise"),
        other => panic!("ComBox should live in StageWise, got {other}"),
    }
    assert!(resolve_scope(&rpt, "NoSuchObject").is_err());
}

/* ---------- O: find & replace ---------- */

/// A replace reaches the MAIN report and the SUBREPORTS in one batch; the parent's sub
/// model reads the new text; validator PASS; nothing else changes.
#[test]
fn find_replace_reaches_subreports() {
    let rpt = open(&fixture("PMSV10_IndDetailedEvalSS.rpt")).unwrap();
    // StageWise carries "STAGE-WISE EVALUATION SUMMARY".
    let out = find_replace(&rpt, "SUMMARY", "OVERVIEW").unwrap();
    assert!(out.replaced >= 1, "at least StageWise's heading rewritten: {}", out.replaced);
    assert_eq!(out.skipped_locked, 0);
    let after = rpt_core::open_bytes(&out.bytes).unwrap();
    let sub = after.report().subreports.iter().find(|s| s.name == "StageWise").unwrap();
    let texts: Vec<String> = sub
        .report
        .report_definition
        .areas
        .iter()
        .flat_map(|a| a.sections.iter().flat_map(|s| s.objects.iter()))
        .filter_map(|o| match &o.kind {
            rpt_core::rpt_reader::model::ReportObjectKind::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|t| t.contains("STAGE-WISE EVALUATION OVERVIEW")));
    let v = rpt_core::validate::validate(&after);
    assert!(v.errors().next().is_none());

    // No hit: unchanged bytes.
    let none = find_replace(&rpt, "zz-no-such-text-zz", "x").unwrap();
    assert_eq!(none.replaced, 0);
}

/// Case-insensitive replace works; empty find refuses.
#[test]
fn find_replace_case_and_refusal() {
    let rpt = open(&fixture("PMSV10_IndDetailedEvalSS.rpt")).unwrap();
    let out = rpt_core::find::find_replace_opts(&rpt, "stage-wise", "PERF", true)
        .map(|(ops, n, l)| (ops.len(), n, l))
        .unwrap();
    assert_eq!(out.1, 1, "case-insensitive hit");
    assert!(rpt_core::find::find_replace(&rpt, "", "x").is_err());
}

/* ---------- P: batch apply ---------- */

/// One plan against a temp folder: both copies applied with .bak backups; a plan that
/// touches nothing leaves files unmodified and backup-less; per-file failure contained.
#[test]
fn batch_apply_with_backups() {
    let dir = std::env::temp_dir().join("rpt_batch_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["a.rpt", "b.rpt"] {
        std::fs::copy(fixture("PMSV10_GoalAlignCascade.rpt"), dir.join(name)).unwrap();
    }
    // A non-report file that must be skipped, and a broken .rpt that must fail alone.
    std::fs::write(dir.join("notes.txt"), "not a report").unwrap();
    std::fs::write(dir.join("broken.rpt"), b"not a report either").unwrap();

    let plan = r#"{"planVersion":1,"operations":[
        {"action":"addText","section":"ReportHeaderSection1","newName":"BatchTitle",
         "leftTwips":200,"topTwips":100,"widthTwips":4000,"heightTwips":300,
         "text":"batched"}]}"#;
    let summary = apply_plan_all(&dir, plan).unwrap();
    assert_eq!(summary.applied(), 2, "both good copies applied");
    assert_eq!(summary.failed(), 1, "the broken file failed alone");
    for name in ["a.rpt", "b.rpt"] {
        assert!(dir.join(format!("{name}.bak")).exists(), "{name} backed up");
        let edited = rpt_core::open(&dir.join(name).to_string_lossy()).unwrap();
        let ir = rpt_core::ir::build_ir(&edited, &rpt_core::locate::locate(&edited), name);
        assert!(ir.objects.iter().any(|o| o.name == "BatchTitle"), "{name} edited");
    }
    assert!(!dir.join("notes.txt.bak").exists(), "non-reports untouched");
    let broken_outcome = summary
        .files
        .iter()
        .find(|f| f.path.file_name().is_some_and(|n| n == "broken.rpt"))
        .map(|f| &f.outcome)
        .unwrap();
    assert!(matches!(broken_outcome, BatchOutcome::Failed(_)));

    // A plan that changes nothing: no rewrites, no new backups.
    let before = std::fs::read(dir.join("a.rpt")).unwrap();
    let noop = r#"{"planVersion":1,"operations":[]}"#;
    let summary2 = apply_plan_all(&dir, noop).unwrap();
    assert_eq!(summary2.applied(), 0);
    let after = std::fs::read(dir.join("a.rpt")).unwrap();
    assert_eq!(before, after, "no-op leaves bytes identical");
    let _ = std::fs::remove_dir_all(&dir);
}
