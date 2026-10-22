//! The vendored Crystal Reports bundle in `crystal-reports/` builds with this
//! crate and still reads the six sample reports shipped beside it.

use std::path::PathBuf;

use rpt_core::{build_ir, locate::locate};

#[test]
fn the_vendored_bundle_opens_every_sample_report() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crystal-reports/Sample Report");
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).expect("the Sample Report folder is part of the vendored copy") {
        let path = entry.expect("a readable directory entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rpt") {
            continue;
        }
        let path = path.to_string_lossy().into_owned();
        let rpt = rpt_core::open(&path).unwrap_or_else(|e| panic!("{path} did not open: {e}"));
        let ir = build_ir(&rpt, &locate(&rpt), &path);
        assert!(!ir.objects.is_empty(), "{path} produced no objects");
        seen += 1;
    }
    assert_eq!(seen, 6, "the sample folder should hold exactly six reports");
}

/// The live data check (`crate::crystal::live_target`) looks only at the main
/// report's plan, which covers everything the bundle runs while its live
/// preview never hands `RenderOptions` a subreport data provider. The day a
/// refreshed copy does, a subreport's procedure or SQL command would run with
/// no check at all: this fails so that the check is extended first.
#[test]
fn the_vendored_live_preview_still_fetches_no_subreport_data() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crystal-reports/rpt-core/src/preview.rs");
    let source = std::fs::read_to_string(&path).expect("preview.rs is part of the vendored copy");
    let code: String = source
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let start = code.find("RenderOptions {").expect("the live preview builds RenderOptions");
    let block = &code[start..start + code[start..].find('}').expect("RenderOptions block closes")];
    assert!(
        !block.contains("scope"),
        "the vendored live preview now passes RenderOptions.scope: cover every subreport in \
         live_target (src/crystal/live.rs) before this refresh ships"
    );
}
