import { expect, test } from "vitest";
import { TOUR_STEPS, tourStepsFor } from "./tourScript";

test("the Database Read Access stop needs Database Access", () => {
  const off = tourStepsFor(new Set());
  expect(off.some((s) => s.title === "Database Read Access")).toBe(false);
  expect(off.length).toBe(TOUR_STEPS.length - 1);
  const on = tourStepsFor(new Set(["db_access"] as const));
  expect(on).toEqual(TOUR_STEPS);
});
