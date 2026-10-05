// Authoring the actions for one case, as JSON, and choosing who it runs as.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ScriptEditor from "./ScriptEditor";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

const AREAS = [
  { area: "Cycle Setup", module: "PMS", clicks: [], arrived: "/pms/cycle/setup", recorded: "t" },
  { area: "Manage Cycle", module: "PMS", clicks: [], arrived: "/pms/cycle/manage", recorded: "t" },
];

function mountWith(
  script: unknown,
  accounts: unknown[],
  saved: unknown[],
  steps: { action: string; expected: string; shared?: number | null }[] = [],
) {
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_load_script") return script;
    if (cmd === "auto_run_list_accounts") return accounts;
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: AREAS };
    if (cmd === "auto_run_save_script") {
      saved.push((args as { script: unknown }).script);
      return null;
    }
    return null;
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ScriptEditor caseId={7} title="t" steps={steps} org="acme" project="Web" onClose={vi.fn()} />
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

test("the area select lists the recorded areas after the case's Module, and saves the one chosen", async () => {
  const saved: unknown[] = [];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP }, ACCOUNTS, saved);
  const pick = await screen.findByRole("combobox", { name: "Area" });
  expect(pick).toHaveTextContent("the case's Module");
  fireEvent.click(pick);
  await screen.findByRole("option", { name: "Manage Cycle" });
  const options = screen.getAllByRole("option").map((o) => o.textContent);
  expect(options).toEqual(["the case's Module", "Cycle Setup", "Manage Cycle"]);
  fireEvent.click(screen.getByRole("option", { name: "Manage Cycle" }));
  const save = screen.getByRole("button", { name: "Save script" });
  await waitFor(() => expect(save).not.toBeDisabled());
  fireEvent.click(save);
  await waitFor(() =>
    expect(saved).toEqual([{ case_id: 7, title: "t", steps: ONE_STEP, account: null, area: "Manage Cycle" }]),
  );
});

test("a script's own area shows, and choosing the case's Module again saves it with no area", async () => {
  const saved: unknown[] = [];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP, area: "Cycle Setup" }, ACCOUNTS, saved);
  const pick = await screen.findByRole("combobox", { name: "Area" });
  await waitFor(() => expect(pick).toHaveTextContent("Cycle Setup"));
  fireEvent.click(pick);
  fireEvent.click(await screen.findByRole("option", { name: "the case's Module" }));
  const save = screen.getByRole("button", { name: "Save script" });
  await waitFor(() => expect(save).not.toBeDisabled());
  fireEvent.click(save);
  await waitFor(() => expect(saved).toEqual([{ case_id: 7, title: "t", steps: ONE_STEP, account: null }]));
});

test("an area the project has not recorded is kept and marked, not silently dropped", async () => {
  const saved: unknown[] = [];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP, area: "Assessments" }, ACCOUNTS, saved);
  const pick = await screen.findByRole("combobox", { name: "Area" });
  await waitFor(() => expect(pick).toHaveTextContent("Assessments (not recorded)"));
  const save = screen.getByRole("button", { name: "Save script" });
  await waitFor(() => expect(save).not.toBeDisabled());
  fireEvent.click(save);
  await waitFor(() => expect(saved).toHaveLength(1));
  expect((saved[0] as { area: string }).area).toBe("Assessments");
});

test("Must not save is off for an old script, and ticking it writes no_save", async () => {
  const saved: unknown[] = [];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP }, ACCOUNTS, saved);
  const box = await screen.findByRole("checkbox", { name: "Must not save" });
  expect(box).toHaveAttribute("aria-checked", "false");
  fireEvent.click(box);
  await waitFor(() => expect(box).toHaveAttribute("aria-checked", "true"));
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(saved).toHaveLength(1));
  expect((saved[0] as { no_save?: boolean }).no_save).toBe(true);
});

test("a person saving from the editor can turn Must not save off", async () => {
  const saved: unknown[] = [];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP, no_save: true }, ACCOUNTS, saved);
  const box = await screen.findByRole("checkbox", { name: "Must not save" });
  await waitFor(() => expect(box).toHaveAttribute("aria-checked", "true"));
  fireEvent.click(box);
  await waitFor(() => expect(box).toHaveAttribute("aria-checked", "false"));
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0]).not.toHaveProperty("no_save");
});

test("a script marked Must not save keeps the flag when saved untouched", async () => {
  const saved: unknown[] = [];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP, no_save: true }, ACCOUNTS, saved);
  const box = await screen.findByRole("checkbox", { name: "Must not save" });
  await waitFor(() => expect(box).toHaveAttribute("aria-checked", "true"));
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(saved).toHaveLength(1));
  expect((saved[0] as { no_save?: boolean }).no_save).toBe(true);
});

test("a script's preconditions are kept when it is saved from the editor", async () => {
  const saved: unknown[] = [];
  const preconditions = [{ flow: "pms-performance-cycle", stage: "publish", value: 274, why: "it opens a published cycle" }];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP, no_save: true, preconditions }, ACCOUNTS, saved);
  // The flag showing is the sign the saved script has loaded.
  const box = await screen.findByRole("checkbox", { name: "Must not save" });
  await waitFor(() => expect(box).toHaveAttribute("aria-checked", "true"));
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(saved).toHaveLength(1));
  expect((saved[0] as { preconditions?: unknown }).preconditions).toEqual(preconditions);
});

test("a script with no preconditions is saved without the key", async () => {
  const saved: unknown[] = [];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP, no_save: true }, ACCOUNTS, saved);
  const box = await screen.findByRole("checkbox", { name: "Must not save" });
  await waitFor(() => expect(box).toHaveAttribute("aria-checked", "true"));
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0]).not.toHaveProperty("preconditions");
});

const TWO_PRECONDITIONS = [
  { flow: "pms-performance-cycle", stage: "publish", value: 274 },
  { flow: "pms-named-cycle", stage: "setup", value: "Q4 cycle" },
];

test("a script's preconditions are listed under its steps by flow, stage and value", async () => {
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP, preconditions: TWO_PRECONDITIONS }, ACCOUNTS, []);
  const list = await screen.findByRole("list", { name: "Preconditions" });
  const rows = within(list).getAllByRole("listitem");
  expect(rows.map((r) => r.textContent)).toEqual([
    "pms-performance-cycle / publish: 274Remove",
    "pms-named-cycle / setup: Q4 cycleRemove",
  ]);
  expect(screen.getByRole("button", { name: "Remove precondition 1" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Remove precondition 2" })).toBeInTheDocument();
});

test("Remove then Save sends the script without that precondition", async () => {
  const saved: unknown[] = [];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP, preconditions: TWO_PRECONDITIONS }, ACCOUNTS, saved);
  fireEvent.click(await screen.findByRole("button", { name: "Remove precondition 1" }));
  expect(screen.queryByRole("button", { name: "Remove precondition 1" })).not.toBeInTheDocument();
  expect(saved).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(saved).toHaveLength(1));
  expect((saved[0] as { preconditions?: unknown }).preconditions).toEqual([TWO_PRECONDITIONS[1]]);
});

test("removing every precondition saves the script without the key", async () => {
  const saved: unknown[] = [];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP, preconditions: [TWO_PRECONDITIONS[0]] }, ACCOUNTS, saved);
  fireEvent.click(await screen.findByRole("button", { name: "Remove precondition 1" }));
  expect(screen.queryByRole("list", { name: "Preconditions" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0]).not.toHaveProperty("preconditions");
});

// ---- Marks: the shared state a case changes or needs unchanged ----

test("a script's marks show as chips in the Changes and Needs unchanged rows", async () => {
  mountWith(
    {
      case_id: 7,
      title: "t",
      steps: ONE_STEP,
      changes: ["cycle published"],
      needs_unchanged: ["cycle published", "appraisal submitted for A001"],
    },
    ACCOUNTS,
    [],
  );
  const changes = await screen.findByRole("list", { name: "Changes" });
  expect(within(changes).getAllByRole("listitem").map((r) => r.textContent)).toEqual(["cycle published"]);
  const needs = screen.getByRole("list", { name: "Needs unchanged" });
  expect(within(needs).getAllByRole("listitem").map((r) => r.textContent)).toEqual([
    "cycle published",
    "appraisal submitted for A001",
  ]);
  expect(screen.getByRole("button", { name: "Remove change cycle published" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Remove needs unchanged cycle published" })).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "Remove needs unchanged appraisal submitted for A001" }),
  ).toBeInTheDocument();
});

test("a name is added with Enter or the Add button, and saved with the script", async () => {
  const saved: unknown[] = [];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP }, ACCOUNTS, saved);
  const change = await screen.findByRole("textbox", { name: "Add to Changes" });
  fireEvent.change(change, { target: { value: "  cycle published " } });
  fireEvent.keyDown(change, { key: "Enter" });
  expect(change).toHaveValue("");
  const need = screen.getByRole("textbox", { name: "Add to Needs unchanged" });
  fireEvent.change(need, { target: { value: "appraisal submitted for A001" } });
  fireEvent.click(screen.getByRole("button", { name: "Add needs unchanged" }));
  expect(need).toHaveValue("");
  expect(screen.getByRole("button", { name: "Remove change cycle published" })).toBeInTheDocument();
  const save = screen.getByRole("button", { name: "Save script" });
  await waitFor(() => expect(save).not.toBeDisabled());
  fireEvent.click(save);
  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0]).toEqual(
    expect.objectContaining({ changes: ["cycle published"], needs_unchanged: ["appraisal submitted for A001"] }),
  );
});

test("a name that differs only in case or spacing from one in the row is not added twice", async () => {
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP, changes: ["Cycle Published"] }, ACCOUNTS, []);
  const change = await screen.findByRole("textbox", { name: "Add to Changes" });
  await screen.findByRole("button", { name: "Remove change Cycle Published" });
  fireEvent.change(change, { target: { value: " cycle   published " } });
  fireEvent.keyDown(change, { key: "Enter" });
  const list = screen.getByRole("list", { name: "Changes" });
  expect(within(list).getAllByRole("listitem").map((r) => r.textContent)).toEqual(["Cycle Published"]);
  // A blank box adds nothing.
  fireEvent.change(change, { target: { value: "   " } });
  fireEvent.click(screen.getByRole("button", { name: "Add change" }));
  expect(within(list).getAllByRole("listitem")).toHaveLength(1);
});

test("Remove then Save sends the script without that name, and an emptied row without the key", async () => {
  const saved: unknown[] = [];
  mountWith(
    { case_id: 7, title: "t", steps: ONE_STEP, changes: ["cycle published"], needs_unchanged: ["a", "b"] },
    ACCOUNTS,
    saved,
  );
  fireEvent.click(await screen.findByRole("button", { name: "Remove change cycle published" }));
  fireEvent.click(screen.getByRole("button", { name: "Remove needs unchanged a" }));
  expect(screen.queryByRole("button", { name: "Remove change cycle published" })).not.toBeInTheDocument();
  expect(saved).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0]).not.toHaveProperty("changes");
  expect((saved[0] as { needs_unchanged?: unknown }).needs_unchanged).toEqual(["b"]);
});

test("a script with no marks is saved without either key", async () => {
  const saved: unknown[] = [];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP, no_save: true }, ACCOUNTS, saved);
  const box = await screen.findByRole("checkbox", { name: "Must not save" });
  await waitFor(() => expect(box).toHaveAttribute("aria-checked", "true"));
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0]).not.toHaveProperty("changes");
  expect(saved[0]).not.toHaveProperty("needs_unchanged");
});

test("a script's marks are kept when it is saved untouched", async () => {
  const saved: unknown[] = [];
  mountWith(
    { case_id: 7, title: "t", steps: ONE_STEP, changes: ["cycle published"], needs_unchanged: ["cycle published"] },
    ACCOUNTS,
    saved,
  );
  await screen.findByRole("button", { name: "Remove change cycle published" });
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0]).toEqual(
    expect.objectContaining({ changes: ["cycle published"], needs_unchanged: ["cycle published"] }),
  );
});
