//! Reviewer-note markdown rendering: the useful subset renders, and nothing
//! in the source can become live HTML or a live link.

use v2_lib::markdown::to_html;

#[test]
fn renders_the_things_a_review_note_is_made_of() {
    let html = to_html(
        "## Spec link\n\nSee [AC 3](https://example.invalid/spec#ac3).\n\n\
         - covers **login**\n- covers `POST /session`\n\n\
         | Req | Case |\n|---|---|\n| AC1 | yes |\n",
    );
    assert!(html.contains("<h5>Spec link</h5>"), "headings shift under the page's own: {html}");
    assert!(html.contains(r#"<a href="https://example.invalid/spec#ac3""#));
    assert!(html.contains("rel=\"noreferrer noopener\""));
    assert!(html.contains("<strong>login</strong>"));
    assert!(html.contains("<code>POST /session</code>"));
    assert!(html.contains("<table>") && html.contains("<td>AC1</td>"));
}

/// The reason this module exists rather than a string filter.
#[test]
fn html_in_the_source_cannot_become_html_in_the_output() {
    let html = to_html(
        "<img src=x onerror=\"alert(1)\">\n\nliteral <b>bold</b> inline\n\n\
         <script>alert(2)</script>\n",
    );
    assert!(!html.contains("<img"), "{html}");
    assert!(!html.contains("<script"), "{html}");
    assert!(!html.contains("onerror"), "{html}");
    assert!(!html.contains("<b>"), "an inline tag is not markdown either: {html}");
    // The words survive; only the markup is gone.
    assert!(html.contains("literal"));
}

#[test]
fn text_is_escaped_so_it_cannot_close_a_tag() {
    let html = to_html("a < b && c > d, \"quoted\"");
    assert!(html.contains("a &lt; b &amp;&amp; c &gt; d"), "{html}");
}

/// Every `href="..."` value the output actually emits.
///
/// Asserting on the emitted attribute rather than on a substring of the
/// whole document is the point: `!html.contains("javascript:")` is
/// satisfied by `java\tscript:`, which is a live link.
fn hrefs(html: &str) -> Vec<String> {
    html.match_indices("href=\"")
        .map(|(i, m)| {
            let rest = &html[i + m.len()..];
            rest[..rest.find('"').expect("unterminated href")].to_string()
        })
        .collect()
}

#[test]
fn only_inert_link_schemes_survive() {
    for (src, expect_link) in [
        ("[x](https://ok.invalid)", true),
        ("[x](http://ok.invalid)", true),
        ("[x](mailto:a@b.invalid)", true),
        ("[x](./relative/page.md)", true),
        ("[x](#anchor)", true),
        ("[x](javascript:alert(1))", false),
        ("[x](JaVaScRiPt:alert(1))", false),
        ("[x](data:text/html;base64,PHNjcmlwdD4=)", false),
        ("[x](vbscript:msgbox)", false),
        ("[x](file:///C:/Windows/System32/)", false),
        ("[x](search-ms:query=secrets)", false),
    ] {
        let html = to_html(src);
        assert_eq!(hrefs(&html).is_empty(), !expect_link, "{src} produced {html}");
    }
}

/// The bug this test exists for. A URL parser strips ASCII tab, CR and
/// LF from an href *before* resolving the scheme, and markdown will
/// decode `&Tab;` / `&#9;` into a link destination for you. Checking the
/// undecoded spelling and emitting it anyway made the allowlist a
/// formality for every scheme, not just `javascript:`.
#[test]
fn a_control_character_cannot_smuggle_a_scheme_past_the_allowlist() {
    for src in [
        "[x](java&Tab;script:alert(1))",
        "[x](java&NewLine;script:alert(1))",
        "[x](java&#9;script:alert(1))",
        "[x](java&#10;script:alert(1))",
        "[x](java&#13;script:alert(1))",
        "[x](&#1;javascript:alert(1))",
        "[x](da&Tab;ta:text/html;base64,PHNjcmlwdD4=)",
        "[x](fi&Tab;le:///C:/Windows/System32/)",
        "[x](sea&Tab;rch-ms:query=secrets)",
        "[x](vsc&Tab;ode:extension/x)",
    ] {
        assert!(hrefs(&to_html(src)).is_empty(), "{src} produced a live link: {}", to_html(src));
    }
}

/// A link that survives is emitted with the controls already gone, so
/// what was checked and what reaches the browser are the same string.
#[test]
fn a_surviving_href_is_the_cleaned_string() {
    let html = to_html("[x](https://ok.inva&Tab;lid/a)");
    assert_eq!(hrefs(&html), vec!["https://ok.invalid/a".to_string()]);
}

/// A refused link opened a `<span>` and closed an `</a>`, which left the
/// span open and dragged the rest of the paragraph into the
/// refused-link style. Benign input hit it too - a bare `[text]()`.
#[test]
fn a_refused_link_closes_the_element_it_opened() {
    let html = to_html("see [x](javascript:alert(1)) and then plain words");
    assert!(html.contains("<span class=\"md-badlink\">x</span>"), "{html}");
    assert!(!html.contains("</a>"), "{html}");
    assert_eq!(html.matches("<span").count(), html.matches("</span>").count(), "{html}");

    // No destination is nothing to refuse; it is not a dangerous link.
    let bare = to_html("see [spec]() here");
    assert!(!bare.contains("md-badlink"), "{bare}");
    assert!(!bare.contains("<a href="), "{bare}");
    assert!(bare.contains("spec"), "{bare}");
    assert_eq!(bare.matches("<span").count(), bare.matches("</span>").count(), "{bare}");
}

/// A requirement-to-case table is the shape a reviewer note is most
/// often written in, and its header row has to look like one.
#[test]
fn table_header_cells_are_th() {
    let html = to_html("| Req | Case |\n|---|---|\n| AC1 | yes |\n");
    assert!(html.contains("<thead><tr><th>Req</th><th>Case</th></tr></thead>"), "{html}");
    assert!(html.contains("<tbody><tr><td>AC1</td>"), "{html}");
    assert!(html.contains("</tbody></table>"), "{html}");
}

/// A remote image in a page opened from a temp file is a network call
/// nobody asked for. The alt text stays.
#[test]
fn images_are_not_fetched() {
    let html = to_html("![a diagram](https://tracker.invalid/pixel.png)");
    assert!(!html.contains("<img"), "{html}");
    assert!(html.contains("a diagram"), "{html}");
}

#[test]
fn empty_input_renders_nothing_at_all() {
    assert_eq!(to_html(""), "");
    assert_eq!(to_html("   \n  \n"), "");
}

#[test]
fn a_code_block_keeps_its_line_breaks_and_escapes_its_contents() {
    let html = to_html("```\nif (a < b) {\n  go();\n}\n```");
    assert!(html.contains("<pre><code>"));
    assert!(html.contains("if (a &lt; b) {\n"), "{html}");
}
