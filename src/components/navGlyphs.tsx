import { useId, type ReactNode } from "react";
import { cn } from "../lib/cn";

/**
 * The sidebar's own glyphs for the rows whose hover animation moves a PART
 * of the picture: the pen writes its line, the eyelid closes, the play
 * button leaves its box. A lucide icon is one opaque <svg>, so all it can do
 * is move as a whole - which read as the picture warping. These draw the
 * same geometry as the lucide icon they replace (Pen Line, File Up, Rotate
 * Ccw, Eye, Square Play, Radar, Folder Tree, List Ordered, Bot, Braces, and Work
 * Manager's Git Pull Request, Square Kanban, File Plus Corner, and the context
 * bar's Bell; lucide is ISC), split
 * into the parts the animation needs, plus a few pieces that only show
 * mid-animation. At rest every one looks exactly like the lucide original.
 *
 * The motion itself is CSS (index.css, "Sidebar glyph motion"): once per
 * hover entry, and none at all under reduced motion.
 */

export type GlyphProps = { size?: number; className?: string };

function Glyph({ kind, size = 16, className, children }: GlyphProps & { kind: string; children: ReactNode }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className={cn("nav-glyph", `ng-${kind}`, className)}
    >
      {children}
    </svg>
  );
}

/** An id for a clipPath that is safe inside url(#...). useId's own has colons. */
function useClipId(name: string): string {
  return `ng-${name}-${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
}

/** Manual Entry: the pen comes in from the right, writes the line, and settles. */
export function GlyphManual(p: GlyphProps) {
  return (
    <Glyph kind="manual" {...p}>
      <path className="ng-line" pathLength={1} d="M13 21h8" />
      <path
        className="ng-pen"
        d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"
      />
    </Glyph>
  );
}

/** Import Test Cases: the arrow lifts, the file rises out of the top with trail
 *  lines behind it, and the icon pops back in. */
export function GlyphImport(p: GlyphProps) {
  return (
    <Glyph kind="import" {...p}>
      <g className="ng-file">
        <path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z" />
        <path d="M14 2v5a1 1 0 0 0 1 1h5" />
        <g className="ng-arrow">
          <path d="M12 12v6" />
          <path d="m15 15-3-3-3 3" />
        </g>
        {/* Below the file, outside the 24-unit box until it rises. */}
        <g className="ng-trails" strokeWidth={1.5}>
          <path d="M8 24.5v3.5" />
          <path d="M12 24.5v5.5" />
          <path d="M16 24.5v3" />
        </g>
      </g>
    </Glyph>
  );
}

/** Update Test Cases: the line runs off round the circle into its arrowhead,
 *  draws itself back in from the start, and the arrowhead snaps back on. */
export function GlyphUpdate(p: GlyphProps) {
  return (
    <Glyph kind="update" {...p}>
      <path className="ng-arc" pathLength={1} d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
      <path className="ng-head" d="M3 3v5h5" />
    </Glyph>
  );
}

/** View Test Cases: the corners stay put and the upper lid closes down onto
 *  the lower one, covering the pupil as it goes. */
export function GlyphView(p: GlyphProps) {
  const clip = useClipId("eye");
  return (
    <Glyph kind="view" {...p}>
      <defs>
        <clipPath id={clip}>
          {/* The pupil shows below this edge: it follows the lid down. */}
          <rect className="ng-lidclip" x="0" y="4" width="24" height="20" />
        </clipPath>
      </defs>
      <path d="M2.062 12.348a1 1 0 0 1 0-.696" />
      <path d="M21.938 11.652a1 1 0 0 1 0 .696" />
      <path className="ng-lid" d="M2.062 11.652a10.75 10.75 0 0 1 19.876 0" />
      <path d="M21.938 12.348a10.75 10.75 0 0 1-19.876 0" />
      <g clipPath={`url(#${clip})`}>
        <circle cx="12" cy="12" r="3" />
      </g>
    </Glyph>
  );
}

/** Run Tests: the play button slides out through the right side of its box,
 *  then pops back in. */
export function GlyphRun(p: GlyphProps) {
  const clip = useClipId("run");
  return (
    <Glyph kind="run" {...p}>
      <defs>
        <clipPath id={clip}>
          {/* The inside of the box: x 4..20 clears the 2-unit frame. */}
          <rect x="4" y="4" width="16" height="16" />
        </clipPath>
      </defs>
      <rect x="3" y="3" width="18" height="18" rx="2" />
      <g clipPath={`url(#${clip})`}>
        <path
          className="ng-play"
          d="M9 9.003a1 1 0 0 1 1.517-.859l4.997 2.997a1 1 0 0 1 0 1.718l-4.997 2.997A1 1 0 0 1 9 14.996z"
        />
      </g>
    </Glyph>
  );
}

/** Auto Run: a radar sweep - the arm turns once with a fading wedge behind
 *  it, and each blip flares as the arm passes. */
export function GlyphAutoRun(p: GlyphProps) {
  return (
    <Glyph kind="autorun" {...p}>
      <path d="M19.07 4.93A10 10 0 0 0 6.99 3.34" />
      <path className="ng-blip ng-blip-a" d="M4 6h.01" />
      <path d="M2.29 9.62A10 10 0 1 0 21.31 8.35" />
      <path d="M16.24 7.76A6 6 0 1 0 8.23 16.67" />
      <path className="ng-blip ng-blip-b" d="M12 18h.01" />
      <path d="M17.99 11.66A6 6 0 0 1 15.77 16.67" />
      <circle cx="12" cy="12" r="2" />
      <g className="ng-sweep">
        {/* The wedge trails the arm: 40 degrees behind it, counter-clockwise. */}
        <path className="ng-wedge ng-fill" d="M12 12L18.01 5.99A8.5 8.5 0 0 0 12.74 3.53Z" fill="currentColor" stroke="none" />
        <path d="m13.41 10.59 5.66-5.66" />
      </g>
    </Glyph>
  );
}

/** API Templates: lucide's Braces, one brace per part. On hover the braces
 *  open, three values fill the gap one after another - a template taking
 *  its parameters - and the braces close on them again. The values only
 *  exist mid-animation. */
export function GlyphApiTemplates(p: GlyphProps) {
  return (
    <Glyph kind="apitemplates" {...p}>
      <path className="ng-brace-l" d="M8 3H7a2 2 0 0 0-2 2v5a2 2 0 0 1-2 2 2 2 0 0 1 2 2v5c0 1.1.9 2 2 2h1" />
      <path className="ng-brace-r" d="M16 21h1a2 2 0 0 0 2-2v-5c0-1.1.9-2 2-2a2 2 0 0 1-2-2V5a2 2 0 0 0-2-2h-1" />
      <path className="ng-arg ng-arg-1" d="M9 12h.01" />
      <path className="ng-arg ng-arg-2" d="M12 12h.01" />
      <path className="ng-arg ng-arg-3" d="M15 12h.01" />
    </Glyph>
  );
}

/** Search Suites: a bright segment runs from the tree's root out to both
 *  folders, the way the Test map's pulse runs along its lines. */
export function GlyphSuites(p: GlyphProps) {
  return (
    <Glyph kind="suites" {...p}>
      <path className="ng-folder" d="M20 10a1 1 0 0 0 1-1V6a1 1 0 0 0-1-1h-2.5a1 1 0 0 1-.8-.4l-.9-1.2A1 1 0 0 0 15 3h-2a1 1 0 0 0-1 1v5a1 1 0 0 0 1 1Z" />
      <path className="ng-folder" d="M20 21a1 1 0 0 0 1-1v-3a1 1 0 0 0-1-1h-2.9a1 1 0 0 1-.88-.55l-.42-.85a1 1 0 0 0-.92-.6H13a1 1 0 0 0-1 1v5a1 1 0 0 0 1 1Z" />
      <path d="M3 5a2 2 0 0 0 2 2h3" />
      <path d="M3 3v13a2 2 0 0 0 2 2h3" />
      {/* The pulse: both routes start at the root (3,3), so they leave together. */}
      <path className="ng-pulse" pathLength={1} d="M3 3v2a2 2 0 0 0 2 2h3" />
      <path className="ng-pulse" pathLength={1} d="M3 3v13a2 2 0 0 0 2 2h3" />
    </Glyph>
  );
}

/** Suite Management: the rows light up one at a time, top to bottom. */
export function GlyphManage(p: GlyphProps) {
  return (
    <Glyph kind="manage" {...p}>
      <path className="ng-row ng-row-1" pathLength={1} d="M11 5h10" />
      <path className="ng-row ng-row-2" pathLength={1} d="M11 12h10" />
      <path className="ng-row ng-row-3" pathLength={1} d="M11 19h10" />
      <path d="M4 4h1v5" />
      <path d="M4 9h2" />
      <path d="M6.5 20H3.4c0-1 2.6-1.925 2.6-3.5a1.5 1.5 0 0 0-2.6-1.02" />
    </Glyph>
  );
}

/** Pull Requests: a change leaves the source branch's dot, runs along the
 *  line and lands in the target's, which lights up as it arrives. */
export function GlyphPrs(p: GlyphProps) {
  return (
    <Glyph kind="prs" {...p}>
      <circle className="ng-target" cx="18" cy="18" r="3" />
      <circle className="ng-source" cx="6" cy="6" r="3" />
      <path d="M13 6h3a2 2 0 0 1 2 2v7" />
      <line x1="6" x2="6" y1="9" y2="21" />
      {/* The travelling dot: from the source's edge, across the gap, along the line. */}
      <path className="ng-change" pathLength={1} strokeWidth={3} d="M9 6h7a2 2 0 0 1 2 2v7" />
    </Glyph>
  );
}

/** Board: the first column's bottom card hops to the middle column, then
 *  on to the last, and the columns settle back. */
export function GlyphBoard(p: GlyphProps) {
  return (
    <Glyph kind="board" {...p}>
      <rect width="18" height="18" x="3" y="3" rx="2" />
      <path className="ng-col-1" d="M8 7v7" />
      <path d="M12 7v4" />
      <path d="M16 7v9" />
      {/* The card: at rest it is the first column's last two units, and hidden. */}
      <path className="ng-card" d="M0 0v2" />
    </Glyph>
  );
}

/** New Work Item: the plus spins out and back in with a pop, and the page's
 *  folded corner flips open and shut. */
export function GlyphCreate(p: GlyphProps) {
  return (
    <Glyph kind="create" {...p}>
      <path d="M11.35 22H6a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.706.706l3.588 3.588A2.4 2.4 0 0 1 20 8v5.35" />
      <path className="ng-fold" d="M14 2v5a1 1 0 0 0 1 1h5" />
      <g className="ng-plus">
        <path d="M14 19h6" />
        <path d="M17 16v6" />
      </g>
    </Glyph>
  );
}

/** AI Bridge: ones and zeros stream across the robot's face, flipping as
 *  they go, while its eyes step aside. */
export function GlyphAi(p: GlyphProps) {
  const clip = useClipId("ai");
  const row = (y: number, a: string, b: string, cls: string) => (
    <g className={cn("ng-bits", cls)}>
      <text className="ng-bits-a" x="20" y={y}>{a}</text>
      <text className="ng-bits-b" x="20" y={y}>{b}</text>
    </g>
  );
  return (
    <Glyph kind="ai" {...p}>
      <defs>
        <clipPath id={clip}>
          {/* The face inside its 2-unit outline. */}
          <rect x="5" y="9" width="14" height="10" />
        </clipPath>
      </defs>
      <path d="M12 8V4H8" />
      <rect width="16" height="12" x="4" y="8" rx="2" />
      <path d="M2 14h2" />
      <path d="M20 14h2" />
      <g className="ng-eyes">
        <path d="M15 13v2" />
        <path d="M9 13v2" />
      </g>
      <g clipPath={`url(#${clip})`} className="ng-code">
        {row(13.4, "10110100110", "01001011001", "ng-bits-1")}
        {row(18, "01101001011", "10010110100", "ng-bits-2")}
      </g>
    </Glyph>
  );
}

/** The context bar's notification bell: lucide's Bell, with the housing and
 *  the clapper drawn apart. A ring swings the housing, and the clapper -
 *  hung from the same crown - follows on its own momentum a beat behind,
 *  overshoots and keeps swinging after the housing has settled. The motion
 *  is `ico-bell-ring` in index.css, started on click, not on hover. */
export function GlyphBell(p: GlyphProps) {
  return (
    <Glyph kind="bell" {...p}>
      <path
        className="ng-bell-housing"
        d="M3.262 15.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673C19.41 13.956 18 12.499 18 8A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326"
      />
      <path className="ng-bell-clapper" d="M10.268 21a2 2 0 0 0 3.464 0" />
    </Glyph>
  );
}
