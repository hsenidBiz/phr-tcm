//! The receiving-application simulation: everything a consumer needs, by path dependency only.
//! Proves (a) the commands module compiles outside its own crate, (b) the full flow works via
//! the public API alone. Prints `portable-ok` on success.

// The handoff file, compiled as a module of THIS crate — the strongest standalone proof.
#[path = "../../tauri-commands/src/commands.rs"]
#[allow(dead_code)]
mod commands;

use commands::EditorState;

fn main() {
    let fixture = format!("{}/../rpt-core/tests/fixtures/PMSV10_GoalAlignCascade.rpt", env!("CARGO_MANIFEST_DIR"))
        .replace("\\", "/");

    let state = EditorState::default();
    let ir = commands::open_report(&state, fixture.clone()).expect("open");
    assert_eq!(ir.objects.len(), 82);

    let plan = serde_json::json!({
        "planVersion": 1,
        "operations": [
            {"action": "setFont", "target": "FtConf", "fontName": "Tahoma"},
            {"action": "addText", "section": "PageFooterSection1", "newName": "ConsumerNote",
             "leftTwips": 2000, "topTwips": 40, "widthTwips": 4000, "heightTwips": 250,
             "text": "placed by the consumer simulation"},
            {"action": "setSubreportLink", "target": "companylogo",
             "mainReportField": "{sp_perf_goal_align_sheet;1.performance_cycle_id}",
             "subreportField": "{sp_perf_company_logo;1.HIE_LOGO_IMAGE}",
             "linkedParameter": "@performance_cycle_id"},
        ]
    })
    .to_string();
    let fresh = commands::apply_plan(&state, plan).expect("apply plan");
    assert!(fresh.objects.iter().any(|o| o.name == "ConsumerNote"), "added object present");

    let out = std::env::temp_dir().join("consumer-check-out.rpt");
    let bytes = commands::save_as_bytes(&state).expect("bytes");
    std::fs::write(&out, bytes).expect("write");

    let rpt = rpt_core::open(&out.to_string_lossy()).expect("reopen saved file");
    let located = rpt_core::locate::locate(&rpt);
    let ir2 = rpt_core::build_ir(&rpt, &located, "saved");
    assert!(ir2.objects.iter().any(|o| o.name == "ConsumerNote"), "saved object present");

    std::fs::remove_file(&out).ok();
    println!("portable-ok");
}
