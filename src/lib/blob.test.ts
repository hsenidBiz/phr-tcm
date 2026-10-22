import { afterEach, expect, test, vi } from "vitest";
import { blobToB64, validImageB64 } from "./blob";

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
