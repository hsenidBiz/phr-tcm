// Authoring the actions for one case, as JSON, and choosing who it runs as.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ScriptEditor from "./ScriptEditor";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

function mountWith(
  script: unknown,
  accounts: unknown[],
  saved: unknown[],
  steps: { action: string; expected: string; shared?: number | null }[] = [],
) {
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
      <ScriptEditor caseId={7} title="t" steps={steps} onClose={vi.fn()} />
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

test("a repaired script shows how many times and, when known, why", async () => {
  mountWith(
    { case_id: 7, title: "t", steps: ONE_STEP, repairs: 2, last_repair: "the locator moved after a redesign" },
    ACCOUNTS,
    [],
  );
  await screen.findByText(
    "Repaired 2 of 3 times by an assistant since you last saved. Last reason: the locator moved after a redesign",
  );
});

test("a script with no repairs shows no warning line", async () => {
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP }, ACCOUNTS, []);
  await screen.findByRole("combobox", { name: "Runs as" });
  expect(screen.queryByText(/Repaired/)).toBeNull();
});

test("the Checks line shows checked, explained and NOT CHECKED", async () => {
  const script = {
    case_id: 7,
    title: "t",
    steps: [
      {
        step_number: 1,
        actions: [{ kind: "expect_visible", selector: { role: "heading", name: "Dashboard" } }],
      },
      {
        step_number: 2,
        actions: [{ kind: "click", selector: "#open" }],
        unchecked: "the PDF cannot be read from the accessibility tree",
      },
    ],
  };
  const caseSteps = [
    { action: "Open the dashboard", expected: "The dashboard is shown" },
    { action: "Open the PDF", expected: "The PDF opens" },
    { action: "Do something unwritten", expected: "" },
    { action: "Do the third thing", expected: "Something happens" },
  ];
  mountWith(script, ACCOUNTS, [], caseSteps);
  await screen.findByText("Step 1: checked");
  expect(
    screen.getByText("Step 2: not checked - the PDF cannot be read from the accessibility tree"),
  ).toBeTruthy();
  expect(screen.getByText("Step 4: NOT CHECKED")).toBeTruthy();
  // Step 3 has no expected result, so it never becomes part of the floor.
  expect(screen.queryByText(/Step 3:/)).toBeNull();
});

test("a Shared Steps entry among the case's steps shows its reference, not a blank item", async () => {
  const caseSteps = [
    { action: "Open the dashboard", expected: "The dashboard is shown" },
    { action: "", expected: "", shared: 812 },
  ];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP }, ACCOUNTS, [], caseSteps);
  expect(await screen.findByText("Shared steps #812")).toBeInTheDocument();
});

test("saving names the organization and project, so the project's address rule applies", async () => {
  const sent: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_load_script") return { case_id: 7, title: "t", steps: ONE_STEP };
    if (cmd === "auto_run_list_accounts") return ACCOUNTS;
    if (cmd === "auto_run_save_script") {
      sent.push(args);
      return null;
    }
    return null;
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ScriptEditor caseId={7} title="t" steps={[]} org="acme" project="Web" onClose={vi.fn()} />
    </QueryClientProvider>,
  );
  await screen.findByRole("combobox", { name: "Runs as" });
  // Save stays disabled until the existing script has loaded (it would
  // otherwise write an empty `steps: []` over what is on disk) - wait for
  // that before clicking, rather than racing the query.
  const saveButton = screen.getByRole("button", { name: "Save script" });
  await waitFor(() => expect(saveButton).not.toBeDisabled());
  fireEvent.click(saveButton);
  await waitFor(() => expect(sent).toHaveLength(1));
  expect(sent[0]).toEqual(expect.objectContaining({ organization: "acme", project: "Web" }));
});
