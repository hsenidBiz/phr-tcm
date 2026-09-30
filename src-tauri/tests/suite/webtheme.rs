//! Palettes for the pages opened in a real browser: both schemes are
//! emitted, per-slot, with per-scheme fallbacks.

use v2_lib::webtheme::{PagePalette, ReportPalette};

fn p(bg: &str, dark: bool) -> ReportPalette {
    ReportPalette { bg: bg.into(), dark, ..ReportPalette::default() }
}

#[test]
fn both_schemes_are_emitted_and_the_light_one_is_also_the_bare_root() {
    let css = PagePalette { light: p("#fff", false), dark: p("#000", true), dark_first: true }.css();
    // The bare :root carries light too, so a page whose script never
    // runs is still styled rather than unpainted.
    assert!(css.contains(":root, :root[data-scheme=\"light\"] { color-scheme: light;"));
    assert!(css.contains(":root[data-scheme=\"dark\"] { color-scheme: dark;"));
    assert!(css.contains("--bg: #fff;"));
    assert!(css.contains("--bg: #000;"));
}

#[test]
fn a_palette_in_the_wrong_slot_still_gets_its_slots_color_scheme() {
    // The frontend claims both are light; the dark slot must not end
    // up telling the browser to render dark surfaces in light mode.
    let css = PagePalette { light: p("#fff", false), dark: p("#101010", false), dark_first: false }.css();
    let dark_block = css.split(":root[data-scheme=\"dark\"]").nth(1).unwrap();
    assert!(dark_block.starts_with(" { color-scheme: dark;"));
}

#[test]
fn empty_fields_fall_back_per_scheme_not_always_to_light() {
    // A blank dark palette must not fill in with white surfaces.
    let blank = ReportPalette { dark: true, ..ReportPalette { dark: true, bg: String::new(), surface: String::new(), surface_2: String::new(), text: String::new(), muted: String::new(), faint: String::new(), border: String::new(), accent: String::new(), success: String::new(), danger: String::new(), warning: String::new() } };
    let css = PagePalette { light: ReportPalette::default(), dark: blank, dark_first: true }.css();
    let dark_block = css.split(":root[data-scheme=\"dark\"]").nth(1).unwrap();
    assert!(dark_block.contains(&ReportPalette::default_dark().bg));
    assert!(!dark_block.contains("--bg: #f3f5f8;"));
}

#[test]
fn the_page_opens_in_the_apps_own_scheme() {
    assert_eq!(PagePalette { dark_first: true, ..PagePalette::default() }.initial_scheme(), "dark");
    assert_eq!(PagePalette::default().initial_scheme(), "light");
}

/// The pages' card shadows come from the palette, like every other colour.
#[test]
fn both_schemes_carry_a_shadow_variable() {
    let css = PagePalette::default().css();
    assert_eq!(css.matches("--shadow:").count(), 2, "{css}");
    assert!(css.contains("color-mix(in srgb,"), "{css}");
}

/// The switch's remembered choice has to be on <html> before anything is
/// painted - in <head>, not in the switch's own script at the end of the
/// page. There, a page set to Light opened dark (the app's scheme) and
/// then went white.
pub fn assert_scheme_restored_before_paint(page: &str, html: &str) {
    let head_end = html.find("</head>").unwrap_or_else(|| panic!("{page}: no </head>"));
    let restore = html
        .find(v2_lib::webtheme::RESTORE_JS)
        .unwrap_or_else(|| panic!("{page}: no scheme restore script"));
    assert!(restore < head_end, "{page}: the restore script must be in <head>");
    let charset = html.find("<meta charset").unwrap_or(0);
    assert!(charset < restore, "{page}: charset first, so the script is read as UTF-8");
    // And only there: the switch no longer restores anything itself.
    assert!(!v2_lib::webtheme::SWITCH_JS.contains("getItem"), "the switch must not restore it a second time");
}

#[test]
fn the_report_and_the_pages_beside_it_restore_the_scheme_before_the_first_paint() {
    use std::collections::HashMap;
    let report = v2_lib::report::build_report_html("T", "o", "p", &[], &HashMap::new(), "now", &PagePalette::default());
    assert_scheme_restored_before_paint("execution report", &report);

    let dir = tempfile::tempdir().unwrap();
    let cases = dir.path().join("cases.html");
    v2_lib::import_parser::export_queue_to_html(&[], cases.to_str().unwrap(), "", None, &PagePalette::default()).unwrap();
    assert_scheme_restored_before_paint("View in browser", &std::fs::read_to_string(&cases).unwrap());

    let map = dir.path().join("map.html");
    v2_lib::test_map::export_test_map_html(&[], map.to_str().unwrap(), "", &PagePalette::default(), None).unwrap();
    assert_scheme_restored_before_paint("test map", &std::fs::read_to_string(&map).unwrap());
}

#[test]
fn the_restore_script_only_takes_the_two_schemes_and_survives_a_file_origin() {
    let js = v2_lib::webtheme::RESTORE_JS;
    assert!(js.contains("'tcm-page-scheme'"), "the key the switch stores under");
    assert!(js.contains("=== 'dark'") && js.contains("=== 'light'"), "a stored value is checked, never applied raw");
    assert!(js.contains("try") && js.contains("catch"), "file:// localStorage can throw");
}
