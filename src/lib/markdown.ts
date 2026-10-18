import DOMPurify from "dompurify";
import { Marked, marked, type MarkedExtension, type Token, type Tokens } from "marked";

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
/** Every special character escaped: text shown exactly as typed. */
const escapeHtml = (s: string) => s.replace(/[&<>"']/g, (c) => ESCAPES[c]);
/** Markdown text: escaped, except that an entity someone typed (`&amp;`,
 * `&lt;`) stays one, so it shows as its character - CommonMark decodes
 * entities, and so does the converter. A `\&lt;` reaches here as an
 * escaped `&` and `lt;`, so it still shows the literal `&lt;`. */
const escapeText = (s: string) =>
  s.replace(/[<>"']|&(?!#\d{1,7};|#[Xx][a-fA-F0-9]{1,6};|[A-Za-z][A-Za-z0-9]*;)/g, (c) => ESCAPES[c]);
const lineBreaks = (html: string) => html.replace(/\n/g, "<br>");

/** The tags Azure DevOps' editors emit - `steps_xml::HTML_TAGS`. Raw HTML
 * naming one is markup, which the converter drops, keeping its text;
 * anything else in angle brackets (`<cycleId>`) is a typed placeholder. */
const HTML_TAGS = new Set([
  "a", "b", "big", "blockquote", "br", "caption", "center", "code", "col", "colgroup", "dd",
  "div", "dl", "dt", "em", "font", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "i", "img", "li",
  "ol", "p", "pre", "s", "small", "span", "strike", "strong", "sub", "sup", "table", "tbody",
  "td", "th", "thead", "tr", "u", "ul",
]);

/** `steps_xml::html_tag_end`: the index of the `>` that closes a real tag
 * opening at `open`, or -1 when this `<` is just a less-than sign. */
function htmlTagEnd(s: string, open: number): number {
  let i = open + 1;
  if (s[i] === "/") i++;
  const nameStart = i;
  while (i < s.length && /[A-Za-z0-9]/.test(s[i])) i++;
  if (i === nameStart || !HTML_TAGS.has(s.slice(nameStart, i).toLowerCase())) return -1;
  // Attributes may quote a '>', so track quoting.
  let quote: string | null = null;
  for (; i < s.length; i++) {
    const c = s[i];
    if (quote) {
      if (c === quote) quote = null;
    } else if (c === '"' || c === "'") {
      quote = c;
    } else if (c === ">") {
      return i;
    } else if (c === "<") {
      return -1;
    }
  }
  return -1;
}

/** `steps_xml::strip_tags`: real markup out, its text and placeholders kept. */
function stripTags(s: string): string {
  let out = "";
  for (let i = 0; i < s.length; i++) {
    const end = s[i] === "<" ? htmlTagEnd(s, i) : -1;
    if (end >= 0) {
      out += " ";
      i = end;
    } else {
      out += s[i];
    }
  }
  return out;
}

const containsHtmlMarkup = (s: string) =>
  [...s].some((c, i) => c === "<" && htmlTagEnd(s, i) >= 0);

/** The `.` or `)` typed after an ordered item's number, as the converter
 * reads it (`ordered_delimiter`). */
const delimiterOf = (itemRaw: string) => (/^\s*\d+\)/.test(itemRaw) ? ")" : ".");

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
 * - Text is ALWAYS escaped. marked flags text after an inline `<code>`,
 *   `<pre>`, `<kbd>` or `<script>` as `escaped` and would pass it through
 *   raw - `<code><img src=x onerror=...` would come out a live element.
 * - Headings, links, images, quotes and rules become their plain text, as
 *   the converter writes them - and a case's link has no business
 *   navigating this webview, nor its image fetching from anywhere.
 * - Raw HTML goes as the converter takes it: a tag Azure DevOps' editor
 *   could have produced (`<b>`, a `<div>` block) is dropped and its text
 *   kept; anything else (`<cycleId>`) shows as typed.
 * - A step has no paragraphs and no lists - Azure DevOps shows lines - see
 *   `renderStep`. The prerequisites are paragraphs and real lists, with
 *   an ordered list's `)` kept as typed.
 */
function caseDialect(kind: CaseTextKind): MarkedExtension {
  return {
    gfm: false,
    async: false,
    renderer: {
      text(token) {
        if ("tokens" in token && token.tokens) return this.parser.parseInline(token.tokens);
        // Never `token.escaped` - see above.
        return lineBreaks(escapeText(token.text));
      },
      html(token) {
        if (token.block) {
          const kept = stripTags(token.text).trim();
          return kept ? `<p>${lineBreaks(escapeHtml(kept))}</p>\n` : "";
        }
        return containsHtmlMarkup(token.text) ? "" : escapeHtml(token.text);
      },
      paragraph(token) {
        const inline = this.parser.parseInline(token.tokens);
        return kind === "step" ? inline : `<p>${inline}</p>\n`;
      },
      heading(token) {
        return `<p>${this.parser.parseInline(token.tokens)}</p>\n`;
      },
      link(token) {
        return this.parser.parseInline(token.tokens);
      },
      image(token) {
        return escapeText(token.text);
      },
      blockquote(token) {
        return this.parser.parse(token.tokens);
      },
      hr() {
        return "";
      },
      list(token) {
        const tag = token.ordered ? "ol" : "ul";
        const start =
          token.ordered && token.start !== "" && token.start !== 1 ? ` start="${Number(token.start)}"` : "";
        // `1) Login` keeps its `)` (a CSS marker on `.paren`).
        const paren = token.ordered && delimiterOf(token.items[0]?.raw ?? "") === ")" ? ' class="paren"' : "";
        const items = token.items.map((item) => this.listitem(item)).join("");
        return `<${tag}${start}${paren}>\n${items}</${tag}>\n`;
      },
    },
  };
}

const CASE_MARKED: Record<CaseTextKind, Marked> = {
  step: new Marked(caseDialect("step")),
  prereq: new Marked(caseDialect("prereq")),
};

/** One visual piece of a step: some lines of inline HTML, a code block, or
 * the number of blank lines between two pieces. */
type StepPiece = { html: string; code: boolean } | { blank: number };

/** Blank lines a `space` token stands for (its newlines, less the one that
 * ends the line before it). */
const blankLines = (raw: string) => Math.max(0, (raw.match(/\n/g)?.length ?? 0) - 1);

function stepPieces(tokens: Token[]): StepPiece[] {
  const md = CASE_MARKED.step;
  const inline = (t: Token) => md.parser([t]) as string;
  const out: StepPiece[] = [];
  for (const t of tokens) {
    switch (t.type) {
      case "space":
        out.push({ blank: blankLines(t.raw) });
        break;
      case "paragraph":
        out.push({ html: inline(t), code: false });
        break;
      case "heading":
      case "text": {
        // As a paragraph: in a step every one is just its line(s).
        const tokens = "tokens" in t && t.tokens ? t.tokens : [{ type: "text", raw: t.raw, text: t.text }];
        out.push({ html: inline({ type: "paragraph", raw: t.raw, text: t.text, tokens } as Token), code: false });
        break;
      }
      case "code":
        out.push({ html: md.parser([t]) as string, code: true });
        break;
      case "html": {
        const kept = stripTags(t.text).trim();
        if (kept) out.push({ html: lineBreaks(escapeHtml(kept)), code: false });
        break;
      }
      case "blockquote":
        out.push(...stepPieces(t.tokens ?? []));
        break;
      case "list": {
        // A step has no lists: each item is a line, its marker as typed -
        // "• item", "1. item", "1) Login" - as the converter writes it.
        let n = t.ordered && t.start !== "" ? Number(t.start) : 1;
        t.items.forEach((item: Tokens.ListItem, i: number) => {
          const marker = escapeHtml(t.ordered ? `${n++}${delimiterOf(item.raw)} ` : "• ");
          const sub = stepPieces(item.tokens);
          while (sub.length && "blank" in sub[sub.length - 1]) sub.pop();
          const first = sub[0];
          if (first && !("blank" in first) && !first.code) first.html = marker + first.html;
          else sub.unshift({ html: marker, code: false });
          out.push(...sub);
          // The blank lines after an item, before the next one.
          if (i < t.items.length - 1) out.push({ blank: blankLines(item.raw.match(/\n*$/)?.[0] ?? "") });
        });
        break;
      }
      default:
        // hr, def: nothing to show.
        break;
    }
  }
  return out;
}

/**
 * A step as Azure DevOps shows it: lines, one `<br>` per line break, so a
 * blank line is two and three blank lines stay three (`step_md_to_html`).
 * No paragraphs, so `Do:` over a list has no gap above the list's lines.
 * A code block stays a block (the one place the app looks richer than the
 * step: its lines are the same).
 */
function renderStep(md: string): string {
  let html = "";
  let prev: { code: boolean } | null = null;
  let blank = 0;
  for (const piece of stepPieces(CASE_MARKED.step.lexer(md))) {
    if ("blank" in piece) {
      blank += piece.blank;
      continue;
    }
    if (prev) {
      // After a block a <br> is a whole line; before one, the first <br>
      // only ends the line it follows.
      if (prev.code) html += "<br>".repeat(blank);
      else if (piece.code) html += blank > 0 ? "<br>".repeat(blank + 1) : "";
      else html += "<br>".repeat(1 + blank);
    }
    html += piece.html;
    prev = piece;
    blank = 0;
  }
  return html;
}

/** Render test case text (a step's action or expected result, or the List
 * of Prerequisites) for a `.md-preview` container, in the converter's
 * dialect (see `caseDialect`) and through the same sanitiser as
 * `renderMarkdown`. */
export function renderCaseMarkdown(md: string, kind: CaseTextKind): string {
  // Leading blank lines and trailing whitespace show as nothing in Azure
  // DevOps; only blank lines BETWEEN content count.
  const text = (md || "").replace(/^(?:[ \t]*\r?\n)+/, "").trimEnd();
  const html = kind === "step" ? renderStep(text) : (CASE_MARKED.prereq.parse(text) as string);
  return sanitize(html);
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
