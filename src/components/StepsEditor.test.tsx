// Reordering steps by drag handle - and by keyboard, which the old
// up/down arrows provided and the handle must not lose.

import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
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

// Step text is Markdown: typed on several lines, kept exactly, and shown
// formatted on Preview.
test("a step keeps the Markdown typed, line breaks included, and Preview shows it formatted", () => {
  render(<SharedHarness initial={[{ action: "", expected: "" }]} />);
  const action = screen.getByLabelText("Step 1 action");
  expect(action.tagName).toBe("TEXTAREA");
  const typed = "Run the query:\n\n```sql\nSELECT COUNT(*)\nFROM leave\n```";
  fireEvent.change(action, { target: { value: typed } });
  fireEvent.change(screen.getByLabelText("Step 1 expected"), { target: { value: "The count is **3**" } });
  expect(currentSteps()[0]).toEqual({ action: typed, expected: "The count is **3**" });

  const toggle = screen.getByRole("button", { name: "Preview steps" });
  fireEvent.click(toggle);
  expect(toggle.getAttribute("aria-pressed")).toBe("true");
  expect(screen.queryByLabelText("Step 1 action")).toBeNull();
  expect(screen.getByText("3").tagName).toBe("STRONG");
  expect(document.querySelector(".md-preview pre code")?.textContent).toContain("SELECT COUNT(*)");
  // Nothing typed changed by looking at it.
  expect(currentSteps()[0].action).toBe(typed);

  fireEvent.click(toggle);
  expect((screen.getByLabelText("Step 1 action") as HTMLTextAreaElement).value).toBe(typed);
});


// --- step attachments ---------------------------------------------------

const { openDialog, toastMock } = vi.hoisted(() => ({
  openDialog: vi.fn(),
  toastMock: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openDialog }));
vi.mock("../lib/toast", () => ({ toast: toastMock }));

function latest() {
  const out: { steps: Step[] } = { steps: [] };
  const Probe = ({ initial }: { initial: Step[] }) => {
    const [steps, setSteps] = useState(initial);
    out.steps = steps;
    return <StepsEditor steps={steps} onChange={setSteps} />;
  };
  return { out, Probe };
}

test("Attach adds a checked file to that step as a name and size, not a path in the UI", async () => {
  openDialog.mockResolvedValue(["C:\\shots\\a.png"]);
  mockIPC((cmd) => {
    if (cmd === "describe_step_files")
      return [{ path: "C:\\shots\\a.png", name: "a.png", size: 2048, refused: null }];
  });
  const { out, Probe } = latest();
  render(<Probe initial={three} />);
  fireEvent.click(screen.getByRole("button", { name: "Attach a file to step 2" }));
  expect(await screen.findByText("a.png")).toBeTruthy();
  expect(out.steps[1].attachments).toEqual([{ path: "C:\\shots\\a.png", name: "a.png", size: 2048 }]);
  expect(out.steps[0].attachments ?? []).toEqual([]);
  expect(document.body.textContent).not.toContain("shots");
});

test("a refused file shows its sentence and adds nothing", async () => {
  openDialog.mockResolvedValue(["C:\\big.bin"]);
  mockIPC((cmd) => {
    if (cmd === "describe_step_files")
      return [{ path: "C:\\big.bin", name: "big.bin", size: null, refused: "big.bin is over 25 MB." }];
  });
  const { out, Probe } = latest();
  render(<Probe initial={three} />);
  fireEvent.click(screen.getByRole("button", { name: "Attach a file to step 1" }));
  await vi.waitFor(() => expect(toastMock.error).toHaveBeenCalledWith("big.bin is over 25 MB."));
  expect(out.steps[0].attachments ?? []).toEqual([]);
});

test("cancelling the picker changes nothing", async () => {
  openDialog.mockResolvedValue(null);
  mockIPC(() => []);
  const { out, Probe } = latest();
  render(<Probe initial={three} />);
  fireEvent.click(screen.getByRole("button", { name: "Attach a file to step 1" }));
  await vi.waitFor(() => expect(openDialog).toHaveBeenCalled());
  expect(out.steps[0].attachments ?? []).toEqual([]);
});

test("each attachment is listed with its size and removed by its own button", () => {
  mockIPC(() => []);
  const { out, Probe } = latest();
  const steps: Step[] = [
    three[0],
    { ...three[1], attachments: [{ id: "g1", name: "a.png", size: 2048 }] },
  ];
  render(<Probe initial={steps} />);
  expect(screen.getByText("a.png")).toBeTruthy();
  expect(screen.getByText("2 KB")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Remove a.png from step 2" }));
  expect(out.steps[1].attachments).toEqual([]);
  expect(screen.queryByText("a.png")).toBeNull();
});

test("a shared step row has no Attach button", () => {
  mockIPC(() => []);
  render(<Harness initial={[three[0], { action: "", expected: "", shared: 812 }]} />);
  expect(screen.getByRole("button", { name: "Attach a file to step 1" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Attach a file to step 2" })).toBeNull();
});

test("a picker or check that fails says so instead of vanishing", async () => {
  openDialog.mockRejectedValue(new Error("boom"));
  mockIPC(() => []);
  const { out, Probe } = latest();
  render(<Probe initial={three} />);
  fireEvent.click(screen.getByRole("button", { name: "Attach a file to step 1" }));
  await vi.waitFor(() => expect(toastMock.error).toHaveBeenCalled());
  expect(out.steps[0].attachments ?? []).toEqual([]);
});
