import { expect, test } from "vitest";
import { iterationDetails } from "./iterations";

test("dated sprints get dd/mm/yyyy ranges; undated nodes get nothing", () => {
  const d = iterationDetails([
    { path: "P", start_date: null, finish_date: null },
    { path: "P\S1", start_date: "2026-07-20T00:00:00Z", finish_date: "2026-07-31T00:00:00Z" },
  ]);
  expect(d["P\S1"]).toBe("20/07/2026 - 31/07/2026");
  expect(d["P"]).toBeUndefined();
});
