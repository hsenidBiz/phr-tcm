// The Assistants office: one shared room across the top of the Assistants card,
// with a numbered desk for each assistant, spread evenly in the card's order,
// and one pixel character per assistant whose pose shows what it is doing.
// Typing at its desk while it replays or works, dozing there (dimmed, a "z"
// drifting) when idle, and pacing back and forth near its own desk while
// it discovers, maps or explores (pacing near its own desk, which is faded). Walkers start at different
// points of the walk so they do not move as one. A speech bubble floats above
// the character's head and travels with it; only the character flips at each
// end of the walk, never the bubble. All of it is decorative (the rows below
// say it in words), so the whole office is aria-hidden.
//
// The characters are inline SVG rects, so they take their colours from theme
// tokens like everything else; the motion is CSS keyframes in src/index.css
// (`.assistant-scene*`), which a reduced-motion setting stills while the pose
// itself still differs.

import type { CSSProperties } from "react";
import type { AgentActivity, AgentView } from "../../bindings";

export type ScenePose = "walk" | "type" | "doze";

/** The pose an activity gets: moving about, at the desk, or asleep. */
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

/** Where desk `i` of `n` is centred across the room: the middle of its equal
 * share of the width, so a single desk is dead centre. */
export function deskLeft(i: number, n: number): string {
  return `${((i + 0.5) / n) * 100}%`;
}

/** One pixel character, 14 x 17 "pixels" drawn at twice that. */
function Bot({ asleep, style }: { asleep: boolean; style?: CSSProperties }) {
  return (
    <svg
      className="assistant-scene__bot block"
      style={style}
      viewBox="0 0 14 17"
      width="28"
      height="34"
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

export default function AssistantOffice({ agents }: { agents: AgentView[] }) {
  const n = agents.length;
  return (
    <div
      aria-hidden="true"
      className="assistant-scene relative h-24 w-full overflow-hidden rounded-md border border-border bg-bg"
    >
      {/* The floor. */}
      <div className="absolute inset-x-0 bottom-0 h-6 border-t border-border bg-surface-2" />
      {agents.map((a, i) => {
        const pose = poseFor(a.activity);
        const number = deskNumber(a.label);
        return (
          <div
            key={`desk-${a.label}`}
            className={`assistant-scene__desk absolute bottom-5 h-7 w-14 -translate-x-1/2 rounded-sm border border-border-strong bg-surface-2${pose === "walk" ? " assistant-scene__desk--empty" : ""}`}
            style={{ left: deskLeft(i, n) }}
          >
            {/* The screen only lights up while its assistant types. */}
            {pose === "type" && (
              <div className="assistant-scene__screen absolute bottom-full left-1/2 mb-0.5 flex h-5 w-7 -translate-x-1/2 flex-col justify-center gap-0.5 rounded-sm border border-border-strong bg-surface px-1">
                <div className="h-0.5 w-4 bg-muted" />
                <div className="h-0.5 w-3 bg-muted" />
              </div>
            )}
            {number && (
              <span className="assistant-scene__number flex h-full items-center justify-center text-[11px] text-muted">
                {number}
              </span>
            )}
          </div>
        );
      })}
      {agents.map((a, i) => {
        const pose = poseFor(a.activity);
        const bubble = bubbleFor(a.activity);
        // Sitting poses stand left of their desk, far enough that the bubble
        // clears the monitor. A walker paces in a small area around its own
        // desk: the css animates `left` between --walk-from and --walk-to
        // (kept inside the room), and the start point is staggered by index.
        const delay = `${-(i * 2.7)}s`;
        const desk = deskLeft(i, n);
        const inRoom = (x: string) => `clamp(0.5rem, ${x}, calc(100% - 2.25rem))`;
        const walkerStyle: Record<string, string> = { "--assistant-hue": hueFor(a.slot) };
        if (pose === "walk") {
          walkerStyle["--walk-from"] = inRoom(`calc(${desk} - 5rem)`);
          walkerStyle["--walk-to"] = inRoom(`calc(${desk} + 3rem)`);
          walkerStyle.animationDelay = delay;
        } else {
          walkerStyle.left = `calc(${desk} - 3.5rem)`;
        }
        return (
          <div
            key={`walker-${a.label}`}
            data-pose={pose}
            data-slot={a.slot === null ? "none" : String(a.slot)}
            className="assistant-scene__walker absolute bottom-6 h-[34px] w-7"
            style={walkerStyle as CSSProperties}
          >
            {bubble && (
              <span className="assistant-scene__bubble label-trim absolute bottom-full left-1/2 mb-0.5 max-w-[12rem] -translate-x-1/2 overflow-x-clip text-ellipsis whitespace-nowrap rounded border border-border bg-surface px-1.5 text-[11px] text-text">
                {bubble}
              </span>
            )}
            {pose === "doze" && (
              <span className="assistant-scene__z absolute left-full top-0 text-[10px] font-bold text-muted">z</span>
            )}
            <Bot asleep={pose === "doze"} style={pose === "walk" ? { animationDelay: delay } : undefined} />
          </div>
        );
      })}
    </div>
  );
}
