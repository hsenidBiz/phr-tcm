// Reordering steps by drag handle - and by keyboard, which the old
// up/down arrows provided and the handle must not lose.

import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test } from "vitest";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
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

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function SharedHarness({ initial, org }: { initial: Step[]; org?: string }) {
  const [steps, setSteps] = useState(initial);
  const [qc] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  return (
    <QueryClientProvider client={qc}>
      <StepsEditor steps={steps} onChange={setSteps} org={org} />
      <output data-testid="steps">{JSON.stringify(steps)}</output>
    </QueryClientProvider>
  );
}

const currentSteps = () => JSON.parse(screen.getByTestId("steps").textContent ?? "[]") as Step[];

const withShared: Step[] = [
  { action: "Open the login page", expected: "Form shows" },
  { action: "", expected: "", shared: 812 },
  { action: "Press Sign in", expected: "Dashboard opens" },
];

const workItem = (id: number, title: string) => ({
  id, title, tags: "", automation_status: "Not Automated", steps: [], step_ids: [],
  steps_xml: "", module_value: "", preconditions: "",
});

test("a shared step is a locked row naming its reference and title, fetched once per id", async () => {
  const asked: number[][] = [];
  mockIPC((cmd, args) => {
    if (cmd === "test_cases_by_ids") {
      const ids = (args as { ids: number[] }).ids;
      asked.push(ids);
      return ids.map((id) => workItem(id, "Sign in as an admin"));
    }
  });
  render(<SharedHarness org="acme" initial={[...withShared, { action: "", expected: "", shared: 812 }]} />);

  expect(screen.getAllByText("Shared steps #812")).toHaveLength(2);
  expect(await screen.findAllByText(/Sign in as an admin/)).toHaveLength(2);
  // Nothing to type into: the steps are edited in that work item.
  expect(screen.queryByLabelText("Step 2 action")).toBeNull();
  expect(screen.queryByLabelText("Step 2 expected")).toBeNull();
  expect(asked).toEqual([[812]]);
});

test("a shared step moves and is removed like any other, keeping its reference", () => {
  mockIPC(() => []);
  render(<SharedHarness org="acme" initial={withShared} />);

  fireEvent.keyDown(screen.getByRole("button", { name: "Reorder step 2" }), { key: "ArrowUp" });
  expect(currentSteps().map((s) => s.shared ?? s.action)).toEqual([812, "Open the login page", "Press Sign in"]);

  fireEvent.click(screen.getAllByTitle("Remove step")[0]);
  expect(currentSteps().some((s) => s.shared === 812)).toBe(false);
  expect(currentSteps()).toHaveLength(2);
});

test("without an org the reference still shows, and nothing is fetched", () => {
  let calls = 0;
  mockIPC(() => {
    calls++;
    return [];
  });
  render(<SharedHarness initial={withShared} />);
  expect(screen.getByText("Shared steps #812")).toBeInTheDocument();
  expect(calls).toBe(0);
});
