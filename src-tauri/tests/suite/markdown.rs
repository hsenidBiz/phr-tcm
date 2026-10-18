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

// ---------------------------------------------------------------------------
// Test case text: the review page shows it as the dialect of case_markdown.rs
// ---------------------------------------------------------------------------

use v2_lib::case_markdown::Field;
use v2_lib::markdown::case_text_to_html;

#[test]
fn case_text_shows_bold_italic_lists_and_code() {
    // A step is lines: no paragraph around it.
    let html = case_text_to_html("Click **Save** and *close*", Field::Step);
    assert_eq!(html, "Click <strong>Save</strong> and <em>close</em>");

    let html = case_text_to_html("Run:\n```sql\nSELECT 1\n```", Field::Step);
    assert_eq!(html, "Run:<pre><code>SELECT 1\n</code></pre>");

    // A step has no lists: each item is a line, as Azure DevOps shows it.
    let html = case_text_to_html("Do:\n- one\n- two with `x`\n\n1. first\n2) second", Field::Step);
    assert_eq!(
        html,
        "Do:<br>• one<br>• two with <code>x</code><br><br>1. first<br>2) second"
    );

    // The prerequisites are paragraphs and real lists.
    let html = case_text_to_html("Needs:\n\n- one\n- two with `x`", Field::Prereq);
    assert_eq!(html, "<p>Needs:</p>\n<ul><li>one</li><li>two with <code>x</code></li></ul>\n");
    let html = case_text_to_html("```\nSELECT 1\n```", Field::Prereq);
    assert_eq!(html, "<pre><code>SELECT 1\n</code></pre>\n");
}

#[test]
fn a_single_newline_is_a_line_break_as_in_azure_devops() {
    assert_eq!(case_text_to_html("first\nsecond", Field::Step), "first<br>second");
    assert_eq!(case_text_to_html("first\nsecond", Field::Prereq), "<p>first<br>second</p>\n");
}

#[test]
fn case_text_is_plain_commonmark_so_old_text_never_renders_differently() {
    for field in [Field::Step, Field::Prereq] {
        let html = case_text_to_html("Status ~~old~~", field);
        assert!(html.contains("~~old~~") && !html.contains("<del"), "{html}");
        let html = case_text_to_html("a | b\n--- | ---\nc | d", field);
        assert!(!html.contains("<table"), "{html}");
        let html = case_text_to_html("- [ ] x", field);
        assert!(html.contains("[ ] x") && !html.contains("<input"), "{html}");
        let html = case_text_to_html("SELECT COUNT(*) FROM t WHERE a * b", field);
        assert!(!html.contains("<em>") && !html.contains("<strong>"), "{html}");
    }
}

#[test]
fn raw_html_in_case_text_is_never_an_element() {
    for field in [Field::Step, Field::Prereq] {
        let html = case_text_to_html(
            "<img src=x onerror=alert(1)>\n\nenter <cycleId> and <b>bold</b>\n\n<script>alert(2)</script>",
            field,
        );
        for bad in ["<img", "<script", "<b>", "<cycleId>", "onerror"] {
            assert!(!html.contains(bad), "{bad} must not be an element: {html}");
        }
        // A typed placeholder stays visible; an editor's tag leaves its text.
        assert!(html.contains("enter &lt;cycleId&gt; and bold"), "{html}");
    }
}

#[test]
fn headings_links_images_quotes_and_rules_are_their_plain_text() {
    for field in [Field::Step, Field::Prereq] {
        let html = case_text_to_html(
            "# Title\n\nsee [the spec](http://example.invalid/x) ![shot](http://example.invalid/i.png)\n\n> quoted\n\n---",
            field,
        );
        for bad in ["<h1", "<a ", "<img", "<blockquote", "<hr"] {
            assert!(!html.contains(bad), "{bad}: {html}");
        }
        for text in ["Title", "the spec", "shot", "quoted"] {
            assert!(html.contains(text), "{text}: {html}");
        }
    }
}

#[test]
fn a_step_keeps_every_blank_line_and_the_prerequisites_make_one_gap() {
    // One <br> per line break, so a blank line is two and three stay three.
    assert_eq!(case_text_to_html("a\n\nb", Field::Step), "a<br><br>b");
    assert_eq!(case_text_to_html("a\n\n\n\nb", Field::Step), "a<br><br><br><br>b");
    // Text, then a block: the block ends the line itself.
    assert_eq!(case_text_to_html("a\n```\nx\n```\nb", Field::Step), "a<pre><code>x\n</code></pre>b");
    assert_eq!(
        case_text_to_html("a\n\n```\nx\n```\n\nb", Field::Step),
        "a<br><br><pre><code>x\n</code></pre><br>b"
    );
    // The prerequisites are paragraphs: one gap however many blank lines.
    assert_eq!(case_text_to_html("a\n\n\n\nb", Field::Prereq), "<p>a</p>\n<p>b</p>\n");
}

#[test]
fn empty_case_text_renders_nothing() {
    for field in [Field::Step, Field::Prereq] {
        assert_eq!(case_text_to_html("", field), "");
        assert_eq!(case_text_to_html("  \n\n ", field), "");
    }
}

/// The review page must read the same as the app's own screens. These are
/// `renderCaseMarkdown(md, "step")`'s outputs from src/lib/markdown.ts for the
/// same inputs (a block's trailing newline is whitespace in HTML and left out).
#[test]
fn a_step_has_the_same_line_breaks_as_the_apps_own_rendering() {
    let cases: &[(&str, &str)] = &[
        ("a\n\nb", "a<br><br>b"),
        ("a\n\n\n\nb", "a<br><br><br><br>b"),
        // Not blank lines: a rule, a link definition, a tag-only HTML block.
        ("a\n\n---\nb", "a<br><br>b"),
        ("a\n\n---\n\nb", "a<br><br><br>b"),
        ("a\n---\nb", "a<br>b"),
        ("a\n\n\n---\n\n\nb", "a<br><br><br><br><br>b"),
        ("a\n\n[x]: http://e.com\nb", "a<br><br>b"),
        ("a\n\n[x]: http://e.com\n\nb", "a<br><br><br>b"),
        ("a\n\n<div></div>\n\nb", "a<br><br><br>b"),
        // Quotes and lists.
        ("> a\n>\n> b", "a<br><br>b"),
        ("> 1) x\n> 2) y", "1) x<br>2) y"),
        ("1) Login\n2) Open Leave", "1) Login<br>2) Open Leave"),
        ("3. a\n4. b", "3. a<br>4. b"),
        ("1) a\n2) b\n\n3. c", "1) a<br>2) b<br><br>3. c"),
        ("- a\n- **b**", "\u{2022} a<br>\u{2022} <strong>b</strong>"),
        ("Do:\n- a\n- b", "Do:<br>\u{2022} a<br>\u{2022} b"),
        ("- a\n\n- b", "\u{2022} a<br><br>\u{2022} b"),
        ("- a\n  - b", "\u{2022} a<br>\u{2022} b"),
        ("- a\n\n  b\n- c", "\u{2022} a<br><br>b<br>\u{2022} c"),
        ("x\n\n- a\n\n\n- b", "x<br><br>\u{2022} a<br><br><br>\u{2022} b"),
        ("- [ ] x", "\u{2022} [ ] x"),
        // A block ends its own line.
        ("a\n```\nx\n```\nb", "a<pre><code>x\n</code></pre>b"),
        ("a\n\n```\nx\n```\n\nb", "a<br><br><pre><code>x\n</code></pre><br>b"),
    ];
    for (md, want) in cases {
        assert_eq!(case_text_to_html(md, Field::Step), *want, "{md:?}");
    }
}

#[test]
fn a_prerequisites_list_typed_with_a_paren_keeps_it() {
    assert_eq!(
        case_text_to_html("1) a\n2) b", Field::Prereq),
        "<ol class=\"paren\"><li>a</li><li>b</li></ol>\n"
    );
    assert_eq!(
        case_text_to_html("3) a\n4) b", Field::Prereq),
        "<ol start=\"3\" class=\"paren\"><li>a</li><li>b</li></ol>\n"
    );
    assert_eq!(case_text_to_html("2. a", Field::Prereq), "<ol start=\"2\"><li>a</li></ol>\n");
    assert_eq!(case_text_to_html("1. a", Field::Prereq), "<ol><li>a</li></ol>\n");
    assert!(case_text_to_html("> 1) x", Field::Prereq).contains("<ol class=\"paren\">"));
}
