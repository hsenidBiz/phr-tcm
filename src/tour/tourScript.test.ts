import { expect, test } from "vitest";
import { CASE_ITEMS, WORK_ITEMS, type Section } from "../components/Sidebar";
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

test("no tab gets more than two stops - except AI Bridge (four) and Settings (three)", () => {
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
  expect(counts.get("settings"), "Settings").toBe(3); // its own cap, exercised
  const cap = (key: string) => (key === "ai" ? 4 : key === "settings" ? 3 : 2);
  for (const [key, n] of counts) {
    expect(n, `${key} has ${n} stops`).toBeLessThanOrEqual(cap(key));
  }
});

test("the route covers both areas and every tab a user gets", () => {
  const keys = TOUR_STEPS.filter((s) => s.where).map((s) =>
    s.where!.area === "cases" ? s.where!.section : `work/${s.where!.workSection}`,
  );
  for (const expected of [
    "manual",
    "import",
    "edit",
    "view",
    "run",
    "suites",
    "manage",
    "ai",
    "work/board",
    // The tour now walks the user into Settings and lets them pick a
    // theme on the real screen, so this IS a destination like any other.
    "settings",
  ]) {
    expect(keys, `missing ${expected}`).toContain(expected);
  }
  // Auto Run never ships to users.
  expect(keys).not.toContain("autorun");
});

test("the script visits Suite Management, the review controls and the three Settings stops", () => {
  const at = (i: number) => {
    const w = tourDestination(i, TOUR_STEPS);
    return w?.area === "cases" ? w.section : w ? `work/${w.workSection}` : "";
  };
  // Anchored stops only: the closing card rings nothing and inherits
  // whatever destination the tour left it on.
  const stops = TOUR_STEPS.map((s, i) => ({ where: at(i), anchor: s.anchor })).filter(
    (s) => s.anchor,
  );

  expect(stops).toContainEqual({ where: "manage", anchor: "manage-plans" });
  // Import File gets a second stop, on the controls that review and send.
  expect(stops.filter((s) => s.where === "import").map((s) => s.anchor)).toEqual([
    "import-drop",
    "queue-review",
  ]);
  expect(stops.filter((s) => s.where === "settings").map((s) => s.anchor)).toEqual([
    "theme",
    "settings-backup",
    "settings-updates",
  ]);
  // The theme is picked on the real screen now, not on a control the card
  // carries of its own.
  for (const s of TOUR_STEPS) {
    expect(s, s.title).not.toHaveProperty("picker");
  }
});

test("the Manual Entry queue stop no longer speaks for the Import File tab", () => {
  const queue = TOUR_STEPS.find((s) => s.anchor === "queue")!;
  expect(queue.body).not.toMatch(/Import File/);
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
  "Arrange a PBI's suite",
  // Back to Import File for the review controls, so this is a move again.
  "Review before you upload",
  "Working repositories",
  "The other half of the app",
  "Make it yours",
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
// "the route covers both areas..." above excludes Auto Run: `tourControl`
// can only ever be asked for a destination that appears as a `where`, so a
// label that is never one (Auto Run, New Work Item) can never reach this
// card. Derived from TOUR_STEPS, not a hand-kept exclude list, so a future
// stop that starts targeting one of them pulls it into this test
// automatically - which is how Settings arrived here: it is not a rail row
// at all, so it is taken from the script rather than from CASE_ITEMS.

test("the waiting card's copy, for every control the tour can actually ask for, obeys the script's own gates", () => {
  const reachableSections = new Set(
    TOUR_STEPS.filter((s) => s.where?.area === "cases").map((s) => (s.where as { section: Section }).section),
  );
  const reachableWorkSections = new Set(
    TOUR_STEPS.filter((s) => s.where?.area === "work").map((s) => (s.where as { workSection: string }).workSection),
  );
  const controls: TourControl[] = [
    ...[...reachableSections].map((section): TourControl => ({ kind: "case", section })),
    ...WORK_ITEMS.filter((w) => reachableWorkSections.has(w.id)).map((w): TourControl => ({ kind: "work", workSection: w.id })),
    { kind: "switch", to: "work" },
    { kind: "switch", to: "cases" },
  ];
  // Every rail row the script names still has a label to show: a section
  // with no row of its own (Settings) must be spelled out by hand in
  // `tourWaitingCard`, and an empty one there would read "Go to ".
  for (const c of controls) {
    if (c.kind !== "case" || CASE_ITEMS.some((i) => i.id === c.section)) continue;
    expect(tourWaitingCard(c).title, `no label for ${c.section}`).not.toBe("Go to ");
  }
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

test("the Settings stop asks for the gear, which is not on the rail", () => {
  const { title, body } = tourWaitingCard({ kind: "case", section: "settings" });
  expect(title).toBe("Go to Settings");
  expect(body).toBe("Click the Settings gear at the top right to carry on.");
  // ...and it must never say "in the menu on the left", where there is no
  // Settings row to click.
  expect(body).not.toMatch(/menu on the left/);
});
