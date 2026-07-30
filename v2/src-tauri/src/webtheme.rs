//! Palettes for the pages this app opens in a REAL browser - the
//! execution report and the test case view/export.
//!
//! Two things are going on here.
//!
//! **The page arrives in the app's theme.** The values are read live from
//! the running UI's CSS variables (see `src/lib/reportTheme.ts`) rather
//! than duplicated in Rust, so a new theme - or an accent preset composed
//! on top of one - needs no change on this side. Every field falls back to
//! the original light styling if it arrives empty, which is also what a
//! caller that supplies no palette gets.
//!
//! **The page can be flipped once it is open.** A browser tab outlives the
//! app's theme: a report opened at night gets read in the morning, and a
//! saved export gets sent to someone whose eyes work differently. So both
//! palettes ship in every page and a switch in the corner chooses between
//! them, starting on whichever one the app was wearing.
//!
//! The accent is deliberately NOT flipped with the rest. Themes carry
//! their own default accent (green, indigo, amber, cyan...), so deriving
//! the light palette independently would change the app's colour identity
//! halfway through a page. The accent the user is looking at is used for
//! both schemes; only the surfaces and text change.

/// One scheme's worth of colour, named for the app's own tokens.
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct ReportPalette {
    pub bg: String,
    pub surface: String,
    pub surface_2: String,
    pub text: String,
    pub muted: String,
    pub faint: String,
    pub border: String,
    pub accent: String,
    pub success: String,
    pub danger: String,
    pub warning: String,
    /// Drives `color-scheme`, so form controls and scrollbars follow too.
    pub dark: bool,
}

impl Default for ReportPalette {
    /// The pages' original light styling.
    fn default() -> Self {
        Self {
            bg: "#f3f5f8".into(),
            surface: "#ffffff".into(),
            surface_2: "#eef1f5".into(),
            text: "#1f2530".into(),
            muted: "#5b6472".into(),
            faint: "#8a93a1".into(),
            border: "#e8ecf1".into(),
            accent: "#15803d".into(),
            success: "#16a34a".into(),
            danger: "#dc2626".into(),
            warning: "#d97706".into(),
            dark: false,
        }
    }
}

impl ReportPalette {
    /// A dark scheme to fall back on when a caller supplies only one
    /// palette (or none). Slate - the app's own default dark theme.
    pub fn default_dark() -> Self {
        Self {
            bg: "#0f172a".into(),
            surface: "#1e293b".into(),
            surface_2: "#263449".into(),
            text: "#e2e8f0".into(),
            muted: "#94a3b8".into(),
            faint: "#64748b".into(),
            border: "#334155".into(),
            accent: "#22c55e".into(),
            success: "#22c55e".into(),
            danger: "#f87171".into(),
            warning: "#fbbf24".into(),
            dark: true,
        }
    }

    /// Replace any empty field with the matching default: a half-populated
    /// palette must never render unreadable text on an unstyled page.
    fn filled(&self) -> Self {
        let d = if self.dark { Self::default_dark() } else { Self::default() };
        let or = |v: &str, fallback: &str| {
            let v = v.trim();
            if v.is_empty() { fallback.to_string() } else { v.to_string() }
        };
        Self {
            bg: or(&self.bg, &d.bg),
            surface: or(&self.surface, &d.surface),
            surface_2: or(&self.surface_2, &d.surface_2),
            text: or(&self.text, &d.text),
            muted: or(&self.muted, &d.muted),
            faint: or(&self.faint, &d.faint),
            border: or(&self.border, &d.border),
            accent: or(&self.accent, &d.accent),
            success: or(&self.success, &d.success),
            danger: or(&self.danger, &d.danger),
            warning: or(&self.warning, &d.warning),
            dark: self.dark,
        }
    }

    /// The variable block for one scheme, under the given `:root` selector.
    fn vars(&self, selector: &str) -> String {
        let p = self.filled();
        format!(
            "{selector} {{ color-scheme: {scheme};\n\
             --bg: {bg}; --surface: {surface}; --surface-2: {surface_2};\n\
             --text: {text}; --muted: {muted}; --faint: {faint}; --border: {border};\n\
             --accent: {accent}; --success: {success}; --danger: {danger}; --warning: {warning}; }}",
            selector = selector,
            scheme = if p.dark { "dark" } else { "light" },
            bg = p.bg,
            surface = p.surface,
            surface_2 = p.surface_2,
            text = p.text,
            muted = p.muted,
            faint = p.faint,
            border = p.border,
            accent = p.accent,
            success = p.success,
            danger = p.danger,
            warning = p.warning,
        )
    }
}

/// Both schemes a page can wear, and which one it opens in.
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct PagePalette {
    pub light: ReportPalette,
    pub dark: ReportPalette,
    /// True when the app is currently dark, so the page opens to match.
    pub dark_first: bool,
}

impl Default for PagePalette {
    fn default() -> Self {
        Self {
            light: ReportPalette::default(),
            dark: ReportPalette::default_dark(),
            dark_first: false,
        }
    }
}

impl PagePalette {
    /// Which scheme the page starts in - also the value baked into the
    /// `<html>` tag, so there is no unstyled flash before the script runs.
    pub fn initial_scheme(&self) -> &'static str {
        if self.dark_first { "dark" } else { "light" }
    }

    /// Both variable blocks. `dark` is forced true/false on its own block
    /// regardless of what the caller sent, so a palette mislabelled by the
    /// frontend still gets the right `color-scheme` for its slot.
    pub fn css(&self) -> String {
        let light = ReportPalette { dark: false, ..self.light.clone() };
        let dark = ReportPalette { dark: true, ..self.dark.clone() };
        format!(
            "{}\n{}\n{}",
            light.vars(":root, :root[data-scheme=\"light\"]"),
            dark.vars(":root[data-scheme=\"dark\"]"),
            SWITCH_CSS,
        )
    }
}

/// The switch itself. Fixed to the corner rather than placed in the flow:
/// these pages have three different layouts (report, single column, two
/// column) and one control that never moves beats three placements. It
/// prints as nothing.
const SWITCH_CSS: &str = r#"
.scheme-switch { position: fixed; top: 12px; right: 12px; z-index: 20;
                 display: flex; align-items: center; gap: 6px;
                 font: inherit; font-size: 12px; cursor: pointer;
                 padding: 6px 12px; border-radius: 999px;
                 color: var(--muted); background: var(--surface);
                 border: 1px solid var(--border); }
.scheme-switch:hover { color: var(--accent); border-color: var(--accent); }
.scheme-switch svg { width: 14px; height: 14px; }
/* One button, two glyphs: whichever scheme is showing hides its own icon
   so the button always pictures where it will TAKE you. */
:root[data-scheme="dark"] .scheme-switch .to-dark,
:root:not([data-scheme="dark"]) .scheme-switch .to-light { display: none; }
@media print { .scheme-switch { display: none; } }
"#;

/// The switch's markup. `data-scheme` on `<html>` is the single source of
/// truth; the script only flips it and tries to remember the choice.
pub const SWITCH_HTML: &str = r#"<button class="scheme-switch" type="button" id="scheme-switch" aria-label="Switch between light and dark">
<span class="to-dark" aria-hidden><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/></svg></span>
<span class="to-light" aria-hidden><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M19.1 4.9l-1.4 1.4M6.3 17.7l-1.4 1.4"/></svg></span>
<span class="scheme-label">Dark</span></button>"#;

/// Flips `data-scheme` and remembers it. The remembering is best-effort:
/// these pages are opened from a temp file, and a `file://` origin has no
/// usable localStorage in some browsers - which throws rather than
/// returning null, so every access is guarded. The switch still works for
/// the life of the tab either way.
pub const SWITCH_JS: &str = r#"
(function () {
  var root = document.documentElement;
  var KEY = 'tcm-page-scheme';
  var store = function (v) { try { localStorage.setItem(KEY, v); } catch (e) { /* file:// */ } };
  var remembered = null;
  try { remembered = localStorage.getItem(KEY); } catch (e) { /* file:// */ }
  if (remembered === 'dark' || remembered === 'light') root.setAttribute('data-scheme', remembered);
  var btn = document.getElementById('scheme-switch');
  var label = btn && btn.querySelector('.scheme-label');
  var paint = function () {
    var dark = root.getAttribute('data-scheme') === 'dark';
    if (label) label.textContent = dark ? 'Light' : 'Dark';
    if (btn) btn.setAttribute('aria-pressed', dark ? 'true' : 'false');
  };
  if (btn) {
    btn.addEventListener('click', function () {
      var next = root.getAttribute('data-scheme') === 'dark' ? 'light' : 'dark';
      root.setAttribute('data-scheme', next);
      store(next);
      paint();
    });
  }
  paint();
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

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
}
