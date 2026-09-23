// The tester's own accounts: entering them, masking passwords, and what
// happens when the app refuses a save.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import AccountsDialog from "./AccountsDialog";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => { clearMocks(); vi.clearAllMocks(); });

function mount(existing: unknown[], onSave: (accounts: unknown[]) => unknown = () => []) {
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_list_accounts") return existing;
    if (cmd === "auto_run_save_accounts") return onSave((args as { accounts: unknown[] }).accounts);
    return null;
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <AccountsDialog onClose={onClose} />
    </QueryClientProvider>,
  );
  return onClose;
}

// Row fields are named by POSITION ("Key for account 1"), not by the
// account's own key: a name that changed while the key itself was being
// typed would make the field un-findable mid-edit.
test("existing accounts load with their passwords masked until asked", async () => {
  mount([{ key: "admin", label: "Administrator", username: "kim", password: "p1" }]);
  expect(await screen.findByDisplayValue("kim")).toBeInTheDocument();
  const pw = screen.getByLabelText("Password for account 1") as HTMLInputElement;
  expect(pw.type).toBe("password");
  fireEvent.click(screen.getByRole("checkbox", { name: "Show passwords" }));
  expect((screen.getByLabelText("Password for account 1") as HTMLInputElement).type).toBe("text");
});

test("adding an account and saving sends the whole list", async () => {
  const saved: unknown[][] = [];
  const onClose = mount([], (a) => { saved.push(a); return []; });
  await screen.findByText("No accounts yet.");
  fireEvent.click(screen.getByRole("button", { name: "Add account" }));
  fireEvent.change(screen.getByLabelText("Key for account 1"), { target: { value: "hr.admin" } });
  fireEvent.change(screen.getByLabelText("Name for account 1"), { target: { value: "HR Admin" } });
  fireEvent.change(screen.getByLabelText("Username for account 1"), { target: { value: "kim" } });
  fireEvent.change(screen.getByLabelText("Password for account 1"), { target: { value: "p1" } });
  fireEvent.click(screen.getByRole("button", { name: "Save accounts" }));
  await waitFor(() => expect(saved).toEqual([[{ key: "hr.admin", label: "HR Admin", username: "kim", password: "p1" }]]));
  expect(onClose).toHaveBeenCalled();
});

test("what the app refuses is shown and the dialog stays open", async () => {
  const onClose = mount([{ key: "admin", label: "A", username: "kim", password: "p" }], () => {
    throw new Error('the account "admin" has no username');
  });
  await screen.findByDisplayValue("kim");
  fireEvent.click(screen.getByRole("button", { name: "Save accounts" }));
  expect(await screen.findByText(/has no username/)).toBeInTheDocument();
  expect(onClose).not.toHaveBeenCalled();
});

test("removing an account takes its row away", async () => {
  mount([{ key: "admin", label: "A", username: "kim", password: "p" }]);
  await screen.findByDisplayValue("kim");
  fireEvent.click(screen.getByRole("button", { name: "Remove admin" }));
  expect(screen.queryByDisplayValue("kim")).not.toBeInTheDocument();
});
