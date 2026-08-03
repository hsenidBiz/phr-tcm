import { afterEach, expect, test, vi } from "vitest";
import { commands } from "../bindings";
import { applyDevLatency, latencyMs, setLatencyMs, LATENCY_STEPS } from "./latency";

// This file gets its own vitest environment, so wrapping the shared
// `commands` object here cannot leak into other suites.

afterEach(() => {
  localStorage.clear();
  vi.useRealTimers();
});

test("the knob persists, and junk reads as off", () => {
  expect(latencyMs()).toBe(0);
  setLatencyMs(300);
  expect(latencyMs()).toBe(300);
  setLatencyMs(0);
  expect(latencyMs()).toBe(0);
  localStorage.setItem("tcm-v2-dev-latency", "banana");
  expect(latencyMs()).toBe(0);
  expect(LATENCY_STEPS[0]).toBe(0);
});

test("a wrapped command waits the configured delay, read live per call", async () => {
  vi.useFakeTimers();
  let calls = 0;
  (commands as Record<string, unknown>).ping = () => {
    calls += 1;
    return Promise.resolve("pong");
  };
  applyDevLatency();

  setLatencyMs(1000);
  const p = (commands as unknown as { ping: () => Promise<string> }).ping();
  // The real command must NOT have run yet - the delay comes first, so
  // loading states actually appear.
  await Promise.resolve();
  expect(calls).toBe(0);
  await vi.advanceTimersByTimeAsync(1000);
  expect(calls).toBe(1);
  await expect(p).resolves.toBe("pong");

  // Turning the knob off applies to the NEXT call - no reload, no rewrap.
  setLatencyMs(0);
  await (commands as unknown as { ping: () => Promise<string> }).ping();
  expect(calls).toBe(2);
});
