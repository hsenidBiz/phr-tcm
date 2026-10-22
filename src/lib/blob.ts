/** Blob -> base64 payload (no data: prefix), for attachment uploads. */
export function blobToB64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(String(r.result).split(",")[1] ?? "");
    r.onerror = () => reject(r.error);
    r.readAsDataURL(blob);
  });
}

/** What turns bytes into "this is a picture". Resolves true for an image
 * the platform can decode, false (or throws) for anything it cannot. */
export type ImageDecoder = (bytes: Uint8Array, mime: string) => Promise<boolean>;

/** How long a decode may take before the image is treated as unreadable. */
export const IMAGE_DECODE_TIMEOUT_MS = 2000;

/** The webview's own decoder: createImageBitmap on the bytes, or - where
 * that does not exist - an Image load of the data URL. */
const decodeInWebview: ImageDecoder = async (bytes, mime) => {
  if (typeof createImageBitmap === "function") {
    const bitmap = await createImageBitmap(new Blob([bytes as BlobPart], { type: mime }));
    const ok = bitmap.width > 0 && bitmap.height > 0;
    bitmap.close?.();
    return ok;
  }
  if (typeof Image === "undefined") return false;
  return new Promise<boolean>((resolve) => {
    const img = new Image();
    img.onload = () => resolve(img.naturalWidth > 0 && img.naturalHeight > 0);
    img.onerror = () => resolve(false);
    let bin = "";
    for (let i = 0; i < bytes.length; i += 0x8000) {
      bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    }
    img.src = `data:${mime};base64,${btoa(bin)}`;
  });
};

const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/** A PNG is whole only if it opens with the signature and ends with its
 * IEND chunk. A paste cut off mid-transfer has the first and not the last,
 * and some decoders show the top of such a file instead of refusing it. */
function pngIsWhole(bytes: Uint8Array): boolean {
  if (bytes.length < PNG_SIGNATURE.length + 12) return false;
  if (!PNG_SIGNATURE.every((b, i) => bytes[i] === b)) return false;
  // IEND is the last chunk: length(4) "IEND" crc(4), so its tag sits 8 bytes
  // from the end. A little slack covers a stray trailing byte or two.
  const tail = bytes.subarray(Math.max(0, bytes.length - 16));
  return String.fromCharCode(...tail).includes("IEND");
}

/**
 * Is this base64 payload (no data: prefix) a whole image? True only for
 * non-empty data that decodes as one, within IMAGE_DECODE_TIMEOUT_MS.
 *
 * A virtual machine's clipboard can offer image/png before the image is
 * ready, so a paste may hold nothing or the front half of a picture. The
 * runner checks every image with this BEFORE attaching it, so nothing
 * broken is attached or uploaded. `decode` is injectable because jsdom
 * cannot decode an image; the default is the webview's own.
 */
export async function validImageB64(
  b64: string,
  mime = "image/png",
  decode: ImageDecoder = decodeInWebview,
): Promise<boolean> {
  const clean = b64.trim();
  if (!clean) return false;
  let bytes: Uint8Array;
  try {
    bytes = Uint8Array.from(atob(clean), (c) => c.charCodeAt(0));
  } catch {
    return false; // not base64
  }
  if (bytes.length === 0) return false;
  if (mime === "image/png" && !pngIsWhole(bytes)) return false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      decode(bytes, mime).catch(() => false),
      new Promise<boolean>((resolve) => {
        timer = setTimeout(() => resolve(false), IMAGE_DECODE_TIMEOUT_MS);
      }),
    ]);
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}
