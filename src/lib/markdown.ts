import DOMPurify from "dompurify";
import { Marked, marked, type MarkedExtension } from "marked";

/**
 * Markdown for the in-app previews, sanitised on the way out.
 *
 * # Why this needs a sanitiser at all
 *
 * The obvious reading is that these previews render what the user just
 * typed, so there is nothing to defend against. That is not what happens.
 * A work item's description arrives from Azure DevOps as stored HTML, gets
 * turned into markdown for the editor, and is rendered straight back out -
 * so merely OPENING the drawer on someone else's work item renders their
 * text. ADO stores typed markup escaped, but `<` survives the round trip
 * through turndown unescaped, and `marked` then emits it as live markup:
 *
 *     stored  "<p>&lt;img src=x onerror=alert(1)&gt;</p>"
 *     draft   "<img src=x onerror=alert(1)>"
 *     output  "<img src=x onerror=alert(1)>"     <- an element, not text
 *
 * The result reaches `dangerouslySetInnerHTML` in a webview that has this
 * app's own commands on `__TAURI_INTERNALS__`, which is a very short
 * distance from someone else's field content to this app's privileges. A
 * `<script>` inserted that way does not run; an inline handler does, and
 * that is enough.
 *
 * # Why a library and not a filter
 *
 * Writing the filter means being right about every tag, attribute and URL
 * spelling forever, including the ones that exist only because a parser
 * normalises something before the browser resolves it. That is a bet this
 * codebase has already lost once. So: an allowlist of exactly the tags
 * markdown can produce, applied by DOMPurify, which parses the output the
 * way the browser will instead of pattern-matching the string.
 *
 * The Rust side (`markdown.rs`) reaches the same place by a different
 * route - it never emits the raw-HTML events at all - because it renders
 * for a page with no IPC on it and can afford to be structural.
 */

/** Everything `marked` can emit. Anything else was not markdown. */
const ALLOWED_TAGS = [
  "p",
  "br",
  "hr",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "blockquote",
  "pre",
  "code",
  "ul",
  "ol",
  "li",
  "table",
  "thead",
  "tbody",
  "tr",
  "th",
  "td",
  "em",
  "strong",
  "del",
  "a",
  "img",
  "input",
];

/**
 * DOMPurify matches attributes globally rather than per tag, so this is
 * the union: link and image targets, table alignment and spans,
 * ordered-list numbering, the task-list checkbox, and `class` for
 * `language-*` on code. `on*` handlers are not here, which is the point.
 */
const ALLOWED_ATTR = [
  "href",
  "title",
  "src",
  "alt",
  "width",
  "height",
  "class",
  "align",
  "colspan",
  "rowspan",
  "start",
  "type",
  "checked",
  "disabled",
];

/** Render markdown to HTML for display in a `.md-preview` container.
 * `breaks: true` matches Azure DevOps, which treats single newlines as
 * line breaks in work-item and PR descriptions. */
export function renderMarkdown(md: string): string {
  return sanitize(marked.parse(md || "", { async: false, breaks: true }) as string);
}

function sanitize(html: string): string {
  return DOMPurify.sanitize(html, {
    ALLOWED_TAGS,
    ALLOWED_ATTR,
    ALLOW_DATA_ATTR: false,
    // Keep the text of anything dropped: a stripped tag should lose its
    // markup, not the sentence it was wrapped around.
    KEEP_CONTENT: true,
  });
}

// ---------------------------------------------------------------------------
// Test case text
// ---------------------------------------------------------------------------

/** Which field a case text belongs to: a step's action or expected result,
 * or the List of Prerequisites. */
export type CaseTextKind = "step" | "prereq";

const ESCAPES: Record<string, string> = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
  "'": "&#39;",
};
const escapeHtml = (s: string) => s.replace(/[&<>"']/g, (c) => ESCAPES[c]);
const lineBreaks = (html: string) => html.replace(/\n/g, "<br>");

/**
 * The dialect of test case text, which is the converter's
 * (`case_markdown.rs`, `CASE_MARKDOWN_OPTIONS`): plain CommonMark, and
 * only what the two Azure DevOps fields can hold shows as formatting.
 *
 * - No GFM. A pipe-table, `~~x~~` or `- [ ] x` is text in Azure DevOps, so
 *   it is text here; old cases that happen to hold one never change look.
 *   `breaks` would turn GFM back on (marked only honours it with `gfm`),
 *   so a soft break is made a `<br>` in the `text` renderer instead: a
 *   single newline is a line break in both fields, as the converter
 *   writes it (`<BR/>` in a step, `<br>` in the prerequisites).
 * - Headings, links, images, quotes and rules become their plain text, as
 *   the converter writes them - and a case's link has no business
 *   navigating this webview, nor its image fetching from anywhere.
 * - Raw HTML is shown as typed: a `<cycleId>` placeholder is text in Azure
 *   DevOps and must not vanish here as an unknown tag.
 * - A step keeps every blank line (one `<BR/>` per line break); the
 *   prerequisites are paragraphs, one gap however many blank lines.
 */
function caseDialect(kind: CaseTextKind): MarkedExtension {
  return {
    gfm: false,
    async: false,
    renderer: {
      text(token) {
        if ("tokens" in token && token.tokens) return this.parser.parseInline(token.tokens);
        const html = "escaped" in token && token.escaped ? token.text : escapeHtml(token.text);
        return lineBreaks(html);
      },
      html(token) {
        const text = lineBreaks(escapeHtml(token.text.replace(/\s+$/, "")));
        return token.block ? `<p>${text}</p>\n` : text;
      },
      heading(token) {
        return `<p>${this.parser.parseInline(token.tokens)}</p>\n`;
      },
      link(token) {
        return this.parser.parseInline(token.tokens);
      },
      image(token) {
        return escapeHtml(token.text);
      },
      blockquote(token) {
        return this.parser.parse(token.tokens);
      },
      hr() {
        return "";
      },
      space(token) {
        if (kind !== "step") return "";
        // Between two blocks the gap itself is one blank line; each line
        // beyond it is one more break.
        const extra = (token.raw.match(/\n/g)?.length ?? 0) - 2;
        return extra > 0 ? "<br>".repeat(extra) : "";
      },
    },
  };
}

const CASE_MARKED: Record<CaseTextKind, Marked> = {
  step: new Marked(caseDialect("step")),
  prereq: new Marked(caseDialect("prereq")),
};

/** Render test case text (a step's action or expected result, or the List
 * of Prerequisites) for a `.md-preview` container, in the converter's
 * dialect (see `caseDialect`) and through the same sanitiser as
 * `renderMarkdown`. */
export function renderCaseMarkdown(md: string, kind: CaseTextKind): string {
  // Leading blank lines and trailing whitespace show as nothing in Azure
  // DevOps; only blank lines BETWEEN content count.
  const text = (md || "").replace(/^(?:[ \t]*\r?\n)+/, "").trimEnd();
  return sanitize(CASE_MARKED[kind].parse(text) as string);
}

/** Test case text as one line of plain words, for the places that show a
 * step in a single truncated line or a heading: the Markdown's formatting
 * is dropped (never shown as `**`), and each line break is a space. */
export function caseTextLine(md: string): string {
  const html = renderCaseMarkdown(md, "step").replace(/<br>|<\/(?:p|li|pre)>/g, " ");
  // DOMParser builds an inert document: nothing in it loads or runs.
  const doc = new DOMParser().parseFromString(html, "text/html");
  return (doc.body.textContent ?? "").replace(/\s+/g, " ").trim();
}
