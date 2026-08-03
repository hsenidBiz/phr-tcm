/**
 * Dev-only fake latency for every IPC command.
 *
 * On a dev machine everything answers instantly - localhost bridge, demo
 * data, warm caches - so every skeleton, spinner and progress state ships
 * having never once been seen. This wraps each command with a delay so the
 * in-between states are lived with, not imagined.
 *
 * Dev build only: imported through the same statically-false branch as the
 * demo module, so none of this exists in a release. The delay is read live
 * per call, so moving the DevPanel knob applies to the very next request -
 * no reload.
 */
import { commands } from "../bindings";

const KEY = "tcm-v2-dev-latency";

/** The delays on offer. 0 is off; 3000 finds the spinners nobody added. */
export const LATENCY_STEPS = [0, 300, 1000, 3000] as const;

export function latencyMs(): number {
  try {
    const n = Number(localStorage.getItem(KEY) ?? "0");
    return Number.isFinite(n) && n > 0 ? n : 0;
  } catch {
    return 0;
  }
}

export function setLatencyMs(ms: number): void {
  try {
    if (ms > 0) localStorage.setItem(KEY, String(ms));
    else localStorage.removeItem(KEY);
  } catch {
    // storage unavailable - the knob just won't persist
  }
}

/**
 * Wrap every command with the delay. Called AFTER the demo patches, so the
 * wrappers close over whichever implementation is live - real IPC or the
 * demo fakes - and both get the same treatment.
 */
export function applyDevLatency(): void {
  for (const [name, fn] of Object.entries(commands)) {
    if (typeof fn !== "function") continue;
    (commands as Record<string, unknown>)[name] = async (...args: unknown[]) => {
      const ms = latencyMs();
      if (ms > 0) await new Promise((r) => setTimeout(r, ms));
      return (fn as (...a: unknown[]) => unknown)(...args);
    };
  }
}
