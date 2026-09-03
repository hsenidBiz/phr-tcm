import { expect, test } from "vitest";
import { TOUR_ANCHORS, TOUR_CHROME_ANCHORS, TOUR_STEPS, tourAwaitedWhere, tourControl, tourDestination, type TourWhere } from "./tourScript";

/** Goal 4: the tour describes the app the user can see, in the words a
 * tester would use. Anything on this list is about the inside. */
const JARGON =
  /\b(MCP|IPC|API|JSON|cache|caching|query|endpoint|binding|localStorage|repo|work item|payload|schema|backend|frontend)\b/i;

test("every stop is short, plain and finished", () => {
  for (const s of TOUR_STEPS) {
    expect(s.title.length, s.title).toBeLessThanOrEqual(34);
    expect(s.body.length, s.title).toBeLessThanOrEqual(160);
    expect(s.body.trim().endsWith("."), s.title).toBe(true);
    expect(JARGON.test(`${s.title} ${s.body}`), `jargon in "${s.title}"`).toBe(false);
    // One or two sentences, never a paragraph.
    expect((s.body.match(/\.\s/g) ?? []).length, s.title).toBeLessThanOrEqual(1);
  }
});

test("no tab gets more than two stops - except AI Bridge, which gets four", () => {
  // A stop with no `where` stays wherever the last one left the app, so
  // the destination has to be carried forward - counting only the stops
  // that declare one would measure nothing.
  const counts = new Map<string, number>();
  let at = "";
  for (const s of TOUR_STEPS) {
    if (s.where) at = s.where.area === "cases" ? s.where.section : `work/${s.where.workSection}`;
    // The bar above every screen and the rail beside it are not a tab's
    // own controls, and neither is a card with nothing ringed.
    if (!s.anchor || (TOUR_CHROME_ANCHORS as readonly string[]).includes(s.anchor)) continue;
    counts.set(at, (counts.get(at) ?? 0) + 1);
  }
  expect(counts.get("manual"), "Manual Entry").toBe(2); // the cap, exercised
  for (const [key, n] of counts) {
    expect(n, `${key} has ${n} stops`).toBeLessThanOrEqual(key === "ai" ? 4 : 2);
  }
});

test("the route covers both areas and every tab a user gets", () => {
  const keys = TOUR_STEPS.filter((s) => s.where).map((s) =>
    s.where!.area === "cases" ? s.where!.section : `work/${s.where!.workSection}`,
  );
  for (const expected of ["manual", "import", "edit", "view", "run", "suites", "ai", "work/board"]) {
    expect(keys, `missing ${expected}`).toContain(expected);
  }
  // Auto Run never ships to users, and Settings is spotlighted from the
  // context bar rather than opened.
  expect(keys).not.toContain("autorun");
  expect(keys).not.toContain("settings");
});

test("every anchor a stop names is a declared anchor", () => {
  for (const s of TOUR_STEPS) {
    if (s.anchor) expect(TOUR_ANCHORS, s.title).toContain(s.anchor);
  }
});

test("it opens and closes with a card that needs nothing on screen", () => {
  expect(TOUR_STEPS[0].anchor).toBeUndefined();
  expect(TOUR_STEPS[TOUR_STEPS.length - 1].anchor).toBeUndefined();
});

// --- "is this stop waiting for the user?" -------------------------------
//
// The tour never moves the app: a stop whose destination is somewhere else
// waits for the user to walk there. Which stops those are is DERIVED - it
// depends on where the app was when the tour started, so the script alone
// cannot answer it.

/** Walk the whole route from `start`, clicking through each stop that
 * waits, and report the stops that asked to be walked to. */
function walkFrom(start: TourWhere): string[] {
  let at = start;
  const waited: string[] = [];
  for (let i = 0; i < TOUR_STEPS.length; i++) {
    const dest = tourAwaitedWhere(i, at, TOUR_STEPS);
    if (dest) {
      waited.push(TOUR_STEPS[i].title);
      at = dest; // the user clicks the control the tour asked for
    }
  }
  return waited;
}

const AFTER_MANUAL = [
  "Bring cases in from a file",
  "Change what you already have",
  "Read without changing",
  "Run your tests",
  "Find any set of tests",
  "Working repositories",
  "The other half of the app",
];

test("a tour started from Settings waits for Manual Entry first", () => {
  expect(walkFrom({ area: "cases", section: "settings" })).toEqual([
    "Choose where you work",
    ...AFTER_MANUAL,
  ]);
});

test("a tour started on Manual Entry does not wait for it", () => {
  expect(walkFrom({ area: "cases", section: "manual" })).toEqual(AFTER_MANUAL);
});

test("a stop with no destination of its own inherits the last declared one", () => {
  // Stop 5 (the queue) declares nothing and belongs to Manual Entry.
  expect(tourDestination(4, TOUR_STEPS)).toEqual({ area: "cases", section: "manual" });
  // The opening card is before any destination at all.
  expect(tourDestination(0, TOUR_STEPS)).toBeUndefined();
  // ...and it is the script's own object, so consecutive stops that share
  // a destination compare equal by reference.
  expect(tourDestination(4, TOUR_STEPS)).toBe(tourDestination(3, TOUR_STEPS));
});

test("the control to click is the rail row, or the pill when the half changes", () => {
  const manual = { area: "cases", section: "manual" } as const;
  const board = { area: "work", workSection: "board" } as const;
  const prs = { area: "work", workSection: "prs" } as const;
  expect(tourControl({ area: "cases", section: "import" }, manual)).toEqual({
    kind: "case",
    section: "import",
  });
  expect(tourControl(board, manual)).toEqual({ kind: "switch", to: "work" });
  expect(tourControl(manual, board)).toEqual({ kind: "switch", to: "cases" });
  expect(tourControl(board, prs)).toEqual({ kind: "work", workSection: "board" });
  expect(tourControl(manual, manual)).toBeNull();
});
