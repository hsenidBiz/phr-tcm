// Reordering steps by drag handle - and by keyboard, which the old
// up/down arrows provided and the handle must not lose.

import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { expect, test } from "vitest";
import type { Step } from "../bindings";
import StepsEditor from "./StepsEditor";

function Harness({ initial }: { initial: Step[] }) {
  const [steps, setSteps] = useState(initial);
  return <StepsEditor steps={steps} onChange={setSteps} />;
}

const three: Step[] = [
  { action: "Open the login page", expected: "Form shows" },
  { action: "Enter credentials", expected: "Fields accept input" },
  { action: "Press Sign in", expected: "Dashboard opens" },
];

/** The action column, top to bottom, as rendered. */
function actions(): string[] {
  return screen
    .getAllByLabelText(/Step \d+ action/)
    .map((el) => (el as HTMLInputElement).value);
}

test("dragging a step drops it where it was released, not one place over", () => {
  render(<Harness initial={three} />);

  // Step 1 dragged past step 2 onto step 3 - a single gesture that would
  // have been two arrow clicks.
  const handle = screen.getByRole("button", { name: "Reorder step 1" });
  const targetRow = screen.getByLabelText("Step 3 action").closest("div")!;
  fireEvent.dragStart(handle);
  fireEvent.dragOver(targetRow);
  fireEvent.drop(targetRow);

  expect(actions()).toEqual(["Enter credentials", "Press Sign in", "Open the login page"]);
});

test("dragging up works the same as dragging down", () => {
  render(<Harness initial={three} />);
  const handle = screen.getByRole("button", { name: "Reorder step 3" });
  const targetRow = screen.getByLabelText("Step 1 action").closest("div")!;
  fireEvent.dragStart(handle);
  fireEvent.dragOver(targetRow);
  fireEvent.drop(targetRow);

  expect(actions()).toEqual(["Press Sign in", "Open the login page", "Enter credentials"]);
});

test("dropping a step on itself changes nothing", () => {
  render(<Harness initial={three} />);
  const handle = screen.getByRole("button", { name: "Reorder step 2" });
  const ownRow = screen.getByLabelText("Step 2 action").closest("div")!;
  fireEvent.dragStart(handle);
  fireEvent.dragOver(ownRow);
  fireEvent.drop(ownRow);

  expect(actions()).toEqual(three.map((s) => s.action));
});

test("the handle still moves one place per arrow key, both directions", () => {
  render(<Harness initial={three} />);

  fireEvent.keyDown(screen.getByRole("button", { name: "Reorder step 1" }), {
    key: "ArrowDown",
  });
  expect(actions()).toEqual(["Enter credentials", "Open the login page", "Press Sign in"]);

  // The moved step is now step 2; push it back up.
  fireEvent.keyDown(screen.getByRole("button", { name: "Reorder step 2" }), {
    key: "ArrowUp",
  });
  expect(actions()).toEqual(three.map((s) => s.action));
});

test("arrow keys at the edges are a no-op, not a crash or a wrap", () => {
  render(<Harness initial={three} />);
  fireEvent.keyDown(screen.getByRole("button", { name: "Reorder step 1" }), { key: "ArrowUp" });
  fireEvent.keyDown(screen.getByRole("button", { name: "Reorder step 3" }), { key: "ArrowDown" });
  expect(actions()).toEqual(three.map((s) => s.action));
});
