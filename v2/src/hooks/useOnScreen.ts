/**
 * Is this element in view right now?
 *
 * For controls that get a floating copy while the real one is scrolled
 * out of reach: the copy shows while this is false, and stands down the
 * moment the real control is back on screen.
 *
 * Starts true, and stays true where there is no IntersectionObserver: a
 * floating button that cannot tell when to leave is worse than no
 * floating button at all.
 */
import { useEffect, useState, type RefObject } from "react";

export function useOnScreen(ref: RefObject<Element | null>, rootMargin = "0px"): boolean {
  const [onScreen, setOnScreen] = useState(true);

  useEffect(() => {
    const el = ref.current;
    if (!el || typeof IntersectionObserver === "undefined") return;
    const io = new IntersectionObserver(
      (entries) => setOnScreen(entries[entries.length - 1].isIntersecting),
      { rootMargin },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [ref, rootMargin]);

  return onScreen;
}
