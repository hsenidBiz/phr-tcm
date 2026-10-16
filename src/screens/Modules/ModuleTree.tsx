import { useEffect, useMemo, useRef, useState, type ComponentType } from "react";
import ElectricLogo from "../../components/ElectricLogo";
import {
  IconModuleApiTemplates,
  IconModuleAutoRun,
  IconModuleDatabase,
  IconModulePlaywright,
  IconModuleWritingStyle,
} from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import type { Module } from "../../lib/modules";
import { MODULE_INFO, STATUS_OFF, STATUS_ON, infoOf, statusLocked } from "./modulesCopy";
import PowerCore from "./PowerCore";

// The tree is drawn on a fixed pixel board (the scroll area scrolls it), so
// the paths are plain SVG in the same units as the cards and need no
// measuring. The core's centre, and each card's top centre, in px: cards in a
// row share a top edge whatever their height. The core sits low enough that
// its rings (they grow to about 61px from the orb's centre) stay on the board.
const BOARD = { w: 800, h: 750 };
const CORE_AT = { x: 400, y: 106, tail: 166 };
const AT: Record<Module, { x: number; y: number }> = {
  db_access: { x: 230, y: 195 },
  writing_style: { x: 590, y: 195 },
  auto_run: { x: 230, y: 375 },
  api_templates: { x: 150, y: 565 },
  playwright: { x: 450, y: 565 },
};

/** The board grows to fill its area (and shrinks a little before it
 * scrolls); cards and paths scale together, so the paths stay attached. */
const SCALE = { min: 0.75, max: 1.8 };
const FRAME_PAD = 24;

/** The scale that fits the board in its frame, kept current as the window changes. */
function useFit() {
  const frame = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(1);
  useEffect(() => {
    const el = frame.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const fit = () => {
      const w = el.clientWidth - FRAME_PAD * 2;
      const h = el.clientHeight - FRAME_PAD * 2;
      if (w <= 0 || h <= 0) return;
      const s = Math.min(SCALE.max, Math.max(SCALE.min, Math.min(w / BOARD.w, h / BOARD.h)));
      setScale((prev) => (Math.abs(prev - s) < 0.005 ? prev : s));
    };
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return { frame, scale };
}

const ICON: Record<Module, ComponentType<{ className?: string; "aria-hidden"?: boolean }>> = {
  db_access: IconModuleDatabase,
  writing_style: IconModuleWritingStyle,
  auto_run: IconModuleAutoRun,
  api_templates: IconModuleApiTemplates,
  playwright: IconModulePlaywright,
};

/** How long the light takes to run a path, and when the card lights. */
const RUN_MS = 700;
const LIGHT_MS = 450;
const FLASH_MS = 600;
const SHAKE_MS = 400;
/** How long a switched-off path takes to empty, and how far apart each
 * level of a chain starts (the deepest first). Matches .mod-drain. */
const DRAIN_MS = 600;
const DRAIN_STEP_MS = 450;

/** The most pixels the electric layer draws a frame. The board is 800x750
 * (600k px) at 1x; this renders it a little under that and lets the browser
 * scale it up, which a soft glow hides, to go easier on a software renderer. */
const ELECTRIC_PIXEL_BUDGET = 400_000;

/** How far down the tree a module hangs: Core's children are 1. */
function depthOf(m: Module): number {
  let depth = 1;
  for (let p = infoOf(m).parent; p; p = infoOf(p).parent) depth++;
  return depth;
}

function reducedMotion(): boolean {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
}

/** A card's height until it has been measured (jsdom never measures). */
const HEIGHT_GUESS = 140;

/**
 * A path runs from the parent card's bottom edge to the child card's top
 * edge, never behind a card: a locked card is see-through, and a line drawn
 * centre to centre showed through it.
 */
function pathFor(m: Module, height: Partial<Record<Module, number>>): string {
  const parent = infoOf(m).parent;
  const from = parent
    ? { x: AT[parent].x, y: AT[parent].y + (height[parent] ?? HEIGHT_GUESS) }
    : { x: CORE_AT.x, y: CORE_AT.tail };
  const to = AT[m];
  const mid = (from.y + to.y) / 2;
  return `M${from.x},${from.y} C${from.x},${mid} ${to.x},${mid} ${to.x},${to.y}`;
}

/**
 * The picture the electric layer traces: the paths of `mods` as plain strokes
 * on a canvas the size of the board, so a point of it is the same point of
 * the board (the layer maps the whole picture onto the board, see `fitTo`).
 * Only its alpha is read, so the stroke colour never shows.
 */
function electricSrc(mods: readonly Module[], height: Partial<Record<Module, number>>): string {
  const paths = mods
    .map((m) => `<path d="${pathFor(m, height)}" fill="none" stroke="white" stroke-width="5" stroke-linecap="round"/>`)
    .join("");
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${BOARD.w}" height="${BOARD.h}" viewBox="0 0 ${BOARD.w} ${BOARD.h}">${paths}</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

type ElectricColors = { color: string; glowColor: string; theme: "dark" | "light" };

/** The electricity's colours, from the theme's own tokens, kept current as the
 * theme or accent changes (both are attributes on <html>). On a dark theme
 * the strand's core is the lighter accent (on-accent is near black there);
 * on a light one it is on-accent, drawn over the accent glow. */
function readElectricColors(): ElectricColors {
  const root = document.documentElement;
  const css = getComputedStyle(root);
  const token = (name: string) => css.getPropertyValue(name).trim();
  const dark = root.classList.contains("dark");
  return {
    glowColor: token("--color-accent"),
    color: token(dark ? "--color-accent-hover" : "--color-on-accent"),
    theme: dark ? "dark" : "light",
  };
}

function useElectricColors(): ElectricColors {
  const [colors, setColors] = useState(readElectricColors);
  useEffect(() => {
    if (typeof MutationObserver === "undefined") return;
    const update = () => {
      const next = readElectricColors();
      setColors((prev) =>
        prev.color === next.color && prev.glowColor === next.glowColor && prev.theme === next.theme ? prev : next,
      );
    };
    const mo = new MutationObserver(update);
    mo.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class", "data-theme", "data-accent", "style"],
    });
    return () => mo.disconnect();
  }, []);
  return colors;
}

/** Each card's height, kept current as text wraps or the font changes. */
function useHeights() {
  const [height, setHeight] = useState<Partial<Record<Module, number>>>({});
  const nodes = useRef(new Map<Module, HTMLElement>());
  useEffect(() => {
    const measure = () => {
      const next: Partial<Record<Module, number>> = {};
      for (const [m, el] of nodes.current) if (el.offsetHeight) next[m] = el.offsetHeight;
      setHeight((prev) =>
        Object.keys(next).length === Object.keys(prev).length &&
        (Object.keys(next) as Module[]).every((m) => prev[m] === next[m])
          ? prev
          : next,
      );
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    for (const el of nodes.current.values()) ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const refFor = (m: Module) => (el: HTMLElement | null) => {
    if (el) nodes.current.set(m, el);
    else nodes.current.delete(m);
  };
  return { height, refFor };
}

/**
 * The visible state lags the real one on purpose: a module that has just come
 * on first has a light run along its path, then the path stays lit and the
 * card glows. `aria-checked` always follows the real state at once. Nothing
 * travels on the first answer from Rust, or under reduced motion.
 */
function useLit(modules: ReadonlySet<Module>, hydrated: boolean) {
  const [lit, setLit] = useState<ReadonlySet<Module>>(() => new Set(modules));
  const [running, setRunning] = useState<ReadonlySet<Module>>(new Set());
  const [flashing, setFlashing] = useState<ReadonlySet<Module>>(new Set());
  /** Modules just switched off, and when (ms from now) their power starts
   * draining back up the path: the deepest drain first. */
  const [draining, setDraining] = useState<ReadonlyMap<Module, number>>(new Map());
  const seen = useRef<ReadonlySet<Module>>(new Set(modules));
  const wasHydrated = useRef(false);
  const timers = useRef<number[]>([]);

  useEffect(() => {
    const first = !wasHydrated.current;
    if (hydrated) wasHydrated.current = true;
    const prev = seen.current;
    seen.current = modules;
    const added = [...modules].filter((m) => !prev.has(m));
    const removed = [...prev].filter((m) => !modules.has(m));
    if (!hydrated || first || (added.length === 0 && removed.length === 0) || reducedMotion()) {
      setLit(new Set(modules));
      setDraining(new Map());
      return;
    }
    // A module back on before its drain finished stops draining.
    setDraining((d) => new Map([...d].filter(([m]) => !added.includes(m))));
    if (removed.length > 0) {
      // Each switched-off path empties from the module back to its parent;
      // a chain empties from the bottom up, so the deepest goes first.
      const deepest = Math.max(...removed.map(depthOf));
      const starts = new Map(removed.map((m) => [m, (deepest - depthOf(m)) * DRAIN_STEP_MS] as const));
      setDraining((d) => new Map([...d, ...starts]));
      for (const [m, start] of starts) {
        timers.current.push(
          window.setTimeout(() => {
            if (seen.current.has(m)) return;
            setDraining((d) => new Map([...d].filter(([x]) => x !== m)));
          }, start + DRAIN_MS),
        );
      }
      // A module switched off while it was still lighting up stops at once:
      // its light-up pulse and flash would otherwise run over the drain.
      setRunning((r) => new Set([...r].filter((m) => !removed.includes(m))));
      setFlashing((f) => new Set([...f].filter((m) => !removed.includes(m))));
    }
    // Everything else follows at once. A switched-off card has lost its
    // power, so it greys straight away; only its path drains.
    setLit(new Set([...modules].filter((m) => !added.includes(m))));
    if (added.length === 0) return;
    setRunning((r) => new Set([...r, ...added]));
    const t1 = window.setTimeout(() => {
      setLit((l) => new Set([...l, ...added].filter((m) => seen.current.has(m))));
      setFlashing((f) => new Set([...f, ...added]));
    }, LIGHT_MS);
    const t2 = window.setTimeout(() => setRunning((r) => new Set([...r].filter((m) => !added.includes(m)))), RUN_MS);
    const t3 = window.setTimeout(
      () => setFlashing((f) => new Set([...f].filter((m) => !added.includes(m)))),
      LIGHT_MS + FLASH_MS,
    );
    timers.current.push(t1, t2, t3);
  }, [modules, hydrated]);

  useEffect(
    () => () => {
      for (const t of timers.current) window.clearTimeout(t);
    },
    [],
  );
  return { lit, running, flashing, draining };
}

export default function ModuleTree({
  modules,
  hydrated,
  busy,
  onToggle,
}: {
  modules: ReadonlySet<Module>;
  hydrated: boolean;
  /** The module whose change is being saved, if any; every switch waits. */
  busy: Module | null;
  /** Ask to switch `m` on or off. A locked module never reaches this. */
  onToggle: (m: Module, next: boolean) => void;
}) {
  const { lit, running, flashing, draining } = useLit(modules, hydrated);
  const { height, refFor } = useHeights();
  const { frame, scale } = useFit();
  const electricColors = useElectricColors();
  // The electricity runs along every path that is lit and not draining; the
  // picture changes only when that set (or a card's height) does, and the
  // layer morphs from the old picture to the new one.
  const electric = useMemo(() => {
    const mods = MODULE_INFO.map((i) => i.module).filter((m) => lit.has(m) && !draining.has(m));
    return mods.length > 0 ? electricSrc(mods, height) : null;
  }, [lit, draining, height]);
  const [shaking, setShaking] = useState<Module | null>(null);
  const shakeTimer = useRef<number>(0);
  useEffect(() => () => window.clearTimeout(shakeTimer.current), []);

  const shake = (m: Module) => {
    setShaking(m);
    window.clearTimeout(shakeTimer.current);
    shakeTimer.current = window.setTimeout(() => setShaking(null), SHAKE_MS);
  };

  return (
    <div
      ref={frame}
      className="h-[calc(100vh-13rem)] min-h-80 overflow-auto rounded-lg border border-border bg-bg"
      style={{ padding: FRAME_PAD }}
    >
      {/* Sized at the scaled size so the frame scrolls only when the board
          really does not fit; the board itself is scaled from its top left. */}
      <div className="mx-auto" style={{ width: BOARD.w * scale, height: BOARD.h * scale }}>
        <div
          className="relative origin-top-left"
          style={{ width: BOARD.w, height: BOARD.h, transform: scale === 1 ? undefined : `scale(${scale})` }}
        >
          <svg
            aria-hidden="true"
            className="mod-edges pointer-events-none absolute inset-0"
            width={BOARD.w}
            height={BOARD.h}
            viewBox={`0 0 ${BOARD.w} ${BOARD.h}`}
          >
            {MODULE_INFO.map(({ module: m }) => (
              <g key={m}>
                <path
                  d={pathFor(m, height)}
                  data-edge-line={m}
                  className={cn("mod-edge", lit.has(m) && !draining.has(m) && "mod-edge-lit")}
                />
                {/* Switched off: the glow empties from the module back up to
                    its parent, after the modules below it have emptied. */}
                {draining.has(m) && (
                  <path
                    d={pathFor(m, height)}
                    pathLength={1}
                    data-draining={m}
                    className="mod-drain"
                    style={{ animationDelay: `${draining.get(m)}ms` }}
                  />
                )}
                <path
                  d={pathFor(m, height)}
                  pathLength={1}
                  data-edge={m}
                  data-running={running.has(m)}
                  className="mod-pulse"
                />
              </g>
            ))}
          </svg>

          {/* The power in the lit paths: one WebGL layer for the whole board,
              tuned to hug the line: the strands' sway scales with the board
              (up to about 9 board units per 0.1 of `bend`), so `bend` stays near 0,
              over the paths and under the cards, sized in board units like
              everything else. Nothing lit, or reduced motion: no layer, and
              a lit path is just its steady glow. */}
          {electric && !reducedMotion() && (
            <div aria-hidden="true" data-electric className="pointer-events-none absolute inset-0">
              <ElectricLogo
                src={electric}
                color={electricColors.color}
                glowColor={electricColors.glowColor}
                theme={electricColors.theme}
                fitTo="image"
                scale={1}
                interactive={false}
                maxDpr={1}
                pixelBudget={ELECTRIC_PIXEL_BUDGET}
                strands={2}
                arcs={0.5}
                speed={2}
                glow={1}
                crackle={1.2}
                bend={0.05}
                thickness={1.2}
              />
            </div>
          )}

          <div className="absolute z-10 -translate-x-1/2 -translate-y-1/2" style={{ left: CORE_AT.x, top: CORE_AT.y }}>
            <PowerCore />
          </div>

          {MODULE_INFO.map(({ module: m, name, parent, description }) => {
            const on = modules.has(m);
            const locked = !on && parent !== null && !modules.has(parent);
            const status = on ? STATUS_ON : locked ? statusLocked(parent!) : STATUS_OFF;
            const Glyph = ICON[m];
            const isLit = lit.has(m);
            return (
              <div
                key={m}
                ref={refFor(m)}
                data-module={m}
                data-lit={isLit}
                className="absolute z-10 w-[280px] -translate-x-1/2"
                style={{ left: AT[m].x, top: AT[m].y }}
              >
                <button
                  type="button"
                  role="switch"
                  aria-checked={on}
                  aria-disabled={locked || busy !== null || undefined}
                  aria-labelledby={`mod-${m}-name mod-${m}-status`}
                  aria-describedby={`mod-${m}-desc${locked ? ` mod-${m}-needs` : ""}`}
                  onClick={() => {
                    // One save at a time: replies arriving out of order could
                    // otherwise leave the page showing the older set.
                    if (busy !== null) return;
                    if (locked) return shake(m);
                    onToggle(m, !on);
                  }}
                  className={cn(
                    "block w-full rounded-xl border bg-surface p-3 text-left transition-[border-color,box-shadow,opacity] duration-200",
                    isLit ? "border-accent mod-node-lit" : "border-border-strong",
                    flashing.has(m) && "mod-flash",
                    locked ? "cursor-not-allowed opacity-50" : "cursor-pointer",
                    shaking === m && "t-shake",
                  )}
                >
                  <span className="flex items-center gap-2.5">
                    <span
                      className={cn(
                        "grid size-9 flex-none place-items-center rounded-lg transition-colors duration-200",
                        isLit ? "bg-accent text-on-accent" : "bg-surface-2 text-muted",
                      )}
                    >
                      <Glyph aria-hidden className="size-5" />
                    </span>
                    <span className="block min-w-0">
                      <span id={`mod-${m}-name`} className="block text-sm font-semibold text-text">
                        {name}
                      </span>
                      <span
                        id={`mod-${m}-status`}
                        className={cn("block text-xs", isLit ? "text-accent" : "text-muted")}
                      >
                        {status}
                      </span>
                    </span>
                  </span>
                  <span id={`mod-${m}-desc`} className="mt-2 block text-xs leading-snug text-muted">
                    {description}
                  </span>
                  {locked && (
                    <span id={`mod-${m}-needs`} className="sr-only">
                      Switch on {infoOf(parent!).name} first.
                    </span>
                  )}
                </button>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
