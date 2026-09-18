import { renderHook } from "@testing-library/react";
import { expect, test, vi } from "vitest";

const h = vi.hoisted(() => ({
  starts: [] as Array<() => void>,
  listen: vi.fn(),
  unlisten: vi.fn(),
  stop: vi.fn(() => Promise.resolve()),
}));

vi.mock("../bindings", () => ({
  commands: {
    audioCaptureStart: () => new Promise<void>((resolve) => h.starts.push(resolve)),
    audioCaptureStop: h.stop,
  },
  events: { audioSpectrum: { listen: h.listen } },
}));

import { useAudioSpectrum } from "./audioSpectrum";

const settle = () => new Promise((r) => setTimeout(r, 0));

test("unmounting before the audio spectrum capture has started leaves no listener behind", async () => {
  h.listen.mockReset().mockImplementation(() => Promise.resolve(h.unlisten));
  h.unlisten.mockReset();
  const { unmount } = renderHook(() => useAudioSpectrum(true));
  unmount(); // released while the start call is still in flight
  h.starts.shift()!();
  await settle();
  expect(h.listen.mock.calls.length).toBe(h.unlisten.mock.calls.length);
});

test("a normal mount listens once and unmount removes it", async () => {
  h.listen.mockReset().mockImplementation(() => Promise.resolve(h.unlisten));
  h.unlisten.mockReset();
  const { unmount } = renderHook(() => useAudioSpectrum(true));
  h.starts.shift()!();
  await settle();
  expect(h.listen).toHaveBeenCalledTimes(1);
  unmount();
  expect(h.unlisten).toHaveBeenCalledTimes(1);
});
