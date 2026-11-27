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

// ---- Diagram contrast: the pages' text and essential marks, both palettes.

mod diagram_contrast {
    use crate::common::{contrast, mix};
    use v2_lib::webtheme::ReportPalette;

    fn both() -> [(&'static str, ReportPalette); 2] {
        [("light", ReportPalette::default()), ("dark", ReportPalette::default_dark())]
    }

    #[test]
    fn flow_page_text_reaches_4_5_to_1_and_its_marks_3_to_1_in_both_palettes() {
        for (name, p) in both() {
            let mut texts = vec![
                ("text on surface", contrast(&p.text, &p.surface)),
                ("muted on surface (under line, failure line)", contrast(&p.muted, &p.surface)),
                ("muted on surface (the outline tag: Unconfirmed here, Failed here)", contrast(&p.muted, &p.surface)),
                ("muted on page (subtitle, legend)", contrast(&p.muted, &p.bg)),
                ("text on surface-2 (template chips)", contrast(&p.text, &p.surface_2)),
            ];
            // The tag: normal text on a 14% tint of the tone over the surface.
            for (tone, c) in [("success", &p.success), ("warning", &p.warning), ("danger", &p.danger), ("muted", &p.muted), ("accent", &p.accent)] {
                texts.push((tone, contrast(&p.text, &mix(c, 14.0, &p.surface))));
            }
            for (what, ratio) in &texts {
                assert!(*ratio >= 4.5, "{name}: {what} is {ratio:.2}:1");
            }
            // Essential marks: the connectors and arrowheads (muted on the
            // page), and the accent outline of an open stage.
            for (what, ratio) in [
                ("connector on page", contrast(&p.muted, &p.bg)),
                ("accent outline on surface", contrast(&p.accent, &p.surface)),
            ] {
                assert!(ratio >= 3.0, "{name}: {what} is {ratio:.2}:1");
            }
        }
    }

    #[test]
    fn the_hairline_border_is_not_what_draws_an_essential_mark() {
        for (name, p) in both() {
            assert!(contrast(&p.border, &p.bg) < 3.0, "{name}: --border really is a hairline");
            assert!(contrast(&p.muted, &p.bg) >= 3.0, "{name}: the test map's limbs use --muted");
        }
    }

    #[test]
    fn the_run_reports_trend_bars_reach_3_to_1() {
        let tokens = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/web/autorun-report-tokens.css")).unwrap();
        let block = |marker: &str| -> String {
            let at = tokens.find(marker).expect(marker);
            tokens[at..].split('}').next().unwrap().to_string()
        };
        let var = |b: &str, name: &str| -> String {
            let at = b.find(&format!("{name}:")).unwrap_or_else(|| panic!("{name}"));
            b[at + name.len() + 1..].split(';').next().unwrap().to_string()
        };
        for mode in ["light", "dark"] {
            let b = block(&format!(":root[data-mode=\"{mode}\"]{{"));
            let (bg, strong, border) = (var(&b, "--bg"), var(&b, "--border-strong"), var(&b, "--border"));
            let ratio = contrast(&strong, &bg);
            assert!(ratio >= 3.0, "{mode}: --border-strong is {ratio:.2}:1");
            assert!(contrast(&border, &bg) < 3.0, "{mode}: the old bar colour was below 3:1");
        }
    }
}

mod diagram_contrast_review {
    use crate::common::{contrast, mix};
    use v2_lib::webtheme::ReportPalette;

    #[test]
    fn muted_text_reads_on_the_selected_rows_accent_tint_and_on_surface_2() {
        for (name, p) in [("light", ReportPalette::default()), ("dark", ReportPalette::default_dark())] {
            // The selected row is the accent at 14% over the page; its "#id" is muted.
            let tint = mix(&p.accent, 14.0, &p.bg);
            let r = contrast(&p.muted, &tint);
            assert!(r >= 4.5, "{name}: muted on the selected row is {r:.2}:1");
            assert!(contrast(&p.text, &tint) >= 4.5, "{name}: text on the selected row");
            let r = contrast(&p.muted, &p.surface_2);
            assert!(r >= 4.5, "{name}: muted on surface-2 is {r:.2}:1");
        }
    }

    #[test]
    fn the_run_reports_highlighted_bar_stands_out_from_the_others_by_3_to_1() {
        let tokens = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/web/autorun-report-tokens.css")).unwrap();
        for mode in ["light", "dark"] {
            let at = tokens.find(&format!(":root[data-mode=\"{mode}\"]{{")).unwrap();
            let b = tokens[at..].split('}').next().unwrap();
            let var = |name: &str| -> String {
                let i = b.find(&format!("{name}:")).unwrap();
                b[i + name.len() + 1..].split(';').next().unwrap().to_string()
            };
            let r = contrast(&var("--text"), &var("--border-strong"));
            assert!(r >= 3.0, "{mode}: the last bar (--text) against the other bars is {r:.2}:1");
        }
        let css = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/web/autorun-report.css")).unwrap();
        assert!(css.contains(".trend__bar--last { fill: var(--text); }"));
    }
}

mod diagram_contrast_round2 {
    use crate::common::contrast;

    #[test]
    fn the_average_line_halo_separates_the_line_from_every_bar_in_both_modes() {
        let tokens = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/web/autorun-report-tokens.css")).unwrap();
        for mode in ["light", "dark"] {
            let at = tokens.find(&format!(":root[data-mode=\"{mode}\"]{{")).unwrap();
            let b = tokens[at..].split('}').next().unwrap();
            let var = |name: &str| -> String {
                let i = b.find(&format!("{name}:")).unwrap();
                b[i + name.len() + 1..].split(';').next().unwrap().to_string()
            };
            let (text, bg, bar) = (var("--text"), var("--bg"), var("--border-strong"));
            // The line is --text, the last bar is --text: only the --bg halo between them
            // keeps the line visible, and it must stand out from both the line and the bars.
            assert!(contrast(&text, &bg) >= 4.5, "{mode}: line on its halo");
            assert!(contrast(&bar, &bg) >= 3.0, "{mode}: the halo against the ordinary bars");
        }
    }
}
