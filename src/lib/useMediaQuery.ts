import { useCallback, useSyncExternalStore } from "react";

/** Whether a CSS media query matches right now, following it as the window
 * changes (a resize across a height or width breakpoint). False where the
 * browser has no `matchMedia`. */
export function useMediaQuery(query: string): boolean {
  const subscribe = useCallback(
    (onChange: () => void) => {
      const mq = window.matchMedia?.(query);
      if (!mq) return () => {};
      if (mq.addEventListener) {
        mq.addEventListener("change", onChange);
        return () => mq.removeEventListener("change", onChange);
      }
      mq.addListener?.(onChange);
      return () => mq.removeListener?.(onChange);
    },
    [query],
  );
  return useSyncExternalStore(subscribe, () => window.matchMedia?.(query).matches ?? false);
}
