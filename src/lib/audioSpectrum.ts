import { useEffect, useRef } from "react";
import { commands, events } from "../bindings";

/** Bands the Rust capture emits (keep in sync with audio.rs BAND_COUNT). */
export const BAND_COUNT = 36;

// One capture + one event subscription per webview, shared by every ring
// via a module-level refcount; `latest` holds the newest raw frame.
const latest = new Float32Array(BAND_COUNT);
let refs = 0;
let unlisten: (() => void) | null = null;

async function acquire() {
  refs++;
  if (refs > 1) return;
  try {
    await commands.audioCaptureStart(); // decorative: ignore failures
    unlisten = await events.audioSpectrum.listen((e) => {
      const bands = e.payload.bands;
      for (let i = 0; i < BAND_COUNT; i++) latest[i] = bands[i] ?? 0;
    });
  } catch {
    // no audio device / capture unsupported - bars stay at baseline
  }
}

function release() {
  refs = Math.max(0, refs - 1);
  if (refs > 0) return;
  try {
    // The unlisten fn invokes the event plugin and can reject (teardown,
    // window closing) - decorative feature, swallow either failure mode.
    void Promise.resolve(unlisten?.() as unknown).catch(() => {});
  } catch {
    // synchronous throw - same story
  }
  unlisten = null;
  latest.fill(0);
  commands.audioCaptureStop().catch(() => {});
}

/**
 * Subscribe to the system-audio spectrum while mounted. Returns a stable
 * Float32Array that is smoothed toward the newest frame every animation
 * frame (fast attack, slow release) - read it inside your own rAF loop;
 * it never triggers React re-renders.
 */
export function useAudioSpectrum(enabled: boolean): Float32Array {
  const smoothed = useRef(new Float32Array(BAND_COUNT)).current;

  useEffect(() => {
    if (!enabled) return;
    let raf = 0;
    acquire();
    const tick = () => {
      for (let i = 0; i < BAND_COUNT; i++) {
        const target = latest[i];
        const cur = smoothed[i];
        smoothed[i] = cur + (target - cur) * (target > cur ? 0.5 : 0.12);
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(raf);
      release();
    };
  }, [enabled, smoothed]);

  return smoothed;
}
