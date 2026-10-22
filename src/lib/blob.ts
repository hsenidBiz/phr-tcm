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

/** The picture types a result or a bug can carry, each with the extension
 * its file is named with. Anything else is not attached. */
const IMAGE_TYPES: Record<string, string> = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/gif": "gif",
  "image/bmp": "bmp",
  "image/webp": "webp",
};

/** The supported MIME type a clipboard item or file names ("image/jpg" and
 * parameters included), or null for a type that is not a picture we attach. */
export function imageMime(type: string): string | null {
  const t = type.split(";")[0].trim().toLowerCase();
  const mime = t === "image/jpg" || t === "image/pjpeg" ? "image/jpeg" : t === "image/x-png" ? "image/png" : t;
  return mime in IMAGE_TYPES ? mime : null;
}

/** The file extension for a supported MIME type (png when it is not one). */
export function imageExt(mime: string): string {
  return IMAGE_TYPES[imageMime(mime) ?? ""] ?? "png";
}

/** The MIME type a file name's extension stands for (png when it has none
 * we know), so a thumbnail and the upload check read an image the way it
 * was named. */
export function imageMimeForName(name: string): string {
  if (/\.jpe?g$/i.test(name)) return "image/jpeg";
  if (/\.gif$/i.test(name)) return "image/gif";
  if (/\.bmp$/i.test(name)) return "image/bmp";
  if (/\.webp$/i.test(name)) return "image/webp";
  return "image/png";
}

const startsWith = (bytes: Uint8Array, head: number[]) => head.every((b, i) => bytes[i] === b);
const ascii = (bytes: Uint8Array, from: number, to: number) =>
  String.fromCharCode(...bytes.subarray(from, to));
const u32le = (b: Uint8Array, at: number) => (b[at] | (b[at + 1] << 8) | (b[at + 2] << 16) | (b[at + 3] << 24)) >>> 0;

/** Does `bytes` open with the signature of `mime` and reach its end? A
 * paste cut off mid-transfer has the front of a picture and not the back,
 * and some decoders show the top of such a file instead of refusing it, so
 * the structure is checked before any decode. The same rule the Rust side
 * applies before it uploads a bug's screenshot (`bug_image_ext`). */
function imageIsWhole(bytes: Uint8Array, mime: string): boolean {
  switch (mime) {
    case "image/png": {
      if (bytes.length < PNG_SIGNATURE.length + 12 || !startsWith(bytes, PNG_SIGNATURE)) return false;
      // IEND is the last chunk: length(4) "IEND" crc(4), so its tag sits 8
      // bytes from the end. A little slack covers a stray trailing byte.
      return ascii(bytes, Math.max(0, bytes.length - 16), bytes.length).includes("IEND");
    }
    case "image/jpeg": {
      if (!startsWith(bytes, [0xff, 0xd8, 0xff])) return false;
      let end = bytes.length;
      while (end > 0 && bytes[end - 1] === 0) end--; // padding after the end marker
      return end >= 2 && bytes[end - 2] === 0xff && bytes[end - 1] === 0xd9;
    }
    case "image/gif": {
      const head = ascii(bytes, 0, 6);
      return (head === "GIF87a" || head === "GIF89a") && bytes[bytes.length - 1] === 0x3b;
    }
    case "image/bmp":
      // The file size the header declares; a few writers leave it 0.
      return bytes.length >= 26 && ascii(bytes, 0, 2) === "BM" && u32le(bytes, 2) <= bytes.length;
    case "image/webp":
      return (
        bytes.length >= 12 &&
        ascii(bytes, 0, 4) === "RIFF" &&
        ascii(bytes, 8, 12) === "WEBP" &&
        u32le(bytes, 4) + 8 <= bytes.length
      );
    default:
      return false;
  }
}

/**
 * Is this base64 payload (no data: prefix) a whole picture of type `mime`
 * (the real type of the clipboard item or file)? True only for non-empty
 * data that is a whole PNG, JPEG, GIF, BMP or WebP and that decodes as one,
 * within `timeoutMs`.
 *
 * A virtual machine's clipboard can offer an image before it is ready, so
 * a paste may hold nothing or the front half of a picture. Every image is
 * checked with this BEFORE it is attached, so nothing broken is attached
 * or uploaded. `decode` is injectable because jsdom cannot decode an
 * image; the default is the webview's own.
 */
export async function validImageB64(
  b64: string,
  mime = "image/png",
  decode: ImageDecoder = decodeInWebview,
  timeoutMs = IMAGE_DECODE_TIMEOUT_MS,
): Promise<boolean> {
  const type = imageMime(mime);
  if (!type) return false;
  const clean = b64.trim();
  if (!clean) return false;
  let bytes: Uint8Array;
  try {
    bytes = Uint8Array.from(atob(clean), (c) => c.charCodeAt(0));
  } catch {
    return false; // not base64
  }
  if (bytes.length === 0 || !imageIsWhole(bytes, type)) return false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      decode(bytes, type).catch(() => false),
      new Promise<boolean>((resolve) => {
        timer = setTimeout(() => resolve(false), timeoutMs);
      }),
    ]);
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}
