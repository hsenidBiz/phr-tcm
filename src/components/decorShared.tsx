import { createContext, useContext, useEffect, useRef, useState } from "react";
import { isCaptureMode } from "../dev/capture";

/** What the seasonal scenery (HalloweenDecor, ChristmasDecor) shares: the
 * theme switch, the reduced-motion setting, the brief-state timer and the
 * document listener that lets a decoration answer a press on the page under
 * it without ever taking a click itself. */

export const HIT_PAD = 4;
export const INTERACTIVE =
  'button, a, input, select, textarea, label, summary, [role="button"], [role="link"], [role="menuitem"], [role="option"], [role="tab"], [role="checkbox"], [role="switch"], [role="combobox"], [contenteditable=""], [contenteditable="true"], [tabindex]:not([tabindex="-1"])';

/** A decoration's clickable body and what it does when pressed. */
export type Hit = { el: Element; react: () => void };
export type Register = (hit: Hit) => () => void;
export const HitContext = createContext<Register>(() => () => {});

/** Registers a body with the decor's document listener; `react` may change
 * every render. */
export function useHit(body: React.RefObject<Element | null>, react: () => void) {
  const register = useContext(HitContext);
  const latest = useRef(react);
  latest.current = react;
  useEffect(() => {
    const el = body.current;
    return el ? register({ el, react: () => latest.current() }) : undefined;
  }, [register, body]);
}

const REDUCE = "(prefers-reduced-motion: reduce)";

function prefersReducedMotion(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia(REDUCE).matches;
}

/** The reduced-motion setting, live. */
export function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(prefersReducedMotion);
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const mq = window.matchMedia(REDUCE);
    if (typeof mq?.addEventListener !== "function") return;
    const sync = () => setReduced(mq.matches);
    mq.addEventListener("change", sync);
    sync();
    return () => mq.removeEventListener("change", sync);
  }, []);
  return reduced;
}

/** True while `theme` is the document's theme (and this is not a docs
 * capture, which keeps the app as a normal install shows it). */
export function useThemeIs(theme: string): boolean {
  const is = () => document.documentElement.getAttribute("data-theme") === theme;
  const [on, setOn] = useState(is);

  useEffect(() => {
    const sync = () => setOn(is());
    sync();
    const observer = new MutationObserver(sync);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  }, [theme]);

  return on && !isCaptureMode();
}

/** True for `ms` after start(); a new start restarts it, unmounting cancels.
 * `live` reads true at once, for guards that run before a re-render. */
export function useBriefState(ms: number): [boolean, () => void, React.RefObject<boolean>] {
  const [active, setActive] = useState(false);
  const live = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const start = () => {
    clearTimeout(timer.current);
    live.current = true;
    setActive(true);
    timer.current = setTimeout(() => {
      live.current = false;
      setActive(false);
    }, ms);
  };
  return [active, start, live];
}

/** The decor's registry plus, while `on`, the one document listener: a press
 * on the page under a registered body sets it off, unless the press landed on
 * a control. Returns the register function for HitContext. */
export function useDecorPresses(on: boolean): Register {
  const hits = useRef(new Set<Hit>());
  const register = useRef<Register>((hit) => {
    hits.current.add(hit);
    return () => void hits.current.delete(hit);
  });

  useEffect(() => {
    if (!on) return;
    const onDown = (e: PointerEvent) => {
      const target = e.target;
      if (e.button !== 0 || !(target instanceof Element) || target.closest(INTERACTIVE)) return;
      for (const { el, react } of hits.current) {
        const r = el.getBoundingClientRect();
        if (
          e.clientX >= r.left - HIT_PAD &&
          e.clientX <= r.right + HIT_PAD &&
          e.clientY >= r.top - HIT_PAD &&
          e.clientY <= r.bottom + HIT_PAD
        ) {
          react();
        }
      }
    };
    document.addEventListener("pointerdown", onDown, { capture: true, passive: true });
    return () => document.removeEventListener("pointerdown", onDown, { capture: true });
  }, [on]);

  return register.current;
}
