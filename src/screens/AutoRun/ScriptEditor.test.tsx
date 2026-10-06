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

test("settings the editor has no control for are kept when it saves", async () => {
  const saved: unknown[] = [];
  mountWith(
    {
      case_id: 7,
      title: "t",
      steps: ONE_STEP,
      page_errors: "flag",
      ignore_page_errors: ["ResizeObserver"],
      fail_on_unexpected_dialog: true,
    },
    ACCOUNTS,
    saved,
  );
  const save = await screen.findByRole("button", { name: "Save script" });
  await waitFor(() => expect(save).not.toBeDisabled());
  fireEvent.click(save);
  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0]).toMatchObject({
    page_errors: "flag",
    ignore_page_errors: ["ResizeObserver"],
    fail_on_unexpected_dialog: true,
  });
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

// ---- The Setup section -------------------------------------------------

const SETUP_SCRIPT = { case_id: 7, title: "t", steps: ONE_STEP, setup: { fixture: "Draft cycle" } };

const setupView = (approval: string, fingerprint = "fp-1") => ({
  fixture_name: "Draft cycle",
  account: "hr.admin",
  steps: ["Create cycle: name=Annual", "Open cycle: id={{cycle_id}}"],
  creates: ["cycle Annual"],
  approval,
  approved_at: approval === "approved" ? "2026-10-06 09:30:00" : null,
  fingerprint,
});

/** The editor with a script that has a setup, answering the three setup
 * commands from `handlers`. */
function mountSetup(
  views: unknown[],
  handlers: Record<string, (args: Record<string, unknown>) => unknown> = {},
  script: unknown = SETUP_SCRIPT,
) {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  const saved: unknown[] = [];
  let reads = 0;
  mockIPC((cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: a });
    if (handlers[cmd]) return handlers[cmd](a);
    if (cmd === "auto_run_load_script") return script;
    if (cmd === "auto_run_list_accounts") return ACCOUNTS;
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: AREAS };
    if (cmd === "auto_run_setup_view") return views[Math.min(reads++, views.length - 1)];
    if (cmd === "auto_run_save_script") {
      saved.push(a.script);
      return null;
    }
    return null;
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ScriptEditor caseId={7} title="t" steps={[]} org="acme" project="Web" onClose={vi.fn()} />
    </QueryClientProvider>,
  );
  return { calls, saved };
}

test("a script with no setup has no Setup section", async () => {
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP }, ACCOUNTS, []);
  await screen.findByRole("combobox", { name: "Runs as" });
  expect(screen.queryByRole("region", { name: "Setup" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Approve setup" })).not.toBeInTheDocument();
});

test("the Setup section shows the fixture, account, steps and what it creates, with Approve setup", async () => {
  mountSetup([setupView("none")]);
  const section = await screen.findByRole("region", { name: "Setup" });
  expect(within(section).getByText("Draft cycle")).toBeInTheDocument();
  expect(within(section).getByText("hr.admin")).toBeInTheDocument();
  expect(within(section).getByText("Create cycle: name=Annual")).toBeInTheDocument();
  expect(within(section).getByText("Open cycle: id={{cycle_id}}")).toBeInTheDocument();
  expect(section).toHaveTextContent("Creates cycle Annual");
  expect(within(section).getByRole("button", { name: "Approve setup" })).toBeInTheDocument();
  expect(within(section).queryByText("Changed since you approved it")).not.toBeInTheDocument();
});

test("Approve setup passes the fingerprint that was shown, then shows Approved with Withdraw approval", async () => {
  const { calls } = mountSetup([setupView("none", "fp-shown")], {
    auto_run_approve_setup: () => setupView("approved", "fp-shown"),
    auto_run_withdraw_setup: () => setupView("none", "fp-shown"),
  });
  fireEvent.click(await screen.findByRole("button", { name: "Approve setup" }));
  expect(await screen.findByText(/^Approved \d/)).toBeInTheDocument();
  expect(calls.find((c) => c.cmd === "auto_run_approve_setup")!.args).toEqual({
    organization: "acme",
    project: "Web",
    caseId: 7,
    expectedFingerprint: "fp-shown",
  });
  expect(screen.queryByRole("button", { name: "Approve setup" })).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Withdraw approval" }));
  expect(await screen.findByRole("button", { name: "Approve setup" })).toBeInTheDocument();
  expect(calls.some((c) => c.cmd === "auto_run_withdraw_setup")).toBe(true);
});

test("a setup that changed since it was approved says so and offers Approve setup again", async () => {
  mountSetup([setupView("changed")]);
  expect(await screen.findByText("Changed since you approved it")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Approve setup" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Withdraw approval" })).not.toBeInTheDocument();
});

test("the changed-while-looking refusal is shown and the view is read again", async () => {
  const REFUSAL = "the setup changed while you were looking at it - review it again before approving";
  const { calls } = mountSetup([setupView("none", "old"), setupView("none", "new")], {
    auto_run_approve_setup: () => {
      throw REFUSAL;
    },
  });
  fireEvent.click(await screen.findByRole("button", { name: "Approve setup" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(REFUSAL);
  await waitFor(() => expect(calls.filter((c) => c.cmd === "auto_run_setup_view")).toHaveLength(2));
  // The next press signs what is on screen now.
  fireEvent.click(screen.getByRole("button", { name: "Approve setup" }));
  await waitFor(() =>
    expect(calls.filter((c) => c.cmd === "auto_run_approve_setup").map((c) => c.args.expectedFingerprint)).toEqual([
      "old",
      "new",
    ]),
  );
});

test("saving from the editor sends no setup: Rust keeps the stored one", async () => {
  const { saved } = mountSetup([setupView("approved")]);
  await screen.findByRole("region", { name: "Setup" });
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0]).not.toHaveProperty("setup");
});

// ---- The readable script and Edit script -------------------------------

const READABLE_SCRIPT = {
  case_id: 7,
  title: "t",
  steps: [
    {
      step_number: 1,
      actions: [
        { kind: "navigate", url: "https://hr.example.test/hr/home/index?x=1" },
        { kind: "fill", selector: { css: "#txtpassword" }, value: "Hunter2!" },
        {
          kind: "when_visible",
          selector: { role: "dialog", name: "Another active session" },
          then: [{ kind: "click", selector: { role: "button", name: "Continue" } }],
        },
      ],
    },
    {
      step_number: 2,
      actions: [{ kind: "click", selector: "#btnContinue-button" }],
      unchecked: "the PDF cannot be read",
    },
  ],
};
const READABLE_STEPS = [
  { action: "Sign in", expected: "" },
  { action: "Open the report", expected: "The PDF opens" },
];

test("the window opens on the script in plain sentences, grouped by the case's steps", async () => {
  mountWith(READABLE_SCRIPT, ACCOUNTS, [], READABLE_STEPS);
  const view = await screen.findByRole("region", { name: "Action script" });
  const one = await within(view).findByRole("region", { name: "Step 1" });
  expect(within(one).getByRole("heading")).toHaveTextContent("Step 1 Sign in");
  expect(one).toHaveTextContent("Go to /hr/home/index");
  expect(one).toHaveTextContent("Type the account's password into the element #txtpassword");
  expect(one).not.toHaveTextContent("Hunter2");
  expect(one).toHaveTextContent('If the "Another active session" dialog appears within 2 s:');
  expect(one).toHaveTextContent('Click the "Continue" button');
  const two = within(view).getByRole("region", { name: "Step 2" });
  expect(within(two).getByRole("heading")).toHaveTextContent("Step 2 Open the report");
  expect(within(two).getByText("#btnContinue-button")).toHaveAttribute("title", "#btnContinue-button");
  expect(within(two).getByText("Not checked: the PDF cannot be read")).toHaveClass("text-muted");
  expect(screen.queryByRole("textbox", { name: "Action script JSON" })).not.toBeInTheDocument();
});

test("the settings stay editable in the readable view", async () => {
  const saved: unknown[] = [];
  mountWith({ ...READABLE_SCRIPT, changes: [] }, ACCOUNTS, saved, READABLE_STEPS);
  await screen.findByRole("region", { name: "Step 1" });
  fireEvent.click(screen.getByRole("checkbox", { name: "Must not save" }));
  const change = screen.getByRole("textbox", { name: "Add to Changes" });
  fireEvent.change(change, { target: { value: "cycle published" } });
  fireEvent.keyDown(change, { key: "Enter" });
  fireEvent.click(screen.getByRole("combobox", { name: "Runs as" }));
  fireEvent.click(await screen.findByRole("option", { name: "HR Admin (hr.admin)" }));
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0]).toEqual(
    expect.objectContaining({
      steps: READABLE_SCRIPT.steps,
      account: "hr.admin",
      no_save: true,
      changes: ["cycle published"],
    }),
  );
});

test("Edit script shows the JSON, and Back to readable view returns with the edit in sentences", async () => {
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP }, ACCOUNTS, []);
  await screen.findByText('Check the page shows "ok"');
  fireEvent.click(screen.getByRole("button", { name: "Edit script" }));
  const box = screen.getByRole("textbox", { name: "Action script JSON" }) as HTMLTextAreaElement;
  expect(JSON.parse(box.value)).toEqual(ONE_STEP);
  expect(screen.queryByRole("region", { name: "Action script" })).not.toBeInTheDocument();
  fireEvent.change(box, {
    target: { value: JSON.stringify([{ step_number: 1, actions: [{ kind: "reload" }] }]) },
  });
  fireEvent.click(screen.getByRole("button", { name: "Back to readable view" }));
  expect(await screen.findByText("Reload the page")).toBeInTheDocument();
  expect(screen.queryByRole("textbox", { name: "Action script JSON" })).not.toBeInTheDocument();
});

test("Back to readable view is refused while the JSON does not parse", async () => {
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP }, ACCOUNTS, []);
  fireEvent.click(await screen.findByRole("button", { name: "Edit script" }));
  const box = screen.getByRole("textbox", { name: "Action script JSON" });
  fireEvent.change(box, { target: { value: "{ not json" } });
  fireEvent.click(screen.getByRole("button", { name: "Back to readable view" }));
  expect(await screen.findByText(/That is not valid JSON/)).toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Action script JSON" })).toHaveValue("{ not json");
  // An object is valid JSON but not a script.
  fireEvent.change(box, { target: { value: "{}" } });
  fireEvent.click(screen.getByRole("button", { name: "Back to readable view" }));
  expect(
    await screen.findByText("That is not valid JSON: the script must be an array of steps."),
  ).toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Action script JSON" })).toBeInTheDocument();
});

test("Save from the JSON editor saves the JSON as it stands", async () => {
  const saved: unknown[] = [];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP }, ACCOUNTS, saved);
  fireEvent.click(await screen.findByRole("button", { name: "Edit script" }));
  const edited = [{ step_number: 1, actions: [{ kind: "press_key", key: "Tab" }] }];
  fireEvent.change(screen.getByRole("textbox", { name: "Action script JSON" }), {
    target: { value: JSON.stringify(edited) },
  });
  const save = screen.getByRole("button", { name: "Save script" });
  await waitFor(() => expect(save).not.toBeDisabled());
  fireEvent.click(save);
  await waitFor(() => expect(saved).toHaveLength(1));
  expect((saved[0] as { steps: unknown }).steps).toEqual(edited);
});

test("Save from the readable view saves the JSON edited before going back", async () => {
  const saved: unknown[] = [];
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP }, ACCOUNTS, saved);
  fireEvent.click(await screen.findByRole("button", { name: "Edit script" }));
  const edited = [{ step_number: 1, actions: [{ kind: "expire_session" }] }];
  fireEvent.change(screen.getByRole("textbox", { name: "Action script JSON" }), {
    target: { value: JSON.stringify(edited) },
  });
  fireEvent.click(screen.getByRole("button", { name: "Back to readable view" }));
  await screen.findByText("End the session");
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(saved).toHaveLength(1));
  expect((saved[0] as { steps: unknown }).steps).toEqual(edited);
});

test("a case with no script yet opens readable, saying there are no actions", async () => {
  mountWith(null, ACCOUNTS, []);
  expect(await screen.findByText("No actions yet. Press Edit script to write them.")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Edit script" })).toBeInTheDocument();
});

// ---- An odd script, focus, and keyboard access to selectors -------------

const UNREADABLE_LINE = "Could not read this action - press Edit script to see it";

test.each([
  ["a null action", [{ step_number: 1, actions: [null] }]],
  ["a null locator step", [{ step_number: 1, actions: [{ kind: "click", selector: [{ role: "dialog" }, null] }] }]],
  ["an unknown kind", [{ step_number: 1, actions: [{ kind: "teleport" }] }]],
  ["a missing actions array", [{ step_number: 1, unchecked: 5 }]],
])("%s shows the unreadable line, and Edit script still shows the JSON", async (_, steps) => {
  mountWith({ case_id: 7, title: "t", steps }, ACCOUNTS, [], [{ action: "Open it", expected: "It opens" }]);
  const one = await screen.findByRole("region", { name: "Step 1" });
  expect(within(one).getByText(UNREADABLE_LINE)).toBeInTheDocument();
  // The rest of the window is still there.
  expect(screen.getByText("Step 1: NOT CHECKED")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Edit script" }));
  const box = screen.getByRole("textbox", { name: "Action script JSON" }) as HTMLTextAreaElement;
  expect(JSON.parse(box.value)).toEqual(steps);
});

test("an odd action typed in the editor reads as the unreadable line, and the typed JSON is kept", async () => {
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP }, ACCOUNTS, []);
  fireEvent.click(await screen.findByRole("button", { name: "Edit script" }));
  const typed = '[{"step_number":1,"actions":[null,{"kind":"reload"}]}]';
  fireEvent.change(screen.getByRole("textbox", { name: "Action script JSON" }), { target: { value: typed } });
  fireEvent.click(screen.getByRole("button", { name: "Back to readable view" }));
  expect(await screen.findByText(UNREADABLE_LINE)).toBeInTheDocument();
  expect(screen.getByText("Reload the page")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Edit script" }));
  expect(screen.getByRole("textbox", { name: "Action script JSON" })).toHaveValue(typed);
});

test("entries that are not steps are counted at the end of the readable script", async () => {
  mountWith({ case_id: 7, title: "t", steps: [...ONE_STEP, null, "x", { step_number: "two" }] }, ACCOUNTS, []);
  expect(await screen.findByText("3 more entries are shown only in Edit script.")).toBeInTheDocument();
  expect(screen.getByText('Check the page shows "ok"')).toBeInTheDocument();
});

test("one entry that is not a step is counted in the singular", async () => {
  mountWith({ case_id: 7, title: "t", steps: [null] }, ACCOUNTS, []);
  expect(await screen.findByText("1 more entry is shown only in Edit script.")).toBeInTheDocument();
  expect(screen.queryByText("No actions yet. Press Edit script to write them.")).not.toBeInTheDocument();
});

test("Edit script moves the focus into the JSON, and Back to readable view onto Edit script", async () => {
  mountWith({ case_id: 7, title: "t", steps: ONE_STEP }, ACCOUNTS, []);
  fireEvent.click(await screen.findByRole("button", { name: "Edit script" }));
  await waitFor(() => expect(screen.getByRole("textbox", { name: "Action script JSON" })).toHaveFocus());
  fireEvent.click(screen.getByRole("button", { name: "Back to readable view" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Edit script" })).toHaveFocus());
});

test("an element named from its CSS carries the selector for a screen reader as well as on hover", async () => {
  const steps = [{ step_number: 1, actions: [{ kind: "click", selector: 'input[placeholder="Email"]' }] }];
  mountWith({ case_id: 7, title: "t", steps }, ACCOUNTS, []);
  const one = await screen.findByRole("region", { name: "Step 1" });
  expect(one).toHaveTextContent('Click the "Email" field (selector input[placeholder="Email"])');
  const hidden = within(one).getByText('(selector input[placeholder="Email"])', { exact: false });
  expect(hidden).toHaveClass("sr-only");
  expect(hidden.parentElement).toHaveAttribute("title", 'input[placeholder="Email"]');
});
