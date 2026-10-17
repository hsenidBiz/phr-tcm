// New folder: the form that makes a feature folder in the clone, what it
// sends, and what it shows when the folder is made or refused.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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

/** Opens the path field and types into its search box, as a person does. */
function typePath(text: string) {
  fireEvent.click(screen.getByRole("combobox", { name: "New folder path" }));
  const box = screen.getByPlaceholderText("Search…");
  fireEvent.change(box, { target: { value: text } });
  return box;
}

test("makes the folder typed, says so, and tells the caller", async () => {
  const { sent, onCreated } = mount((path) => ({ path: path.trim(), files: [] }));
  fireEvent.click(screen.getByRole("button", { name: "New folder" }));
  const field = screen.getByRole("combobox", { name: "New folder path" });
  // The parents of the clone's features are offered, once each - in the app's own list.
  expect(document.querySelector("datalist")).toBeNull();
  fireEvent.click(field);
  const offered = within(screen.getByRole("listbox")).getAllByRole("option").map((o) => o.textContent);
  expect(offered).toEqual(["sl/admin/performance/", "sl/self/leave/"]);
  fireEvent.click(field);
  expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
  // A path that is not offered can still be typed.
  const box = typePath("sl/admin/performance/DW-competencies");
  fireEvent.keyDown(box, { key: "Enter" });
  expect(field).toHaveTextContent("sl/admin/performance/DW-competencies");
  fireEvent.click(screen.getByRole("button", { name: "Create" }));
  await waitFor(() => expect(onCreated).toHaveBeenCalledWith("sl/admin/performance/DW-competencies"));
  expect(sent).toEqual([{ clone: "C:/repo", path: "sl/admin/performance/DW-competencies" }]);
  expect(screen.getByText("Created suites/sl/admin/performance/DW-competencies")).toBeInTheDocument();
  expect(screen.queryByRole("combobox", { name: "New folder path" })).not.toBeInTheDocument();
});

test("Enter on the chosen path creates it", async () => {
  const { sent, onCreated } = mount((path) => ({ path, files: [] }));
  fireEvent.click(screen.getByRole("button", { name: "New folder" }));
  // The field has the keyboard when the form opens; Enter with no path chosen makes nothing.
  const field = screen.getByRole("combobox", { name: "New folder path" });
  expect(field).toHaveFocus();
  fireEvent.keyDown(field, { key: "Enter" });
  expect(sent).toEqual([]);
  fireEvent.keyDown(typePath("sl/self/leave/new-leave"), { key: "Enter" });
  // The first Enter chose the path; nothing is made until the second.
  expect(sent).toEqual([]);
  expect(field).toHaveFocus();
  fireEvent.keyDown(field, { key: "Enter" });
  await waitFor(() => expect(onCreated).toHaveBeenCalledWith("sl/self/leave/new-leave"));
  expect(sent).toEqual([{ clone: "C:/repo", path: "sl/self/leave/new-leave" }]);
});

test("a refusal is shown and the typed path is kept", async () => {
  const { onCreated } = mount(() => {
    throw '"sl/admin/performance/rating-methods" is already a feature folder.';
  });
  fireEvent.click(screen.getByRole("button", { name: "New folder" }));
  const field = screen.getByRole("combobox", { name: "New folder path" });
  fireEvent.keyDown(typePath("sl/admin/performance/rating-methods/"), { key: "Enter" });
  fireEvent.keyDown(field, { key: "Enter" });
  expect(await screen.findByText(/is already a feature folder/)).toBeInTheDocument();
  expect(field).toHaveTextContent("sl/admin/performance/rating-methods/");
  expect(onCreated).not.toHaveBeenCalled();
  // Escape closes only the form.
  fireEvent.keyDown(field, { key: "Escape" });
  expect(screen.queryByRole("combobox", { name: "New folder path" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "New folder" })).toBeInTheDocument();
});

test("Escape in the open list closes the list first, then the form, and never a dialog behind", () => {
  const behind = vi.fn();
  mount(() => null);
  window.addEventListener("keydown", behind);
  fireEvent.click(screen.getByRole("button", { name: "New folder" }));
  const field = screen.getByRole("combobox", { name: "New folder path" });
  fireEvent.click(field);
  fireEvent.keyDown(screen.getByPlaceholderText("Search…"), { key: "Escape" });
  expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  expect(field).toBeInTheDocument();
  fireEvent.keyDown(field, { key: "Escape" });
  expect(screen.queryByRole("combobox", { name: "New folder path" })).not.toBeInTheDocument();
  expect(behind).not.toHaveBeenCalled();
  window.removeEventListener("keydown", behind);
});
