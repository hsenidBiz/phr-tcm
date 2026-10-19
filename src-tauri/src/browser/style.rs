//! What `expect_style` compares, and the words it fails in.
//!
//! The page answers with its computed style (`getComputedStyle`), which
//! writes every colour as `rgb()` or `rgba()`. A script may write the same
//! colour any way CSS allows - `red`, `#f00`, `hsl(0, 100%, 50%)` - so a
//! value that reads as a colour is compared in that one form on both sides.
//! Any other value is compared trimmed, with its runs of spaces made one,
//! and ignoring case - unless it holds a quote, where the case is the
//! page's own text (`content: "Hello"`, a font's quoted family name).

/// Which comparison a check makes, and with what.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Want<'a> {
    Equals(&'a str),
    Contains(&'a str),
    NotEquals(&'a str),
}

/// Between the value seen and the one wanted, in every failure:
/// `color is rgb(0, 0, 0), expected rgb(255, 0, 0)`.
pub const EXPECTED: &str = ", expected ";
/// After [`EXPECTED`], for `contains`.
pub const TO_CONTAIN: &str = "it to contain ";
/// After [`EXPECTED`], for `not_equals`.
pub const ANYTHING_BUT: &str = "anything but ";
/// A property a transition or animation is still changing:
/// `color is still changing (rgb(9, 0, 0) so far)`.
pub const STILL_CHANGING: &str = "still changing";
/// How an empty computed value is shown.
pub const EMPTY: &str = "(empty)";

/// The CSS basic colours, `orange`, the two spellings of grey and the
/// `aqua`/`cyan` and `fuchsia`/`magenta` pairs, as red, green, blue.
const NAMED: &[(&str, [u8; 3])] = &[
    ("black", [0, 0, 0]),
    ("silver", [192, 192, 192]),
    ("gray", [128, 128, 128]),
    ("grey", [128, 128, 128]),
    ("white", [255, 255, 255]),
    ("maroon", [128, 0, 0]),
    ("red", [255, 0, 0]),
    ("purple", [128, 0, 128]),
    ("fuchsia", [255, 0, 255]),
    ("magenta", [255, 0, 255]),
    ("green", [0, 128, 0]),
    ("lime", [0, 255, 0]),
    ("olive", [128, 128, 0]),
    ("yellow", [255, 255, 0]),
    ("navy", [0, 0, 128]),
    ("blue", [0, 0, 255]),
    ("teal", [0, 128, 128]),
    ("aqua", [0, 255, 255]),
    ("cyan", [0, 255, 255]),
    ("orange", [255, 165, 0]),
];

/// `s` as the browser writes a computed colour - `rgb(255, 0, 0)`, or
/// `rgba(255, 0, 0, 0.5)` when it is not opaque - or `None` when it is not
/// a colour this reads: a name above or `transparent`, `#rgb`, `#rgba`,
/// `#rrggbb`, `#rrggbbaa`, `rgb()`/`rgba()` (commas or spaces, an alpha
/// after a comma or a slash, numbers or percentages) or `hsl()`/`hsla()`.
pub fn colour(s: &str) -> Option<String> {
    let [r, g, b, a] = parse(s)?;
    Some(if a == 255 { format!("rgb({r}, {g}, {b})") } else { format!("rgba({r}, {g}, {b}, {})", alpha_text(a)) })
}

fn parse(s: &str) -> Option<[u8; 4]> {
    let s = s.trim().to_ascii_lowercase();
    if s == "transparent" {
        return Some([0, 0, 0, 0]);
    }
    if let Some(&(_, [r, g, b])) = NAMED.iter().find(|(n, _)| *n == s) {
        return Some([r, g, b, 255]);
    }
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    let (name, rest) = s.split_once('(')?;
    let inner = rest.strip_suffix(')')?;
    let parts: Vec<&str> = inner.split(|c: char| c == ',' || c == '/' || c.is_whitespace()).filter(|p| !p.is_empty()).collect();
    if parts.len() != 3 && parts.len() != 4 {
        return None;
    }
    let alpha = match parts.get(3) {
        Some(p) => alpha_byte(p)?,
        None => 255,
    };
    match name.trim() {
        "rgb" | "rgba" => Some([channel(parts[0])?, channel(parts[1])?, channel(parts[2])?, alpha]),
        "hsl" | "hsla" => {
            let [r, g, b] = hsl(hue(parts[0])?, percent(parts[1])?, percent(parts[2])?);
            Some([r, g, b, alpha])
        }
        _ => None,
    }
}

fn parse_hex(hex: &str) -> Option<[u8; 4]> {
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok().map(|d| d * 17);
    let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    match hex.len() {
        3 => Some([digit(0)?, digit(1)?, digit(2)?, 255]),
        4 => Some([digit(0)?, digit(1)?, digit(2)?, digit(3)?]),
        6 => Some([pair(0)?, pair(2)?, pair(4)?, 255]),
        8 => Some([pair(0)?, pair(2)?, pair(4)?, pair(6)?]),
        _ => None,
    }
}

fn number(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|n| n.is_finite())
}

fn to_byte(v: f64) -> u8 {
    v.clamp(0.0, 255.0).round() as u8
}

/// One of rgb()'s three: 0 to 255, or a percentage of it.
fn channel(p: &str) -> Option<u8> {
    match p.strip_suffix('%') {
        Some(pct) => Some(to_byte(number(pct)? * 255.0 / 100.0)),
        None => Some(to_byte(number(p)?)),
    }
}

/// An alpha, 0 to 1 or a percentage, as the byte the browser keeps.
fn alpha_byte(p: &str) -> Option<u8> {
    let a = match p.strip_suffix('%') {
        Some(pct) => number(pct)? / 100.0,
        None => number(p)?,
    };
    Some(to_byte(a.clamp(0.0, 1.0) * 255.0))
}

/// The alpha as CSS serialises it: two decimals when they name the same
/// byte, else three - `0.5` for `#ff000080`, as the browser shows it.
fn alpha_text(byte: u8) -> String {
    let exact = f64::from(byte) / 255.0;
    let two = (exact * 100.0).round() / 100.0;
    let shown = if to_byte(two * 255.0) == byte { two } else { (exact * 1000.0).round() / 1000.0 };
    let text = format!("{shown:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A hue in degrees: a number, or `deg`, `turn`, `rad` or `grad`.
fn hue(p: &str) -> Option<f64> {
    for (unit, per) in [("deg", 1.0), ("grad", 0.9), ("rad", 180.0 / std::f64::consts::PI), ("turn", 360.0)] {
        if let Some(n) = p.strip_suffix(unit) {
            return Some(number(n)? * per);
        }
    }
    number(p)
}

/// hsl()'s saturation or lightness, 0 to 1 (`50%`, or a bare 50).
fn percent(p: &str) -> Option<f64> {
    Some((number(p.strip_suffix('%').unwrap_or(p))? / 100.0).clamp(0.0, 1.0))
}

/// CSS Color 4's hsl-to-rgb.
fn hsl(h: f64, s: f64, l: f64) -> [u8; 3] {
    let h = h.rem_euclid(360.0);
    let a = s * l.min(1.0 - l);
    let f = |n: f64| {
        let k = (n + h / 30.0) % 12.0;
        l - a * (k - 3.0).min(9.0 - k).min(1.0).max(-1.0)
    };
    [to_byte(f(0.0) * 255.0), to_byte(f(8.0) * 255.0), to_byte(f(4.0) * 255.0)]
}

/// A value as it is compared and shown: its colour form, or the value
/// trimmed with its runs of spaces made one.
pub fn normal(v: &str) -> String {
    colour(v).unwrap_or_else(|| v.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// A value for a sentence: [`normal`], or [`EMPTY`].
pub fn shown(v: &str) -> String {
    let n = normal(v);
    if n.is_empty() {
        EMPTY.to_string()
    } else {
        n
    }
}

fn quoted(v: &str) -> bool {
    v.contains(['"', '\''])
}

fn same(got: &str, want: &str) -> bool {
    if quoted(got) || quoted(want) {
        got == want
    } else {
        got.eq_ignore_ascii_case(want)
    }
}

fn holds(got: &str, want: &str) -> bool {
    if quoted(got) || quoted(want) {
        got.contains(want)
    } else {
        got.to_ascii_lowercase().contains(&want.to_ascii_lowercase())
    }
}

/// `got` (the computed value of `property`) against what is wanted.
/// `Ok` is the passed clause (`has color rgb(255, 0, 0)`), `Err` the
/// failure (`color is rgb(0, 0, 0), expected rgb(255, 0, 0)`).
pub fn judge(property: &str, got: &str, want: Want<'_>) -> Result<String, String> {
    let g = normal(got);
    let seen = shown(got);
    match want {
        Want::Equals(w) => {
            let w = normal(w);
            if same(&g, &w) {
                Ok(format!("has {property} {seen}"))
            } else {
                Err(format!("{property} is {seen}{EXPECTED}{}", shown(&w)))
            }
        }
        Want::Contains(w) => {
            let w = normal(w);
            if holds(&g, &w) {
                Ok(format!("has {property} {seen}"))
            } else {
                Err(format!("{property} is {seen}{EXPECTED}{TO_CONTAIN}{w}"))
            }
        }
        Want::NotEquals(w) => {
            let w = normal(w);
            if same(&g, &w) {
                Err(format!("{property} is {seen}{EXPECTED}{ANYTHING_BUT}{}", shown(&w)))
            } else {
                Ok(format!("has {property} {seen}, not {}", shown(&w)))
            }
        }
    }
}

/// Is `p` a CSS property name: a custom property (`--brand-colour`, any
/// case), or lower-case letters, digits and hyphens (`background-color`,
/// `-webkit-line-clamp`)?
pub fn is_property_name(p: &str) -> bool {
    if let Some(custom) = p.strip_prefix("--") {
        return !custom.is_empty() && custom.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    }
    p.chars().any(|c| c.is_ascii_lowercase())
        && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `backgroundColor` as CSS writes it: `background-color`.
pub fn css_name(p: &str) -> String {
    let mut out = String::new();
    for c in p.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Why `property` cannot be checked, or `None` when it can.
pub fn property_problem(property: &str) -> Option<String> {
    let p = property.trim();
    if p.is_empty() {
        return Some("expect_style has an empty property".to_string());
    }
    if is_property_name(p) {
        return None;
    }
    let kebab = css_name(p);
    if p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && is_property_name(&kebab) {
        return Some(format!("expect_style: write the property as CSS does, \"{kebab}\", not \"{p}\""));
    }
    let shown: String = p.chars().take(40).collect();
    Some(format!("expect_style: \"{shown}\" is not a CSS property name"))
}
