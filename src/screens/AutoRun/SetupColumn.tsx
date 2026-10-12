// The right-hand column the Setup panel lives in. Beside the list (the
// two-column layout) the column runs the list's whole height and the panel
// follows the scroll down it, a little behind (followScroll). Above the
// list, the column is only as tall as the panel, so it stays put. Under
// prefers-reduced-motion the panel is simply sticky: no animation.

import { useEffect, useRef, useState, type ReactNode } from "react";
import { prefersReducedMotion, useFollowScroll } from "./followScroll";

/** Whether the person asked for less motion, kept current. */
function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(prefersReducedMotion);
  useEffect(() => {
    const query = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    if (!query?.addEventListener) return;
    const changed = () => setReduced(query.matches);
    query.addEventListener("change", changed);
    return () => query.removeEventListener("change", changed);
  }, []);
  return reduced;
}

export default function SetupColumn({ children }: { children: ReactNode }) {
  const column = useRef<HTMLDivElement | null>(null);
  const panel = useRef<HTMLDivElement | null>(null);
  const reduced = useReducedMotion();
  useFollowScroll(column, panel, reduced);
  return (
    <div ref={column} className="min-w-0 @min-[69rem]:order-last @min-[69rem]:self-stretch" data-follow={reduced ? "sticky" : "eased"}>
      <div ref={panel} className={reduced ? "@min-[69rem]:sticky @min-[69rem]:top-4" : "will-change-transform"}>
        {children}
      </div>
    </div>
  );
}
