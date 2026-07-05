"""HTML <-> Markdown conversion for ADO rich-text fields (Qt-free)."""

from app.utils.richtext import html_to_markdown, markdown_to_html


# ----------------------------- HTML -> Markdown ---------------------------- #

def test_empty():
    assert html_to_markdown("") == ""
    assert html_to_markdown("   ") == ""
    assert markdown_to_html("") == ""


def test_heading_and_bold():
    md = html_to_markdown("<h1>Step 1</h1><p>Do <strong>this</strong> now</p>")
    assert "# Step 1" in md
    assert "Do **this** now" in md


def test_italic_code_link():
    md = html_to_markdown(
        '<p><em>note</em> <code>x=1</code> <a href="http://a.b">link</a></p>')
    assert "*note*" in md
    assert "`x=1`" in md
    assert "[link](http://a.b)" in md


def test_unordered_list():
    md = html_to_markdown("<ul><li>one</li><li>two</li></ul>")
    assert "- one" in md
    assert "- two" in md


def test_ordered_list():
    md = html_to_markdown("<ol><li>first</li><li>second</li></ol>")
    assert "1. first" in md
    assert "2. second" in md


def test_table_with_th_header():
    html = ("<table><tr><th>Task</th><th>Hours</th></tr>"
            "<tr><td>Seed</td><td>1.0</td></tr></table>")
    md = html_to_markdown(html)
    lines = [ln for ln in md.splitlines() if ln.strip()]
    assert lines[0] == "| Task | Hours |"
    assert lines[1] == "| --- | --- |"
    assert lines[2] == "| Seed | 1.0 |"


def test_table_bold_td_header():
    """ADO often bolds the first row instead of using <th>."""
    html = ("<table><tr><td><b>Task</b></td><td><b>Hours</b></td></tr>"
            "<tr><td>Seed</td><td>1.0</td></tr></table>")
    md = html_to_markdown(html)
    assert "| **Task** | **Hours** |" in md
    assert "| Seed | 1.0 |" in md


def test_table_escapes_pipes():
    html = "<table><tr><th>A|B</th></tr><tr><td>c</td></tr></table>"
    md = html_to_markdown(html)
    assert r"A\|B" in md


# ----------------------------- Markdown -> HTML ---------------------------- #

def test_md_heading_bold_html():
    html = markdown_to_html("# Title\n\nsome **bold** text")
    assert "<h1>Title</h1>" in html
    assert "<strong>bold</strong>" in html
    assert "<p>some <strong>bold</strong> text</p>" in html


def test_md_escapes_html():
    html = markdown_to_html("a < b & c > d")
    assert "&lt;" in html and "&amp;" in html and "&gt;" in html
    assert "<b" not in html.replace("<br>", "")


def test_md_list_html():
    html = markdown_to_html("- one\n- two")
    assert html == "<ul><li>one</li><li>two</li></ul>"
    html2 = markdown_to_html("1. a\n2. b")
    assert html2 == "<ol><li>a</li><li>b</li></ol>"


def test_md_table_html():
    md = "| Task | Hours |\n| --- | --- |\n| Seed | 1.0 |"
    html = markdown_to_html(md)
    assert "<table>" in html and "</table>" in html
    assert "<th>Task</th><th>Hours</th>" in html
    assert "<td>Seed</td><td>1.0</td>" in html


def test_md_table_no_padding():
    """Qt's toMarkdown emits tables without pipe padding — parse that too."""
    md = "|Task|Hours|\n|----|-----|\n|Seed|1.0 |"
    html = markdown_to_html(md)
    assert "<th>Task</th><th>Hours</th>" in html
    assert "<td>Seed</td><td>1.0</td>" in html


def test_md_link_and_code():
    html = markdown_to_html("see [docs](http://x) and `code`")
    assert '<a href="http://x">docs</a>' in html
    assert "<code>code</code>" in html


# ------------------------------- Round trips ------------------------------- #

def test_table_round_trip():
    md = "| Task | Hours |\n| --- | --- |\n| Seed | 1.0 |"
    back = html_to_markdown(markdown_to_html(md))
    lines = [ln for ln in back.splitlines() if ln.strip()]
    assert lines == ["| Task | Hours |", "| --- | --- |", "| Seed | 1.0 |"]


def test_heading_list_round_trip():
    md = "# Steps\n\n- one\n- two"
    back = html_to_markdown(markdown_to_html(md))
    assert "# Steps" in back
    assert "- one" in back and "- two" in back
