// The furniture of the Assistants office's three corners (AssistantScene.tsx):
// a chemistry Test lab, a Map room with a holographic map, and a furnished
// Lounge. Shapes only, every colour a theme token, all of it under the
// office's aria-hidden root. The lab's bubbles, the hologram's scan line and
// flicker, and the coffee's steam move; under reduced motion they hold still
// (index.css).

import type { CSSProperties } from "react";
import type { Corner } from "./AssistantScene";

/** A liquid: a token colour, a little softened by the surface behind it. */
const liquid = (token: string) => `color-mix(in srgb, var(--color-${token}) 80%, var(--color-surface))`;
const GLASS = "var(--color-border-strong)";
const INSIDE = "var(--color-surface-2)";

/** A conical flask, 6 x 8 pixels. */
function Flask({ className, fill }: { className: string; fill: string }) {
  return (
    <svg className={`assistant-scene__flask block ${className}`} viewBox="0 0 6 8" shapeRendering="crispEdges">
      <rect x="2" y="0" width="2" height="4" fill={GLASS} />
      <rect x="1" y="3" width="4" height="1" fill={GLASS} />
      <rect x="0" y="4" width="6" height="4" fill={GLASS} />
      <rect x="1" y="4" width="4" height="1" fill={INSIDE} />
      <rect x="1" y="5" width="4" height="2" fill={fill} />
    </svg>
  );
}

/** A stoppered bottle, 4 x 8 pixels. */
function Bottle({ className, fill }: { className: string; fill: string }) {
  return (
    <svg className={`block ${className}`} viewBox="0 0 4 8" shapeRendering="crispEdges">
      <rect x="1" y="0" width="2" height="1" fill="var(--color-muted)" />
      <rect x="1" y="1" width="2" height="1" fill={GLASS} />
      <rect x="0" y="2" width="4" height="6" fill={GLASS} />
      <rect x="1" y="3" width="2" height="1" fill={INSIDE} />
      <rect x="1" y="4" width="2" height="3" fill={fill} />
    </svg>
  );
}

function TestLab() {
  return (
    <>
      {/* A wall shelf: bottles and a flask on the left, test tubes in a rack on the right. */}
      <div className="absolute inset-x-3 top-[3.25rem] h-[2px] bg-border-strong" />
      <div className="absolute left-4 top-7 flex h-6 items-end gap-1.5">
        <Bottle className="h-4 w-2" fill={liquid("info")} />
        <Flask className="h-4 w-3" fill={liquid("success")} />
        <Bottle className="h-5 w-2.5" fill={liquid("accent")} />
      </div>
      <div className="absolute right-4 top-7 flex h-6 items-end">
        <div className="relative flex gap-1 px-0.5 pb-0.5">
          {["info", "accent", "success"].map((t) => (
            <span
              key={t}
              className="assistant-scene__tube relative h-4 w-1.5 overflow-hidden rounded-b-full border border-border-strong bg-surface-2"
            >
              <span className="absolute inset-x-0 bottom-0 h-1/2" style={{ background: liquid(t) }} />
            </span>
          ))}
          <span className="absolute inset-x-0 bottom-1 h-[3px] bg-muted" />
        </div>
      </div>
      {/* The bench, with a flask bubbling on it. */}
      <div className="absolute inset-x-3 bottom-7 h-5 rounded-t-sm border border-b-0 border-border-strong bg-surface-2" />
      <div className="absolute bottom-12 left-4 h-8 w-5">
        <Flask className="absolute bottom-0 h-6 w-5" fill={liquid("accent")} />
        {[0, 1, 2].map((i) => (
          <span
            key={i}
            className="assistant-scene__fizz absolute bottom-6 left-2 size-1 rounded-full"
            style={{ background: liquid("accent"), animationDelay: `${-i * 0.6}s`, marginLeft: `${(i - 1) * 3}px` }}
          />
        ))}
      </div>
    </>
  );
}

function MapRoom() {
  return (
    <>
      {/* The projector on the floor, its beam, and the map floating in it. */}
      <div className="assistant-scene__beam absolute bottom-8 left-1/2 h-12 w-28 -translate-x-1/2" />
      <div className="absolute bottom-6 left-1/2 h-2 w-12 -translate-x-1/2 rounded-full border border-border-strong bg-surface-2" />
      <div className="assistant-scene__holo absolute left-1/2 top-6 h-14 w-28 -translate-x-1/2 overflow-hidden rounded-sm border">
        <span className="assistant-scene__land absolute left-2 top-2 h-5 w-8 rounded-[45%]" />
        <span className="assistant-scene__land absolute left-11 top-6 h-6 w-7 rounded-[40%]" />
        <span className="assistant-scene__land absolute right-2 top-3 h-4 w-6 rounded-[50%]" />
        <span className="absolute left-5 top-3.5 h-1.5 w-1.5 rounded-full bg-muted" />
        <span className="assistant-scene__pin--live absolute right-4 top-4 h-1.5 w-1.5 rounded-full bg-accent" />
        <span className="absolute bottom-3 left-14 h-1.5 w-1.5 rounded-full bg-muted" />
        <span className="assistant-scene__scan absolute inset-x-0 top-0 h-px" />
      </div>
      <div className="absolute bottom-7 right-3 h-6 w-3 rounded-t-full bg-success" />
    </>
  );
}

function Lounge() {
  const book = (token: string, h: string): CSSProperties => ({ background: liquid(token), height: h });
  return (
    <>
      {/* A rug, a bookcase, a picture, a sofa, a lamp and the coffee machine. */}
      <div className="assistant-scene__rug absolute bottom-1.5 left-1/2 h-3.5 w-28 -translate-x-1/2 rounded-[50%]" />
      <div className="assistant-scene__bookcase absolute bottom-7 left-3 flex h-16 w-8 flex-col justify-around border border-border-strong bg-surface-2 px-0.5">
        {[
          ["info", "accent", "success", "muted"],
          ["success", "info", "faint"],
          ["accent", "muted", "info", "success"],
        ].map((row, i) => (
          <div key={i} className="flex h-4 items-end gap-px border-b border-border-strong">
            {row.map((t, j) => (
              <span key={j} className="w-1" style={book(t, j % 2 ? "0.75rem" : "0.875rem")} />
            ))}
          </div>
        ))}
      </div>
      <div className="absolute left-1/2 top-6 h-6 w-9 -translate-x-1/2 border-2 border-border-strong bg-surface-2">
        <span className="absolute inset-x-0 bottom-0 h-1/2" style={{ background: liquid("success") }} />
        <span className="absolute right-1 top-0.5 size-1.5 rounded-full" style={{ background: liquid("warning") }} />
      </div>
      <div className="assistant-scene__sofa absolute bottom-7 left-1/2 h-8 w-24 -translate-x-1/2">
        <div className="absolute inset-x-2 top-0 h-5 rounded-t-md border border-b-0 border-border-strong bg-surface-2" />
        <span className="absolute left-4 top-1.5 h-3 w-6 rounded-sm" style={{ background: liquid("success") }} />
        <span className="absolute right-4 top-1.5 h-3 w-6 rounded-sm" style={{ background: liquid("info") }} />
        <div className="absolute inset-x-2 bottom-0 h-3 border border-b-0 border-border-strong bg-surface" />
        <div className="absolute bottom-0 left-0 h-5 w-2.5 rounded-t-sm border border-b-0 border-border-strong bg-surface-2" />
        <div className="absolute bottom-0 right-0 h-5 w-2.5 rounded-t-sm border border-b-0 border-border-strong bg-surface-2" />
      </div>
      <div className="assistant-scene__lamp absolute bottom-7 right-9 h-12 w-5">
        <span className="assistant-scene__glow absolute -left-2 top-0 h-9 w-9 rounded-full" />
        <span className="assistant-scene__shade absolute left-0 top-0 h-2.5 w-5" />
        <span className="absolute left-1/2 top-2.5 h-[2.375rem] w-[2px] -translate-x-1/2 bg-border-strong" />
        <span className="absolute bottom-0 left-1/2 h-[3px] w-3 -translate-x-1/2 bg-border-strong" />
      </div>
      <div className="absolute bottom-7 right-3 h-8 w-4 rounded-t-sm border border-b-0 border-border-strong bg-surface">
        <span className="absolute left-1 top-1 h-1 w-2 bg-muted" />
        <span className="absolute -top-2 left-1 h-2 w-2 rounded-b-sm border border-t-0 border-border-strong bg-surface-2" />
        {[0, 1].map((i) => (
          <span
            key={i}
            className="assistant-scene__steam absolute -top-3 left-1.5 h-1.5 w-[2px] rounded-full bg-muted"
            style={{ animationDelay: `${-i * 1}s`, marginLeft: `${i * 3}px` }}
          />
        ))}
      </div>
    </>
  );
}

/** What stands in one corner. */
export default function OfficeProps({ corner }: { corner: Corner }) {
  if (corner === "lab") return <TestLab />;
  if (corner === "map") return <MapRoom />;
  return <Lounge />;
}
