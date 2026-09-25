import type { InlineImage } from "../bindings";

/** An HTML `<img src="...">` tag, capturing the URL. */
const IMG_TAG_RE = /<img\b[^>]*\ssrc=["']([^"']+)["'][^>]*>/gi;

/** Markdown `![alt](url)` image syntax, capturing the URL. Markdown's own
 * grammar forbids whitespace and an unescaped `)` in the plain (non
 * angle-bracket) form, so stopping at the first one of either is the
 * grammar, not a heuristic. */
const MD_IMG_RE = /!\[[^\]]*\]\(([^)\s]+)\)/g;

/** Entity-unescape a `src` pulled out of HTML; a no-op for a markdown URL,
 * which is never entity-encoded. */
function unescapeAmp(url: string): string {
  return url.replace(/&amp;/g, "&");
}

/** Whether Rust's `attachment_download_url` guard could ever fetch this -
 * the SHAPE check only (the path carries "/_apis/" and "attachment"); the
 * host itself is Rust's call, made with the caller's token, and is not
 * repeated here. Used only to decide whether a src is worth asking about,
 * or still needs its own placeholder after asking. */
function looksLikeAdoAttachment(url: string): boolean {
  try {
    const path = new URL(url).pathname.toLowerCase();
    return path.includes("/_apis/") && path.includes("attachment");
  } catch {
    return false;
  }
}

/** Every ADO attachment image URL referenced across these texts (HTML
 * `text_html` for work item comments, markdown `content` for PR threads),
 * sorted and de-duplicated - used as a stable react-query key and as the
 * `enabled` gate, not as what gets sent to `commentImages` (that gets the
 * raw texts; Rust does its own extraction). */
export function attachmentUrls(texts: string[]): string[] {
  const urls = new Set<string>();
  for (const text of texts) {
    for (const m of text.matchAll(IMG_TAG_RE)) {
      const url = unescapeAmp(m[1]);
      if (looksLikeAdoAttachment(url)) urls.add(url);
    }
    for (const m of text.matchAll(MD_IMG_RE)) {
      const url = unescapeAmp(m[1]);
      if (looksLikeAdoAttachment(url)) urls.add(url);
    }
  }
  return [...urls].sort();
}

/** Swap authenticated attachment URLs for the data: URIs Rust downloaded
 * (a plain <img> gets 401 - the WebView sends no bearer header). Works on
 * HTML or markdown text alike: it replaces the URL substring itself, in
 * both its raw and `&amp;`-escaped forms, rather than parsing the markup
 * around it. Moved out of `WorkItemDrawer` so comments and PR threads
 * share it instead of re-implementing the loop. */
export function swapInlineImages(text: string, images: InlineImage[]): string {
  let out = text;
  for (const img of images) {
    out = out
      .split(img.url.replace(/&/g, "&amp;"))
      .join(img.data)
      .split(img.url)
      .join(img.data);
  }
  return out;
}

/** After swapping, any `<img>` (HTML) or `![alt](url)` (markdown) still
 * pointing at what looks like an ADO attachment - fetch failed, refused by
 * the token-host guard, or the fetch has not finished - becomes a small
 * note instead of a broken image icon. `kind` picks the placeholder's own
 * syntax: the Astryx Markdown island PR threads render through does not
 * accept raw HTML, so its placeholder has to be markdown too. A non-ADO
 * image is left exactly alone. */
export function markUnavailableImages(text: string, kind: "html" | "md"): string {
  if (kind === "html") {
    return text.replace(IMG_TAG_RE, (whole, url: string) =>
      looksLikeAdoAttachment(unescapeAmp(url))
        ? '<span class="text-faint text-xs">Image unavailable</span>'
        : whole,
    );
  }
  return text.replace(MD_IMG_RE, (whole, url: string) =>
    looksLikeAdoAttachment(unescapeAmp(url)) ? "*Image unavailable*" : whole,
  );
}

/** A `data:` image URI in `data`, converted to a `blob:` object URL.
 *
 * Astryx's Markdown island refuses to render a `data:` image src at all -
 * `sanitizeUrl` blocks it outright, in the same list as `javascript:` and
 * `vbscript:`, so swapping one into the markdown text before it reaches
 * `<Markdown>` would just fall back to the `[alt]` bracket forever. A
 * `blob:` URL is not on that list, and Chromium (the WebView) renders it
 * exactly like any other image src - so PR threads swap to one of these
 * instead of the data: URI CommentsPanel uses directly in HTML. */
function toBlobUrl(data: string): string {
  const match = /^data:([^;,]+)?(?:;charset=[^;,]+)?;base64,(.*)$/s.exec(data);
  if (!match) return data;
  const [, mime, base64] = match;
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return URL.createObjectURL(new Blob([bytes], { type: mime || "application/octet-stream" }));
}

/** `commentImages`'s result, with each data: URI swapped for a `blob:`
 * object URL - what PR threads need before handing text to `swapInlineImages`
 * (see `toBlobUrl` for why). Not used by CommentsPanel, which renders HTML
 * via `dangerouslySetInnerHTML` and has no such restriction. */
export function toBlobImages(images: InlineImage[]): InlineImage[] {
  return images.map((img) => ({ url: img.url, data: toBlobUrl(img.data) }));
}
