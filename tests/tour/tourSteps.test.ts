import { expect, test } from "vitest";
import { TOUR_STEPS, tourStepsFor } from "../../src/tour/tourScript";

test("the Database Read Access stop needs Database Access", () => {
  const off = tourStepsFor(new Set());
  expect(off.some((s) => s.title === "Database Read Access")).toBe(false);
  expect(off.length).toBe(TOUR_STEPS.length - 1);
  const on = tourStepsFor(new Set(["db_access"] as const));
  expect(on).toEqual(TOUR_STEPS);
});

import { SIMPLE_HIDDEN_SECTIONS } from "../../src/lib/modules";

const ALL = new Set(["db_access", "autorun", "api_templates"] as never[]);

test("Simple mode drops the stops on screens Simple mode hides, and off keeps today's list", () => {
  const simple = tourStepsFor(ALL, true);
  const dests = simple.map((s) => s.where).filter((w) => w?.area === "cases");
  for (const w of dests) {
    expect(SIMPLE_HIDDEN_SECTIONS.has((w as { section: string }).section)).toBe(false);
  }
  expect(simple.length).toBeLessThan(tourStepsFor(ALL, false).length);
  expect(tourStepsFor(ALL, false)).toEqual(tourStepsFor(ALL));
});

test("Simple mode keeps the stops on screens it leaves", () => {
  const sections = tourStepsFor(ALL, true).map((s) => (s.where?.area === "cases" ? s.where.section : null));
  expect(sections).toContain("manual");
  expect(sections).toContain("view");
  expect(sections).toContain("run");
});
