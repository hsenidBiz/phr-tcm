// Authoring the actions for one case, as JSON, and choosing who it runs as.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ScriptEditor from "./ScriptEditor";

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

function mountWith(script: unknown, accounts: unknown[], saved: unknown[]) {
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_load_script") return script;
    if (cmd === "auto_run_list_accounts") return accounts;
    if (cmd === "auto_run_save_script") {
      saved.push((args as { script: unknown }).script);
      return null;
    }
    return null;
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ScriptEditor caseId={7} title="t" steps={[]} onClose={vi.fn()} />
    </QueryClientProvider>,
  );
}

const ONE_STEP = [{ step_number: 1, actions: [{ kind: "check_text", value: "ok" }] }];
const ACCOUNTS = [{ key: "hr.admin", label: "HR Admin", username: "kim", password: "p" }];

test("the account a script runs as is chosen from the tester's own list", async () => {
  const saved: unknown[] = [];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP }, ACCOUNTS, saved);
  const pick = await screen.findByRole("combobox", { name: "Runs as" });
  expect(pick).toHaveTextContent("No sign-in");
  fireEvent.click(pick);
  fireEvent.click(await screen.findByRole("option", { name: "HR Admin (hr.admin)" }));
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(saved).toEqual([{ case_id: 7, title: "t", steps: ONE_STEP, account: "hr.admin" }]));
});

test("an account this machine does not have is kept, not silently dropped", async () => {
  const saved: unknown[] = [];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP, account: "payroll.officer" }, ACCOUNTS, saved);
  const pick = await screen.findByRole("combobox", { name: "Runs as" });
  await waitFor(() => expect(pick).toHaveTextContent("payroll.officer (not on this machine)"));
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(saved).toHaveLength(1));
  expect((saved[0] as { account: string }).account).toBe("payroll.officer");
});

test("choosing No sign-in writes a script with no account", async () => {
  const saved: unknown[] = [];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP, account: "hr.admin" }, ACCOUNTS, saved);
  const pick = await screen.findByRole("combobox", { name: "Runs as" });
  await waitFor(() => expect(pick).toHaveTextContent("HR Admin (hr.admin)"));
  fireEvent.click(pick);
  fireEvent.click(await screen.findByRole("option", { name: "No sign-in" }));
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(saved).toEqual([{ case_id: 7, title: "t", steps: ONE_STEP, account: null }]));
});
