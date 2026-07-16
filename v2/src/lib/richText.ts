import TurndownService from "turndown";
import { gfm } from "turndown-plugin-gfm";

/** ADO stores rich text as HTML; converting to markdown means the Write tab
 * shows the formatting ADO has (bold, lists, links, tables) as markdown
 * source, and saving (marked, gfm) round-trips it. */
const turndown = new TurndownService({
  headingStyle: "atx",
  codeBlockStyle: "fenced",
  bulletListMarker: "-",
});
// GFM plugin: convert <table>/<del> to pipe tables / ~~strike~~ (turndown
// core drops table structure, stacking each cell on its own line).
turndown.use(gfm);

/** Content that was authored as markdown SOURCE pasted into ADO's rich-text
 * field: plain text carrying literal markdown tokens. */
const MD_TOKENS = /(^|\n)#{1,6} |\*\*|```|(^|\n)- |(^|\n)\d+\. /;

/**
 * HTML -> markdown for the drawer's editors. Two field flavors exist here:
 * - Real rich HTML (bold/lists authored in ADO's editor): turndown as-is.
 * - Markdown source pasted as plain text (how this org writes RCA fields):
 *   the tokens must render, so keep the raw newlines (HTML would collapse
 *   them) and undo turndown's backslash-escaping of the literal tokens.
 */
export function htmlToMd(html: string): string {
  if (!html.trim()) return "";
  const looksLikeMdSource = MD_TOKENS.test(html);
  const src = looksLikeMdSource ? html.replace(/\r?\n/g, "<br>") : html;
  const md = turndown.turndown(src);
  return looksLikeMdSource ? md.replace(/\\([\\`*_#[\]>+.!-])/g, "$1") : md;
}
