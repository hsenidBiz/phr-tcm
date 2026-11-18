import { expect, test } from "vitest";
import { ALL_MODULES, SIMPLE_HIDDEN_SECTIONS } from "../../src/lib/modules";
import { TOUR_STEPS, tourDestination, tourStepsFor } from "../../src/tour/tourScript";

test("the Database Read Access stop needs Database Access", () => {
  const off = tourStepsFor(new Set());
  expect(off.some((s) => s.title === "Database Read Access")).toBe(false);
  expect(off.length).toBe(TOUR_STEPS.length - 1);
  const on = tourStepsFor(new Set(["db_access"] as const));
  expect(on).toEqual(TOUR_STEPS);
});

const ALL = new Set(ALL_MODULES);

test("Simple mode drops the stops on screens Simple mode hides, and off keeps today's list", () => {
  const simple = tourStepsFor(ALL, true);
  expect(simple.length).toBeLessThan(TOUR_STEPS.length);
  for (let i = 0; i < simple.length; i++) {
    const where = tourDestination(i, simple);
    if (where?.area === "cases") expect(SIMPLE_HIDDEN_SECTIONS.has(where.section)).toBe(false);
  }
  expect(tourStepsFor(ALL, false)).toEqual(TOUR_STEPS);
});

test("Simple mode keeps the stops on screens it leaves", () => {
  const sections = tourStepsFor(ALL, true).map((s) => (s.where?.area === "cases" ? s.where.section : null));
  expect(sections).toContain("manual");
  expect(sections).toContain("view");
  expect(sections).toContain("run");
});
