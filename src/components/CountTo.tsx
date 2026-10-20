// A number that counts up to its value: from 0 when it starts to play (a tab
// opening), and from what it showed when the value changes after. About
// 700 ms, easing out, one requestAnimationFrame step at a time. A re-render
// with the same value starts nothing.
//
// Nothing moves under prefers-reduced-motion, or in the docs' screenshot
// capture: the value is simply there - and a count already going when
// reduced motion is switched on jumps to its value. A screen reader never
// hears the counting - while it runs, the moving digits are aria-hidden and
// the final value is in visually hidden text beside them; at rest there is
// only the value. The app's one counter: every count-up uses it.

import { useLayoutEffect, useRef, useState } from "react";
import { isCaptureMode } from "../dev/capture";

/** How long a count takes, in ms. */
export const COUNT_MS = 700;

const REDUCE = "(prefers-reduced-motion: reduce)";

const still = () => isCaptureMode() || (window.matchMedia?.(REDUCE).matches ?? false);

/** Ease-out: quick at first, settling onto the value. */
const easeOut = (t: number) => 1 - (1 - t) ** 3;

/**
 * The number to show for `value` this frame. `play` false shows the value
 * as it is and remembers nothing; each time it turns true (and on a first
 * mount with it true) the count starts again from 0. While playing, a new
 * value counts on from the number on screen.
 */
export function useCountUp(value: number, play: boolean, ms = COUNT_MS): number {
  const [shown, setShown] = useState(() => (play && !still() ? 0 : value));
  /** The number on screen, for a count that starts mid-way. */
  const onScreen = useRef(shown);
  /** Whether the last run was playing: a run that was not starts from 0. */
  const wasPlaying = useRef(false);

  // A layout effect: a tab shown again is set back to 0 before it paints,
  // so the old value never flashes first.
  useLayoutEffect(() => {
    if (!play) {
      wasPlaying.current = false;
      return;
    }
    const put = (n: number) => {
      onScreen.current = n;
      setShown(n);
    };
    const from = wasPlaying.current ? onScreen.current : 0;
    wasPlaying.current = true;
    if (still() || from === value) {
      put(value);
      return;
    }
    put(from);
    let frame = 0;
    let start: number | null = null;
    // Reduced motion switched on mid-count: the value, at once.
    const query = window.matchMedia?.(REDUCE);
    /** The count is over: no frame waits, nothing listens. */
    const stop = () => {
      cancelAnimationFrame(frame);
      query?.removeEventListener?.("change", onChange);
    };
    const onChange = (e: MediaQueryListEvent) => {
      if (!e.matches) return;
      stop();
      put(value);
    };
    const step = (now: number) => {
      if (start === null) start = now;
      const t = Math.min(1, (now - start) / ms);
      put(t === 1 ? value : Math.round(from + (value - from) * easeOut(t)));
      if (t < 1) frame = requestAnimationFrame(step);
      else stop();
    };
    frame = requestAnimationFrame(step);
    query?.addEventListener?.("change", onChange);
    return stop;
  }, [value, play, ms]);

  return play ? shown : value;
}

/** `value`, counting up to it while `play` (see `useCountUp`). */
export default function CountTo({ value, play }: { value: number; play: boolean }) {
  const shown = useCountUp(value, play);
  if (shown === value) return <>{value}</>;
  return (
    <>
      <span aria-hidden="true">{shown}</span>
      <span className="sr-only">{value}</span>
    </>
  );
}
