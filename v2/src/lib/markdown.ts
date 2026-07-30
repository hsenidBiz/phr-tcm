import DOMPurify from "dompurify";
import { marked } from "marked";

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
  const html = marked.parse(md || "", { async: false, breaks: true }) as string;
  return DOMPurify.sanitize(html, {
    ALLOWED_TAGS,
    ALLOWED_ATTR,
    ALLOW_DATA_ATTR: false,
    // Keep the text of anything dropped: a stripped tag should lose its
    // markup, not the sentence it was wrapped around.
    KEEP_CONTENT: true,
  });
}
