import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import NetworkWaitPanel, { waitedLabel } from "./NetworkWaitPanel";

afterEach(() => vi.useRealTimers());

test("says what is wrong, what to do, and when it gives up (minutes rounded up)", () => {
  render(<NetworkWaitPanel giveUpAfterS={1800} onStop={() => {}} />);
  expect(screen.getByText("Paused - the company network is unreachable")).toBeInTheDocument();
  expect(
    screen.getByText("Reconnect the VPN; the run carries on by itself. Gives up in 30 min."),
  ).toBeInTheDocument();
});

test("rounds a part minute up", () => {
  render(<NetworkWaitPanel giveUpAfterS={90} onStop={() => {}} />);
  expect(screen.getByText(/Gives up in 2 min\./)).toBeInTheDocument();
});

test("counts the time waited as it ticks", () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-10-14T10:00:00Z"));
  render(<NetworkWaitPanel giveUpAfterS={1800} since={Date.now() - 65_000} onStop={() => {}} />);
  expect(screen.getByText("Waited 1:05")).toBeInTheDocument();
  act(() => {
    vi.advanceTimersByTime(5000);
  });
  expect(screen.getByText("Waited 1:10")).toBeInTheDocument();
  expect(waitedLabel(0)).toBe("0:00");
});

test("Stop calls onStop and is disabled while the stop is pending", async () => {
  let done = () => {};
  const onStop = vi.fn(() => new Promise<void>((r) => (done = r)));
  render(<NetworkWaitPanel giveUpAfterS={1800} onStop={onStop} />);
  const stop = screen.getByRole("button", { name: /^Stop/ });
  fireEvent.click(stop);
  expect(onStop).toHaveBeenCalledTimes(1);
  expect(stop).toBeDisabled();
  await act(async () => done());
  expect(stop).toBeEnabled();
});
