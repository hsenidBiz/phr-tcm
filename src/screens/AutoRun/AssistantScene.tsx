// The cubicle on each Assistants card cell: a full-width scene with the
// assistant's numbered desk and one pixel character whose pose shows what it is
// doing. Walking back and forth across the cubicle while it explores (the desk
// left empty and faded), typing at the desk while it replays or works, dozing
// at the desk when it is idle. A speech bubble in the top-left carries the
// short version of the activity, the desk number sits in the top-right. All of
// it is decorative (the text lines below already say it in words), so the
// whole scene is aria-hidden.
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

export default function AssistantScene({
  activity,
  slot,
  label,
}: {
  activity: AgentActivity;
  slot: number | null;
  label: string;
}) {
  const pose = poseFor(activity);
  const asleep = pose === "doze";
  const bubble = bubbleFor(activity);
  const number = deskNumber(label);
  const style = { "--assistant-hue": hueFor(slot) } as CSSProperties;
  return (
    <div
      aria-hidden="true"
      data-pose={pose}
      data-slot={slot === null ? "none" : String(slot)}
      className={`assistant-scene assistant-scene--${pose} relative h-24 w-full overflow-hidden rounded-md border border-border bg-bg`}
      style={style}
    >
      {/* The floor. */}
      <div className="absolute inset-x-0 bottom-0 h-6 border-t border-border bg-surface-2" />
      {/* The desk stays in the cubicle whatever the pose; it is faded while its
          assistant is away walking. The screen only lights up while typing. */}
      <div className={`assistant-scene__desk${pose === "walk" ? " assistant-scene__desk--empty" : ""}`}>
        {pose === "type" && (
          <div className="assistant-scene__screen absolute bottom-12 left-1/2 flex h-5 w-7 -translate-x-1/2 flex-col justify-center gap-0.5 rounded-sm border border-border-strong bg-surface px-1">
            <div className="h-0.5 w-4 bg-muted" />
            <div className="h-0.5 w-3 bg-muted" />
          </div>
        )}
        <div className="absolute bottom-5 left-1/2 h-7 w-14 -translate-x-1/2 rounded-sm border border-border-strong bg-surface-2" />
      </div>
      {bubble && (
        <span className="assistant-scene__bubble label-trim absolute left-1.5 top-1.5 max-w-[70%] truncate rounded border border-border bg-surface px-1.5 text-[11px] text-text">
          {bubble}
        </span>
      )}
      {number && (
        <span className="assistant-scene__number absolute right-1.5 top-1.5 text-[11px] text-muted">{number}</span>
      )}
      {asleep && <span className="assistant-scene__z absolute left-[calc(50%-4.25rem)] top-5 text-[10px] font-bold text-muted">z</span>}
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
