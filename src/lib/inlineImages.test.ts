import { expect, test } from "vitest";
import { attachmentUrls, markUnavailableImages, swapInlineImages, toBlobImages } from "./inlineImages";

const RAW_URL = "https://dev.azure.com/acme/Web/_apis/wit/attachments/att-1?fileName=x.png&download=true";
const ESCAPED_URL = "https://dev.azure.com/acme/Web/_apis/wit/attachments/att-1?fileName=x.png&amp;download=true";
const DATA_URI = "data:image/png;base64,iVBORw0KGgo=";

test("swapInlineImages replaces both the raw and &amp;-escaped form of a URL", () => {
  const images = [{ url: RAW_URL, data: DATA_URI }];

  const htmlRaw = `<img src="${RAW_URL}">`;
  expect(swapInlineImages(htmlRaw, images)).toBe(`<img src="${DATA_URI}">`);

  const htmlEscaped = `<img src="${ESCAPED_URL}">`;
  expect(swapInlineImages(htmlEscaped, images)).toBe(`<img src="${DATA_URI}">`);

  const md = `![shot](${RAW_URL})`;
  expect(swapInlineImages(md, images)).toBe(`![shot](${DATA_URI})`);
});

test("markUnavailableImages replaces an unswapped ADO attachment image and leaves a non-ADO one alone", () => {
  const html = `<img src="${RAW_URL}"> and <img src="https://example.com/logo.png">`;
  const outHtml = markUnavailableImages(html, "html");
  expect(outHtml).toContain('<span class="text-faint text-xs">Image unavailable</span>');
  expect(outHtml).not.toContain(RAW_URL);
  expect(outHtml).toContain('<img src="https://example.com/logo.png">');

  const md = `![shot](${RAW_URL}) and ![logo](https://example.com/logo.png)`;
  const outMd = markUnavailableImages(md, "md");
  expect(outMd).toContain("*Image unavailable*");
  expect(outMd).not.toContain(RAW_URL);
  expect(outMd).toContain("![logo](https://example.com/logo.png)");
});

test("markUnavailableImages leaves a fully swapped data: URI alone", () => {
  const html = `<img src="${DATA_URI}">`;
  expect(markUnavailableImages(html, "html")).toBe(html);
});

test("attachmentUrls finds ADO attachment URLs from HTML and markdown, sorted and de-duplicated", () => {
  const b = "https://dev.azure.com/acme/Web/_apis/wit/attachments/att-2";
  const texts = [
    `<div>See <img src="${RAW_URL.replace(/&/g, "&amp;")}"></div>`,
    `Review: ![img](${b})`,
    `Again: ![img](${RAW_URL})`, // duplicate of the first URL
    `<p>Not an attachment: <img src="https://example.com/x.png"></p>`,
  ];
  expect(attachmentUrls(texts)).toEqual([b, RAW_URL].sort());
});

test("attachmentUrls returns nothing for texts with no attachment images", () => {
  expect(attachmentUrls(["plain text", "<p>no images here</p>"])).toEqual([]);
});

/** Astryx's Markdown island (PR threads) blocks a `data:` image src
 * outright - `toBlobImages` swaps it for a `blob:` object URL first, which
 * `swapInlineImages` can then put straight into the markdown text. */
test("toBlobImages converts each data: URI to a blob: object URL, keeping the url unchanged", () => {
  const images = [
    { url: "https://dev.azure.com/x/_apis/wit/attachments/1", data: "data:image/png;base64,aGVsbG8=" },
  ];
  const out = toBlobImages(images);
  expect(out).toHaveLength(1);
  expect(out[0].url).toBe(images[0].url);
  expect(out[0].data.startsWith("blob:")).toBe(true);
});
