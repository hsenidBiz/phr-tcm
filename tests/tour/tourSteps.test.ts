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

test("Basic mode drops the stops on screens Basic mode hides, and off keeps today's list", () => {
  const simple = tourStepsFor(ALL, true);
  expect(simple.length).toBeLessThan(TOUR_STEPS.length);
  for (let i = 0; i < simple.length; i++) {
    const where = tourDestination(i, simple);
    if (where?.area === "cases") expect(SIMPLE_HIDDEN_SECTIONS.has(where.section)).toBe(false);
  }
  expect(tourStepsFor(ALL, false)).toEqual(TOUR_STEPS);
});

test("Basic mode keeps the stops on screens it leaves, and skips Manual Entry's", () => {
  const sections = tourStepsFor(ALL, true).map((s) => (s.where?.area === "cases" ? s.where.section : null));
  expect(sections).not.toContain("manual");
  expect(sections).toContain("view");
  expect(sections).toContain("run");
});

test("Basic mode skips the Working repositories stop; off keeps it", () => {
  expect(tourStepsFor(ALL, true).some((s) => s.anchor === "ai-repos")).toBe(false);
  expect(tourStepsFor(ALL, false).some((s) => s.anchor === "ai-repos")).toBe(true);
});

test("Basic mode skips the tool list stop (its card is hidden); off keeps it", () => {
  expect(tourStepsFor(ALL, true).some((s) => s.anchor === "ai-toolset")).toBe(false);
  expect(tourStepsFor(ALL, false).some((s) => s.anchor === "ai-toolset")).toBe(true);
});

test("Basic mode: every kept stop still resolves to a screen Basic mode offers", () => {
  const steps = tourStepsFor(ALL, true);
  const titles = steps.map((s) => s.title);
  expect(titles).toContain("Choose where you work");
  expect(titles).toContain("Connect your assistants");
  steps.forEach((s, i) => {
    const where = tourDestination(i, steps);
    // The stops after a dropped one carry their own destination.
    if (s.anchor === "ai-tools") expect(where).toEqual({ area: "cases", section: "ai" });
    if (s.anchor === "org" || s.anchor === "pbi") expect(where).toEqual({ area: "cases", section: "view" });
    if (where?.area === "cases") expect(SIMPLE_HIDDEN_SECTIONS.has(where.section)).toBe(false);
  });
  expect(steps.some((s) => s.anchor === "case-form" || s.anchor === "queue")).toBe(false);
});
