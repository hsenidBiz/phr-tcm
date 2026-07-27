use std::collections::HashMap;
use v2_lib::ado_testplan::TestPoint;
use v2_lib::report::{build_report_html, format_epoch_utc, FailureInfo, ReportPalette};

fn point(id: i32, name: &str, outcome: &str) -> TestPoint {
    TestPoint {
        point_id: id,
        test_case_id: Some(200 + id),
        test_case_name: name.into(),
        config_name: "W10".into(),
        tester: String::new(),
        last_outcome: outcome.into(),
        last_run_id: Some(1),
        last_result_id: Some(10 + id),
    }
}

#[test]
fn epoch_formatting_matches_known_dates() {
    assert_eq!(format_epoch_utc(0), "1970-01-01 00:00 UTC");
    // 2026-01-01 00:00:00 UTC
    assert_eq!(format_epoch_utc(1_767_225_600), "2026-01-01 00:00 UTC");
    // Leap-day check: 2024-02-29 12:30 UTC = 1709209800
    assert_eq!(format_epoch_utc(1_709_209_800), "2024-02-29 12:30 UTC");
}

#[test]
fn report_counts_pass_rate_and_orders_failures_first() {
    let points = vec![
        point(1, "Alpha works", "Passed"),
        point(2, "Beta breaks", "Failed"),
        point(3, "Gamma pending", ""),
        point(4, "Delta blocked", "Blocked"),
    ];
    let mut failures = HashMap::new();
    failures.insert(
        2,
        FailureInfo { comment: "Broke on save".into(), bug_ids: vec![900] },
    );

    let html = build_report_html("PBI #42", "org", "proj", &points, &failures, "2026-07-13 00:00 UTC", &ReportPalette::default());

    // Pass rate: 1 passed of 3 executed (never-run excluded) = 33%.
    assert!(html.contains("33%"), "pass rate missing: {html}");
    assert!(html.contains("pass rate over 3 executed of 4 cases"));
    // Legend counts.
    assert!(html.contains("Passed: 1"));
    assert!(html.contains("Failed: 1"));
    assert!(html.contains("Never run: 1"));
    // Failures-first row ordering: Beta before Delta before Alpha before Gamma.
    let beta = html.find("Beta breaks").unwrap();
    let delta = html.find("Delta blocked").unwrap();
    let alpha = html.find("Alpha works").unwrap();
    let gamma = html.find("Gamma pending").unwrap();
    assert!(beta < delta && delta < alpha && alpha < gamma);
    // Failure card: comment + bug link.
    assert!(html.contains("Broke on save"));
    assert!(html.contains("_workitems/edit/900"));
    // Footer timestamp injected verbatim.
    assert!(html.contains("Generated 2026-07-13 00:00 UTC"));
}

#[test]
fn report_escapes_html_in_names_and_comments() {
    let points = vec![point(1, "<script>alert(1)</script>", "Failed")];
    let mut failures = HashMap::new();
    failures.insert(1, FailureInfo { comment: "x < y & z".into(), bug_ids: vec![] });
    let html = build_report_html("T", "o", "p", &points, &failures, "now", &ReportPalette::default());
    assert!(!html.contains("<script>alert"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("x &lt; y &amp; z"));
}

/// The report opens in a real browser, so it carries the app's palette
/// rather than a hardcoded light one.
#[test]
fn the_report_is_rendered_in_the_apps_palette() {
    let points = vec![point(1, "Alpha works", "Passed")];
    let failures = HashMap::new();
    let dark = ReportPalette {
        bg: "#000000".into(),
        surface: "#0b0b0d".into(),
        surface_2: "#141418".into(),
        text: "#e5e7eb".into(),
        muted: "#9ca3af".into(),
        faint: "#6b7280".into(),
        border: "#27272a".into(),
        accent: "#22c55e".into(),
        success: "#22c55e".into(),
        danger: "#ef4444".into(),
        warning: "#f59e0b".into(),
        dark: true,
    };
    let html = build_report_html("T", "o", "p", &points, &failures, "now", &dark);

    assert!(html.contains("--bg: #000000"), "the palette reaches the page");
    assert!(html.contains("--danger: #ef4444"));
    // color-scheme drives the browser's own scrollbars and controls.
    assert!(html.contains("color-scheme: dark"));
    // The stylesheet must consume the variables, not hardcoded colours.
    assert!(html.contains("background: var(--bg)"));
    assert!(!html.contains("#f3f5f8"), "no light default left behind");
}

#[test]
fn the_light_default_is_used_when_no_palette_is_supplied() {
    let points = vec![point(1, "Alpha works", "Passed")];
    let html = build_report_html("T", "o", "p", &points, &HashMap::new(), "now", &ReportPalette::default());
    assert!(html.contains("color-scheme: light"));
    assert!(html.contains("--bg: #f3f5f8"));
}

/// A half-read palette (a token the UI could not resolve) must degrade to
/// the light default for that token, never to an empty value that would
/// leave text unstyled on an unstyled background.
#[test]
fn empty_tokens_fall_back_instead_of_rendering_blank() {
    let partial = ReportPalette {
        bg: "#101010".into(),
        surface: "  ".into(), // whitespace counts as unset
        surface_2: String::new(),
        text: "#eeeeee".into(),
        muted: String::new(),
        faint: String::new(),
        border: String::new(),
        accent: String::new(),
        success: String::new(),
        danger: String::new(),
        warning: String::new(),
        dark: true,
    };
    let html = build_report_html("T", "o", "p", &[], &HashMap::new(), "now", &partial);

    assert!(html.contains("--bg: #101010"), "what WAS read is honoured");
    assert!(html.contains("--surface: #ffffff"), "what wasn't falls back");
    assert!(!html.contains("--muted: ;"), "never an empty declaration");
}
