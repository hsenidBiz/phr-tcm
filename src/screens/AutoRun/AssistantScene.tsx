// The Assistants office: one shared floor across the top of the Assistants
// card, split by two low partitions into three themed corners (Test lab, Map
// room, Lounge) with one pixel character per assistant. Each stands in the
// corner matching its activity: replaying or working in the Test lab, typing
// at the bench beside a lit screen; discovering, mapping or exploring in the
// Map room, pacing a short stretch around its own spot; idle in the Lounge,
// dozing (dimmed, a "z" drifting). When its activity moves it to another
// corner it walks across the floor (the wrapper's `left` is transitioned,
// legs moving, facing the way it goes) instead of jumping. A corner with
// anyone in it lights up; an empty one is dimmed.
//
// Each character is a flex column in normal flow: speech bubble, the bot, then
// its name tag. Everything is sized in rem, so the bubble always sits directly
// above the head whatever the scale; only the bot flips, never the bubble.
// All of it is decorative (the rows below say it in words), so the whole
// office is aria-hidden.
//
// The characters are inline SVG rects, so they take their colours from theme
// tokens like everything else; the motion is CSS in src/index.css
// (`.assistant-scene*`), which a reduced-motion setting stills (the walker
// jumps) while the pose itself still differs.

import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import type { AgentActivity, AgentView } from "../../bindings";

export type ScenePose = "walk" | "type" | "doze";

/** The pose an activity gets: moving about, at the bench, or asleep. */
export function poseFor(a: AgentActivity): ScenePose {
  switch (a.kind) {
    case "discovering":
    case "mapping":
    case "exploring":
      return "walk";
    case "replaying":
    case "working":
      return "type";
    case "idle":
      return "doze";
  }
}

/** The character's colour for a browser slot, so two assistants can be told
 * apart (and matched to their row's square). Tokens only, and never danger or
 * warning, which mean a problem. Slot 0 (the main browser) takes the theme
 * accent, slot 1 the info blue. Slot 2 and any above it take a blend of the
 * two: teal in the default green theme, so it is neither of the first two and
 * neither red nor amber. An assistant with no browser is the muted text
 * colour. */
export function hueFor(slot: number | null): string {
  if (slot === null) return "var(--color-muted)";
  if (slot === 0) return "var(--color-accent)";
  if (slot === 1) return "var(--color-info)";
  return "color-mix(in srgb, var(--color-accent) 50%, var(--color-info))";
}

/** The short line for the speech bubble, or null when there is nothing short
 * worth saying (idle, working, exploring, or discovering with no area). */
export function bubbleFor(a: AgentActivity): string | null {
  switch (a.kind) {
    case "replaying":
      return a.of > 0 ? `Step ${a.step} of ${a.of}` : "Starting";
    case "discovering":
      return a.area ? a.area : null;
    case "mapping":
      return "Menus";
    default:
      return null;
  }
}

/** The assistant's own number, from the end of its label ("Assistant 2" is
 * "2"), or null when the label carries none. */
export function deskNumber(label: string): string | null {
  return /(\d+)$/.exec(label)?.[1] ?? null;
}

export type Corner = "lab" | "map" | "lounge";

const CORNERS: { id: Corner; name: string; colour: string }[] = [
  { id: "lab", name: "Test lab", colour: "var(--color-accent)" },
  { id: "map", name: "Map room", colour: "var(--color-info)" },
  { id: "lounge", name: "Lounge", colour: "var(--color-success)" },
];

/** The corner an activity belongs in. */
export function cornerFor(a: AgentActivity): Corner {
  switch (a.kind) {
    case "replaying":
    case "working":
      return "lab";
    case "discovering":
    case "mapping":
    case "exploring":
      return "map";
    case "idle":
      return "lounge";
  }
}

const THIRD = 100 / 3;
/** How close to a corner edge a spot may be. The Map room needs 3.5rem: half
 * the character (0.875rem) plus its pacing (2.5rem each way) is 3.375rem. */
export function edgeFor(corner: Corner): string {
  return corner === "map" ? "3.5rem" : "2.75rem";
}
const round = (n: number) => Math.round(n * 1000) / 1000;
const cornerStart = (c: Corner) => CORNERS.findIndex((x) => x.id === c) * THIRD;

/** The spot's centre as a percentage of the floor: occupant `k` of `m` in the
 * corner, spread evenly across its third. */
function spotPercent(corner: Corner, k: number, m: number): number {
  return round(cornerStart(corner) + ((k + 0.5) / m) * THIRD);
}

/** Where occupant `k` of `m` stands in `corner`, as a CSS length: its even
 * share of the corner's third, but never closer to either corner edge than
 * `edgeFor(corner)` (2.75rem; 3.5rem in the Map room, whose character paces up
 * to 2.5rem each way), so the character and its pacing stay inside the corner
 * even on the narrowest card. */
export function spotLeft(corner: Corner, k: number, m: number): string {
  const start = round(cornerStart(corner));
  const end = round(cornerStart(corner) + THIRD);
  const edge = edgeFor(corner);
  return `clamp(calc(${start}% + ${edge}), ${spotPercent(corner, k, m)}%, calc(${end}% - ${edge}))`;
}

/** One pixel character, 14 x 17 "pixels" drawn at twice that, sized in rem so
 * it scales with everything around it. */
function Bot({ asleep, style }: { asleep: boolean; style?: CSSProperties }) {
  return (
    <svg
      className="assistant-scene__bot block h-[2.125rem] w-7 shrink-0"
      style={style}
      viewBox="0 0 14 17"
      shapeRendering="crispEdges"
      fill="currentColor"
    >
      <rect x="6" y="0" width="2" height="3" />
      <rect x="2" y="3" width="10" height="7" />
      <rect className="assistant-scene__arm assistant-scene__arm--a" x="0" y="10" width="2" height="4" />
      <rect className="assistant-scene__arm assistant-scene__arm--b" x="12" y="10" width="2" height="4" />
      <rect x="3" y="10" width="8" height="5" />
      <rect className="assistant-scene__leg assistant-scene__leg--a" x="4" y="15" width="2" height="2" />
      <rect className="assistant-scene__leg assistant-scene__leg--b" x="8" y="15" width="2" height="2" />
      {asleep ? (
        <>
          <rect className="assistant-scene__eye assistant-scene__eye--closed" x="4" y="6" width="2" height="1" />
          <rect className="assistant-scene__eye assistant-scene__eye--closed" x="8" y="6" width="2" height="1" />
        </>
      ) : (
        <>
          <rect className="assistant-scene__eye" x="4" y="5" width="2" height="2" />
          <rect className="assistant-scene__eye" x="8" y="5" width="2" height="2" />
        </>
      )}
    </svg>
  );
}

const WALK_MS = 2500;

/** Tracks, per label, where each assistant stood last render; when that
 * changes it reports which way the assistant is walking until the walk ends.
 * Keyed by label, so an assistant that leaves and a new one that arrives never
 * share movement state. */
function useWalking(spots: Map<string, number>): Map<string, "left" | "right"> {
  const last = useRef(new Map<string, number>());
  const timers = useRef(new Map<string, ReturnType<typeof setTimeout>>());
  const [walking, setWalking] = useState<Map<string, "left" | "right">>(new Map());
  const signature = [...spots].map(([l, p]) => `${l}:${p}`).join("|");

  useLayoutEffect(() => {
    const started = new Map<string, "left" | "right">();
    for (const [label, pct] of spots) {
      const prev = last.current.get(label);
      if (prev !== undefined && prev !== pct) {
        started.set(label, pct < prev ? "left" : "right");
        clearTimeout(timers.current.get(label));
        timers.current.set(
          label,
          setTimeout(() => {
            timers.current.delete(label);
            setWalking((w) => {
              const next = new Map(w);
              next.delete(label);
              return next;
            });
          }, WALK_MS),
        );
      }
      last.current.set(label, pct);
    }
    const gone: string[] = [];
    for (const label of [...last.current.keys()]) {
      if (!spots.has(label)) {
        last.current.delete(label);
        clearTimeout(timers.current.get(label));
        timers.current.delete(label);
        gone.push(label);
      }
    }
    if (started.size > 0 || gone.length > 0) {
      setWalking((w) => {
        const next = new Map(w);
        for (const [label, dir] of started) next.set(label, dir);
        for (const label of gone) next.delete(label);
        return next;
      });
    }
    // `spots` is rebuilt every render; its signature is what changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature]);

  useEffect(
    () => () => {
      for (const t of timers.current.values()) clearTimeout(t);
    },
    [],
  );
  return walking;
}

function Props({ corner }: { corner: Corner }) {
  if (corner === "lab") {
    return (
      <div className="absolute inset-x-3 bottom-7 h-5 rounded-t-sm border border-b-0 border-border-strong bg-surface-2" />
    );
  }
  if (corner === "map") {
    return (
      <>
        <div className="absolute left-1/2 top-7 h-11 w-20 -translate-x-1/2 rounded-sm border border-border-strong bg-surface">
          <span className="absolute left-2 top-2 h-1.5 w-1.5 rounded-full bg-muted" />
          <span className="assistant-scene__pin--live absolute right-3 top-3 h-1.5 w-1.5 rounded-full bg-accent" />
          <span className="absolute bottom-2 left-1/2 h-1.5 w-1.5 rounded-full bg-muted" />
        </div>
        <div className="absolute bottom-7 right-3 h-6 w-3 rounded-t-full bg-success" />
      </>
    );
  }
  return (
    <>
      <div className="absolute bottom-7 left-1/2 h-6 w-20 -translate-x-1/2 rounded-t-md border border-b-0 border-border-strong bg-surface-2" />
      <div className="absolute bottom-7 right-3 h-8 w-4 rounded-t-sm border border-b-0 border-border-strong bg-surface" />
    </>
  );
}

export default function AssistantOffice({ agents }: { agents: AgentView[] }) {
  const placed = agents.map((a) => {
    const corner = cornerFor(a.activity);
    const same = agents.filter((o) => cornerFor(o.activity) === corner);
    return { a, corner, k: same.indexOf(a), m: same.length };
  });
  const walking = useWalking(new Map(placed.map((p) => [p.a.label, spotPercent(p.corner, p.k, p.m)])));

  return (
    <div
      aria-hidden="true"
      className="assistant-scene relative h-40 w-full overflow-hidden rounded-md border border-border bg-surface"
    >
      {/* The floor. */}
      <div className="absolute inset-x-0 bottom-0 h-7 border-t border-border bg-surface-2" />
      {CORNERS.map((c) => {
        const empty = !placed.some((p) => p.corner === c.id);
        return (
          <div
            key={c.id}
            data-corner={c.id}
            data-empty={empty ? "true" : "false"}
            className="assistant-scene__corner absolute inset-y-0 w-1/3"
            style={{ left: `${round(cornerStart(c.id))}%`, "--corner-c": c.colour } as CSSProperties}
          >
            <span className="assistant-scene__corner-name absolute left-2 top-1.5 text-[0.6875rem] font-medium uppercase tracking-wider">
              {c.name}
            </span>
            <div className="assistant-scene__props absolute inset-0">
              <Props corner={c.id} />
            </div>
            <div className="assistant-scene__strip absolute inset-x-0 bottom-0 h-[3px]" />
          </div>
        );
      })}
      <div className="absolute bottom-7 left-1/3 h-16 w-[3px] bg-border-strong" />
      <div className="absolute bottom-7 left-2/3 h-16 w-[3px] bg-border-strong" />
      {/* A lit screen for each Test lab occupant, beside its seat. */}
      {placed
        .filter((p) => p.corner === "lab")
        .map((p) => (
          <div
            key={`screen-${p.a.label}`}
            className="assistant-scene__screen absolute bottom-12 flex h-6 w-8 flex-col justify-center gap-0.5 rounded-sm border border-border-strong bg-surface px-1"
            style={{ "--spot": `calc(${spotLeft(p.corner, p.k, p.m)} + 1.5rem)` } as CSSProperties}
          >
            <div className="h-0.5 w-4 bg-muted" />
            <div className="h-0.5 w-3 bg-muted" />
          </div>
        ))}
      {placed.map(({ a, corner, k, m }) => {
        const pose = poseFor(a.activity);
        const bubble = bubbleFor(a.activity);
        const dir = walking.get(a.label);
        const style = { "--spot": spotLeft(corner, k, m), "--assistant-hue": hueFor(a.slot) } as CSSProperties;
        const delay = `${-(k * 2.7)}s`;
        const pace = corner === "map";
        return (
          <div
            key={`walker-${a.label}`}
            data-corner={corner}
            data-pose={pose}
            data-slot={a.slot === null ? "none" : String(a.slot)}
            data-moving={dir ? "true" : undefined}
            data-facing={dir}
            className="assistant-scene__walker absolute bottom-3 w-32 -translate-x-1/2"
            style={style}
          >
            {/* A column in normal flow (bubble, bot, tag), so the bubble sits
                right above the head at any scale. The walker's bottom is the
                floor line less the tag's 1rem. Only the bubble and bot pace;
                the tag stays fixed under the spot. */}
            <div className="flex w-full flex-col items-center">
              <div
                className={`flex flex-col items-center${pace ? " assistant-scene__pace" : ""}`}
                style={pace ? { animationDelay: delay } : undefined}
              >
              {bubble && (
                <span className="assistant-scene__bubble label-trim mb-0.5 max-w-[8rem] whitespace-normal break-words rounded border border-border bg-surface px-1.5 py-0.5 text-center text-[0.6875rem] leading-tight text-text">
                  {bubble}
                </span>
              )}
              <div className="relative">
                <Bot asleep={pose === "doze"} style={pace ? { animationDelay: delay } : undefined} />
                {pose === "doze" && (
                  <span className="assistant-scene__z absolute left-full top-0 text-[0.625rem] font-bold text-muted">z</span>
                )}
              </div>
              </div>
              <span className="assistant-scene__tag label-trim h-4 max-w-[5rem] overflow-x-clip text-ellipsis whitespace-nowrap text-[0.6875rem] leading-4 text-muted">
                {a.label}
              </span>
            </div>
          </div>
        );
      })}
    </div>
  );
}
