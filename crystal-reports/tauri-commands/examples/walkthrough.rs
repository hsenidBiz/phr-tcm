//! The full editor loop against the portable command layer — the integration walkthrough
//! as runnable code. Run with:
//!
//! ```text
//! cargo run -p tauri-commands --example walkthrough
//! ```
//!
//! create → open (from bytes) → parameter surface → edit (plan) → undo → redo →
//! export PDF → thumbnail → save — every step prints what a front end receives.

use tauri_commands::commands::{
    apply_plan, export_pdf, get_parameters, new_report, open_report_bytes, preview_thumbnail,
    redo_report, save_report, undo_report, EditorState,
};

fn main() {
    let state = EditorState::default();

    // 1. Create a report from scratch (no donor file) and open it from memory.
    let bytes = new_report(rpt_core::blank::NewReportOptions {
        title: Some("Walkthrough".into()),
        orientation: Some("landscape".into()),
        paper_size: Some(9), // A4
        ..Default::default()
    })
    .expect("create");
    let ir = open_report_bytes(&state, bytes).expect("open");
    println!("created: {} areas, {} objects", ir.areas.len(), ir.objects.len());

    // 2. The parameter surface (a fresh blank declares none).
    println!("parameters: {:?}", get_parameters(&state).expect("params"));

    // 3. Edit through the plan vocabulary (the seed donors supply the object shapes).
    let plan = r#"{"planVersion":1,"operations":[
        {"action":"addText","section":"ReportHeaderSection1","newName":"Title",
         "leftTwips":500,"topTwips":200,"widthTwips":6000,"heightTwips":400,
         "text":"Built by the walkthrough"}]}"#;
    let ir1 = apply_plan(&state, plan.to_string()).expect("edit");
    println!("after edit: {} objects", ir1.objects.len());

    // 4. Undo, redo — the refreshed IR each step.
    let ir0 = undo_report(&state).expect("undo");
    println!("after undo: {} objects", ir0.objects.len());
    let ir1 = redo_report(&state).expect("redo");
    println!("after redo: {} objects", ir1.objects.len());

    // 5. Export the open report as a PDF (with metadata + archival options available) and
    //    a one-page thumbnail.
    let pdf = export_pdf(
        &state,
        rpt_core::PdfExportOptions {
            title: Some("Walkthrough export".into()),
            ..Default::default()
        },
    )
    .expect("export");
    println!("export: {} pages, {} KB", pdf.page_count, pdf.pdf.len() / 1024);
    let thumb = preview_thumbnail(&state).expect("thumbnail");
    println!("thumbnail: {} pages, {} KB", thumb.page_count, thumb.pdf.len() / 1024);

    // 6. Save — atomically — wherever the app chooses.
    let out = std::env::temp_dir().join("walkthrough.rpt");
    save_report(&state, Some(out.to_string_lossy().to_string())).expect("save");
    println!("saved: {}", out.display());
}
