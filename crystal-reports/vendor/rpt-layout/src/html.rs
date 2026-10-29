//! Crystal's **HTML text interpretation** — the `TextFormat::HTMLText` string format.
//!
//! A field whose string format says HTML renders its resolved value as markup, not literal
//! text: line breaks where `<br>`/`<p>` sit, entities decoded, document wrappers and unknown
//! tags dropped, and the active style at each line's start applied to that line (the layout
//! line model carries one font per line, so a mid-line style change applies from the next
//! line — the corpus's authored pattern, a `<br>` before every style change, renders
//! exactly). Crystal's own interpretation covers a similar subset; tags it does not know are
//! ignored rather than printed, which is what an author relying on the format sees in the
//! designer.
//!
//! The subset handled here: `<b>`/`<strong>`, `<i>`/`<em>`, `<u>`, `<s>`/`<strike>`/
//! `<del>`, `<br>`, `<p>`, `<span style="font-size:Xpt; …">`, `<font size=… face=…>`, and
//! the entities `&amp; &lt; &gt; &quot; &apos; &nbsp; &#39; &#NN; &#xHH;`. Whole-document
//! wrappers (`<html>`, `<head>` with everything inside it, `<meta>`, `<body>`) are
//! structural and invisible. Anything else is dropped, its text kept.

use rpt_pages::FontSpec;

/// One interpreted line: the display text and the font the active styles resolve to at the
/// line's first character.
pub(crate) struct HtmlLine {
    pub(crate) text: String,
    pub(crate) font: FontSpec,
}

/// Interpret `html` into styled lines over `base`. Always returns at least one line (an
/// empty input is one empty line), matching the plain-text path's invariant.
pub(crate) fn interpret_lines(html: &str, base: &FontSpec) -> Vec<HtmlLine> {
    let mut out: Vec<HtmlLine> = Vec::new();
    let mut cur = String::new();
    let mut cur_font = base.clone();

    // Style state (a stack per property, so nested same-property tags close correctly).
    let mut bold = Vec::new();
    let mut italic = Vec::new();
    let mut underline = Vec::new();
    let mut strike = Vec::new();
    let mut size: Vec<f32> = Vec::new();
    let mut family: Vec<String> = Vec::new();
    // Blocks whose entire content is invisible.
    let mut skip_until: Option<&str> = None;

    let snapshot = |bold: &[bool], italic: &[bool], underline: &[bool], strike: &[bool],
                    size: &[f32], family: &[String], base: &FontSpec|
     -> FontSpec {
        FontSpec {
            family: family.last().cloned().unwrap_or_else(|| base.family.clone()),
            size_pt: size.last().copied().unwrap_or(base.size_pt),
            bold: *bold.last().unwrap_or(&false) || base.bold,
            italic: *italic.last().unwrap_or(&false) || base.italic,
            underline: *underline.last().unwrap_or(&false) || base.underline,
            strikethrough: *strike.last().unwrap_or(&false) || base.strikethrough,
        }
    };

    // The font a line renders in is the style active at its FIRST character — captured when
    // the line opens (at the previous break, or the base font for line one), not at the
    // flush that closes it.
    let mut line_font = base.clone();
    let flush = |out: &mut Vec<HtmlLine>, cur: &mut String, line_font: &mut FontSpec, next: &FontSpec| {
        out.push(HtmlLine { text: std::mem::take(cur), font: line_font.clone() });
        *line_font = next.clone();
    };

    let bytes = html.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let rest = &html[i..];
        if let Some(tag_end) = find_tag(rest) {
            let (tag, after) = tag_end;
            let lower = tag.to_ascii_lowercase();
            // Inside an invisible block: only its closing tag matters.
            if let Some(until) = skip_until {
                if lower.starts_with(until) {
                    skip_until = None;
                }
                i += after;
                continue;
            }
            // `lower` drives tag-NAME matching; attributes keep the original case, so the
            // open text is re-derived from `tag` for value extraction.
            let open_orig = tag
                .strip_prefix('<')
                .filter(|t| !t.starts_with('/'))
                .map(|t| t.trim_end_matches('>').trim_end_matches('/').trim().to_string());
            if let Some(open) = lower.strip_prefix('<').filter(|t| !t.starts_with('/')) {
                let open = open.trim_end_matches('>').trim_end_matches('/').trim();
                match open {
                    "head" | "style" | "script" | "title" => {
                        skip_until = Some(match open {
                            "head" => "</head",
                            "style" => "</style",
                            "script" => "</script",
                            _ => "</title",
                        });
                    }
                    "br" => flush(&mut out, &mut cur, &mut line_font, &cur_font),
                    "p" | "p " => {
                        if !cur.is_empty() {
                            flush(&mut out, &mut cur, &mut line_font, &cur_font);
                        }
                    }
                    "b" | "strong" => bold.push(true),
                    "i" | "em" => italic.push(true),
                    "u" => underline.push(true),
                    "s" | "strike" | "del" => strike.push(true),
                    _ => {
                        if open.strip_prefix("span").is_some_and(|r| r.is_empty() || r.starts_with(' ')) {
                            let attrs = open_orig.as_deref().and_then(|t| t.strip_prefix("span")).unwrap_or("");
                            if let Some(sz) = style_attr_font_size(attrs) {
                                size.push(sz);
                            }
                        } else if open.strip_prefix("font").is_some_and(|r| r.is_empty() || r.starts_with(' ')) {
                            let attrs = open_orig.as_deref().and_then(|t| t.strip_prefix("font")).unwrap_or("");
                            if let Some(sz) = attr_value(attrs, "size").and_then(|v| parse_html_size(&v)) {
                                size.push(sz);
                            }
                            if let Some(face) = attr_value(attrs, "face") {
                                family.push(face);
                            }
                        }
                        // Everything else (html, body, meta, div, a, table, …) is structural:
                        // ignored, its text rendered.
                    }
                }
            } else if let Some(close) = lower.strip_prefix("</") {
                let close = close.trim_end_matches('>').trim();
                match close {
                    "p" => {
                        if !cur.is_empty() {
                            flush(&mut out, &mut cur, &mut line_font, &cur_font);
                        }
                    }
                    "b" | "strong" => {
                        bold.pop();
                    }
                    "i" | "em" => {
                        italic.pop();
                    }
                    "u" => {
                        underline.pop();
                    }
                    "s" | "strike" | "del" => {
                        strike.pop();
                    }
                    "span" => {
                        size.pop();
                        family.pop();
                    }
                    "font" => {
                        size.pop();
                        family.pop();
                    }
                    _ => {}
                }
            }
            cur_font = snapshot(&bold, &italic, &underline, &strike, &size, &family, base);
            i += after;
        } else if rest.starts_with('&') {
            let (decoded, len) = decode_entity(rest);
            match decoded {
                Some(ch) => {
                    if cur.is_empty() {
                        line_font = cur_font.clone();
                    }
                    cur.push(ch);
                    i += len;
                }
                None => {
                    // A bare `&` with no entity behind it stays literal.
                    cur.push('&');
                    i += 1;
                }
            }
        } else {
            let ch = rest.chars().next().unwrap();
            // The line's first character: any style opened between the break and this
            // character is the line's style (a leading `<b>`/`<span>` before text).
            if cur.is_empty() {
                line_font = cur_font.clone();
            }
            cur.push(ch);
            i += ch.len_utf8();
        }
    }
    flush(&mut out, &mut cur, &mut line_font, &cur_font);
    if out.is_empty() {
        out.push(HtmlLine { text: String::new(), font: base.clone() });
    }
    out
}

/// The next `<tag>` (or `</tag>`) at the START of `s`: its full text and the offset just
/// past it.
fn find_tag(s: &str) -> Option<(String, usize)> {
    if !s.starts_with('<') {
        return None;
    }
    let end = s.find('>')?;
    // A `<` with no `>` for a long stretch is literal text (a formula may print math).
    if end > 512 {
        return None;
    }
    Some((s[..=end].to_string(), end + 1))
}

/// `style="font-size:5.5pt"` (also `'…'`, unquoted) from a tag's attribute text.
fn style_attr_font_size(attrs: &str) -> Option<f32> {
    let lower = attrs.to_ascii_lowercase();
    let at = lower.find("font-size")?;
    let rest = &attrs[at + "font-size".len()..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let end = rest
        .find(|c: char| c == ';' || c == '"' || c == '\'' || c.is_whitespace())
        .unwrap_or(rest.len());
    parse_css_size(&rest[..end])
}

/// An attribute's value (`face="Arial"`, `size=3`) from a tag's attribute text.
fn attr_value<'a>(attrs: &'a str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let at = lower.find(name)?;
    let rest = attrs[at + name.len()..].trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let quote = rest.chars().next()?;
    if quote == '"' || quote == '\'' {
        let end = rest[1..].find(quote)? + 1;
        Some(rest[1..end].to_string())
    } else {
        let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
}

/// CSS size: `5.5pt` (points), `12px` (0.75pt/px), or a bare number (points).
fn parse_css_size(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(v) = s.strip_suffix("pt") {
        v.trim().parse().ok()
    } else if let Some(v) = s.strip_suffix("px") {
        Some(v.trim().parse::<f32>().ok()? * 0.75)
    } else {
        s.parse().ok()
    }
}

/// HTML's legacy `<font size=…>` scale (1–7) — the engine maps it to the 8–36pt ladder.
fn parse_html_size(s: &str) -> Option<f32> {
    let n: i32 = s.trim().parse().ok()?;
    Some(match n {
        1 => 8.0,
        2 => 10.0,
        3 => 12.0,
        4 => 14.0,
        5 => 18.0,
        6 => 24.0,
        7 => 36.0,
        _ => return None,
    })
}

/// The entity at the START of `s`: its character and consumed length.
fn decode_entity(s: &str) -> (Option<char>, usize) {
    let Some(semi) = s[..s.len().min(12)].find(';') else {
        return (None, 0);
    };
    let name = &s[1..semi];
    let ch = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        _ => {
            if let Some(num) = name.strip_prefix('#') {
                let cp = if let Some(hex) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
                    u32::from_str_radix(hex, 16).ok()
                } else {
                    num.parse::<u32>().ok()
                };
                return (cp.and_then(char::from_u32), semi + 1);
            }
            return (None, 0);
        }
    };
    (Some(ch), semi + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> FontSpec {
        FontSpec { family: "Arial".into(), size_pt: 10.0, bold: false, italic: false, underline: false, strikethrough: false }
    }

    fn lines(html: &str) -> Vec<(String, f32, bool)> {
        interpret_lines(html, &base())
            .into_iter()
            .map(|l| (l.text, l.font.size_pt, l.font.bold))
            .collect()
    }

    /// The corpus's authored shape: a full document wrapper around content, a `<br>` then a
    /// small-font non-breaking-space spacer before `</body></html>`.
    #[test]
    fn document_wrapper_and_spacer() {
        let html = "<html><head><meta charset=\"utf-8\"></head><body>Stage One<br><span style='font-size:5.5pt'>&nbsp;</span></body></html>";
        assert_eq!(
            lines(html),
            vec![("Stage One".to_string(), 10.0, false), ("\u{a0}".to_string(), 5.5, false)]
        );
    }

    #[test]
    fn entities_decode() {
        assert_eq!(lines("a &amp; b &lt;c&gt; &#65; &nbsp;")[0].0, "a & b <c> A \u{a0}");
    }

    #[test]
    fn breaks_and_bold() {
        assert_eq!(
            lines("one<br><b>two</b><br>three"),
            vec![
                ("one".to_string(), 10.0, false),
                ("two".to_string(), 10.0, true),
                ("three".to_string(), 10.0, false),
            ]
        );
    }

    #[test]
    fn paragraphs_and_unknown_tags_dropped() {
        assert_eq!(
            lines("<p>first</p><div>second</div>"),
            vec![("first".to_string(), 10.0, false), ("second".to_string(), 10.0, false)]
        );
    }

    #[test]
    fn bare_lt_stays_literal() {
        assert_eq!(lines("5 < 6")[0].0, "5 < 6");
    }

    #[test]
    fn font_tag_family_and_size() {
        let l = interpret_lines("<font face=\"Tahoma\" size=\"3\">x</font>", &base());
        assert_eq!(l[0].font.family, "Tahoma");
        assert_eq!(l[0].font.size_pt, 12.0);
    }
}
