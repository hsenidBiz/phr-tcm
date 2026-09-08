/**
 * Is this element in view right now?
 *
 * For controls that get a floating copy while the real one is scrolled
 * out of reach: the copy shows while this is false, and stands down the
 * moment the real control is back on screen.
 *
 * Returns a CALLBACK ref, not a plain one, and that is the whole point:
 * the watched element usually is not in the page at mount (the queue's
 * action row only exists once a case is queued). A ref read once inside
 * an effect is null then and never looked at again, so the observer is
 * never created and the answer stays frozen at its default. Holding the
 * node in state means React hands it over the moment it arrives - and
 * hands over null when it leaves.
 *
 * Starts true, and stays true where there is nothing to watch or no
 * IntersectionObserver: a floating button that cannot tell when to leave
 * is worse than no floating button at all.
 */
import { useEffect, useState } from "react";

export function useOnScreen(
  rootMargin = "0px",
): [ref: (node: Element | null) => void, onScreen: boolean] {
  const [node, setNode] = useState<Element | null>(null);
  const [onScreen, setOnScreen] = useState(true);

  useEffect(() => {
    // Nothing to watch, or no way to watch it: fall back to the safe
    // answer rather than leaving whatever the last one was standing.
    if (!node || typeof IntersectionObserver === "undefined") {
      setOnScreen(true);
      return;
    }
    const io = new IntersectionObserver(
      (entries) => setOnScreen(entries[entries.length - 1].isIntersecting),
      { rootMargin },
    );
    io.observe(node);
    return () => io.disconnect();
  }, [node, rootMargin]);

  return [setNode, onScreen];
}
