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

/// Every CSS named colour (CSS Color 4: 148 names, both spellings of
/// grey and the aqua/cyan and fuchsia/magenta pairs among them), as
/// red, green, blue, in alphabetical order. `transparent` is its own case.
pub const NAMED_COLOURS: &[(&str, [u8; 3])] = &[
    ("aliceblue", [240, 248, 255]),
    ("antiquewhite", [250, 235, 215]),
    ("aqua", [0, 255, 255]),
    ("aquamarine", [127, 255, 212]),
    ("azure", [240, 255, 255]),
    ("beige", [245, 245, 220]),
    ("bisque", [255, 228, 196]),
    ("black", [0, 0, 0]),
    ("blanchedalmond", [255, 235, 205]),
    ("blue", [0, 0, 255]),
    ("blueviolet", [138, 43, 226]),
    ("brown", [165, 42, 42]),
    ("burlywood", [222, 184, 135]),
    ("cadetblue", [95, 158, 160]),
    ("chartreuse", [127, 255, 0]),
    ("chocolate", [210, 105, 30]),
    ("coral", [255, 127, 80]),
    ("cornflowerblue", [100, 149, 237]),
    ("cornsilk", [255, 248, 220]),
    ("crimson", [220, 20, 60]),
    ("cyan", [0, 255, 255]),
    ("darkblue", [0, 0, 139]),
    ("darkcyan", [0, 139, 139]),
    ("darkgoldenrod", [184, 134, 11]),
    ("darkgray", [169, 169, 169]),
    ("darkgreen", [0, 100, 0]),
    ("darkgrey", [169, 169, 169]),
    ("darkkhaki", [189, 183, 107]),
    ("darkmagenta", [139, 0, 139]),
    ("darkolivegreen", [85, 107, 47]),
    ("darkorange", [255, 140, 0]),
    ("darkorchid", [153, 50, 204]),
    ("darkred", [139, 0, 0]),
    ("darksalmon", [233, 150, 122]),
    ("darkseagreen", [143, 188, 143]),
    ("darkslateblue", [72, 61, 139]),
    ("darkslategray", [47, 79, 79]),
    ("darkslategrey", [47, 79, 79]),
    ("darkturquoise", [0, 206, 209]),
    ("darkviolet", [148, 0, 211]),
    ("deeppink", [255, 20, 147]),
    ("deepskyblue", [0, 191, 255]),
    ("dimgray", [105, 105, 105]),
    ("dimgrey", [105, 105, 105]),
    ("dodgerblue", [30, 144, 255]),
    ("firebrick", [178, 34, 34]),
    ("floralwhite", [255, 250, 240]),
    ("forestgreen", [34, 139, 34]),
    ("fuchsia", [255, 0, 255]),
    ("gainsboro", [220, 220, 220]),
    ("ghostwhite", [248, 248, 255]),
    ("gold", [255, 215, 0]),
    ("goldenrod", [218, 165, 32]),
    ("gray", [128, 128, 128]),
    ("green", [0, 128, 0]),
    ("greenyellow", [173, 255, 47]),
    ("grey", [128, 128, 128]),
    ("honeydew", [240, 255, 240]),
    ("hotpink", [255, 105, 180]),
    ("indianred", [205, 92, 92]),
    ("indigo", [75, 0, 130]),
    ("ivory", [255, 255, 240]),
    ("khaki", [240, 230, 140]),
    ("lavender", [230, 230, 250]),
    ("lavenderblush", [255, 240, 245]),
    ("lawngreen", [124, 252, 0]),
    ("lemonchiffon", [255, 250, 205]),
    ("lightblue", [173, 216, 230]),
    ("lightcoral", [240, 128, 128]),
    ("lightcyan", [224, 255, 255]),
    ("lightgoldenrodyellow", [250, 250, 210]),
    ("lightgray", [211, 211, 211]),
    ("lightgreen", [144, 238, 144]),
    ("lightgrey", [211, 211, 211]),
    ("lightpink", [255, 182, 193]),
    ("lightsalmon", [255, 160, 122]),
    ("lightseagreen", [32, 178, 170]),
    ("lightskyblue", [135, 206, 250]),
    ("lightslategray", [119, 136, 153]),
    ("lightslategrey", [119, 136, 153]),
    ("lightsteelblue", [176, 196, 222]),
    ("lightyellow", [255, 255, 224]),
    ("lime", [0, 255, 0]),
    ("limegreen", [50, 205, 50]),
    ("linen", [250, 240, 230]),
    ("magenta", [255, 0, 255]),
    ("maroon", [128, 0, 0]),
    ("mediumaquamarine", [102, 205, 170]),
    ("mediumblue", [0, 0, 205]),
    ("mediumorchid", [186, 85, 211]),
    ("mediumpurple", [147, 112, 219]),
    ("mediumseagreen", [60, 179, 113]),
    ("mediumslateblue", [123, 104, 238]),
    ("mediumspringgreen", [0, 250, 154]),
    ("mediumturquoise", [72, 209, 204]),
    ("mediumvioletred", [199, 21, 133]),
    ("midnightblue", [25, 25, 112]),
    ("mintcream", [245, 255, 250]),
    ("mistyrose", [255, 228, 225]),
    ("moccasin", [255, 228, 181]),
    ("navajowhite", [255, 222, 173]),
    ("navy", [0, 0, 128]),
    ("oldlace", [253, 245, 230]),
    ("olive", [128, 128, 0]),
    ("olivedrab", [107, 142, 35]),
    ("orange", [255, 165, 0]),
    ("orangered", [255, 69, 0]),
    ("orchid", [218, 112, 214]),
    ("palegoldenrod", [238, 232, 170]),
    ("palegreen", [152, 251, 152]),
    ("paleturquoise", [175, 238, 238]),
    ("palevioletred", [219, 112, 147]),
    ("papayawhip", [255, 239, 213]),
    ("peachpuff", [255, 218, 185]),
    ("peru", [205, 133, 63]),
    ("pink", [255, 192, 203]),
    ("plum", [221, 160, 221]),
    ("powderblue", [176, 224, 230]),
    ("purple", [128, 0, 128]),
    ("rebeccapurple", [102, 51, 153]),
    ("red", [255, 0, 0]),
    ("rosybrown", [188, 143, 143]),
    ("royalblue", [65, 105, 225]),
    ("saddlebrown", [139, 69, 19]),
    ("salmon", [250, 128, 114]),
    ("sandybrown", [244, 164, 96]),
    ("seagreen", [46, 139, 87]),
    ("seashell", [255, 245, 238]),
    ("sienna", [160, 82, 45]),
    ("silver", [192, 192, 192]),
    ("skyblue", [135, 206, 235]),
    ("slateblue", [106, 90, 205]),
    ("slategray", [112, 128, 144]),
    ("slategrey", [112, 128, 144]),
    ("snow", [255, 250, 250]),
    ("springgreen", [0, 255, 127]),
    ("steelblue", [70, 130, 180]),
    ("tan", [210, 180, 140]),
    ("teal", [0, 128, 128]),
    ("thistle", [216, 191, 216]),
    ("tomato", [255, 99, 71]),
    ("turquoise", [64, 224, 208]),
    ("violet", [238, 130, 238]),
    ("wheat", [245, 222, 179]),
    ("white", [255, 255, 255]),
    ("whitesmoke", [245, 245, 245]),
    ("yellow", [255, 255, 0]),
    ("yellowgreen", [154, 205, 50]),
];

/// `s` as the browser writes a computed colour - `rgb(255, 0, 0)`, or
/// `rgba(255, 0, 0, 0.5)` when it is not opaque - or `None` when it is not
/// a colour this reads: a name in [`NAMED_COLOURS`] or `transparent`, `#rgb`, `#rgba`,
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
    if let Some(&(_, [r, g, b])) = NAMED_COLOURS.iter().find(|(n, _)| *n == s) {
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
/// trimmed with its runs of spaces made one and every comma followed by
/// one space and preceded by none - the way the browser writes a computed
/// list, so `matrix(1,0,0,1,0,0)` reads as `matrix(1, 0, 0, 1, 0, 0)`.
pub fn normal(v: &str) -> String {
    colour(v).unwrap_or_else(|| {
        let spaced = v.split_whitespace().collect::<Vec<_>>().join(" ");
        spaced.split(',').map(str::trim).collect::<Vec<_>>().join(", ").trim().to_string()
    })
}

/// Does a wanted value name `currentcolor`? Its computed value is always
/// the actual colour, so it can never be compared.
pub fn names_current_colour(v: &str) -> bool {
    v.to_ascii_lowercase().contains("currentcolor")
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
