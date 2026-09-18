import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import type { TestCase } from "../bindings";
import type { CaseDiff } from "../lib/caseDiff";
import { QueueRowInner, type QueueRowProps } from "./QueueRow";

const tc: TestCase = {
  title: "Versioning",
  steps: [
    { action: "Open", expected: "Shown" },
    { action: "Wait", expected: "" },
    { action: "Lock", expected: "Locked" },
  ],
  tags: "",
  automation_status: "Not Automated",
  module_value: "",
  preconditions: "",
  update_id: 154650,
};

function props(diff: CaseDiff): QueueRowProps {
  const noop = () => {};
  return {
    tc,
    index: 0,
    org: "acme",
    project: "Web",
    isSelected: false,
    stepsOpen: false,
    diffOpen: true,
    editing: false,
    failed: false,
    uploaded: false,
    held: false,
    ambiguous: false,
    touched: undefined,
    reviewing: false,
    problem: null,
    duplicate: null,
    diff,
    diffFailed: false,
    busy: false,
    onToggleSelect: noop,
    onToggleSteps: noop,
    onToggleDiff: noop,
    onToggleEdit: noop,
    onRemove: noop,
    onSave: noop,
    onCancelEdit: noop,
  };
}

/// Field report 1.25.2: "Click to view 12 step types changing" opened an
/// empty panel. The panel drew field and step-text changes only, and a
/// step-type repair has neither.
test("a case whose only change is step types says which steps change, in the open diff", () => {
  const diff: CaseDiff = {
    fields: [],
    steps: {
      added: 0,
      removed: 0,
      changed: 0,
      retyped: 2,
      retypedDetail: [
        { index: 0, from: "ActionStep", to: "ValidateStep" },
        { index: 2, from: "ActionStep", to: "ValidateStep" },
      ],
      detail: [],
    },
    blankSkipped: [],
    noop: false,
  };
  render(<ul>{QueueRowInner(props(diff))}</ul>);
  expect(screen.getByText("Step types:")).toBeInTheDocument();
  expect(
    screen.getByText("Steps 1 and 3 become validation steps, because they have an Expected Result."),
  ).toBeInTheDocument();
});

test("a row whose upload outcome is unknown says so", () => {
  render(<QueueRowInner {...props(null as unknown as CaseDiff)} diff={null} held />);
  expect(screen.getByText("Outcome unknown - check before uploading again")).toBeInTheDocument();
});

// Deviation from brief (controller ruling): reconcile_upload cannot tell
// "not found" from "ambiguous" - a title with more matches in Azure DevOps
// than rows being checked stays held with its own reason, not the generic
// "outcome unknown" text.
test("a row held because its title is ambiguous in Azure DevOps says that instead", () => {
  render(<QueueRowInner {...props(null as unknown as CaseDiff)} diff={null} held ambiguous />);
  expect(
    screen.getByText(
      "More than one test case with this title exists in Azure DevOps - check there before uploading again.",
    ),
  ).toBeInTheDocument();
  expect(screen.queryByText("Outcome unknown - check before uploading again")).not.toBeInTheDocument();
});
