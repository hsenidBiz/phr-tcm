import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import EqRing from "./EqRing";
import { BAND_COUNT } from "../lib/audioSpectrum";

afterEach(() => clearMocks());

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
