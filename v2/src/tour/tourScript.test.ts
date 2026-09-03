import { expect, test } from "vitest";
import { CASE_ITEMS, WORK_ITEMS } from "../components/Sidebar";
import { TOUR_ANCHORS, TOUR_CHROME_ANCHORS, TOUR_STEPS, tourAwaitedWhere, tourControl, tourDestination, type TourControl, type TourWhere } from "./tourScript";
import { tourWaitingCard } from "./UiTour";

/** Goal 4: the tour describes the app the user can see, in the words a
 * tester would use. Anything on this list is about the inside.
 *
 * JSON was on this list and has been taken off deliberately. The test the
 * ban exists to pass is "would a tester recognise this word", and here
 * they would: the Import screen's own heading says "Import your test cases
 * from a JSON file", and the file an assistant hands them really is a
 * .json. Banning it forced the tour to describe that file more vaguely
 * than the screen it is pointing at - which is the opposite of the goal.
 * Everything else on the list is still about the inside. */
const JARGON =
  /\b(MCP|IPC|API|cache|caching|query|endpoint|binding|localStorage|repo|work item|payload|schema|backend|frontend)\b/i;

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

// --- crossing into Work Manager can take two clicks -----------------------
//
// The pill's real handler is `onToggleWork` in App.tsx, which is just
// `setWorkMode((w) => !w)` - it does not also reset `workSection`. So a
// user who last had Pull Requests open lands there, not on Board, and has
// to click Board separately. `walkFrom` above always starts with
// `workSection: "board"` (its default), so it never exercises this second
// hop - this test does, by hand.

test("crossing into Work Manager can take two clicks when the user's last section was not Board", () => {
  const boardIndex = TOUR_STEPS.findIndex(
    (s) => s.where?.area === "work" && s.where.workSection === "board",
  );
  const dest = tourDestination(boardIndex, TOUR_STEPS)!;
  expect(dest).toEqual({ area: "work", workSection: "board" });

  // Start from the tour's last cases stop (AI Bridge), the way the route
  // actually arrives at this stop.
  const atCases: TourWhere = { area: "cases", section: "ai" };
  expect(tourAwaitedWhere(boardIndex, atCases, TOUR_STEPS)).toEqual(dest);
  expect(tourControl(dest, atCases)).toEqual({ kind: "switch", to: "work" });

  // Click the pill: the user crosses into Work Manager, but lands wherever
  // they last were there - Pull Requests, not Board.
  const atWorkPrs: TourWhere = { area: "work", workSection: "prs" };
  expect(tourAwaitedWhere(boardIndex, atWorkPrs, TOUR_STEPS)).toEqual(dest); // still waiting
  expect(tourControl(dest, atWorkPrs)).toEqual({ kind: "work", workSection: "board" });

  // Click Board: now the stop is satisfied.
  const atBoard: TourWhere = { area: "work", workSection: "board" };
  expect(tourAwaitedWhere(boardIndex, atBoard, TOUR_STEPS)).toBeNull();
});

// --- the waiting card's own copy obeys the same gates ----------------------
//
// The card shown while the tour waits builds its title/body at runtime
// (`tourWaitingCard` in UiTour.tsx) from the rail's own labels, so the gates
// above - which only scan TOUR_STEPS - never see it. Drive every rail label
// the tour can actually put in front of a user through the same helper the
// app renders, and hold it to the same caps and the same jargon regex as the
// script copy.
//
// Scoped to sections a stop's `where` can actually name - the same reason
// "the route covers both areas..." above excludes Auto Run and Settings:
// `tourControl` can only ever be asked for a destination that appears as a
// `where`, so a label that is never one (Auto Run, Settings, New Work Item)
// can never reach this card. Derived from TOUR_STEPS, not a hand-kept
// exclude list, so a future stop that starts targeting one of them pulls it
// into this test automatically.

test("the waiting card's copy, for every rail label the tour can actually show, obeys the script's own gates", () => {
  const reachableSections = new Set(
    TOUR_STEPS.filter((s) => s.where?.area === "cases").map((s) => (s.where as { section: string }).section),
  );
  const reachableWorkSections = new Set(
    TOUR_STEPS.filter((s) => s.where?.area === "work").map((s) => (s.where as { workSection: string }).workSection),
  );
  const controls: TourControl[] = [
    ...CASE_ITEMS.filter((c) => reachableSections.has(c.id)).map((c): TourControl => ({ kind: "case", section: c.id })),
    ...WORK_ITEMS.filter((w) => reachableWorkSections.has(w.id)).map((w): TourControl => ({ kind: "work", workSection: w.id })),
    { kind: "switch", to: "work" },
    { kind: "switch", to: "cases" },
  ];
  // Sanity check on the scoping itself: it should still cover a real spread
  // of rail rows, not have quietly emptied out.
  expect(controls.length).toBeGreaterThanOrEqual(7);
  for (const control of controls) {
    const { title, body } = tourWaitingCard(control);
    expect(title.length, title).toBeLessThanOrEqual(34);
    expect(body.length, title).toBeLessThanOrEqual(160);
    expect(JARGON.test(`${title} ${body}`), `jargon in "${title}"`).toBe(false);
  }
});
