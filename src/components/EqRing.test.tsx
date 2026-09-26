import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import EqRing from "./EqRing";
import { BAND_COUNT } from "../lib/audioSpectrum";

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.unstubAllEnvs();
});

test("renders one bar per band and starts/stops the loopback capture", async () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    return null;
  });

  const { unmount } = render(<EqRing radius={80} />);
  const ring = screen.getByTestId("eq-ring");
  expect(ring.children).toHaveLength(BAND_COUNT);
  expect(ring).toHaveAttribute("aria-hidden", "true");
  await waitFor(() => expect(calls).toContain("audio_capture_start"));

  unmount();
  await waitFor(() => expect(calls).toContain("audio_capture_stop"));
});

// Screenshots for the help site are taken in capture mode on a real
// machine: the ring must not dance to whatever that machine is playing.
test("draws nothing and never starts the loopback capture in capture mode", async () => {
  vi.stubEnv("DEV", true);
  localStorage.setItem("tcm-v2-dev-capture", "on");
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    return cmd === "plugin:event|listen" ? 1 : null;
  });

  const { unmount } = render(<EqRing radius={80} />);
  expect(screen.queryByTestId("eq-ring")).toBeNull();
  await new Promise((r) => setTimeout(r, 20));
  unmount();
  expect(calls).not.toContain("audio_capture_start");
});
