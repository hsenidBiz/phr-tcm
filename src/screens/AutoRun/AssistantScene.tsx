// The small scene on each Assistants card row: one pixel character whose pose
// shows what the assistant is doing. Walking while it explores, typing at a
// desk while it replays or works, dozing when it is idle. Purely decorative
// (the activity line already says it in words), so it is aria-hidden.
//
// The character is inline SVG rects, so it takes its colours from theme
// tokens like everything else; the motion is CSS keyframes in src/index.css
// (`.assistant-scene*`), which a reduced-motion setting stills while the pose
// itself still differs.

import type { CSSProperties } from "react";
import type { AgentActivity } from "../../bindings";

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
 * apart. Tokens only, and never danger or warning, which mean a problem.
 * Slot 0 (the main browser) takes the theme accent, slot 1 the info blue.
 * Slot 2 and any above it take a blend of the two: teal in the default
 * green theme, so it is neither of the first two and neither red nor amber.
 * An assistant with no browser is the muted text colour. */
function hueFor(slot: number | null): string {
  if (slot === null) return "var(--color-muted)";
  if (slot === 0) return "var(--color-accent)";
  if (slot === 1) return "var(--color-info)";
  return "color-mix(in srgb, var(--color-accent) 50%, var(--color-info))";
}

export default function AssistantScene({ activity, slot }: { activity: AgentActivity; slot: number | null }) {
  const pose = poseFor(activity);
  const asleep = pose === "doze";
  const style = { "--assistant-hue": hueFor(slot) } as CSSProperties;
  return (
    <div
      aria-hidden="true"
      data-pose={pose}
      data-slot={slot === null ? "none" : String(slot)}
      className={`assistant-scene assistant-scene--${pose} relative h-14 w-[4.5rem] shrink-0 overflow-hidden rounded-md border border-border bg-bg`}
      style={style}
    >
      {/* The floor. */}
      <div className="absolute inset-x-0 bottom-0 h-2 border-t border-border bg-surface-2" />
      {pose === "type" && (
        <div className="assistant-scene__desk">
          <div className="absolute bottom-6 right-3 flex h-4 w-5 flex-col justify-center gap-0.5 rounded-sm border border-border-strong bg-surface px-0.5">
            <div className="h-0.5 w-3 bg-muted" />
            <div className="h-0.5 w-2 bg-muted" />
          </div>
          <div className="absolute bottom-2 right-1.5 h-4 w-7 rounded-sm border border-border-strong bg-surface-2" />
        </div>
      )}
      {asleep && <span className="assistant-scene__z absolute left-[3.25rem] top-1.5 text-[10px] font-bold text-muted">z</span>}
      {/* 14 x 17 "pixels", drawn at twice that. */}
      <svg
        className="assistant-scene__bot"
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
    </div>
  );
}
