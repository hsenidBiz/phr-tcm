//! Preview rendering integration tests — the vendored render stack (VENDORED.md #10) over the
//! vendored fixtures: structural PDF validity for every fixture, saved-data pagination for the
//! one fixture that carries data, static-band degradation for the ones that do not, from-scratch
//! reports, determinism + edit sensitivity, and the golden PDF hash.
//!
//! Reproducibility has one deliberate exception: Print Date/Time special fields render at the
//! wall clock (`default_as_of()`), exactly like Crystal's own preview — so determinism and the
//! golden hash are pinned on World Sales Report, which prints no time-dependent field (verified:
//! its render is byte-stable across processes and minutes).

use rpt_core::edit::{apply_edits, EditOp, ObjSel};
use rpt_core::preview::preview;
use rpt_core::PreviewOutput;
use rpt_test_support::pdf;
use sha2::{Digest, Sha256};

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

fn read_fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap_or_else(|e| panic!("cannot read fixture {name}: {e}"))
}

fn assert_valid_pdf(name: &str, out: &PreviewOutput) {
    assert!(out.pdf.starts_with(b"%PDF-1."), "{name}: no PDF header");
    let tail = &out.pdf[out.pdf.len().saturating_sub(64)..];
    assert!(
        tail.windows(5).any(|w| w == b"%%EOF"),
        "{name}: no EOF marker"
    );
    assert!(out.page_count >= 1, "{name}: zero pages");
    let parsed = pdf::structure(&out.pdf);
    assert_eq!(
        parsed.pages.len(),
        out.page_count,
        "{name}: reported page_count disagrees with the PDF object graph"
    );
}

/// Every fixture renders to a structurally valid PDF. The six PMSV10 files carry no saved data,
/// so these exercise the EmptySource path: static bands over an empty row set, never an error.
#[test]
fn all_fixtures_render_valid_pdfs() {
    for name in [
        "PMSV10_GoalAlignCascade.rpt",
        "PMSV10_GoalAlignCascadeSS.rpt",
        "PMSV10_IndDetailedEval.rpt",
        "PMSV10_IndDetailedEvalSS.rpt",
        "PMSV10_IndPerfOverview.rpt",
        "PMSV10_IndPerfOverviewSS.rpt",
        "World Sales Report.rpt",
    ] {
        let out = preview(&read_fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_valid_pdf(name, &out);
    }
}

/// World Sales Report carries saved data (2191 saved records, 1250 surviving record selection):
/// the preview must be data-driven — 37 pages of actual text — and clean (no diagnostics).
#[test]
fn world_sales_renders_saved_data() {
    let out = preview(&read_fixture("World Sales Report.rpt")).expect("render");
    assert_valid_pdf("World Sales Report.rpt", &out);
    assert_eq!(out.page_count, 37, "saved-data page count");
    assert!(out.diagnostics.is_empty(), "diagnostics: {:?}", out.diagnostics);
    let listing = pdf::operator_listing(&out.pdf);
    assert!(listing.contains("Tj"), "no text-showing operators: data never rendered");
    // The logo picture (misclassified by the upstream reader — VENDORED.md #7) must embed.
    let structure = pdf::structure(&out.pdf);
    assert!(
        structure.pages.iter().any(|p| !p.images.is_empty()),
        "no embedded image: the RO_Xtreme_Logo picture did not render"
    );
}

/// A report saved WITHOUT data degrades to static bands and says so in diagnostics (the byline
/// formulas reference table columns that an empty row set cannot resolve) — a preview, not an
/// error. This pins those semantics.
#[test]
fn no_saved_data_renders_static_bands_with_diagnostics() {
    let out = preview(&read_fixture("PMSV10_IndPerfOverview.rpt")).expect("render");
    assert_valid_pdf("PMSV10_IndPerfOverview.rpt", &out);
    assert!(!out.diagnostics.is_empty(), "expected formula diagnostics over the empty row set");
}

/// A from-scratch report (blank.rs) previews: one empty page.
#[test]
fn blank_report_previews() {
    let bytes = rpt_core::blank::create_report(&rpt_core::blank::NewReportOptions::default())
        .expect("create");
    let out = preview(&bytes).expect("render");
    assert_valid_pdf("blank", &out);
    assert_eq!(out.page_count, 1);
}

/// Same bytes → byte-identical PDF; one edit → different PDF. Pinned on World Sales Report: it
/// prints no time-dependent special field, so its render is stable to the byte (Print Date/Time
/// fixtures legitimately move with the wall clock — see the module docs). Determinism is what
/// makes the golden hash below meaningful, and edit sensitivity is what makes the preview a
/// preview.
#[test]
fn preview_is_deterministic_and_edit_sensitive() {
    let bytes = read_fixture("World Sales Report.rpt");
    let a = preview(&bytes).expect("render a");
    let b = preview(&bytes).expect("render b");
    assert_eq!(a.pdf, b.pdf, "same bytes must render byte-identically");

    // Edit Text1 ("World Sales Report", IR object 0) — a text change must reach the pixels;
    // a bounds change can be visually inert (single line that still fits), text cannot.
    let edited = apply_edits(
        &fixture("World Sales Report.rpt"),
        &[EditOp::SetText { object: ObjSel { id: None, name: None, index: Some(0) }, text: "Edited Title".to_string() }],
    )
    .expect("edit");
    let c = preview(&edited).expect("render edited");
    assert_ne!(b.pdf, c.pdf, "an edit must change the preview");
}

/// The golden pin: World Sales Report's preview PDF, SHA-256. Byte-reproducible rendering means
/// this hash is stable across runs, profiles, and machines — it moves only when the render
/// pipeline or the vendored fonts change, and any such movement is a review event. Provenance:
/// cross-checked during vendoring against the upstream-built `rpt-render` binary, whose output
/// differs only in the Producer version string and the absence of the #7-fixed logo picture.
#[test]
fn world_sales_preview_pdf_sha256_pinned() {
    let out = preview(&read_fixture("World Sales Report.rpt")).expect("render");
    let hash = Sha256::digest(&out.pdf);
    assert_eq!(
        format!("{hash:x}"),
        "9ac03b109f3c0595a66a624daf5b252edd51dc4af159c202b88b6f543f5d913d",
        "the WSR preview PDF changed — a render-pipeline or font update is a review event, \
         re-pin deliberately (see docs/validation-log.md)"
    );
}
