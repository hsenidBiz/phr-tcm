import { useEffect, useRef, useState, type ComponentType } from "react";
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
// measuring. Centres of each card / the core, in px.
const BOARD = { w: 800, h: 700 };
const CORE_AT = { x: 400, y: 56, tail: 116 };
const AT: Record<Module, { x: number; y: number }> = {
  db_access: { x: 230, y: 215 },
  writing_style: { x: 590, y: 215 },
  auto_run: { x: 230, y: 395 },
  api_templates: { x: 150, y: 585 },
  playwright: { x: 450, y: 585 },
};

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

function reducedMotion(): boolean {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
}

function pathFor(m: Module): string {
  const parent = infoOf(m).parent;
  const from = parent ? AT[parent] : { x: CORE_AT.x, y: CORE_AT.tail };
  const to = AT[m];
  const mid = (from.y + to.y) / 2;
  return `M${from.x},${from.y} C${from.x},${mid} ${to.x},${mid} ${to.x},${to.y}`;
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
  const seen = useRef<ReadonlySet<Module>>(new Set(modules));
  const wasHydrated = useRef(false);
  const timers = useRef<number[]>([]);

  useEffect(() => {
    const first = !wasHydrated.current;
    if (hydrated) wasHydrated.current = true;
    const prev = seen.current;
    seen.current = modules;
    const added = [...modules].filter((m) => !prev.has(m));
    if (!hydrated || first || added.length === 0 || reducedMotion()) {
      setLit(new Set(modules));
      return;
    }
    // Everything not newly on follows at once (a switch-off dims straight away).
    setLit(new Set([...modules].filter((m) => !added.includes(m))));
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
  return { lit, running, flashing };
}

export default function ModuleTree({
  modules,
  hydrated,
  busy,
  onToggle,
}: {
  modules: ReadonlySet<Module>;
  hydrated: boolean;
  /** The module whose change is being saved, if any. */
  busy: Module | null;
  /** Ask to switch `m` on or off. A locked module never reaches this. */
  onToggle: (m: Module, next: boolean) => void;
}) {
  const { lit, running, flashing } = useLit(modules, hydrated);
  const [shaking, setShaking] = useState<Module | null>(null);
  const shakeTimer = useRef<number>(0);
  useEffect(() => () => window.clearTimeout(shakeTimer.current), []);

  const shake = (m: Module) => {
    setShaking(m);
    window.clearTimeout(shakeTimer.current);
    shakeTimer.current = window.setTimeout(() => setShaking(null), SHAKE_MS);
  };

  return (
    <div className="max-h-[calc(100vh-14rem)] min-h-80 overflow-auto rounded-lg border border-border bg-bg">
      <div className="relative mx-auto" style={{ width: BOARD.w, height: BOARD.h }}>
        <svg
          aria-hidden="true"
          className="mod-edges pointer-events-none absolute inset-0"
          width={BOARD.w}
          height={BOARD.h}
          viewBox={`0 0 ${BOARD.w} ${BOARD.h}`}
        >
          {MODULE_INFO.map(({ module: m }) => (
            <g key={m}>
              <path d={pathFor(m)} data-edge-line={m} className={cn("mod-edge", lit.has(m) && "mod-edge-lit")} />
              <path
                d={pathFor(m)}
                pathLength={1}
                data-edge={m}
                data-running={running.has(m)}
                className="mod-pulse"
              />
            </g>
          ))}
        </svg>

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
              data-module={m}
              data-lit={isLit}
              className="absolute z-10 w-[280px] -translate-x-1/2 -translate-y-1/2"
              style={{ left: AT[m].x, top: AT[m].y }}
            >
              <button
                type="button"
                role="switch"
                aria-checked={on}
                aria-disabled={locked || busy === m || undefined}
                aria-labelledby={`mod-${m}-name mod-${m}-status`}
                aria-describedby={`mod-${m}-desc${locked ? ` mod-${m}-needs` : ""}`}
                onClick={() => {
                  if (busy === m) return;
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
                    <span id={`mod-${m}-status`} className={cn("block text-xs", isLit ? "text-accent" : "text-muted")}>
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
  );
}
