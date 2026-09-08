import { expect, test } from "vitest";
import type { WorkRevision } from "../bindings";
import {
  dayLabel,
  displayValue,
  groupByDay,
  humanDuration,
  matchesFilter,
  relativeTime,
  stateJourney,
  summarize,
} from "./history";

const HOUR = 3_600_000;
const DAY = 24 * HOUR;
const NOW = Date.parse("2026-07-27T12:00:00Z");
const ago = (ms: number) => new Date(NOW - ms).toISOString();

const rev = (over: Partial<WorkRevision> = {}): WorkRevision => ({
  rev: 1,
  by: "Avin Alwis",
  avatar_url: "",
  at: ago(HOUR),
  fields: [],
  links_added: [],
  links_removed: [],
  state_from: "",
  state_to: "",
  comment_added: false,
  ...over,
});

const stateRev = (n: number, from: string, to: string, at: string) =>
  rev({
    rev: n,
    at,
    state_from: from,
    state_to: to,
    fields: [{ reference_name: "System.State", label: "State", old: from, new: to }],
  });

// The whole point of the summary: an item that bounced should say so,
// instead of making you count arrows in a sideways-scrolling strip.
test("the state journey counts visits and totals time per state", () => {
  const history = [
    stateRev(4, "New", "In Progress", ago(1 * DAY)), // newest
    stateRev(3, "In Progress", "New", ago(3 * DAY)),
    stateRev(2, "New", "In Progress", ago(6 * DAY)),
    stateRev(1, "", "New", ago(9 * DAY)),
  ];
  const journey = stateJourney(history, NOW);

  expect(journey.map((s) => s.state)).toEqual(["New", "In Progress"]); // order of first entry
  const [newState, inProgress] = journey;

  expect(newState.visits).toBe(2); // entered New twice
  expect(inProgress.visits).toBe(2);
  expect(inProgress.current).toBe(true);
  expect(newState.current).toBe(false);

  // New: 9d->6d (3d) plus 3d->1d (2d) = 5d. In Progress: 6d->3d (3d) plus 1d->now (1d) = 4d.
  expect(newState.totalMs).toBe(5 * DAY);
  expect(inProgress.totalMs).toBe(4 * DAY);
});

test("the newest state's time runs up to now, not to the next revision", () => {
  const journey = stateJourney([stateRev(1, "", "Active", ago(2 * HOUR))], NOW);
  expect(journey[0].totalMs).toBe(2 * HOUR);
  expect(journey[0].current).toBe(true);
});

test("revisions that changed no state do not appear in the journey", () => {
  const journey = stateJourney(
    [rev({ rev: 2, fields: [{ reference_name: "System.Tags", label: "Tags", old: "", new: "x" }] })],
    NOW,
  );
  expect(journey).toEqual([]);
});

test("a revision with no usable date still counts as a visit", () => {
  const journey = stateJourney([stateRev(1, "", "New", "")], NOW);
  expect(journey[0].visits).toBe(1);
  expect(journey[0].totalMs).toBe(0);
});

test("durations read in the largest sensible unit", () => {
  expect(humanDuration(0)).toBe("");
  expect(humanDuration(30_000)).toBe(""); // under a minute is noise
  expect(humanDuration(5 * 60_000)).toBe("5m");
  expect(humanDuration(3 * HOUR)).toBe("3h");
  expect(humanDuration(3 * HOUR + 20 * 60_000)).toBe("3h 20m");
  expect(humanDuration(2 * DAY)).toBe("2d");
  expect(humanDuration(2 * DAY + 4 * HOUR)).toBe("2d 4h");
});

test("relative times are singular or plural correctly", () => {
  expect(relativeTime(ago(30_000), NOW)).toBe("just now");
  expect(relativeTime(ago(60_000), NOW)).toBe("1 minute ago");
  expect(relativeTime(ago(2 * HOUR), NOW)).toBe("2 hours ago");
  expect(relativeTime(ago(1 * DAY), NOW)).toBe("1 day ago");
  expect(relativeTime(ago(45 * DAY), NOW)).toBe("1 month ago");
  expect(relativeTime("", NOW)).toBe("");
});

test("days bucket as Today / Yesterday / a date", () => {
  expect(dayLabel(ago(HOUR), NOW)).toBe("Today");
  expect(dayLabel(ago(DAY), NOW)).toBe("Yesterday");
  expect(dayLabel(ago(5 * DAY), NOW)).toContain("Jul");
  expect(dayLabel("", NOW)).toBe("Unknown date");
});

test("grouping keeps order and merges only adjacent same-day entries", () => {
  const groups = groupByDay(
    [rev({ rev: 3, at: ago(HOUR) }), rev({ rev: 2, at: ago(2 * HOUR) }), rev({ rev: 1, at: ago(DAY) })],
    NOW,
  );
  expect(groups.map((g) => g.label)).toEqual(["Today", "Yesterday"]);
  expect(groups[0].items.map((i) => i.rev)).toEqual([3, 2]);
});

test("the field filter excludes state-only changes", () => {
  const stateOnly = stateRev(1, "New", "Active", ago(HOUR));
  const edit = rev({
    fields: [{ reference_name: "System.Tags", label: "Tags", old: "", new: "smoke" }],
  });
  const link = rev({ links_added: ["Commit link"] });

  expect(matchesFilter(stateOnly, "state")).toBe(true);
  expect(matchesFilter(stateOnly, "fields")).toBe(false);
  expect(matchesFilter(edit, "fields")).toBe(true);
  expect(matchesFilter(edit, "state")).toBe(false);
  expect(matchesFilter(link, "links")).toBe(true);
  expect(matchesFilter(link, "all")).toBe(true);
});

test("the one-line summary names what happened", () => {
  expect(summarize(stateRev(1, "New", "Active", ago(HOUR)))).toBe("New → Active");
  expect(summarize(stateRev(1, "", "New", ago(HOUR)))).toBe("set to New");
  expect(
    summarize(
      rev({ fields: [{ reference_name: "System.Tags", label: "Tags", old: "", new: "x" }] }),
    ),
  ).toBe("edited Tags");
  expect(
    summarize(
      rev({
        fields: [
          { reference_name: "System.Tags", label: "Tags", old: "", new: "x" },
          { reference_name: "System.Title", label: "Title", old: "a", new: "b" },
        ],
      }),
    ),
  ).toBe("edited 2 fields");
  expect(summarize(rev({ links_added: ["Commit link"], comment_added: true }))).toBe(
    "added Commit link · commented",
  );
  expect(summarize(rev())).toBe("made changes");
});

test("ISO dates in diffs render readably; other values pass through", () => {
  const out = displayValue("2026-07-24T09:28:31Z");
  expect(out).not.toContain("T");
  expect(out).toMatch(/Jul/);
  // A date-only field (UTC midnight, as ADO stores Start/Finish Date) must
  // show no invented clock time - and must not shift a day in any timezone.
  const dateOnly = displayValue("2026-07-24T00:00:00Z");
  expect(dateOnly).not.toMatch(/\d\d:\d\d/);
  expect(dateOnly).toMatch(/24/);
  // Everything else is untouched - including things that merely look numeric.
  expect(displayValue("6.8")).toBe("6.8");
  expect(displayValue("Moved out of state Resolved")).toBe("Moved out of state Resolved");
  expect(displayValue("")).toBe("");
});
