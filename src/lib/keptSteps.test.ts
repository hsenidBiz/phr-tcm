import { expect, test } from "vitest";
import { keptSteps } from "./keptSteps";

test("a step with neither an action nor an expected result is dropped", () => {
  expect(
    keptSteps([
      { action: "Open", expected: "Shown" },
      { action: "", expected: "" },
      { action: "  ", expected: " " },
    ]),
  ).toEqual([{ action: "Open", expected: "Shown" }]);
});

test("a step with only one half, or a shared steps reference, is kept", () => {
  const steps = [
    { action: "", expected: "Shown" },
    { action: "Open", expected: "" },
    { action: "", expected: "", shared: 812 },
  ];
  expect(keptSteps(steps)).toEqual(steps);
});

test("a blank step that holds an attachment is kept, so nothing is silently lost", () => {
  const steps = [{ action: "", expected: "", attachments: [{ id: "g1", name: "old.png" }] }];
  expect(keptSteps(steps)).toEqual(steps);
  expect(keptSteps([{ action: "", expected: "", attachments: [] }])).toEqual([]);
});
