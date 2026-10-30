//! Preview rendering integration tests — the vendored render stack (VENDORED.md #10) over the
//! vendored fixtures: structural PDF validity for every fixture, saved-data pagination for the
//! one fixture that carries data, static-band degradation for the ones that do not, from-scratch
//! reports, determinism + edit sensitivity, and the golden PDF hash.
//!
//! Reproducibility has one deliberate exception: date/time-valued content renders at the
//! render's as-of instant (`default_as_of()` = the wall clock), exactly like Crystal's own
//! preview — byte-stable within a day, moving across days. World Sales Report DOES render
//! such values (its page-header ModificationTime/DataDate fields resolve from the as-of
//! instant), so its golden hash pins a FIXED as_of: the pinned bytes are then a function of
//! the pipeline and fonts only, never the calendar (found 2026-10-22, when the original
//! wall-clock pin drifted six days after pinning).

use rpt_core::edit::{apply_edits, EditOp, ObjSel};
use rpt_core::preview::preview;
use rpt_core::PreviewOutput;
use rpt_test_support::pdf;

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

/// A report saved WITHOUT data: the FIRST diagnostic says so in plain language, the
/// empty-row "formula error: unknown name" noise is filtered out, and the group
/// header/footer bands do not print at all (engine parity — Crystal suppresses every
/// group band when there are no rows). A preview, not an error.
#[test]
fn no_saved_data_says_so_and_prints_no_group_bands() {
    let out = preview(&read_fixture("PMSV10_IndPerfOverview.rpt")).expect("render");
    assert_valid_pdf("PMSV10_IndPerfOverview.rpt", &out);
    let notice = out
        .diagnostics
        .iter()
        .find(|d| d.starts_with("this report has no data to render"));
    assert!(notice.is_some(), "the no-data notice is first: {:?}", out.diagnostics);
    assert!(
        !out.diagnostics.iter().any(|d| d.starts_with("formula error: unknown name")),
        "empty-row unknown-name noise filtered: {:?}",
        out.diagnostics
    );
    // Group-band suppression: the employee group's header/footer objects are gone from the
    // page — only the page-level bands remain. The parsed page must carry far fewer text
    // runs than the 30-column data-bearing fixture ever would.
    let parsed = pdf::structure(&out.pdf);
    let text_runs: usize = parsed.pages.iter().map(|p| p.shows.len()).sum();
    assert!(text_runs < 60, "group bands suppressed over the empty row set: {text_runs} runs");
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

/// The golden pin — at a FIXED render clock. WSR's page header renders date-valued content
/// from the render's as-of instant, so a wall-clock pin drifts with the calendar; pinning
/// `as_of` makes the hash a function of the render pipeline and fonts only. The wall-clock
/// `preview()` path is asserted structurally alongside it.
#[test]
fn world_sales_preview_pdf_sha256_pinned_at_fixed_asof() {
    use sha2::{Digest, Sha256};

    let bytes = read_fixture("World Sales Report.rpt");
    let rpt = rpt_core::open_bytes(&bytes).expect("open");
    const PINNED_ASOF: i64 = 1_760_000_000; // 2025-10-09 — any fixed instant pins
    let opts = rpt_render::RenderOptions {
        as_of: Some(rpt_render::DateTimeSpecials::from_unix_seconds(PINNED_ASOF)),
        ..Default::default()
    };
    let doc = rpt_render::render_with(rpt.report(), opts);
    assert_eq!(doc.pages.len(), 37, "saved-data page count");
    let pdf = rpt_render::try_render_document(&doc, &rpt_render::PdfOptions::default())
        .expect("pdf");
    let hash = format!("{:x}", Sha256::digest(&pdf));
    assert_eq!(
        hash,
        "e4c221c37aaa2d3dd1130b28f20ce67dd7d7256f1b92b162bfb32a3d03dc9632",
        "the WSR pin (fixed as_of) changed — a render-pipeline or font update is a review          event, re-pin deliberately (see docs/validation-log.md)"
    );
    // The wall-clock preview() path must agree structurally.
    let out = preview(&bytes).expect("render");
    assert_eq!(out.page_count, 37);
}

/// The thumbnail: exactly one page, structurally valid, far smaller than the full document.
#[test]
fn thumbnail_is_one_page() {
    let out = rpt_core::thumbnail(&read_fixture("World Sales Report.rpt")).expect("thumbnail");
    assert_valid_pdf("thumbnail", &out);
    assert_eq!(out.page_count, 1, "a thumbnail is page 1 only");
    let full = preview(&read_fixture("World Sales Report.rpt")).expect("full");
    assert!(
        out.pdf.len() < full.pdf.len() / 3,
        "one page should be well under the 37-page document ({} vs {})",
        out.pdf.len(),
        full.pdf.len()
    );
}

/// Export options: default options render byte-identically to the plain preview (stability
/// for callers that only vary the title later); a title changes the bytes without touching
/// the pages; a b-level PDF/A renders a valid PDF; the a-levels are refused with the
/// language explanation.
#[test]
fn pdf_export_options() {
    use rpt_core::preview::{preview_report_with, PdfExportOptions};

    let bytes = read_fixture("PMSV10_GoalAlignCascade.rpt");
    let rpt = rpt_core::open_bytes(&bytes).unwrap();

    let plain = preview_report_with(&rpt, &PdfExportOptions::default()).expect("default export");
    let as_preview = preview(&bytes).expect("preview");
    assert_eq!(plain.pdf, as_preview.pdf, "default options == plain preview");

    let titled =
        preview_report_with(&rpt, &PdfExportOptions { title: Some("Perf Review".into()), ..Default::default() })
            .expect("title");
    assert_ne!(titled.pdf, plain.pdf, "the title is written into the metadata");
    assert_eq!(titled.page_count, plain.page_count, "pages untouched");

    let archival = preview_report_with(
        &rpt,
        &PdfExportOptions { pdfa: Some("1b".into()), ..Default::default() },
    )
    .expect("pdfa 1b");
    assert_valid_pdf("pdfa-1b", &archival);
    assert_eq!(archival.page_count, plain.page_count);

    let err = preview_report_with(&rpt, &PdfExportOptions { pdfa: Some("2a".into()), ..Default::default() })
        .unwrap_err();
    assert!(err.contains("language"), "`{err}` explains the a-level gap");
    let err = preview_report_with(&rpt, &PdfExportOptions { pdfa: Some("9z".into()), ..Default::default() })
        .unwrap_err();
    assert!(err.contains("9z"), "`{err}` names the unknown level");
}

/// The accessibility tier: level-A conformance needs `lang` (refused without it, valid
/// with it); PDF/UA needs `lang` AND `title`; the locale option renders (built-in roots,
/// unknown refused).
#[test]
fn pdf_export_accessibility_tier_and_locale() {
    use rpt_core::preview::{preview_report_with, PdfExportOptions};

    let bytes = read_fixture("PMSV10_GoalAlignCascade.rpt");
    let rpt = rpt_core::open_bytes(&bytes).unwrap();

    let e = preview_report_with(&rpt, &PdfExportOptions { pdfa: Some("2a".into()), ..Default::default() })
        .unwrap_err();
    assert!(e.contains("language"), "`{e}`");
    let e = preview_report_with(
        &rpt,
        &PdfExportOptions { pdfa: Some("ua".into()), lang: Some("en".into()), ..Default::default() },
    )
    .unwrap_err();
    assert!(e.contains("title"), "`{e}`");

    let a2 = preview_report_with(
        &rpt,
        &PdfExportOptions { pdfa: Some("2a".into()), lang: Some("en-US".into()), ..Default::default() },
    )
    .expect("2a with lang");
    assert_valid_pdf("pdfa-2a", &a2);
    assert_eq!(a2.page_count, 1);
    let ua = preview_report_with(
        &rpt,
        &PdfExportOptions {
            pdfa: Some("ua".into()),
            lang: Some("en".into()),
            title: Some("UA export".into()),
            ..Default::default()
        },
    )
    .expect("ua with lang+title");
    assert_valid_pdf("pdf-ua", &ua);

    // Locale: a built-in root renders; an unknown tag is refused naming it.
    let gb = preview_report_with(&rpt, &PdfExportOptions { locale: Some("en-GB".into()), ..Default::default() })
        .expect("en-GB renders");
    assert_eq!(gb.page_count, 1);
    let e = preview_report_with(&rpt, &PdfExportOptions { locale: Some("xx-XX".into()), ..Default::default() })
        .unwrap_err();
    assert!(e.contains("xx-XX"), "`{e}`");
}
