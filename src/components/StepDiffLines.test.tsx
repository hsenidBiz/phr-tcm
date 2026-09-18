import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import type { StepDiff } from "../lib/caseDiff";
import StepDiffLines from "./StepDiffLines";

/// A Shared Steps reference has no action/expected text of its own, so
/// diffing those blindly renders "#2" with nothing after it. Re-pointing a
/// step at a different Shared Steps work item must still show what moved.
test("a re-pointed shared reference shows both numbers", () => {
  const d: StepDiff = {
    index: 1,
    kind: "changed",
    old: { action: "", expected: "", shared: 812 },
    new: { action: "", expected: "", shared: 900 },
  };
  render(<StepDiffLines d={d} />);
  expect(screen.getByText("Shared steps #812")).toBeInTheDocument();
  expect(screen.getByText("Shared steps #900")).toBeInTheDocument();
});

/// An added or removed step renders its plain action text (StepDiffLines
/// line 57) - a shared step's action is always "", so that line used to be
/// blank. It must name the reference instead.
test("an added shared step names its reference, not a blank line", () => {
  const d: StepDiff = { index: 1, kind: "added", new: { action: "", expected: "", shared: 812 } };
  render(<StepDiffLines d={d} />);
  expect(screen.getByText("Shared steps #812")).toBeInTheDocument();
});

test("a removed shared step names its reference, not a blank line", () => {
  const d: StepDiff = { index: 1, kind: "removed", old: { action: "", expected: "", shared: 812 } };
  render(<StepDiffLines d={d} />);
  expect(screen.getByText("Shared steps #812")).toBeInTheDocument();
});
