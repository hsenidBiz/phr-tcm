//! The repro steps of a Bug go to Azure DevOps as HTML. The text is Markdown
//! inside TCM, so a step's `**Save**` must arrive as bold, not as asterisks.

use v2_lib::commands::bugs::repro_html;

#[test]
fn bold_in_a_step_is_bold_in_the_bug_not_asterisks() {
    let html = repro_html("Test case #7: Save\n\nSteps:\n1. Click **Save** -> expected: it is *saved*");
    assert!(html.contains("<strong>Save</strong>"), "{html}");
    assert!(html.contains("<em>saved</em>"), "{html}");
    assert!(!html.contains("**"), "{html}");
}

#[test]
fn lines_and_code_survive_in_the_bug() {
    let html = repro_html("Steps:\n1. Run:\n```sql\nSELECT 1\n```\nDone\nsecond line");
    assert!(html.contains("<pre><code>"), "{html}");
    assert!(html.contains("SELECT 1"), "{html}");
    assert!(html.contains("Done<br>second line"), "{html}");
}

#[test]
fn what_the_tester_typed_cannot_become_markup() {
    let html = repro_html("see <img src=x onerror=alert(1)> and <cycleId> & more");
    assert!(!html.contains("<img"), "{html}");
    assert!(html.contains("&lt;cycleId&gt;"), "{html}");
    assert!(html.contains("&amp; more"), "{html}");
}

#[test]
fn plain_text_with_line_breaks_still_reads_the_same() {
    let html = repro_html("Test case #7: Save\n\nSteps:\nOpen the page");
    assert!(html.contains("Test case #7: Save"), "{html}");
    assert!(html.contains("Steps:<br>Open the page"), "{html}");
}

/// The repro text the Bug dialog builds: each step a list item, its other
/// lines indented under it. A fenced query inside a step closes inside that
/// step, so the code block holds only the query, and the next step is a
/// list item of its own (written as one line, the fence's closing line
/// carried the expected result and every later step into the `<pre>`).
#[test]
fn a_fenced_query_in_a_step_keeps_the_next_step_out_of_the_code_block() {
    let text = "Test case #201: Valid login\n\nSteps:\n1. Run the query:\n\n   ```sql\n   SELECT 1\n   ```\n   Expected: one row\n2. Open the report\n   Expected: It shows the row";
    let html = repro_html(text);
    assert_eq!(html.matches("<pre>").count(), 1, "{html}");
    assert!(html.contains("<pre><code>SELECT 1</code></pre>"), "{html}");
    let items: Vec<&str> = html.split("<li>").skip(1).collect();
    assert_eq!(items.len(), 2, "{html}");
    assert!(items[0].contains("Expected: one row") && items[0].contains("<pre>"), "{html}");
    assert!(items[1].contains("Open the report") && items[1].contains("Expected: It shows the row"), "{html}");
    assert!(!items[1].contains("<pre>"), "{html}");
}
