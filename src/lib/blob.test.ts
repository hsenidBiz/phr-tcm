import { afterEach, expect, test, vi } from "vitest";
import { blobToB64, imageExt, imageMime, imageMimeForName, validImageB64 } from "./blob";

/** A real 1x1 PNG (69 bytes), base64. */
const PNG_B64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
const pngBytes = () => Uint8Array.from(atob(PNG_B64), (c) => c.charCodeAt(0));
const toB64 = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes));

afterEach(() => {
  vi.useRealTimers();
});

// jsdom cannot decode an image, so a decoder is injected for the cases that
// reach it; the checks that run before it need none.

test("an empty blob reads as an empty payload - the broken attachment's source", async () => {
  expect(await blobToB64(new Blob([], { type: "image/png" }))).toBe("");
});

test("validImageB64 is false for an empty payload and never asks the decoder", async () => {
  const decoder = vi.fn(async () => true);
  expect(await validImageB64("", "image/png", decoder)).toBe(false);
  expect(await validImageB64("   ", "image/png", decoder)).toBe(false);
  expect(decoder).not.toHaveBeenCalled();
});

test("validImageB64 is false for text that is not base64", async () => {
  expect(await validImageB64("%%% not base64 %%%", "image/png", async () => true)).toBe(false);
});

test("validImageB64 is false for a truncated PNG, even if a decoder would accept it", async () => {
  const cut = toB64(pngBytes().slice(0, 40));
  expect(await validImageB64(cut, "image/png", async () => true)).toBe(false);
  expect(await validImageB64(cut)).toBe(false);
  // Bytes that are not a PNG at all.
  expect(await validImageB64(toB64(new Uint8Array([1, 2, 3, 4])), "image/png", async () => true)).toBe(false);
});

test("validImageB64 is true for a real 1x1 PNG the decoder accepts", async () => {
  const decoder = vi.fn(async () => true);
  expect(await validImageB64(PNG_B64, "image/png", decoder)).toBe(true);
  expect(decoder).toHaveBeenCalledTimes(1);
});

test("validImageB64 is false when the decoder refuses or throws", async () => {
  expect(await validImageB64(PNG_B64, "image/png", async () => false)).toBe(false);
  expect(
    await validImageB64(PNG_B64, "image/png", async () => {
      throw new Error("could not be decoded");
    }),
  ).toBe(false);
});

test("validImageB64 gives up on a decoder that never answers after 2 seconds", async () => {
  vi.useFakeTimers();
  const pending = validImageB64(PNG_B64, "image/png", () => new Promise<boolean>(() => {}));
  await vi.advanceTimersByTimeAsync(1999);
  let settled = false;
  void pending.then(() => (settled = true));
  await vi.advanceTimersByTimeAsync(0);
  expect(settled).toBe(false);
  await vi.advanceTimersByTimeAsync(2);
  expect(await pending).toBe(false);
});

test("the default decoder rejects where the platform has no way to decode", async () => {
  // jsdom has no createImageBitmap, and its Image never loads: with no
  // decoder to trust, an image is not vouched for.
  vi.useFakeTimers();
  const pending = validImageB64(PNG_B64);
  await vi.advanceTimersByTimeAsync(2500);
  expect(await pending).toBe(false);
});

// ---- Every picture type, judged by its own signature and end ----

const bytes = (...n: number[]) => new Uint8Array(n);
const ascii = (s: string) => Uint8Array.from(s, (c) => c.charCodeAt(0));
const cat = (...parts: Uint8Array[]) => Uint8Array.from(parts.flatMap((p) => [...p]));

/** A minimal whole JPEG: start of image, a JFIF header, end of image. */
const JPEG = bytes(0xff, 0xd8, 0xff, 0xe0, 0, 16, 0x4a, 0x46, 0x49, 0x46, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0xff, 0xd9);
const GIF = cat(ascii("GIF89a"), bytes(1, 0, 1, 0, 0, 0, 0, 0x2c, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3b));
const BMP = cat(ascii("BM"), bytes(30, 0, 0, 0), new Uint8Array(24));
const WEBP = cat(ascii("RIFF"), bytes(8, 0, 0, 0), ascii("WEBPVP8 "));

test("a whole JPEG, GIF, BMP and WebP are each valid under their own type", async () => {
  const ok = async () => true;
  expect(await validImageB64(toB64(JPEG), "image/jpeg", ok)).toBe(true);
  expect(await validImageB64(toB64(GIF), "image/gif", ok)).toBe(true);
  expect(await validImageB64(toB64(BMP), "image/bmp", ok)).toBe(true);
  expect(await validImageB64(toB64(WEBP), "image/webp", ok)).toBe(true);
  // The common spelling "image/jpg" is the same type.
  expect(await validImageB64(toB64(JPEG), "image/jpg", ok)).toBe(true);
});

test("a cut-off JPEG, GIF, BMP or WebP is refused even if a decoder would accept it", async () => {
  const ok = async () => true;
  expect(await validImageB64(toB64(JPEG.slice(0, 14)), "image/jpeg", ok)).toBe(false);
  expect(await validImageB64(toB64(GIF.slice(0, 16)), "image/gif", ok)).toBe(false);
  expect(await validImageB64(toB64(BMP.slice(0, 28)), "image/bmp", ok)).toBe(false);
  expect(await validImageB64(toB64(WEBP.slice(0, 14)), "image/webp", ok)).toBe(false);
});

test("a picture is judged by the type it came with, not as a PNG", async () => {
  const ok = async () => true;
  // The same JPEG bytes are not a PNG...
  expect(await validImageB64(toB64(JPEG), "image/png", ok)).toBe(false);
  // ...and a PNG's bytes are not a JPEG.
  expect(await validImageB64(PNG_B64, "image/jpeg", ok)).toBe(false);
});

test("a type that is not a picture we attach is refused", async () => {
  const ok = async () => true;
  expect(await validImageB64(PNG_B64, "image/svg+xml", ok)).toBe(false);
  expect(await validImageB64(PNG_B64, "image/tiff", ok)).toBe(false);
  expect(await validImageB64(PNG_B64, "text/plain", ok)).toBe(false);
});

test("the extension and type helpers agree on every supported type", () => {
  expect(imageExt("image/jpeg")).toBe("jpg");
  expect(imageExt("image/jpg")).toBe("jpg");
  expect(imageExt("image/png")).toBe("png");
  expect(imageExt("image/webp")).toBe("webp");
  expect(imageMime("IMAGE/JPEG; charset=x")).toBe("image/jpeg");
  expect(imageMime("application/pdf")).toBeNull();
  expect(imageMimeForName("photo.JPG")).toBe("image/jpeg");
  expect(imageMimeForName("shot.png")).toBe("image/png");
  expect(imageMimeForName("anim.gif")).toBe("image/gif");
});

test("validImageB64 waits as long as it is told to for the decoder", async () => {
  vi.useFakeTimers();
  const slow = () => new Promise<boolean>((res) => setTimeout(() => res(true), 5000));
  const pending = validImageB64(PNG_B64, "image/png", slow, 10_000);
  await vi.advanceTimersByTimeAsync(5001);
  expect(await pending).toBe(true);
});
