import { afterEach, expect, test, vi } from "vitest";
import {
  CLIPBOARD_RETRIES,
  firstValidClipboardImage,
  imageOfPasteFile,
  type ClipboardImage,
} from "./clipboardImage";

// jsdom cannot decode an image, so the decoder is one that accepts whatever
// the structural checks let through.
vi.mock("./blob", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./blob")>();
  return {
    ...actual,
    validImageB64: (b64: string, mime?: string) => actual.validImageB64(b64, mime, async () => true),
  };
});

/** A real 1x1 PNG (69 bytes), base64. */
const PNG_B64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
const WHOLE: ClipboardImage = { b64: PNG_B64, mime: "image/png" };
const CUT: ClipboardImage = { b64: PNG_B64.slice(0, 40), mime: "image/png" };
const EMPTY: ClipboardImage = { b64: "", mime: "image/png" };

function clipboardAnswering(images: Array<ClipboardImage | null>) {
  let n = 0;
  const read = vi.fn(async () => {
    const img = images[Math.min(n++, images.length - 1)];
    if (!img) return [];
    const bytes = Uint8Array.from(atob(img.b64), (c) => c.charCodeAt(0));
    return [{ types: [img.mime], getType: async () => new Blob([bytes as BlobPart], { type: img.mime }) }];
  });
  Object.defineProperty(navigator, "clipboard", { value: { read }, configurable: true });
  return read;
}

afterEach(() => {
  vi.useRealTimers();
  Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
});

async function settle<T>(p: Promise<T>): Promise<T> {
  await vi.advanceTimersByTimeAsync(CLIPBOARD_RETRIES * 300 + 50);
  return p;
}

test("a whole first image is returned at once, with no re-read", async () => {
  const read = clipboardAnswering([WHOLE]);
  const reread = vi.fn(async () => WHOLE);
  expect(await firstValidClipboardImage(WHOLE, reread)).toEqual(WHOLE);
  expect(reread).not.toHaveBeenCalled();
  expect(read).not.toHaveBeenCalled();
});

test("the event's own file is re-read first, and the async clipboard is left alone", async () => {
  vi.useFakeTimers();
  const read = clipboardAnswering([WHOLE]);
  // The file is cut off at first and whole a moment later.
  const reread = vi.fn().mockResolvedValueOnce(CUT).mockResolvedValue(WHOLE);
  const got = await settle(firstValidClipboardImage(CUT, reread));
  expect(got).toEqual(WHOLE);
  expect(reread).toHaveBeenCalledTimes(2);
  expect(read).not.toHaveBeenCalled();
});

test("an event file that is never whole gives nothing, and the async clipboard is still not read", async () => {
  vi.useFakeTimers();
  const read = clipboardAnswering([WHOLE]);
  const reread = vi.fn(async () => CUT);
  expect(await settle(firstValidClipboardImage(CUT, reread))).toBeNull();
  expect(reread).toHaveBeenCalledTimes(CLIPBOARD_RETRIES);
  expect(read).not.toHaveBeenCalled();
});

test("the async clipboard is read only when the event's own data is empty", async () => {
  vi.useFakeTimers();
  const read = clipboardAnswering([EMPTY, WHOLE]);
  const reread = vi.fn(async () => EMPTY);
  const got = await settle(firstValidClipboardImage(EMPTY, reread));
  expect(got).toEqual(WHOLE);
  expect(read).toHaveBeenCalledTimes(2);
});

test("without an event (the Paste button) only the async clipboard is read, and where it does not exist nothing is", async () => {
  vi.useFakeTimers();
  const read = clipboardAnswering([EMPTY, WHOLE]);
  expect(await settle(firstValidClipboardImage(EMPTY))).toEqual(WHOLE);
  expect(read).toHaveBeenCalledTimes(2);

  Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
  // Nothing to wait for: it answers without a single timer.
  expect(await firstValidClipboardImage(EMPTY)).toBeNull();
});

test("a JPEG is judged as a JPEG on every path", async () => {
  vi.useFakeTimers();
  const jpeg = new Uint8Array([0xff, 0xd8, 0xff, 0xe0, 0, 16, 0x4a, 0x46, 0x49, 0x46, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0xff, 0xd9]);
  const img: ClipboardImage = { b64: btoa(String.fromCharCode(...jpeg)), mime: "image/jpeg" };
  expect(await firstValidClipboardImage(img)).toEqual(img);
  // The same bytes claimed as a PNG are not valid.
  expect(await settle(firstValidClipboardImage({ ...img, mime: "image/png" }, async () => null))).toBeNull();
});

test("a paste file with no type takes the clipboard item's, and a missing file is an empty image", async () => {
  const jpeg = new Blob([new Uint8Array([0xff, 0xd8, 0xff, 0xd9])]); // no type of its own
  expect(await imageOfPasteFile(jpeg, "image/jpeg")).toEqual({ b64: "/9j/2Q==", mime: "image/jpeg" });
  expect(await imageOfPasteFile(null, "image/png")).toEqual({ b64: "", mime: "image/png" });
  const typed = new Blob([new Uint8Array([1])], { type: "image/gif" });
  expect((await imageOfPasteFile(typed, "image/png")).mime).toBe("image/gif");
});
