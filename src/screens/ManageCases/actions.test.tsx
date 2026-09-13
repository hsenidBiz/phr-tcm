import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "sonner";
import { clearMocks } from "@tauri-apps/api/mocks";
import { mountWithSuite, pickSuite } from "./testSupport";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

// Hoisted with the mock: the factory runs when the screen imports the
// plugin, before a plain const at this position would exist.
const { openDialog } = vi.hoisted(() => ({ openDialog: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openDialog }));

afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

test("Apply tester order from file re-orders the list from the file's tester_order", async () => {
  openDialog.mockResolvedValue("C:\\drafts\\auth.json");
  mountWithSuite((cmd, args) => {
    if (cmd === "parse_import_file" && (args as { path: string }).path === "C:\\drafts\\auth.json")
      return {
        cases: [
          { title: "c", steps: [], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "", update_id: 203, tester_order: 1 },
          { title: "a", steps: [], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "", update_id: 201, tester_order: 2 },
        ],
        warnings: [],
      };
  });
  const l = await pickSuite(91);
  fireEvent.click(screen.getByRole("button", { name: "Apply tester order from file" }));
  await waitFor(() => {
    const rows = within(l).getAllByRole("listitem");
    expect(rows.map((r) => r.textContent)).toEqual([
      expect.stringContaining("#203"),
      expect.stringContaining("#201"),
      expect.stringContaining("#202"),
    ]);
  });
  expect(toast.info).toHaveBeenCalledWith("Placed 2 of 3 test cases from the file. Apply order to save.");
  expect(screen.getByRole("button", { name: "Apply order" })).toBeEnabled();
});

test("a file that names none of the suite's cases changes nothing and says so", async () => {
  openDialog.mockResolvedValue("C:\\drafts\\other.json");
  mountWithSuite((cmd) => {
    if (cmd === "parse_import_file")
      return { cases: [{ title: "x", steps: [], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "", update_id: null, tester_order: 1 }], warnings: [] };
  });
  const l = await pickSuite(91);
  fireEvent.click(screen.getByRole("button", { name: "Apply tester order from file" }));
  await waitFor(() => expect(toast.warning).toHaveBeenCalledWith("No test case in that file is in this suite. The file needs ids from an upload."));
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#201");
  expect(screen.getByRole("button", { name: "Apply order" })).toBeDisabled();
});

test("cancelling the file picker does nothing", async () => {
  openDialog.mockResolvedValue(null);
  const { calls } = mountWithSuite();
  await pickSuite(91);
  fireEvent.click(screen.getByRole("button", { name: "Apply tester order from file" }));
  await waitFor(() => expect(openDialog).toHaveBeenCalled());
  expect(calls.some((c) => c.cmd === "parse_import_file")).toBe(false);
});

test("Move to PBI needs a PBI suite and a selection, then hands the picked cases to the dialog", async () => {
  mountWithSuite();
  await pickSuite(91);
  // A static suite has no PBI to move FROM.
  expect(screen.getByRole("button", { name: "Move to PBI" })).toBeDisabled();
  expect(screen.getByText("Move to PBI works on a PBI suite.")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("combobox", { name: "Test suite" }));
  fireEvent.click(screen.getByRole("option", { name: "PBI 42: PBI 42 suite" }));
  await waitFor(() =>
    expect(screen.getByRole("combobox", { name: "Test suite" })).toHaveTextContent("PBI 42 suite"),
  );
  const l2 = await screen.findByRole("list", { name: "Test cases in order" });
  // A PBI suite, nothing picked yet.
  expect(screen.getByRole("button", { name: "Move to PBI" })).toBeDisabled();
  fireEvent.click(within(l2).getByRole("checkbox", { name: "Select #202" }));
  fireEvent.click(screen.getByRole("button", { name: "Move to PBI" }));

  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText("Move 1 test case to another PBI")).toBeInTheDocument();
  expect(within(dialog).getByText("#42")).toBeInTheDocument();
});
