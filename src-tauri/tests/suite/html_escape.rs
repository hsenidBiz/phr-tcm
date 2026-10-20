//! The shared HTML escaper's output, pinned. Every report, page and step
//! fragment escapes through it, so a change here changes all of them.

use v2_lib::html_escape::{esc, esc_attr};

/// The single-pass escaper `esc_attr` replaced (the Auto Run report's own
/// copy): every character mapped once, `'` included.
fn single_pass(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn corpus() -> Vec<String> {
    let mut all: Vec<String> = (0u8..0x80).map(|b| (b as char).to_string()).collect();
    all.push((0u8..0x80).map(|b| b as char).collect());
    all.extend(
        [
            "",
            "plain text",
            "a&b<c>d\"e'f",
            "&amp; already escaped &lt;",
            "<script>alert('x')</script>",
            "caf\u{e9} \u{2022} \u{1f600} & <\u{a0}>",
            "&&<<>>\"\"''",
        ]
        .map(String::from),
    );
    all
}

#[test]
fn esc_escapes_the_four_content_characters_and_leaves_the_single_quote() {
    assert_eq!(esc("a&b<c>d\"e'f"), "a&amp;b&lt;c&gt;d&quot;e'f");
    assert_eq!(esc("&amp;"), "&amp;amp;", "escapes what it is given, every time");
    assert_eq!(esc("caf\u{e9}"), "caf\u{e9}");
    assert_eq!(esc(""), "");
}

#[test]
fn esc_attr_also_escapes_the_single_quote() {
    assert_eq!(esc_attr("a&b<c>d\"e'f"), "a&amp;b&lt;c&gt;d&quot;e&#39;f");
    assert_eq!(esc_attr("'&'"), "&#39;&amp;&#39;", "the quote's own entity is not re-escaped");
}

#[test]
fn esc_attr_matches_the_single_pass_escaper_it_replaced() {
    for text in corpus() {
        assert_eq!(esc_attr(&text), single_pass(&text), "{text:?}");
        assert_eq!(esc(&text), single_pass(&text).replace("&#39;", "'"), "{text:?}");
    }
}
