import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test } from "vitest";
import type { TestCase } from "../bindings";
import type { SyncChange } from "../lib/fileSync";
import SyncReport from "./SyncReport";

const tc = (title: string, over: Partial<TestCase> = {}): TestCase => ({
  title,
  steps: [],
  tags: "",
  automation_status: "Not Automated",
  module_value: "",
  preconditions: "",
  update_id: null,
  comment: "",
  ...over,
});

/** A case whose LAST step was edited. The first three are untouched, so the
 * diff has nothing to say about them - and they are exactly the steps you
 * need in order to judge whether the fourth still follows. */
const fourStepEdit = (): SyncChange => ({
  kind: "changed",
  key: "id:5001",
  title: "Copy from Previous - confirmation names the cycle",
  fields: [],
  steps: [
    {
      index: 3,
      kind: "changed",
      old: { action: "Click one of the listed rows", expected: "A dialog appears" },
      new: { action: "Click one of the listed cycles", expected: "An inline confirmation appears" },
    },
  ],
  full: tc("Copy from Previous - confirmation names the cycle", {
    update_id: 5001,
    preconditions: "A draft cycle exists",
    steps: [
      { action: "Locate the draft cycle in the list", expected: "The row is shown" },
      { action: 'Click "Edit"', expected: "The wizard opens at Step 1" },
      { action: 'In Configuration Level, click "Copy from Previous"', expected: "The modal opens" },
      { action: "Click one of the listed cycles", expected: "An inline confirmation appears" },
    ],
  }),
});

test("a case can be read start to finish, not just where it changed", () => {
  const { container } = render(
    <SyncReport changes={[fourStepEdit()]} fileName="cases.json" onDismiss={() => {}} />,
  );
  fireEvent.click(screen.getByRole("button", { name: /show details/i }));

  // The diff on its own mentions only the edited step. Asserted word by
  // word: InlineDiff emits a span per run, so a phrase that spans an
  // insertion is not contiguous in the text.
  expect(container.textContent).toContain("confirmation");
  // Nothing from the three untouched steps that lead up to it.
  expect(container.textContent).not.toContain("wizard");
  expect(container.textContent).not.toContain("Locate");

  // The case's own toggle brings back the three steps that led up to it.
  const toggle = screen.getByRole("button", { name: /Copy from Previous/ });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(toggle);
  expect(toggle).toHaveAttribute("aria-expanded", "true");

  const table = screen.getByRole("table");
  expect(within(table).getByText("Locate the draft cycle in the list")).toBeInTheDocument();
  expect(within(table).getByText('Click "Edit"')).toBeInTheDocument();
  expect(within(table).getByText(/The wizard opens at Step 1/)).toBeInTheDocument();
  // Numbered, so a step can be referred to by position.
  expect(within(table).getByText("4")).toBeInTheDocument();
  // Preconditions come with it - a step often only makes sense given them.
  expect(screen.getByText(/A draft cycle exists/)).toBeInTheDocument();

  // And it closes again.
  fireEvent.click(toggle);
  expect(screen.queryByRole("table")).not.toBeInTheDocument();
});

test("each case opens on its own", () => {
  const other: SyncChange = {
    kind: "added",
    key: "t:other",
    title: "Some other case",
    fields: [],
    steps: [],
    full: tc("Some other case", { steps: [{ action: "Unrelated action", expected: "x" }] }),
  };
  render(
    <SyncReport changes={[fourStepEdit(), other]} fileName="cases.json" onDismiss={() => {}} />,
  );
  fireEvent.click(screen.getByRole("button", { name: /show details/i }));
  fireEvent.click(screen.getByRole("button", { name: /Copy from Previous/ }));

  expect(screen.getAllByRole("table")).toHaveLength(1);
  expect(screen.queryByText("Unrelated action")).not.toBeInTheDocument();
});

/// A sync that fills an EMPTY queue is a load, not an edit: "+157 added"
/// read like 157 test cases had just been created. The banner says
/// "loaded" and drops the count chips; details stay available.
test("a pure load into an empty queue says loaded, not added", () => {
  const changes: SyncChange[] = ["A", "B", "C"].map((t) => ({
    kind: "added" as const,
    key: `t:${t}`,
    title: t,
    fields: [],
    steps: [],
    full: tc(t),
  }));
  render(
    <SyncReport changes={changes} fileName="FDP.json" intoEmptyQueue onDismiss={() => {}} />,
  );
  expect(screen.getByText("Loaded 3 cases from FDP.json into the queue")).toBeInTheDocument();
  expect(screen.queryByText(/added/)).not.toBeInTheDocument();

  // The same changes WITHOUT the flag (an edit to a live queue) keep the
  // original reporting.
  render(<SyncReport changes={changes} fileName="FDP.json" onDismiss={() => {}} />);
  expect(screen.getByText("Updated from FDP.json")).toBeInTheDocument();
  expect(screen.getByText("+3 added")).toBeInTheDocument();
});

/// Mixed changes into a briefly-empty queue are still an edit report - the
/// "loaded" wording is only for the all-added case.
test("a mixed sync keeps edit wording even into an empty queue", () => {
  const changes: SyncChange[] = [
    { kind: "added", key: "t:a", title: "A", fields: [], steps: [], full: tc("A") },
    { kind: "removed", key: "t:b", title: "B", fields: [], steps: [], full: tc("B") },
  ];
  render(
    <SyncReport changes={changes} fileName="x.json" intoEmptyQueue onDismiss={() => {}} />,
  );
  expect(screen.getByText("Updated from x.json")).toBeInTheDocument();
  expect(screen.getByText("+1 added")).toBeInTheDocument();
});

// One pile per watched file is the designed normal case, so two can be up
// at once. A fixed "Dismiss the change report" on both left a screen
// reader with two identical buttons and no way to tell which pile it was
// clearing.
test("each open report's dismiss button names its own file", () => {
  const dismissed: string[] = [];
  render(
    <>
      <SyncReport
        changes={[fourStepEdit()]}
        fileName="cases.json"
        onDismiss={() => dismissed.push("cases.json")}
      />
      <SyncReport
        changes={[fourStepEdit()]}
        fileName="checkout.json"
        onDismiss={() => dismissed.push("checkout.json")}
      />
    </>,
  );

  const first = screen.getByRole("button", { name: "Dismiss the change report for cases.json" });
  expect(
    screen.getByRole("button", { name: "Dismiss the change report for checkout.json" }),
  ).toBeInTheDocument();

  fireEvent.click(first);
  expect(dismissed).toEqual(["cases.json"]);
});
