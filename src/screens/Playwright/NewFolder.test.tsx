// New folder: the form that makes a feature folder in the clone, what it
// sends, and what it shows when the folder is made or refused.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import NewFolder from "./NewFolder";

afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

const PATHS = ["sl/admin/performance/rating-methods", "sl/admin/performance/goal-setting", "sl/self/leave/apply-leave"];

function mount(reply: (path: string) => unknown) {
  const sent: Record<string, unknown>[] = [];
  mockIPC((cmd, args) => {
    if (cmd !== "pw_repo_create_folder") return null;
    sent.push(args as Record<string, unknown>);
    return reply((args as { path: string }).path);
  });
  const onCreated = vi.fn();
  render(<NewFolder clone="C:/repo" paths={PATHS} onCreated={onCreated} />);
  return { sent, onCreated };
}

test("makes the folder typed, says so, and tells the caller", async () => {
  const { sent, onCreated } = mount((path) => ({ path: path.trim(), files: [] }));
  fireEvent.click(screen.getByRole("button", { name: "New folder" }));
  const input = screen.getByLabelText("New folder path");
  // The parents of the clone's features are offered, once each.
  const offered = Array.from(document.querySelectorAll(`datalist#${CSS.escape(input.getAttribute("list") ?? "")} option`)).map(
    (o) => (o as HTMLOptionElement).value,
  );
  expect(offered).toEqual(["sl/admin/performance/", "sl/self/leave/"]);
  expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
  fireEvent.change(input, { target: { value: "sl/admin/performance/DW-competencies" } });
  fireEvent.click(screen.getByRole("button", { name: "Create" }));
  await waitFor(() => expect(onCreated).toHaveBeenCalledWith("sl/admin/performance/DW-competencies"));
  expect(sent).toEqual([{ clone: "C:/repo", path: "sl/admin/performance/DW-competencies" }]);
  expect(screen.getByText("Created suites/sl/admin/performance/DW-competencies")).toBeInTheDocument();
  expect(screen.queryByLabelText("New folder path")).not.toBeInTheDocument();
});

test("a refusal is shown and the typed path is kept", async () => {
  const { onCreated } = mount(() => {
    throw '"sl/admin/performance/rating-methods" is already a feature folder.';
  });
  fireEvent.click(screen.getByRole("button", { name: "New folder" }));
  const input = screen.getByLabelText("New folder path");
  fireEvent.change(input, { target: { value: "sl/admin/performance/rating-methods" } });
  fireEvent.keyDown(input, { key: "Enter" });
  expect(await screen.findByText(/is already a feature folder/)).toBeInTheDocument();
  expect(input).toHaveValue("sl/admin/performance/rating-methods");
  expect(onCreated).not.toHaveBeenCalled();
  // Escape closes only the form.
  fireEvent.keyDown(input, { key: "Escape" });
  expect(screen.queryByLabelText("New folder path")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "New folder" })).toBeInTheDocument();
});
