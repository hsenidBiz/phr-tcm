// Putting back what an assistant's repairs did to a script's preconditions.

import { expect, test } from "vitest";
import type { PreconditionRepair } from "../../bindings";
import { putBackPreconditions, repairText } from "./preconditionRepairs";

const A = { flow: "f", stage: "a", value: 1 };
const B = { flow: "f", stage: "b", value: 1 };
const C = { flow: "f", stage: "c", value: "Q4" };

test("each kind of repair is undone, newest first", () => {
  const repairs: PreconditionRepair[] = [
    { op: "remove", before: A, reason: "r" },
    { op: "add", after: B, reason: "r" },
    { op: "change", before: A, after: C, reason: "r" },
  ];
  // A removed, then B added, then A (put back by hand) changed to C.
  expect(putBackPreconditions([B, C], repairs, [])).toEqual([B, C]);
  expect(putBackPreconditions([B, C], repairs, [1])).toEqual([C]);
  expect(putBackPreconditions([B, C], repairs, [2])).toEqual([B, A]);
  expect(putBackPreconditions([B, C], repairs, [0, 1, 2])).toEqual([A]);
});

test("a repair whose result is no longer there is left as it is", () => {
  const change: PreconditionRepair = { op: "change", before: A, after: C, reason: "r" };
  expect(putBackPreconditions([B], [change], [0])).toEqual([B]);
  const remove: PreconditionRepair = { op: "remove", before: A, reason: "r" };
  // Not added twice.
  expect(putBackPreconditions([A], [remove], [0])).toEqual([A]);
});

test("a repair reads as what it did", () => {
  expect(repairText({ op: "remove", before: A, reason: "r" })).toBe("Removed f / a: 1");
  expect(repairText({ op: "add", after: C, reason: "r" })).toBe("Added f / c: Q4");
  expect(repairText({ op: "change", before: A, after: B, reason: "r" })).toBe("Changed f / a: 1 to f / b: 1");
});
