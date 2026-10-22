import { blobToB64, validImageB64 } from "./blob";

/** An image off the clipboard: its base64 payload (no data: prefix) and the
 * real MIME type it came with, which decides how it is checked and named. */
export type ClipboardImage = { b64: string; mime: string };

/** A virtual machine's clipboard sync can offer an image before it is
 * ready, so the first read is empty or cut short and the data lands a
 * moment later. The clipboard is read again, this many times this far apart. */
export const CLIPBOARD_RETRIES = 3;
export const CLIPBOARD_RETRY_MS = 300;

/** Said when no whole image ever turned up. Nothing has been attached. */
export const NOT_ON_CLIPBOARD_YET = "The screenshot had not reached the clipboard yet - paste again.";

/** The first image on the async clipboard, or null when it holds none. */
export async function readClipboardImage(): Promise<ClipboardImage | null> {
  const items = await navigator.clipboard.read();
  for (const item of items) {
    const type = item.types.find((t) => t.startsWith("image/"));
    if (!type) continue;
    const blob = await item.getType(type);
    return { b64: await blobToB64(blob), mime: blob.type || type };
  }
  return null;
}

/** A paste event's file as an image. A missing file, or one that cannot be
 * read, is an empty image (b64 ""), which is never valid. The MIME type is
 * the file's own, else the clipboard item's. */
export async function imageOfPasteFile(blob: Blob | null, itemType: string): Promise<ClipboardImage> {
  if (!blob) return { b64: "", mime: itemType };
  return { b64: await blobToB64(blob).catch(() => ""), mime: blob.type || itemType };
}

const isWhole = (img: ClipboardImage) => validImageB64(img.b64, img.mime);

/**
 * The first whole image the clipboard yields: `first` if it is one, else
 * the first whole one within CLIPBOARD_RETRIES more tries. Null when none
 * is.
 *
 * `rereadOwn` re-reads the PASTE EVENT's own file, which needs no
 * permission and is the image the person actually pasted. It is tried
 * first on every retry, and the async clipboard (`navigator.clipboard.read`,
 * which some WebView2 setups prompt for or refuse, and which would hand
 * back a different screenshot if one was copied meanwhile) is read only
 * when the event's own data is empty. Without `rereadOwn` (the Paste
 * button, the snip) the async clipboard is the only source; where it does
 * not exist at all, `first` is the only candidate.
 */
export async function firstValidClipboardImage(
  first: ClipboardImage | null,
  rereadOwn?: () => Promise<ClipboardImage | null>,
): Promise<ClipboardImage | null> {
  if (first && (await isWhole(first))) return first;
  const canReadClipboard = typeof navigator.clipboard?.read === "function";
  if (!rereadOwn && !canReadClipboard) return null;
  for (let i = 0; i < CLIPBOARD_RETRIES; i++) {
    await new Promise((res) => setTimeout(res, CLIPBOARD_RETRY_MS));
    if (rereadOwn) {
      const own = await rereadOwn().catch(() => null);
      if (own && own.b64) {
        // The event has data, just not all of it yet: wait for it rather
        // than swapping in whatever the clipboard holds by now.
        if (await isWhole(own)) return own;
        continue;
      }
    }
    if (canReadClipboard) {
      const again = await readClipboardImage().catch(() => null);
      if (again && (await isWhole(again))) return again;
    }
  }
  return null;
}
