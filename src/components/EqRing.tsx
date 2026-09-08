import { useEffect, useMemo, useRef } from "react";
import { BAND_COUNT, useAudioSpectrum } from "../lib/audioSpectrum";

/**
 * A circular equalizer: BAND_COUNT radial bars around the flask that dance
 * to whatever audio the system is playing (WASAPI loopback streamed from
 * Rust). Silent system or capture failure = bars rest at a hairline
 * baseline, so the ambient animation still carries the scene. Decorative:
 * aria-hidden, and disabled entirely under prefers-reduced-motion.
 */
export default function EqRing({
  radius,
  maxLen = 26,
  color = "currentColor",
}: {
  /** Distance from center to a bar's inner end, in px. */
  radius: number;
  /** Full-scale bar length in px. */
  maxLen?: number;
  color?: string;
}) {
  const reduced = useMemo(
    () => window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false,
    [],
  );
  const spectrum = useAudioSpectrum(!reduced);
  const wrap = useRef<HTMLDivElement>(null);
  const active = useRef(false);

  useEffect(() => {
    if (reduced) return;
    let raf = 0;
    const setActive = (on: boolean) => {
      active.current = on;
      const parent = wrap.current?.parentElement;
      // The host (.login-flask / .empty-art) hides its orbit/halo circles
      // via [data-eq-active="true"] while the equalizer is dancing.
      if (parent) parent.dataset.eqActive = on ? "true" : "false";
    };
    const tick = () => {
      const bars = wrap.current?.children;
      if (bars) {
        let max = 0;
        for (let i = 0; i < bars.length; i++) {
          const v = spectrum[i] ?? 0;
          if (v > max) max = v;
          const el = bars[i] as HTMLElement;
          el.style.transform = `rotate(${(i * 360) / BAND_COUNT}deg) translateY(${-radius}px) scaleY(${(0.06 + v * 0.94).toFixed(3)})`;
          // Silent band = fully invisible bar (no resting baseline).
          el.style.opacity = v < 0.02 ? "0" : (0.2 + v * 0.7).toFixed(2);
        }
        // Hysteresis so the orbits don't flicker between quiet passages.
        const on = max > (active.current ? 0.02 : 0.05);
        if (on !== active.current) setActive(on);
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(raf);
      setActive(false);
    };
  }, [reduced, spectrum, radius]);

  if (reduced) return null;

  return (
    <div
      ref={wrap}
      aria-hidden
      data-testid="eq-ring"
      className="pointer-events-none absolute inset-0"
    >
      {Array.from({ length: BAND_COUNT }, (_, i) => (
        <span
          key={i}
          style={{
            position: "absolute",
            left: "50%",
            top: "50%",
            width: 3,
            height: maxLen,
            marginLeft: -1.5,
            marginTop: -maxLen, // bar grows outward from the ring
            borderRadius: 2,
            background: color,
            opacity: 0,
            transform: `rotate(${(i * 360) / BAND_COUNT}deg) translateY(${-radius}px) scaleY(0.06)`,
            transformOrigin: "50% 100%",
          }}
        />
      ))}
    </div>
  );
}
