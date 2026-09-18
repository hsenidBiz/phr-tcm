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
