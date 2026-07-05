"""Lightweight HTML <-> Markdown conversion for Azure DevOps rich-text fields.

Azure DevOps stores fields like ``System.Description`` / ``ReproSteps`` as HTML.
The Work Manager editor renders them as Markdown (Qt's ``setMarkdown`` speaks the
GitHub dialect, so headings, **bold**, lists and tables all render), and converts
back to HTML on save so nothing is flattened.

These converters are deliberately regex-based (ADO HTML is not always well-formed
XML — unclosed ``<br>`` etc.) and cover the element surface ADO actually emits:
headings, paragraphs/divs, line breaks, bold, italic, inline code, links,
ordered/unordered lists, blockquotes and tables. Anything unknown degrades to its
text content rather than raising. Both functions return ``''`` for empty input.
"""

import html as _html
import re as _re

__all__ = ["html_to_markdown", "markdown_to_html"]


# --------------------------------------------------------------------------- #
#  HTML -> Markdown                                                            #
# --------------------------------------------------------------------------- #

def _inline_html_to_md(s: str) -> str:
    """Convert the inline HTML inside a block (bold/italic/code/links) to
    Markdown, strip any remaining tags and unescape entities. Newlines are
    flattened to spaces — block structure is handled by the caller."""
    s = _re.sub(r"(?is)<\s*(strong|b)\s*>(.*?)<\s*/\s*\1\s*>", r"**\2**", s)
    s = _re.sub(r"(?is)<\s*(em|i)\s*>(.*?)<\s*/\s*\1\s*>", r"*\2*", s)
    s = _re.sub(r"(?is)<\s*code\s*>(.*?)<\s*/\s*code\s*>", r"`\1`", s)
    s = _re.sub(r'(?is)<\s*a\b[^>]*\bhref\s*=\s*["\']([^"\']*)["\'][^>]*>(.*?)'
                r"<\s*/\s*a\s*>", r"[\2](\1)", s)
    s = _re.sub(r"(?is)<\s*br\s*/?\s*>", " ", s)
    s = _re.sub(r"<[^>]+>", "", s)               # drop any leftover tags
    s = _html.unescape(s).replace("\xa0", " ")
    return _re.sub(r"[ \t\r\n]+", " ", s).strip()


def _table_to_md(table_html: str) -> str:
    rows = _re.findall(r"(?is)<\s*tr\b[^>]*>(.*?)<\s*/\s*tr\s*>", table_html)
    parsed = []
    header_from_th = False
    for i, row in enumerate(rows):
        cells = _re.findall(r"(?is)<\s*t[hd]\b[^>]*>(.*?)<\s*/\s*t[hd]\s*>", row)
        if i == 0 and _re.search(r"(?is)<\s*th\b", row):
            header_from_th = True
        parsed.append([_inline_html_to_md(c) or " " for c in cells])
    parsed = [r for r in parsed if r]
    if not parsed:
        return ""
    width = max(len(r) for r in parsed)
    parsed = [r + [" "] * (width - len(r)) for r in parsed]
    # First row is the header (ADO tables lead with a header row, whether it
    # used <th> or bold <td>). Escape pipes so cell text can't break the table.
    esc = lambda c: c.replace("|", "\\|")  # noqa: E731
    header = parsed[0]
    body = parsed[1:] if (header_from_th or len(parsed) > 1) else []
    out = ["| " + " | ".join(esc(c) for c in header) + " |",
           "| " + " | ".join("---" for _ in header) + " |"]
    for r in body:
        out.append("| " + " | ".join(esc(c) for c in r) + " |")
    return "\n".join(out)


def html_to_markdown(html: str) -> str:
    if not html or not html.strip():
        return ""
    s = html.replace("\r\n", "\n").replace("\r", "\n")

    # Tables become fenced markdown blocks up front (protected from the block/
    # inline passes below via placeholders).
    tables = []

    def _stash_table(m):
        tables.append(_table_to_md(m.group(0)))
        return f"\n\x00TBL{len(tables) - 1}\x00\n"

    s = _re.sub(r"(?is)<\s*table\b.*?<\s*/\s*table\s*>", _stash_table, s)

    # Headings -> "# ..." block lines.
    def _heading(m):
        level = int(m.group(1))
        return f"\n\x00\n{'#' * level} {_inline_html_to_md(m.group(2))}\n\x00\n"

    s = _re.sub(r"(?is)<\s*h([1-6])\b[^>]*>(.*?)<\s*/\s*h\1\s*>", _heading, s)

    # List items -> "- " / keep order for <ol> via a simple counter per list.
    def _list(m):
        ordered = m.group(1).lower() == "ol"
        items = _re.findall(r"(?is)<\s*li\b[^>]*>(.*?)<\s*/\s*li\s*>", m.group(2))
        out = []
        for i, it in enumerate(items, 1):
            prefix = f"{i}. " if ordered else "- "
            out.append(prefix + _inline_html_to_md(it))
        return "\n\x00\n" + "\n".join(out) + "\n\x00\n"

    s = _re.sub(r"(?is)<\s*(ul|ol)\b[^>]*>(.*?)<\s*/\s*\1\s*>", _list, s)

    # Blockquotes.
    def _quote(m):
        text = _inline_html_to_md(m.group(1))
        return "\n\x00\n" + "\n".join(f"> {ln}" for ln in text.split("\n")) + "\n\x00\n"

    s = _re.sub(r"(?is)<\s*blockquote\b[^>]*>(.*?)<\s*/\s*blockquote\s*>", _quote, s)

    # Block boundaries -> newlines; <br> -> hard line break within a paragraph.
    s = _re.sub(r"(?is)<\s*br\s*/?\s*>", "\n", s)
    s = _re.sub(r"(?is)<\s*/\s*(p|div|h[1-6]|tr|table|ul|ol)\s*>", "\n\x00\n", s)
    s = _re.sub(r"(?is)<\s*(p|div)\b[^>]*>", "", s)

    # Remaining inline markup + tag strip.
    s = _inline_block_pass(s)

    # Restore tables.
    for i, tbl in enumerate(tables):
        s = s.replace(f"\x00TBL{i}\x00", tbl)

    return _collapse_blocks(s)


def _inline_block_pass(s: str) -> str:
    """Apply inline conversions line-by-line so the block placeholders (\\x00)
    that separate paragraphs survive."""
    parts = s.split("\x00")
    return "\x00".join(
        p if p.strip() in ("",) else _inline_html_to_md_preserve_nl(p)
        for p in parts
    )


def _inline_html_to_md_preserve_nl(s: str) -> str:
    """Like _inline_html_to_md but keeps existing newlines (hard breaks)."""
    out = []
    for ln in s.split("\n"):
        out.append(_inline_html_to_md(ln))
    return "\n".join(out)


def _collapse_blocks(s: str) -> str:
    s = s.replace("\x00", "\n")
    # Trim each line, then squeeze 3+ blank lines down to one.
    lines = [ln.rstrip() for ln in s.split("\n")]
    out, blanks = [], 0
    for ln in lines:
        if ln.strip():
            out.append(ln)
            blanks = 0
        else:
            blanks += 1
            if blanks == 1 and out:
                out.append("")
    return "\n".join(out).strip()


# --------------------------------------------------------------------------- #
#  Markdown -> HTML                                                            #
# --------------------------------------------------------------------------- #

def _esc(text: str) -> str:
    return (text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;"))


def _inline_md_to_html(text: str) -> str:
    """Inline Markdown (code/bold/italic/links) -> HTML. HTML-escapes first so
    user text can't inject markup; code spans are protected from other rules."""
    codes = []

    def _stash_code(m):
        codes.append(_esc(m.group(1)))
        return f"\x00C{len(codes) - 1}\x00"

    text = _re.sub(r"`([^`]+)`", _stash_code, text)
    text = _esc(text)
    text = _re.sub(r"\[([^\]]+)\]\(([^)]+)\)",
                   lambda m: f'<a href="{m.group(2)}">{m.group(1)}</a>', text)
    text = _re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", text)
    text = _re.sub(r"__([^_]+)__", r"<strong>\1</strong>", text)
    text = _re.sub(r"(?<![*\w])\*([^*]+)\*(?![*\w])", r"<em>\1</em>", text)
    text = _re.sub(r"(?<![_\w])_([^_]+)_(?![_\w])", r"<em>\1</em>", text)
    for i, c in enumerate(codes):
        text = text.replace(f"\x00C{i}\x00", f"<code>{c}</code>")
    return text


def _split_cells(row: str) -> list:
    row = row.strip().strip("|")
    # Split on unescaped pipes, then restore escaped ones.
    cells = _re.split(r"(?<!\\)\|", row)
    return [c.strip().replace("\\|", "|") for c in cells]


def _is_separator(line: str) -> bool:
    return bool(_re.fullmatch(r"\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*",
                              line))


def markdown_to_html(md: str) -> str:
    if not md or not md.strip():
        return ""
    lines = md.replace("\r\n", "\n").replace("\r", "\n").split("\n")
    html_parts = []
    i, n = 0, len(lines)
    para: list = []

    def _flush_para():
        if para:
            html_parts.append("<p>" + "<br>".join(
                _inline_md_to_html(x) for x in para) + "</p>")
            para.clear()

    while i < n:
        line = lines[i]
        stripped = line.strip()

        # Table: a header row followed by a separator row.
        if ("|" in line and i + 1 < n and _is_separator(lines[i + 1])
                and not _is_separator(line)):
            _flush_para()
            header = _split_cells(line)
            i += 2
            body = []
            while i < n and "|" in lines[i] and lines[i].strip():
                body.append(_split_cells(lines[i]))
                i += 1
            thead = "".join(f"<th>{_inline_md_to_html(c)}</th>" for c in header)
            rows = ["<thead><tr>" + thead + "</tr></thead>"]
            if body:
                trs = "".join(
                    "<tr>" + "".join(f"<td>{_inline_md_to_html(c)}</td>"
                                     for c in r) + "</tr>" for r in body)
                rows.append("<tbody>" + trs + "</tbody>")
            html_parts.append("<table>" + "".join(rows) + "</table>")
            continue

        # Heading.
        m = _re.match(r"(#{1,6})\s+(.*)$", stripped)
        if m:
            _flush_para()
            lvl = len(m.group(1))
            html_parts.append(f"<h{lvl}>{_inline_md_to_html(m.group(2))}</h{lvl}>")
            i += 1
            continue

        # Blockquote (consecutive "> " lines).
        if stripped.startswith(">"):
            _flush_para()
            quote = []
            while i < n and lines[i].strip().startswith(">"):
                quote.append(_re.sub(r"^\s*>\s?", "", lines[i]))
                i += 1
            html_parts.append("<blockquote>" + "<br>".join(
                _inline_md_to_html(q) for q in quote) + "</blockquote>")
            continue

        # Lists (unordered / ordered).
        if _re.match(r"[-*+]\s+", stripped) or _re.match(r"\d+\.\s+", stripped):
            _flush_para()
            ordered = bool(_re.match(r"\d+\.\s+", stripped))
            tag = "ol" if ordered else "ul"
            items = []
            pat = r"\d+\.\s+(.*)$" if ordered else r"[-*+]\s+(.*)$"
            while i < n and _re.match(pat, lines[i].strip()):
                items.append(_re.match(pat, lines[i].strip()).group(1))
                i += 1
            html_parts.append(f"<{tag}>" + "".join(
                f"<li>{_inline_md_to_html(it)}</li>" for it in items) + f"</{tag}>")
            continue

        # Blank line ends a paragraph.
        if not stripped:
            _flush_para()
            i += 1
            continue

        para.append(stripped)
        i += 1

    _flush_para()
    return "".join(html_parts)
