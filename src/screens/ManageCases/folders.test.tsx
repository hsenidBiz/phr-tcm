import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "sonner";
import { clearMocks } from "@tauri-apps/api/mocks";
import { mountWithSuite, pickSuite } from "./testSupport";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
  localStorage.clear();
});

test("New folder offers the plan root and static suites as parents, creates, and refreshes the tree", async () => {
  const { calls } = mountWithSuite((cmd, args) => {
    if (cmd === "create_static_suite")
      return { id: 94, name: (args as { name: string }).name, suite_type: "staticTestSuite", requirement_id: null, parent_id: 90 };
  });
  await pickSuite(91);
  fireEvent.click(screen.getByRole("button", { name: "New folder" }));
  const dialog = await screen.findByRole("dialog");

  const parent = within(dialog).getByRole("combobox", { name: "Create inside" });
  expect(parent).toHaveTextContent("Plan root (Auth - Test Plan)");
  fireEvent.click(parent);
  const labels = within(dialog).getAllByRole("option").map((o) => o.textContent);
  // The root first, then static suites only: the PBI suite (93) is not offered.
  expect(labels).toEqual(["Plan root (Auth - Test Plan)", "Regression"]);
  fireEvent.click(within(dialog).getByRole("option", { name: "Plan root (Auth - Test Plan)" }));

  fireEvent.change(within(dialog).getByLabelText("Folder name"), { target: { value: "  Smoke  " } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Create folder" }));

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "create_static_suite");
    expect(call?.args).toEqual({ organization: "acme", project: "Web", planId: 9, parentSuiteId: 90, name: "Smoke" });
  });
  expect(toast.success).toHaveBeenCalledWith('Created folder "Smoke".');
  // The plan tree is re-read so the picker and the folder list show it.
  await waitFor(() => expect(calls.filter((c) => c.cmd === "list_plans_with_suites").length).toBeGreaterThan(1));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test("with cases selected, New folder creates and then copies them in", async () => {
  const { calls } = mountWithSuite((cmd) => {
    if (cmd === "create_static_suite")
      return { id: 94, name: "Smoke", suite_type: "staticTestSuite", requirement_id: null, parent_id: 91 };
    if (cmd === "add_cases_to_suite") return [201, 203];
  });
  const l = await pickSuite(91);
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #201" }));
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #203" }));
  fireEvent.click(screen.getByRole("button", { name: "New folder" }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.click(within(dialog).getByRole("combobox", { name: "Create inside" }));
  fireEvent.click(within(dialog).getByRole("option", { name: "Regression" }));
  fireEvent.change(within(dialog).getByLabelText("Folder name"), { target: { value: "Smoke" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Create folder and add 2 test cases" }));

  await waitFor(() => {
    const add = calls.find((c) => c.cmd === "add_cases_to_suite");
    expect(add?.args).toEqual({ organization: "acme", project: "Web", planId: 9, suiteId: 94, caseIds: [201, 203] });
  });
  expect(toast.success).toHaveBeenCalledWith('Created folder "Smoke" and added 2 test cases. They stay in Regression too.');
});

test("an empty name is refused before anything is sent", async () => {
  const { calls } = mountWithSuite();
  await pickSuite(91);
  fireEvent.click(screen.getByRole("button", { name: "New folder" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByRole("button", { name: "Create folder" })).toBeDisabled();
  fireEvent.change(within(dialog).getByLabelText("Folder name"), { target: { value: "   " } });
  expect(within(dialog).getByRole("button", { name: "Create folder" })).toBeDisabled();
  expect(calls.some((c) => c.cmd === "create_static_suite")).toBe(false);
});

test("Add to folder copies the selection into the chosen static suite", async () => {
  const { calls } = mountWithSuite((cmd) => {
    if (cmd === "add_cases_to_suite") return [202];
  });
  const l = await pickSuite(93);
  const target = screen.getByRole("combobox", { name: "Folder" });
  fireEvent.click(target);
  // Static suites of the plan, not the one on screen, not PBI suites.
  expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual([
    "Pick a folder",
    "Regression",
  ]);
  fireEvent.click(screen.getByRole("option", { name: "Pick a folder" }));
  expect(screen.getByRole("button", { name: "Add to folder" })).toBeDisabled();

  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #202" }));
  expect(screen.getByRole("button", { name: "Add to folder" })).toBeDisabled();
  fireEvent.click(target);
  fireEvent.click(screen.getByRole("option", { name: "Regression" }));
  fireEvent.click(screen.getByRole("button", { name: "Add to folder" }));

  await waitFor(() => {
    const add = calls.find((c) => c.cmd === "add_cases_to_suite");
    expect(add?.args).toEqual({ organization: "acme", project: "Web", planId: 9, suiteId: 91, caseIds: [202] });
  });
  expect(toast.success).toHaveBeenCalledWith("Added 1 test case to Regression. It stays in PBI 42 suite too.");
});

test("New folder: the suite is created but the copy fails - the folder still shows up", async () => {
  const { calls } = mountWithSuite((cmd) => {
    if (cmd === "create_static_suite")
      return { id: 94, name: "Smoke", suite_type: "staticTestSuite", requirement_id: null, parent_id: 91 };
    if (cmd === "add_cases_to_suite") throw new Error("boom");
  });
  const l = await pickSuite(91);
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #201" }));
  fireEvent.click(screen.getByRole("button", { name: "New folder" }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.click(within(dialog).getByRole("combobox", { name: "Create inside" }));
  fireEvent.click(within(dialog).getByRole("option", { name: "Regression" }));
  fireEvent.change(within(dialog).getByLabelText("Folder name"), { target: { value: "Smoke" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Create folder and add 1 test case" }));

  await waitFor(() => {
    expect(toast.warning).toHaveBeenCalledWith(
      'Created folder "Smoke", but the test cases could not be added: boom',
      { duration: 20000 },
    );
  });
  // The folder exists in ADO even though the copy failed: refresh the tree and close.
  await waitFor(() => expect(calls.filter((c) => c.cmd === "list_plans_with_suites").length).toBeGreaterThan(1));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});
