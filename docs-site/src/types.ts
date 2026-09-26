// The help site's content model. Later work (the capture script, the
// per-screen content files, the completeness guard) relies on these exact
// names - change them only together with every consumer.
//
// Inline formatting in any prose field (summary, does, tips, steps):
//   [[Ctrl+K]]  renders as key caps (split on "+")
//   **Upload**  renders as a strong UI label
// Nothing else is interpreted; text is never parsed as HTML.

export type Locate =
  | { role: string; name: string } // exact accessible name
  | { role: string; nameRe: string } // RegExp source, e.g. "^Run \\d+ in runner$"
  | { label: string } // getByLabel
  | { text: string } // getByText exact
  | { testId: string }; // data-testid / data-tour

export type Step =
  | { nav: string } // sidebar item's visible label, e.g. "Run Tests"
  | { click: Locate }
  | { press: string } // e.g. "Control+K", "Escape"
  | { waitFor: Locate }
  | { scrollTo: Locate }
  | { runnerWindow: true }; // following steps and the shot target the runner window

/** A size in CSS pixels. */
export type Size = { w: number; h: number };

/** A control's box on its shot, in that shot's pixels. */
export type Box = { x: number; y: number; w: number; h: number };

/** `size` is the window the shot is taken of, when it is not the main
 *  window (SHOT_WIDTH x SHOT_HEIGHT) - the runner is captured at its real
 *  size (src/lib/runnerSize.ts) and framed as a narrow window. */
export type Shot = { id: string; route: Step[]; alt: string; size?: Size };

/** List a screen's controls in on-screen reading order (top to bottom,
 *  then left to right). Once positions.json has boxes, the site numbers the
 *  placed controls by those boxes; the listed order is what numbers them
 *  before that, and orders any control still missing a position. */
export type Control = {
  id: string; // unique within the screen, kebab-case
  shot: string; // Shot.id it is marked on
  locate: Locate;
  name: string; // as the person sees it
  does: string; // what it does, plain words
  tips?: string[];
};

export type Screen = {
  id: string;
  title: string;
  group: Group;
  summary: string;
  shots: Shot[];
  controls: Control[];
  tips?: string[];
  howTo?: { title: string; steps: string[] }[];
};

export type Group =
  | "Getting started"
  | "Test cases"
  | "Running tests"
  | "Work Manager"
  | "AI tools"
  | "Settings";

/** positions.json: per shot, the size it was captured at and each placed
 *  control's box in those pixels. */
export type ShotPositions = { size: Size; controls: Record<string, Box> };
export type Positions = Record<string, ShotPositions>;

export type Recipe = { id: string; title: string; steps: { text: string; link?: string }[] };

/* ------------------------------------------------------------------ */
/* Site-level shapes (not part of the per-screen content contract).    */
/* ------------------------------------------------------------------ */

/** The hero and its Quick start strip. `link` is a section id (a screen
 *  id, or "screen/control"); a step whose target is not on the page yet
 *  renders as plain text instead of a dead link. */
export type Intro = {
  promise: string;
  lead: string;
  /** Shot id shown in the hero's window frame. */
  heroShot?: string;
  quickStart: { label: string; hint: string; link: string }[];
};

/** Everything `render` needs. `available` lists the shot ids that have an
 *  image in both themes; any other shot shows a placeholder frame. */
export type SiteContent = {
  screens: Screen[];
  recipes: Recipe[];
  intro: Intro;
  positions: Positions;
  available: string[];
};

/** The main window's shot size - every shot's, unless it sets `size`. */
export const SHOT_WIDTH = 1440;
export const SHOT_HEIGHT = 900;

/** The size a shot is captured at. */
export const shotSize = (shot: Shot): Size => shot.size ?? { w: SHOT_WIDTH, h: SHOT_HEIGHT };

/** The fixed group order of the sidebar and the page. */
export const GROUPS: Group[] = [
  "Getting started",
  "Test cases",
  "Running tests",
  "Work Manager",
  "AI tools",
  "Settings",
];
