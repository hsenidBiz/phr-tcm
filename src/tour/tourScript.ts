/**
 * The guided tour, stop by stop: where the app has to be, what to ring,
 * and what to say about it.
 *
 * House rules, enforced by tourScript.test.ts:
 * - two stops per tab at most (AI Bridge is allowed four - it is the one
 *   tab a new user never sees past its first card - and Settings three:
 *   it is the one tab the tour asks the person to ACT in rather than
 *   read, so the stop that hands them the theme cannot also carry the two
 *   things they would otherwise never find);
 * - one or two short sentences, in the words a tester uses;
 * - nothing about how the app works inside.
 */
import type { Section, WorkSection } from "../components/Sidebar";

/** The `data-tour` names the script is allowed to ring. Each one must
 * exist in the app - `tourAnchors.test.ts` proves it. */
export const TOUR_ANCHORS = [
  "org",
  "pbi",
  "settings",
  "nav-prs",
  "case-form",
  "queue",
  "queue-review",
  "import-drop",
  "case-list",
  "view-list",
  "run-list",
  "plans-tree",
  "manage-plans",
  "ai-repos",
  "ai-tools",
  "ai-toolset",
  "ai-db",
  "board-columns",
  "theme",
  "settings-backup",
  "settings-updates",
] as const;

export type TourAnchor = (typeof TOUR_ANCHORS)[number];

/** The anchors that are app chrome rather than a tab's own controls: the
 * bar above every screen, and the rail beside it. They do not count
 * against a tab's two-stop allowance. */
export const TOUR_CHROME_ANCHORS = ["org", "pbi", "settings", "nav-prs"] as const;

/** Where the app must be for a stop to make sense. */
export type TourWhere =
  | { area: "cases"; section: Section }
  | { area: "work"; workSection: WorkSection };

export type TourStep = {
  /** Left out = stay wherever the last stop left the app. */
  where?: TourWhere;
  /** Left out = a centred card with nothing ringed. */
  anchor?: TourAnchor;
  title: string;
  body: string;
  /** This stop hands the reader something to DO on the screen behind it
   * (the theme), rather than something to read. The way on then says
   * "Continue" instead of "Next".
   *
   * The theme stop used to ring the Settings gear and say colours lived
   * behind it - a thing to go and do later, and later never comes - and
   * then carried a copy of the swatches in the card, which taught nobody
   * where the real ones are. The tour now walks the reader into Settings
   * and rings the real block, so the pick is made on the screen they will
   * come back to. It is also the one thing the tour deliberately leaves
   * behind: every other part is made-up data that disappears on close, and
   * a theme is the user's own choice. */
  act?: boolean;
};

const cases = (section: Section): TourWhere => ({ area: "cases", section });
const work = (workSection: WorkSection): TourWhere => ({ area: "work", workSection });

export const TOUR_STEPS: TourStep[] = [
  {
    title: "Welcome to Test Case Manager",
    body: "This tour will briefly show you around the app, using made-up data. Skip tour ends it whenever you like.",
  },
  {
    where: cases("manual"),
    anchor: "org",
    title: "Choose where you work",
    body: "Pick your organisation and project up here. Everything you do stays inside that choice.",
  },
  {
    anchor: "pbi",
    title: "Pick what you are testing",
    body: "Search for a PBI by name or number. The test cases you write attach themselves to it.",
  },
  {
    anchor: "case-form",
    title: "Manually write a test case",
    body: "Give it a title, then fill in the numbered steps and the expected result at each one.",
  },
  {
    anchor: "queue",
    title: "Build up a batch",
    // The Import File tab has its own stop on this panel's controls now,
    // so this one no longer has to speak for it.
    body: "Your finished test cases queue up here, ready to upload to Azure DevOps in one go.",
  },
  {
    where: cases("import"),
    anchor: "import-drop",
    title: "Bring cases in from a file",
    body: "Import a .json file your AI assistant generated. Edit that file later and the changes follow through on their own.",
  },
  {
    where: cases("edit"),
    anchor: "case-list",
    title: "Change what you already have",
    body: "Every case on the chosen PBI, ready to edit - one at a time, or a whole selection at once.",
  },
  {
    where: cases("view"),
    anchor: "view-list",
    title: "Read without changing",
    body: "The same cases, safe to browse. Leave a note on anything that needs a second look.",
  },
  {
    where: cases("run"),
    anchor: "run-list",
    title: "Run your tests",
    body: "Pick the cases to run and mark each one as you go. A small floating window keeps them beside whatever you are testing.",
  },
  {
    where: cases("suites"),
    anchor: "plans-tree",
    title: "Find any set of tests",
    body: "Browse the folders your team keeps tests in, then jump straight to editing or running them.",
  },
  {
    where: cases("manage"),
    anchor: "manage-plans",
    title: "Arrange a PBI's suite",
    body: "Every plan that holds the PBI's cases, with the suites and cases underneath. Tick cases to copy them into another suite, and drag to set the order testers see.",
  },
  {
    // Back to Import File, for the half of that panel the first stop there
    // did not cover: nothing leaves this app without going through here.
    where: cases("import"),
    anchor: "queue-review",
    title: "Review before you upload",
    body: "Nothing reaches Azure DevOps until you look. Review shows what will be created and what updated, warns about duplicates, and Upload is the button that sends.",
  },
  {
    where: cases("ai"),
    anchor: "ai-repos",
    title: "Working repositories",
    body: "Point the app at the folder your project lives in. Cases written or imported for it are kept there.",
  },
  {
    anchor: "ai-tools",
    title: "Connect your assistants",
    body: "The coding assistants on your machine show up here. Connect one and it can read and write test cases with you.",
  },
  {
    anchor: "ai-toolset",
    title: "Decide what it may do",
    body: "Switch each tool on or off. Anything switched off is simply never offered to your assistant.",
  },
  {
    anchor: "ai-db",
    title: "Company database",
    body: "Your assistant can look up tables and check real data while it writes, using the connection you choose here.",
  },
  {
    where: work("board"),
    anchor: "board-columns",
    title: "The other half of the app",
    body: "Your own items as cards. Move a card to the next column to change its status.",
  },
  {
    anchor: "nav-prs",
    title: "Reviews waiting for you",
    body: "Pull requests you raised or were asked to look at, with a badge when one needs you.",
  },
  {
    where: cases("settings"),
    anchor: "theme",
    title: "Make it yours",
    body: "Pick a theme and an accent colour. The whole app follows, and this choice stays after the tour.",
    act: true,
  },
  {
    anchor: "settings-backup",
    title: "Take it with you",
    body: "Backup and transfer packs your settings and queue into one file for another machine.",
  },
  {
    anchor: "settings-updates",
    title: "Kept up to date",
    body: "The app checks for updates on its own; this is where you see the version and check by hand.",
  },
  {
    title: "That is the tour",
    body: "Close it and the made-up data disappears. You are back in your own work, exactly where you left it.",
  },
];

/** Where the app is right now, in the same shape a stop declares. */
export type TourAt = TourWhere;

/** Two locations are the same place. */
export function sameTourWhere(a: TourWhere | undefined, b: TourWhere | undefined): boolean {
  if (!a || !b) return a === b;
  if (a.area === "cases") return b.area === "cases" && a.section === b.section;
  return b.area === "work" && a.workSection === b.workSection;
}

/**
 * Where a stop needs the app to be: its own `where`, or - for a stop that
 * leaves it out - the last one declared at or before it. Returns the
 * script's own object, so the identity is stable across calls and two
 * consecutive stops that share a destination compare equal by reference.
 */
export function tourDestination(i: number, steps: TourStep[] = TOUR_STEPS): TourWhere | undefined {
  for (let j = Math.min(i, steps.length - 1); j >= 0; j--) {
    if (steps[j]?.where) return steps[j].where;
  }
  return undefined;
}

/**
 * The destination this stop is waiting for the user to walk to, or null
 * when the app is already there.
 *
 * Deliberately measured against where the app IS, not against the previous
 * stop's destination: the tour can be started from anywhere (usually
 * Settings, where the button lives), so whether the second stop is a move
 * is not something the script alone can answer.
 */
export function tourAwaitedWhere(
  i: number,
  at: TourWhere,
  steps: TourStep[] = TOUR_STEPS,
): TourWhere | null {
  const dest = tourDestination(i, steps);
  if (!dest || sameTourWhere(dest, at)) return null;
  return dest;
}

/** The one control that takes the user from `at` towards `dest`: a rail
 * row when both are in the same half of the app, otherwise the context
 * bar's pill that crosses between the two halves. */
export type TourControl =
  | { kind: "case"; section: Section }
  | { kind: "work"; workSection: WorkSection }
  | { kind: "switch"; to: "cases" | "work" };

export function tourControl(dest: TourWhere, at: TourWhere): TourControl | null {
  if (sameTourWhere(dest, at)) return null;
  if (dest.area !== at.area) return { kind: "switch", to: dest.area };
  return dest.area === "cases"
    ? { kind: "case", section: dest.section }
    : { kind: "work", workSection: dest.workSection };
}
