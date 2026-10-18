//! Test case text: Markdown inside TCM, the HTML each Azure DevOps field
//! supports outside. The rows of the spec's table, both fields; the plain
//! text that has to come back exactly as written; and table-driven
//! stability checks over many inputs in both directions.

use v2_lib::case_markdown::{
    prereq_html_to_md, prereq_md_to_html, step_html_to_md, step_md_to_html, unchanged,
};

/// Both fields, so a table can run over each.
#[derive(Clone, Copy, Debug)]
enum Field {
    Step,
    Prereq,
}

const FIELDS: [Field; 2] = [Field::Step, Field::Prereq];

fn to_html(f: Field, md: &str) -> String {
    match f {
        Field::Step => step_md_to_html(md),
        Field::Prereq => prereq_md_to_html(md),
    }
}

fn to_md(f: Field, html: &str) -> String {
    match f {
        Field::Step => step_html_to_md(html),
        Field::Prereq => prereq_html_to_md(html),
    }
}

/// What a plain text is in each field's HTML: escaped, and nothing else.
fn plain_html(f: Field, text: &str) -> String {
    let esc = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    match f {
        Field::Step => esc.replace('\n', "<BR/>"),
        // A blank line parts paragraphs; a single line break stays in one.
        Field::Prereq => esc
            .split("\n\n")
            .map(|p| format!("<p>{}</p>", p.replace('\n', "<br>")))
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// The spec's table, row by row
// ---------------------------------------------------------------------------

#[test]
fn step_rows_of_the_spec_table() {
    // (markdown, the step HTML, the markdown it reads back as)
    let rows: &[(&str, &str, &str)] = &[
        ("**bold**", "<B>bold</B>", "**bold**"),
        ("Click **Save** now", "Click <B>Save</B> now", "Click **Save** now"),
        ("*italic*", "<I>italic</I>", "*italic*"),
        ("_italic_", "<I>italic</I>", "*italic*"),
        ("***both***", "<B><I>both</I></B>", "***both***"),
        ("line one\nline two", "line one<BR/>line two", "line one\nline two"),
        ("para one\n\npara two", "para one<BR/><BR/>para two", "para one\n\npara two"),
        ("- one\n- two", "\u{2022} one<BR/>\u{2022} two", "\u{2022} one\n\u{2022} two"),
        ("* one\n* two", "\u{2022} one<BR/>\u{2022} two", "\u{2022} one\n\u{2022} two"),
        ("1. one\n2. two", "1. one<BR/>2. two", "1. one\n2. two"),
        ("Use `npm test`", "Use npm test", "Use npm test"),
        (
            "Run the following query.\n```sql\nSELECT 1\nFROM t\n```",
            "Run the following query.<BR/>SELECT 1<BR/>FROM t",
            "Run the following query.\nSELECT 1\nFROM t",
        ),
        ("# Heading", "Heading", "Heading"),
        ("See [the spec](https://example.invalid/x)", "See the spec", "See the spec"),
        ("![a diagram](x.png)", "a diagram", "a diagram"),
        ("> quoted", "quoted", "quoted"),
        ("<b>raw</b> html", "raw html", "raw html"),
        ("a | b\n--|--\n1 | 2", "a | b<BR/>--|--<BR/>1 | 2", "a | b\n--|--\n1 | 2"),
    ];
    for (md, html, back) in rows {
        let got = step_md_to_html(md);
        assert_eq!(&got, html, "step html for {md:?}");
        assert_eq!(&step_html_to_md(&got), back, "step markdown back for {md:?}");
    }
}

#[test]
fn prereq_rows_of_the_spec_table() {
    let rows: &[(&str, &str, &str)] = &[
        ("**bold**", "<p><strong>bold</strong></p>", "**bold**"),
        ("*italic*", "<p><em>italic</em></p>", "*italic*"),
        ("_italic_", "<p><em>italic</em></p>", "*italic*"),
        ("line one\nline two", "<p>line one<br>line two</p>", "line one\nline two"),
        ("para one\n\npara two", "<p>para one</p><p>para two</p>", "para one\n\npara two"),
        ("- one\n- two", "<ul><li>one</li><li>two</li></ul>", "- one\n- two"),
        ("1. one\n2. two", "<ol><li>one</li><li>two</li></ol>", "1. one\n2. two"),
        ("3. three\n4. four", "<ol start=\"3\"><li>three</li><li>four</li></ol>", "3. three\n4. four"),
        (
            "Accounts:\n- E1\n- E2",
            "<p>Accounts:</p><ul><li>E1</li><li>E2</li></ul>",
            "Accounts:\n- E1\n- E2",
        ),
        ("Use `npm test`", "<p>Use <code>npm test</code></p>", "Use `npm test`"),
        (
            "```sql\nSELECT 1\nFROM t\n```",
            "<pre><code>SELECT 1\nFROM t</code></pre>",
            "```\nSELECT 1\nFROM t\n```",
        ),
        ("# Heading", "<p>Heading</p>", "Heading"),
        ("See [the spec](https://example.invalid/x)", "<p>See the spec</p>", "See the spec"),
        ("![a diagram](x.png)", "<p>a diagram</p>", "a diagram"),
        ("> quoted", "<p>quoted</p>", "quoted"),
        ("<b>raw</b> html", "<p>raw html</p>", "raw html"),
        (
            "a | b\n--|--\n1 | 2",
            "<p>a | b<br>--|--<br>1 | 2</p>",
            "a | b\n--|--\n1 | 2",
        ),
    ];
    for (md, html, back) in rows {
        let got = prereq_md_to_html(md);
        assert_eq!(&got, html, "prerequisites html for {md:?}");
        assert_eq!(&prereq_html_to_md(&got), back, "prerequisites markdown back for {md:?}");
    }
}

// ---------------------------------------------------------------------------
// Review Focus 1
// ---------------------------------------------------------------------------

#[test]
fn count_star_is_never_italic() {
    let texts = [
        "Run the following query. SELECT COUNT(*) FROM t WHERE a * b > 1",
        "Run the following query. SELECT COUNT(*) AS graded_items FROM perf_rating_method_item \
         WHERE rating_method_id = {ratingMethodEmpty} AND marks IS NOT NULL;",
    ];
    for f in FIELDS {
        for t in texts {
            let html = to_html(f, t);
            assert_eq!(html, plain_html(f, t), "{f:?}: {t:?} is plain text");
            for tag in ["<I>", "<i>", "<em>", "<EM>"] {
                assert!(!html.contains(tag), "{f:?}: italic in {html}");
            }
            assert_eq!(to_md(f, &html), t, "{f:?}: reads back exactly");
            // Read from Azure DevOps as plain text, it stays plain.
            assert_eq!(to_md(f, &plain_html(f, t)), t, "{f:?}: ADO plain text reads back exactly");
        }
    }
}

#[test]
fn two_count_stars_from_ado_are_escaped_not_italicised() {
    // Two `(*)` on one line WOULD pair up as emphasis, so reading this plain
    // text from Azure DevOps has to protect it - and the HTML it then writes
    // is the same plain text, never italics.
    let t = "SELECT COUNT(*) FROM a WHERE n = (SELECT COUNT(*) FROM b)";
    for f in FIELDS {
        let md = to_md(f, &plain_html(f, t));
        assert_ne!(md, t, "{f:?}: unescaped it would read as italics");
        assert!(md.contains("\\*"), "{f:?}: {md}");
        assert_eq!(to_html(f, &md), plain_html(f, t), "{f:?}: written back as the same plain text");
        // And not more than needed: the parentheses stay as they are.
        assert!(!md.contains("\\("), "{f:?}: {md}");
    }
}

// ---------------------------------------------------------------------------
// Fences, symbols, ADO's own HTML
// ---------------------------------------------------------------------------

#[test]
fn a_fence_in_a_step_becomes_plain_lines() {
    let md = "Run the following query.\n\n```sql\nSELECT COUNT(*) AS n\nFROM t\nWHERE a = 1;\n```\n\nOne row returns.";
    let html = step_md_to_html(md);
    assert_eq!(
        html,
        "Run the following query.<BR/><BR/>SELECT COUNT(*) AS n<BR/>FROM t<BR/>WHERE a = 1;<BR/><BR/>One row returns."
    );
    assert!(!html.contains("```"));
    assert!(!html.contains("sql"), "the info string is not text: {html}");
    // A blank line inside the fence is kept as a blank line.
    assert_eq!(step_md_to_html("```\na\n\nb\n```"), "a<BR/><BR/>b");
    // Escaped text in a fence stays escaped.
    assert_eq!(step_md_to_html("```\nWHERE a < b & c\n```"), "WHERE a &lt; b &amp; c");
    // Read back: plain lines, not re-fenced.
    let back = step_html_to_md(&html);
    assert!(!back.contains("```"), "{back}");
    assert_eq!(step_md_to_html(&back), html);
}

#[test]
fn markdown_symbols_never_reach_ado() {
    let inputs = [
        "**bold** and __bold__",
        "*it* and _it_",
        "***both***",
        "# Heading\n\ntext",
        "## Sub heading",
        "Heading\n===",
        "- one\n- two\n  - nested",
        "* star item",
        "+ plus item",
        "1. one\n2. two",
        "`code` here",
        "``double-tick code``",
        "```sql\nSELECT 1\n```",
        "~~~\nSELECT 1\n~~~",
        "> quote",
        "[link](https://example.invalid)",
        "![img](x.png)",
        "<https://example.invalid>",
        "a\n\n---\n\nb",
        "Step:\n- **Save**\n- `Cancel`",
    ];
    for md in inputs {
        let step = step_md_to_html(md);
        for sym in ["**", "__", "`", "#", "~~~", "](", "!["] {
            assert!(!step.contains(sym), "step html for {md:?} holds {sym:?}: {step}");
        }
        for line in step.split("<BR/>") {
            for lead in ["- ", "* ", "+ ", "> ", "&gt; "] {
                assert!(!line.starts_with(lead), "step line {line:?} from {md:?}");
            }
        }
        assert!(!step.contains("<p>") && !step.contains("<em>") && !step.contains("<strong>"));

        let pre = prereq_md_to_html(md);
        for sym in ["**", "__", "`", "#", "~~~", "](", "!["] {
            assert!(!pre.contains(sym), "prerequisites html for {md:?} holds {sym:?}: {pre}");
        }
        for line in pre.split(['>', '\n']) {
            for lead in ["- ", "* ", "+ "] {
                assert!(!line.starts_with(lead), "prerequisites {pre} from {md:?}");
            }
        }
    }
}

#[test]
fn the_step_html_uses_only_b_i_br() {
    let md = "Click **Save**, *then* `Close`.\n- a\n# h\n> q\n```\nx\n```";
    let html = step_md_to_html(md);
    let tags: Vec<&str> = html
        .split('<')
        .skip(1)
        .map(|s| s.split('>').next().unwrap_or(""))
        .collect();
    for t in tags {
        assert!(["B", "/B", "I", "/I", "BR/"].contains(&t), "tag <{t}> in {html}");
    }
}

#[test]
fn the_prereq_html_uses_only_the_allowed_tags() {
    let md = "Click **Save**, *then* `Close`.\n\n- a\n- b\n\n3. c\n\n# h\n\n> q\n\n```\nx\n```\nline\nbreak";
    let html = prereq_md_to_html(md);
    let allowed = [
        "p", "/p", "br", "strong", "/strong", "em", "/em", "ul", "/ul", "ol", "/ol", "li", "/li",
        "code", "/code", "pre", "/pre",
    ];
    for t in html.split('<').skip(1).map(|s| s.split('>').next().unwrap_or("")) {
        let name = t.split(' ').next().unwrap_or("");
        assert!(allowed.contains(&name), "tag <{t}> in {html}");
    }
}

#[test]
fn ado_html_with_b_strong_div_p_reads_back() {
    for f in FIELDS {
        assert_eq!(to_md(f, "<DIV><B>x</B></DIV>"), "**x**", "{f:?}");
        assert_eq!(to_md(f, "<P><STRONG>x</STRONG></P>"), "**x**", "{f:?}");
        assert_eq!(to_md(f, "<div><b>x</b></div>"), "**x**", "{f:?}");
        assert_eq!(to_md(f, "<DIV><I>x</I> and <EM>y</EM></DIV>"), "*x* and *y*", "{f:?}");
        assert_eq!(to_md(f, "<DIV>a <U>underlined</U> word</DIV>"), "a underlined word", "{f:?}");
        assert_eq!(
            to_md(f, "<DIV><SPAN style=\"color: rgb(0, 0, 0);\">plain</SPAN></DIV>"),
            "plain",
            "{f:?}"
        );
        assert_eq!(to_md(f, "a<BR/>b"), "a\nb", "{f:?}");
        assert_eq!(to_md(f, "a<br>b"), "a\nb", "{f:?}");
        assert_eq!(to_md(f, "<DIV>a</DIV><DIV>b</DIV>"), "a\nb", "{f:?}");
        assert_eq!(to_md(f, "<DIV>a</DIV><DIV><BR/></DIV><DIV>b</DIV>"), "a\n\nb", "{f:?}");
        assert_eq!(to_md(f, "<P>a</P><P>b</P>"), "a\n\nb", "{f:?}");
        assert_eq!(to_md(f, "<DIV>a<BR/></DIV><DIV>b</DIV>"), "a\nb", "{f:?}");
        assert_eq!(to_md(f, "<B>Save </B>now"), "**Save** now", "{f:?}");
        assert_eq!(to_md(f, "<B><I>both</I></B>"), "***both***", "{f:?}");
        assert_eq!(to_md(f, "<B>a<BR/>b</B>"), "**a**\n**b**", "{f:?}");
        // Overlapping, not nested: the space between takes no style, so the
        // closing and reopening fall either side of it.
        assert_eq!(
            to_md(f, "<I>note <B>this</B></I><B> now</B>"),
            "*note **this*** **now**",
            "{f:?}"
        );
        // Punctuation at a styled edge glued to a letter: `a**(x)**` would
        // not be bold, so the bracket gives its bold up.
        assert_eq!(to_md(f, "a<B>(x)</B>"), "a(**x)**", "{f:?}");
        assert_eq!(to_md(f, "<B>Save.</B>Then"), "**Save**.Then", "{f:?}");
        // Emphasis on nothing but punctuation is dropped.
        assert_eq!(to_md(f, "it<B>'</B>s"), "it's", "{f:?}");
        assert_eq!(to_md(f, "a&nbsp;b"), "a b", "{f:?}");
        assert_eq!(to_md(f, "&lt;cycleId&gt; stays"), "<cycleId> stays", "{f:?}");
        assert_eq!(to_md(f, "Tom &amp; Jerry"), "Tom & Jerry", "{f:?}");
        assert_eq!(to_md(f, "<!-- a comment -->text"), "text", "{f:?}");
        assert_eq!(to_md(f, "  "), "", "{f:?}");
        assert_eq!(to_md(f, ""), "", "{f:?}");
    }
    // Lists: a step has no list, so items read back as the bullet lines it
    // shows; the prerequisites keep the list.
    let list = "<ul><li>one</li><li>two</li></ul>";
    assert_eq!(step_html_to_md(list), "\u{2022} one\n\u{2022} two");
    assert_eq!(prereq_html_to_md(list), "- one\n- two");
    let ol = "<ol><li>one</li><li>two</li></ol>";
    assert_eq!(step_html_to_md(ol), "1. one\n2. two");
    assert_eq!(prereq_html_to_md(ol), "1. one\n2. two");
    // A pre: plain lines in a step, a fence in the prerequisites.
    let pre = "<pre><code>SELECT 1\nFROM t</code></pre>";
    assert_eq!(step_html_to_md(pre), "SELECT 1\nFROM t");
    assert_eq!(prereq_html_to_md(pre), "```\nSELECT 1\nFROM t\n```");
    assert_eq!(prereq_html_to_md("<p>Use <code>npm test</code></p>"), "Use `npm test`");
    assert_eq!(step_html_to_md("Use <code>npm test</code>"), "Use npm test");
    // TCM's own older prerequisites: escaped text inside one div, its line
    // breaks as raw newlines.
    assert_eq!(prereq_html_to_md("<div>\u{2022} a\n\u{2022} b</div>"), "\u{2022} a\n\u{2022} b");
    assert_eq!(prereq_html_to_md("<div>\u{2022} a<br>\u{2022} b</div>"), "\u{2022} a\n\u{2022} b");
}

// ---------------------------------------------------------------------------
// Plain text reads back exactly
// ---------------------------------------------------------------------------

/// Plain texts with characters Markdown cares about, used as nothing more
/// than characters. Each must be written as itself and read back as itself.
const PLAIN: &[&str] = &[
    "Click Save.",
    "SELECT COUNT(*) FROM t",
    "a * b",
    "2 * 3 = 6",
    "x = -5",
    "-5 is rejected because only a positive number is allowed",
    "a + b",
    "Click Save & Continue to open the Competencies step.",
    "computed_overall_rating is 0.7620 = ((0.7200 x 0.70) + (0.8600 x 0.30)) x 1.00",
    "perf_rating_method_item and snake_case_name",
    "The donut reads 76.20% = (0.7620 x 5) / 5 x 100",
    "WHERE performance_cycle_id = <cycleId>",
    "Move to <next assessment stage> now",
    "a < b > c",
    "start_date < GETUTCDATE()",
    "Issue #5 is open",
    "Price is $5 (approx.)",
    "Path C:\\temp\\file.txt",
    "[optional] field",
    "Wow!",
    "a ~ b | c",
    "=5 is the formula",
    "e-mail the user",
    "The value is 1.5",
    "Version 2.1.0 ships.",
    "It's \"quoted\" text",
    "E1's goal",
    "\u{2022} a \u{2022} b \u{2022} c",
    "Line one\nLine two",
    "First\n\nSecond",
    "\u{2022} a\n\u{2022} b",
    "Note - flag for review",
    "{ratingMethodEmpty}",
    "50% of 10 = 5",
    "@CycleId and @StageId",
    "under_score_ and _x",
    "f(*args)",
];

#[test]
fn plain_text_round_trips_exactly() {
    for f in FIELDS {
        for t in PLAIN {
            let html = to_html(f, t);
            assert_eq!(html, plain_html(f, t), "{f:?}: {t:?} is written as plain text");
            assert_eq!(&to_md(f, &html), t, "{f:?}: {t:?} reads back exactly");
        }
    }
}

/// Every step and prerequisites text in the repository's `.test-cases`
/// files (git-ignored real cases). Skipped, loudly, when the folder is
/// absent.
#[test]
fn real_case_texts_round_trip_exactly() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.test-cases");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("skipped: {} is absent", dir.display());
        return;
    };
    let mut steps: Vec<String> = vec![];
    let mut prereqs: Vec<String> = vec![];
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        for case in doc["test_cases"].as_array().into_iter().flatten() {
            if let Some(pre) = case["preconditions"].as_str() {
                prereqs.push(pre.to_string());
            }
            for s in case["steps"].as_array().into_iter().flatten() {
                for k in ["action", "expected"] {
                    if let Some(t) = s[k].as_str() {
                        steps.push(t.to_string());
                    }
                }
            }
        }
    }
    let steps: Vec<String> = steps.into_iter().filter(|t| !t.trim().is_empty()).collect();
    if steps.is_empty() {
        eprintln!("skipped: no step texts under {}", dir.display());
        return;
    }
    assert!(steps.len() >= 20, "only {} real step texts", steps.len());
    for t in &steps {
        let html = step_md_to_html(t);
        assert!(!html.contains("<I>") && !html.contains("<B>"), "formatting in {t:?}: {html}");
        assert_eq!(&step_html_to_md(&html), t, "step {t:?}");
        assert_eq!(&step_html_to_md(&plain_html(Field::Step, t)), t, "step from ADO {t:?}");
    }
    for t in prereqs.iter().filter(|t| !t.trim().is_empty()) {
        let html = prereq_md_to_html(t);
        assert!(!html.contains("<em>") && !html.contains("<strong>"), "formatting in {t:?}");
        assert_eq!(&prereq_html_to_md(&html), t, "prerequisites {t:?}");
    }
}

// ---------------------------------------------------------------------------
// Stability, both directions, over many inputs
// ---------------------------------------------------------------------------

const MARKDOWN: &[&str] = &[
    "**bold**",
    "*italic*",
    "***both***",
    "**bold *and italic* inside**",
    "*italic **and bold** inside*",
    "Click **Save** and *wait*.",
    "**a**b",
    "a**b**c",
    "**Save**.",
    "Type `x * y` here",
    "`a` and `b`",
    "``tick ` inside``",
    "line\nbreak",
    "hard  \nbreak",
    "back\\\nslash break",
    "p1\n\np2\n\n\np3",
    "- a\n- b\n- c",
    "- a\n\n- b",
    "- a\n  - nested\n- b",
    "- **bold item**\n- `code item`",
    "1. a\n2. b",
    "5. five\n6. six",
    "1) paren\n2) list",
    "Intro:\n- a\n- b\n\nOutro.",
    "Intro:\n1. a\n2. b",
    "```\ncode\n```",
    "```sql\nSELECT *\nFROM t\nWHERE a = '*x*';\n```",
    "Lead in.\n```\nq1\n\nq2\n```\nAfter.",
    "- item\n\n  ```\n  code in item\n  ```",
    "# H1\ntext",
    "Setext\n---\ntext",
    "> quote\n> more",
    "[a](http://x) and ![b](y)",
    "<b>raw</b> and <cycleId>",
    "<div>\nblock html\n</div>",
    "\\*literal stars\\*",
    "\\# not a heading",
    "\\- not a list",
    "1\\. not a list",
    "a\\\\b",
    "&amp; entity",
    "&copy; sign",
    "COUNT(*) and COUNT(*)",
    "x_y_z and _real_",
    "* * *",
    "a\n***\nb",
    "",
    "   ",
];

#[test]
fn markdown_round_trips_are_stable() {
    for f in FIELDS {
        for md in MARKDOWN {
            let h1 = to_html(f, md);
            let m1 = to_md(f, &h1);
            let h2 = to_html(f, &m1);
            assert_eq!(h2, h1, "{f:?}: html changed after a round trip of {md:?} (read as {m1:?})");
            let m2 = to_md(f, &h2);
            assert_eq!(m2, m1, "{f:?}: markdown of {md:?} is not a fixed point");
        }
    }
}

/// HTML as Azure DevOps (or an older TCM) holds it - including plain text
/// that only LOOKS like Markdown.
const ADO_HTML: &[&str] = &[
    "<DIV><B>Save</B> the form</DIV>",
    "<DIV><P>Open <STRONG>Settings</STRONG></P><P>then <EM>Logs</EM></P></DIV>",
    "<div>one</div><div>two</div><div><br></div><div>three</div>",
    "<B>bold <I>both</I></B><I> italic</I>",
    "<B></B>empty bold",
    "<SPAN style=\"font-weight:bold\">span</SPAN> <FONT color=red>font</FONT>",
    "*not italic*",
    "**not bold**",
    "_not italic_",
    "# not a heading",
    "## not either",
    "- not a list",
    "* not a list",
    "+ not a list",
    "1. maybe a list",
    "1) not a list",
    "7. seven",
    "&gt; not a quote",
    "```not a fence",
    "~~~ not a fence",
    "    four spaces",
    "a<br>===",
    "a<br>---",
    "---",
    "***",
    "`not code`",
    "[not](a link)",
    "[ref]: http://x",
    "![not](an image)",
    "&lt;b&gt;not bold&lt;/b&gt;",
    "&lt;https://example.invalid&gt;",
    "&amp;amp; literally",
    "&amp;copy; literally",
    "back\\slash\\",
    "trailing backslash\\<br>next",
    "a \\* b",
    "SELECT COUNT(*) FROM a WHERE n = (SELECT COUNT(*) FROM b)",
    "SELECT t.* FROM t WHERE x * 2 > y * 3",
    "x*y*z",
    "snake_case_name and _leading",
    "<ul><li>a</li><li><b>b</b></li></ul>",
    "<ul><li>a<ul><li>nested</li></ul></li><li>b</li></ul>",
    "<ol start=\"4\"><li>four</li><li>five</li></ol>",
    "<ul><li>- dash</li><li>1. one</li></ul>",
    "<p>Intro</p><ul><li>a</li></ul><p>Outro</p>",
    "<ul><li>a</li></ul><ul><li>b</li></ul>",
    "<pre>SELECT 1\n  FROM t</pre>",
    "<pre><code>```\nfence inside\n```</code></pre>",
    "<p>a <code>x * y</code> b</p>",
    "<p><code>`tick`</code></p>",
    "<table><tr><td>a</td><td>b</td></tr><tr><td>1</td><td>2</td></tr></table>",
    "<h1>Title</h1><p>body</p>",
    "<blockquote>quoted</blockquote>",
    "<a href=\"http://x\">link text</a>",
    "<img src=\"x.png\">after image",
    "text with <cycleId> raw",
    "<o:p>word paste</o:p>",
    "a&nbsp;&nbsp;b",
    "<div>&nbsp;</div><div>x</div>",
    "\u{2022} a<br>\u{2022} b",
];

#[test]
fn ado_html_round_trips_are_stable() {
    for f in FIELDS {
        for h in ADO_HTML {
            let m1 = to_md(f, h);
            let h1 = to_html(f, &m1);
            let m2 = to_md(f, &h1);
            assert_eq!(m2, m1, "{f:?}: {h:?} read as {m1:?}, written as {h1:?}, read again differently");
            assert_eq!(to_html(f, &m2), h1, "{f:?}: {h:?} html not stable");
        }
    }
}

/// The visible text of HTML this module writes: one entry per line, each
/// whitespace-collapsed, blank runs at the ends dropped (and, for the
/// prerequisites, a run of blank lines counted as one paragraph break).
fn visible(f: Field, html: &str) -> Vec<String> {
    let unesc = |s: &str| {
        s.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let raw: Vec<String> = match f {
        Field::Step => html.split("<BR/>").map(unesc).collect(),
        Field::Prereq => html
            .replace("</p><p>", "<br><br>")
            .replace("<p>", "")
            .replace("</p>", "")
            .split("<br>")
            .map(unesc)
            .collect(),
    };
    let mut out: Vec<String> = vec![];
    for l in raw {
        let blank_run = l.is_empty() && out.last().is_some_and(|p| p.is_empty());
        if matches!(f, Field::Prereq) && blank_run {
            continue;
        }
        out.push(l);
    }
    while out.first().is_some_and(|l| l.is_empty()) {
        out.remove(0);
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out
}

/// A small deterministic generator: strings built from the pieces Markdown
/// cares about, so every pairing of them gets tried.
fn generated(count: usize) -> Vec<String> {
    const PIECES: &[&str] = &[
        "*", "**", "***", "_", "__", "`", "``", "#", "# ", "- ", "+ ", "* ", "1. ", "2) ", "> ",
        "[", "]", "(", ")", "](x)", "!", "<", ">", "&", "&amp;", "&copy;", "\\", "\n", "\n\n",
        " ", "  ", "a", "word", "COUNT(*)", "<b>", "</b>", "<cycleId>", "~~~", "```", "=", "===",
        "---", "|", ".", ":", "\u{2022} ", "x_y", "<http://x>", "    ", "\t", "'", "\"",
    ];
    let mut seed: u64 = 0x5eed_1234_abcd_ef01;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };
    (0..count)
        .map(|_| {
            let n = 1 + next() % 10;
            (0..n).map(|_| PIECES[next() % PIECES.len()]).collect::<String>()
        })
        .collect()
}

#[test]
fn generated_markdown_round_trips_are_stable() {
    for f in FIELDS {
        for md in generated(3000) {
            let h1 = to_html(f, &md);
            let m1 = to_md(f, &h1);
            let h2 = to_html(f, &m1);
            assert_eq!(h2, h1, "{f:?}: {md:?} -> {h1:?} -> {m1:?}");
            assert_eq!(to_md(f, &h2), m1, "{f:?}: {md:?} markdown is not a fixed point");
        }
    }
}

#[test]
fn generated_ado_plain_text_stays_the_same_plain_text() {
    let formatting = ["<B>", "<I>", "<strong>", "<em>", "<code>", "<ul>", "<ol>", "<pre>"];
    for f in FIELDS {
        for text in generated(3000) {
            let html = plain_html(f, &text);
            let md = to_md(f, &html);
            let back = to_html(f, &md);
            for tag in formatting {
                assert!(!back.contains(tag), "{f:?}: plain {text:?} read as {md:?} wrote {back:?}");
            }
            assert_eq!(
                visible(f, &back),
                visible(f, &html),
                "{f:?}: plain {text:?} read as {md:?} wrote {back:?}"
            );
            assert_eq!(to_md(f, &back), md, "{f:?}: {text:?} not stable");
        }
    }
}

/// Formatted HTML as Azure DevOps' editor writes it, with text that holds
/// no Markdown character at all - so any `*`, `_`, `` ` ``, `#` or `\` in
/// what is written back is a Markdown symbol that leaked.
fn generated_formatted_html(count: usize) -> Vec<String> {
    const PIECES: &[&str] = &[
        "<B>", "</B>", "<I>", "</I>", "<STRONG>", "</STRONG>", "<EM>", "</EM>", "<U>", "</U>",
        "<BR/>", "<DIV>", "</DIV>", "<P>", "</P>", "<SPAN>", "</SPAN>", "a", "word", " ", "(", ")",
        ".", ",", ":", "1", "\u{2022}", "&nbsp;", "&quot;", "'", "-", "=", "?",
    ];
    let mut seed: u64 = 0xadd0_5eed_0000_0001;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };
    (0..count)
        .map(|_| {
            let n = 1 + next() % 14;
            (0..n).map(|_| PIECES[next() % PIECES.len()]).collect::<String>()
        })
        .collect()
}

#[test]
fn generated_formatted_ado_html_round_trips_without_leaking_symbols() {
    for f in FIELDS {
        for html in generated_formatted_html(3000) {
            let md = to_md(f, &html);
            let back = to_html(f, &md);
            for sym in ['*', '_', '`', '#', '\\'] {
                assert!(!back.contains(sym), "{f:?}: {html:?} read as {md:?} wrote {back:?}");
            }
            assert_eq!(to_md(f, &back), md, "{f:?}: {html:?} not stable");
            assert_eq!(to_html(f, &to_md(f, &back)), back, "{f:?}: {html:?} html not stable");
        }
    }
}

#[test]
fn ado_plain_text_that_looks_like_markdown_is_written_back_as_the_same_text() {
    // Plain text in Azure DevOps that Markdown would read as formatting must
    // be protected when read, so saving it writes the same plain text.
    let looks_like_markdown = [
        "*not italic*",
        "**not bold**",
        "_not italic_",
        "# not a heading",
        "- not a list",
        "* not a list",
        "+ not a list",
        "1) not a list",
        "> not a quote",
        "```not a fence",
        "`not code`",
        "[not](a link)",
        "[ref]: http://x",
        "![not](an image)",
        "<b>not bold</b>",
        "<https://example.invalid>",
        "&amp; literally",
        "&copy; literally",
        "back\\slash\\",
        "a \\* b",
        "x*y*z",
        "---",
        "***",
        "a\n===",
        "a\n---",
        "SELECT COUNT(*) FROM a WHERE n = (SELECT COUNT(*) FROM b)",
    ];
    for f in FIELDS {
        for t in looks_like_markdown {
            let html = plain_html(f, t);
            let md = to_md(f, &html);
            assert_eq!(to_html(f, &md), html, "{f:?}: {t:?} read as {md:?}");
        }
    }
    // Only what needs it is escaped.
    assert_eq!(step_html_to_md("*not italic*"), "\\*not italic*");
    assert_eq!(step_html_to_md("# not a heading"), "\\# not a heading");
    assert_eq!(step_html_to_md("- not a list"), "\\- not a list");
    assert_eq!(step_html_to_md("1. maybe a list"), "1. maybe a list");
    assert_eq!(prereq_html_to_md("<p>1. not a list</p>"), "1\\. not a list");
    assert_eq!(step_html_to_md("&amp;amp; literally"), "\\&amp; literally");
}

// ---------------------------------------------------------------------------
// Unchanged, and raw HTML
// ---------------------------------------------------------------------------

#[test]
fn unchanged_treats_a_collapsed_copy_as_unchanged() {
    let current = "Run the following query.\nSELECT 1\nFROM t";
    assert!(unchanged(current, current), "the same text");
    assert!(unchanged("Run the following query. SELECT 1 FROM t", current), "a collapsed copy");
    assert!(!unchanged("Run the following query. SELECT 2 FROM t", current), "a real edit");
    assert!(!unchanged("Run the following query.\n\nSELECT 1\nFROM t", current), "added a line");
    assert!(!unchanged("**Run** the following query.\nSELECT 1\nFROM t", current), "added bold");
    assert!(unchanged("", ""));
    assert!(unchanged("a b", "a  b"));
    assert!(!unchanged("a\nb", "a b"), "a copy with MORE line breaks is an edit");
}

#[test]
fn raw_html_is_dropped_on_the_way_in() {
    for f in FIELDS {
        let h = to_html(f, "<img src=x onerror=alert(1)>Hello <b>there</b>");
        assert!(!h.to_lowercase().contains("<img"), "{f:?}: {h}");
        assert!(!h.contains("onerror"), "{f:?}: {h}");
        assert!(h.contains("Hello") && h.contains("there"), "{f:?}: {h}");
        assert!(!h.contains("<b>"), "{f:?}: raw <b> is not bold: {h}");

        let h = to_html(f, "<div onclick=\"x()\">\nblock\n</div>");
        assert!(!h.contains("<div") && !h.contains("onclick"), "{f:?}: {h}");
        assert!(h.contains("block"), "{f:?}: {h}");

        let h = to_html(f, "<script>alert(1)</script>");
        assert!(!h.to_lowercase().contains("<script"), "{f:?}: never an element: {h}");

        let h = to_html(f, "a <span style=\"x\">b</span> c");
        assert!(!h.contains("<span"), "{f:?}: {h}");

        // A placeholder in angle brackets is text, not markup, and survives.
        let h = to_html(f, "WHERE id = <cycleId>");
        assert!(h.contains("&lt;cycleId&gt;"), "{f:?}: {h}");
    }
}

#[test]
fn text_is_escaped_on_the_way_out() {
    assert_eq!(step_md_to_html("a < b & \"c\""), "a &lt; b &amp; &quot;c&quot;");
    assert_eq!(prereq_md_to_html("a < b & c"), "<p>a &lt; b &amp; c</p>");
    assert_eq!(prereq_md_to_html("```\n<b>x</b>\n```"), "<pre><code>&lt;b&gt;x&lt;/b&gt;</code></pre>");
    assert_eq!(step_md_to_html("`<b>`"), "&lt;b&gt;");
    assert_eq!(step_md_to_html(""), "");
    assert_eq!(prereq_md_to_html("  \n "), "");
}
