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
