// One Auto Test case card on its own: a single line while collapsed, and
// the script's summary, steps and files once open. Which buttons show on a
// collapsed card, and which sections an open one leaves out, are what is
// pinned here; how the card looks is not something jsdom can see.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { ComponentProps } from "react";
import type { CaseScript, Divergence } from "../../../src/bindings";
import CaseCard, { actionFacts, assistantNotes, scriptFiles, scriptSummary, stepBecauseParts } from "../../../src/screens/AutoRun/CaseCard";
import { matchesSearch } from "../../../src/screens/AutoRun/CaseSearch";

afterEach(() => {
  clearMocks();
});

const CASE = {
  id: 7,
  title: "Apply for leave",
  steps: [
    { action: "Open the leave form", expected: "" },
    { action: "Attach the medical note", expected: "The file is listed" },
  ],
};

/** A script with every fact set, and an upload and a download check. */
const FULL: CaseScript = {
  case_id: 7,
  title: "Apply for leave",
  account: "emp1",
  area: "Leave requests",
  no_save: true,
  preconditions: [{ flow: "leave-cycle", stage: "open", value: "2026" }],
  changes: ["cycle published"],
  needs_unchanged: ["draft untouched"],
  last_repair: "The Save button moved into a menu",
  steps: [
    { step_number: 1, actions: [{ kind: "navigate", url: "https://app.example/leave" }] },
    {
      step_number: 2,
      actions: [
        { kind: "upload", selector: { css: "#f" }, file: "note.pdf" },
        {
          kind: "when_visible",
          selector: { css: "#export" },
          then: [{ kind: "expect_download", name: "leave-*.xlsx" }],
        },
      ],
    },
  ],
} as CaseScript;

/** A script with nothing but its steps. */
const BARE: CaseScript = {
  case_id: 7,
  title: "Apply for leave",
  steps: [{ step_number: 1, actions: [] }],
} as CaseScript;

function renderCard(props: Partial<ComponentProps<typeof CaseCard>> = {}) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const handlers = {
    onSelect: vi.fn(),
    onToggleOpen: vi.fn(),
    onEdit: vi.fn(),
    onRun: vi.fn(),
    onAskClear: vi.fn(),
    onClearDone: vi.fn(),
  };
  const rendered = render(
    <QueryClientProvider client={qc}>
      <ul>
        <CaseCard
          c={CASE}
          org="acme"
          project="Web"
          script={FULL}
          result="Failed"
          selected={false}
          open={false}
          confirmingClear={false}
          divergences={[]}
          {...handlers}
          {...props}
        />
      </ul>
    </QueryClientProvider>,
  );
  return Object.assign(handlers, { unmount: rendered.unmount });
}

test("a collapsed card shows the checkbox, the id, the title and the result, and no action buttons", () => {
  renderCard();
  const card = screen.getByRole("listitem");
  expect(within(card).getByRole("checkbox", { name: "Select #7" })).toBeInTheDocument();
  expect(within(card).getByText("#7")).toBeInTheDocument();
  expect(within(card).getByText("Failed")).toHaveTextContent("Last result: Failed");
  for (const name of ["View script for #7", "Add script for #7", "Run #7"]) {
    expect(within(card).queryByRole("button", { name })).not.toBeInTheDocument();
  }
  // The only buttons are the two ways to open it.
  expect(within(card).getAllByRole("button").map((b) => b.getAttribute("aria-expanded"))).toEqual([
    "false",
    "false",
  ]);
  expect(within(card).queryByText("Script")).not.toBeInTheDocument();
});

test("the chevron and the title both open the card, and the chevron names its case", () => {
  const { onToggleOpen } = renderCard();
  const chevron = screen.getByRole("button", { name: "Show details for #7" });
  expect(chevron).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(chevron);
  fireEvent.click(screen.getByRole("button", { name: "Apply for leave" }));
  expect(onToggleOpen).toHaveBeenCalledTimes(2);
  expect(onToggleOpen).toHaveBeenCalledWith(7);
});

test("an open card shows the Script, Steps and Files sections, then View script and Run", () => {
  mockIPC(() => null);
  const { onEdit, onRun } = renderCard({ open: true });
  expect(screen.getByRole("button", { name: "Hide details for #7" })).toHaveAttribute("aria-expanded", "true");

  // Summary chips (they replaced the "Script · ..." line and the Must not
  // save fact row), then every fact this script has.
  const summary = screen.getByRole("group", { name: "Summary of #7" });
  expect(within(summary).getByText("2 steps")).toBeInTheDocument();
  expect(within(summary).getByText("Runs as emp1")).toBeInTheDocument();
  expect(within(summary).getByText("Leave requests")).toBeInTheDocument();
  expect(within(summary).getByText("Must not save")).toBeInTheDocument();
  expect(screen.getByText("leave-cycle: open")).toBeInTheDocument();
  expect(screen.getByText("cycle published")).toBeInTheDocument();
  expect(screen.getByText("draft untouched")).toBeInTheDocument();
  expect(screen.getByText("The Save button moved into a menu")).toBeInTheDocument();

  // Steps: numbered, one line each, in the case's own words.
  const steps = screen.getByRole("list", { name: "Script steps of #7" });
  const lines = within(steps).getAllByRole("listitem");
  expect(lines).toHaveLength(2);
  expect(lines[0]).toHaveTextContent("1.Open the leave form1 action");
  expect(lines[1]).toHaveTextContent("2.Attach the medical note2 actions");

  // Files: the upload, and the download it checks inside a when_visible.
  expect(screen.getByText("note.pdf")).toBeInTheDocument();
  expect(screen.getByText("leave-*.xlsx")).toBeInTheDocument();

  const view = screen.getByRole("button", { name: "View script for #7" });
  expect(view).toHaveTextContent("View script");
  expect(screen.queryByRole("button", { name: "Edit script for #7" })).not.toBeInTheDocument();
  fireEvent.click(view);
  fireEvent.click(screen.getByRole("button", { name: "Run #7" }));
  expect(onEdit).toHaveBeenCalledTimes(1);
  expect(onRun).toHaveBeenCalledTimes(1);
  expect(onEdit).toHaveBeenCalledWith(7);
  expect(onRun).toHaveBeenCalledWith(7);
});

test("an open card says how the script treats page errors", () => {
  renderCard({ open: true, script: { ...BARE, page_errors: "flag" } as CaseScript });
  expect(screen.getByText("Page errors")).toBeInTheDocument();
  expect(screen.getByText("flag")).toBeInTheDocument();
});

test("an open card shows the script's dialog and ignored page-error settings", () => {
  renderCard({
    open: true,
    script: { ...BARE, fail_on_unexpected_dialog: true, ignore_page_errors: ["ResizeObserver", "/api/Poll"] } as CaseScript,
  });
  expect(screen.getByText("Unexpected dialogs")).toBeInTheDocument();
  expect(screen.getByText("fail")).toBeInTheDocument();
  expect(screen.getByText("Ignored page errors")).toBeInTheDocument();
  expect(screen.getByText("ResizeObserver, /api/Poll")).toBeInTheDocument();
});

test("an open card leaves out every fact with no value, and Files when there is nothing to list", () => {
  renderCard({ open: true, script: BARE });
  const summary = screen.getByRole("group", { name: "Summary of #7" });
  expect(within(summary).getByText("1 step")).toBeInTheDocument();
  // No account, area or no-save: no chip for them.
  expect(within(summary).queryByText(/Runs as/)).not.toBeInTheDocument();
  expect(within(summary).queryByText("Must not save")).not.toBeInTheDocument();
  expect(summary.children).toHaveLength(1);
  expect(screen.queryByText(/Assistant notes/)).not.toBeInTheDocument();
  for (const label of [
    "Runs as", "Area", "Must not save", "Page errors", "Ignored page errors", "Unexpected dialogs",
    "Preconditions", "Changes", "Needs unchanged", "Last repair", "Assumed values", "Invented values",
  ]) {
    expect(screen.queryByText(label)).not.toBeInTheDocument();
  }
  expect(screen.queryByText("Files")).not.toBeInTheDocument();
  expect(screen.getByText("Steps")).toBeInTheDocument();
});

test("an open card with no script says an assistant writes it and offers no button, with no Script or Steps sections", () => {
  renderCard({ open: true, script: null });
  expect(screen.getByText("No script yet. An assistant writes it.")).toBeInTheDocument();
  expect(screen.queryByText("Steps")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Run #7" })).not.toBeInTheDocument();
  expect(screen.getByRole("checkbox", { name: "Select #7" })).toHaveClass("invisible");
  expect(screen.queryByRole("button", { name: /script for #7/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Add script/ })).not.toBeInTheDocument();
  // The only buttons left are the two ways to close the card.
  expect(within(screen.getByRole("listitem")).getAllByRole("button").map((b) => b.getAttribute("aria-expanded"))).toEqual([
    "true",
    "true",
  ]);
});

test("an open card lists the last run's downloads, each with Open", async () => {
  mockIPC((cmd) => (cmd === "auto_run_download_sizes" ? [{ name: "leave-2026.xlsx", size: 2048 }] : null));
  renderCard({
    open: true,
    script: BARE,
    lastRun: { runId: "run-1", steps: [{ downloads: ["leave-2026.xlsx"] }] },
  });
  expect(screen.getByText("Files")).toBeInTheDocument();
  const list = screen.getByRole("list", { name: "Downloads" });
  expect(within(list).getByRole("button", { name: "Open leave-2026.xlsx" })).toBeInTheDocument();
  expect(await within(list).findByText("2.0 KB")).toBeInTheDocument();
});

test("a suspected defect shows on the open card, with its step and note", () => {
  const { onAskClear } = renderCard({
    open: true,
    script: { ...BARE, suspected_defect: { step_number: 1, note: "No lockout message", marked_at: "1" } } as CaseScript,
  });
  expect(screen.getByText("Suspected defect")).toBeInTheDocument();
  expect(screen.getByText("Step 1: No lockout message")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Clear suspected defect for #7" }));
  expect(onAskClear).toHaveBeenCalledTimes(1);
  expect(onAskClear).toHaveBeenCalledWith(7);
});

test("a mark the assistant cleared shows the person's words and when, with no undo", () => {
  const at = Date.UTC(2026, 9, 22, 8, 30);
  renderCard({
    open: true,
    script: {
      ...BARE,
      cleared_defect: { step_number: 3, said: "that is not a defect, it is by design", note: "Agreed.", cleared_at: String(at) },
    } as CaseScript,
  });
  const note = screen.getByText(/Cleared by the assistant/);
  expect(note).toHaveTextContent(`on ${new Date(at).toLocaleString()}, on your words: "that is not a defect, it is by design".`);
  expect(note).toHaveTextContent("It had marked step 3. Its note: Agreed.");
  expect(screen.queryByText("Suspected defect")).toBeNull();
  expect(screen.queryByRole("button", { name: /Undo/ })).toBeNull();
});

test("a case with a suspected defect and no cleared mark says nothing was cleared", () => {
  renderCard({
    open: true,
    script: { ...BARE, suspected_defect: { step_number: 1, note: "No lockout message", marked_at: "1" } } as CaseScript,
  });
  expect(screen.queryByText(/Cleared by the assistant/)).toBeNull();
});

test("a collapsed card with a suspected defect shows an icon marker, and still no extra button", () => {
  renderCard({
    open: false,
    script: { ...BARE, suspected_defect: { step_number: 1, note: "No lockout message", marked_at: "1" } } as CaseScript,
  });
  const mark = screen.getByRole("img", { name: "Suspected defect: No lockout message" });
  expect(mark).toHaveAttribute("title", "Suspected defect: No lockout message");
  expect(screen.getAllByRole("button")).toHaveLength(2);
});

test("a collapsed card without a suspected defect shows no marker", () => {
  renderCard({ open: false, script: BARE });
  expect(screen.queryByRole("img", { name: /Suspected defect/ })).toBeNull();
});

test("the open card shows the badge, not the marker", () => {
  renderCard({
    open: true,
    script: { ...BARE, suspected_defect: { step_number: 1, note: "No lockout message", marked_at: "1" } } as CaseScript,
  });
  expect(screen.queryByRole("img", { name: /Suspected defect/ })).toBeNull();
});

test("actionFacts, assistantNotes and scriptFiles skip what is not there", () => {
  expect(actionFacts(BARE)).toEqual([]);
  expect(assistantNotes(BARE)).toEqual([]);
  expect(scriptFiles(BARE)).toEqual({ uploads: [], downloads: [] });
  expect(scriptFiles(FULL)).toEqual({ uploads: ["note.pdf"], downloads: ["leave-*.xlsx"] });
  // Blank text is no value either.
  expect(assistantNotes({ ...BARE, account: "  ", changes: [] } as CaseScript)).toHaveLength(0);
});

test("the card lists assumed and invented values", () => {
  const script = {
    ...BARE,
    steps: [
      { step_number: 1, actions: [] },
      {
        step_number: 2,
        actions: [],
        because: [
          { value: "6", why: "one row per DEF level the precondition lists" },
          { value: "USD", why: "one currency the case leaves free", provenance: "invented" },
        ],
      },
    ],
  } as CaseScript;
  // Assumed values are no fact row any more: each shows under its step.
  expect(actionFacts(script).some((f) => f.label === "Assumed values")).toBe(false);
  expect(actionFacts(script)).toContainEqual({
    label: "Invented values",
    value: "step 2: USD (one currency the case leaves free)",
  });
  expect(actionFacts(BARE).some((f) => /Assumed|Invented/.test(f.label))).toBe(false);
  renderCard({ open: true, script });
  expect(screen.queryByText("Assumed values")).not.toBeInTheDocument();
  // Invented values are no fact row either: they fold under their step.
  expect(screen.queryByText("Invented values")).not.toBeInTheDocument();
  const items = within(screen.getByRole("list", { name: "Script steps of #7" })).getAllByRole("listitem");
  // Folded, step 2 only counts them (step 1 has nothing, so nothing to open).
  expect(items[1]).toHaveTextContent("1 assumed");
  expect(items[1]).toHaveTextContent("1 invented");
  expect(items[0]).not.toHaveTextContent(/assumed|invented/);
  expect(screen.queryByRole("button", { name: "Show step 1 of #7" })).not.toBeInTheDocument();
  expect(screen.queryByRole("group", { name: "Assumed in step 2" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Show step 2 of #7" }));
  // Opened: the value, then its reason from the data, in two columns.
  const assumed = screen.getByRole("group", { name: "Assumed in step 2" });
  expect(within(assumed).getByText("6")).toBeInTheDocument();
  expect(within(assumed).getByText("one row per DEF level the precondition lists").tagName).toBe("DD");
  expect(within(assumed).getByText("6").tagName).toBe("DT");
  const invented = screen.getByRole("group", { name: "Invented in step 2" });
  expect(within(invented).getByText("USD")).toBeInTheDocument();
  expect(within(invented).getByText("one currency the case leaves free")).toBeInTheDocument();
  expect(items[1]).toContainElement(assumed);
  expect(screen.queryByRole("group", { name: "Assumed in step 1" })).not.toBeInTheDocument();
});

test("a reason loses only a leading assumed: or invented: prefix, and no reason leaves the value alone", () => {
  const step = {
    step_number: 1,
    actions: [],
    because: [
      { value: "UTC", why: "assumed: no zone given" },
      { value: "USD", why: "Invented: one currency", provenance: "invented" },
      { value: "7", why: "" },
    ],
  } as CaseScript["steps"][number];
  expect(stepBecauseParts(step, false)).toEqual([
    { value: "UTC", why: "no zone given" },
    { value: "7", why: "" },
  ]);
  expect(stepBecauseParts(step, true)).toEqual([{ value: "USD", why: "one currency" }]);
  // Leading spaces, and a space before the colon, are part of the prefix.
  const spaced = { step_number: 1, actions: [], because: [{ value: "a", why: "  assumed: x" }, { value: "b", why: "assumed : y" }] } as CaseScript["steps"][number];
  expect(stepBecauseParts(spaced, false).map((e) => e.why)).toEqual(["x", "y"]);
});

test("an assumed value with no step of the list stays a short list after the steps", () => {
  const script = {
    ...BARE,
    steps: [
      { step_number: 0, actions: [], because: [{ value: "UTC", why: "no zone given" }] },
      { step_number: 1, actions: [] },
    ],
  } as CaseScript;
  renderCard({ open: true, script });
  const list = screen.getByRole("list", { name: "Assumed values of #7" });
  expect(list).toHaveTextContent("step 0: UTC (no zone given)");
  expect(screen.queryByRole("group", { name: /Assumed in step/ })).not.toBeInTheDocument();
});

test("the card lists checks given up", () => {
  const script = {
    ...BARE,
    given_up: [{ step: 3, kind: "expect_count", why: "step 4 already counts the same rows", at: "2026-10-21T09:30:00Z" }],
  } as CaseScript;
  const line = "step 3 expect_count: step 4 already counts the same rows (2026-10-21)";
  expect(assistantNotes(script)).toContainEqual({ label: "Checks given up", value: line });
  expect(assistantNotes(BARE).some((f) => f.label === "Checks given up")).toBe(false);
  renderCard({ open: true, script });
  expect(screen.getByText("Checks given up")).toBeInTheDocument();
  expect(screen.getByText(line)).toBeInTheDocument();
});

test("the search matches the id with or without #, and the title, ignoring case", () => {
  const c = { id: 4321, title: "Login - Locked Account" };
  expect(matchesSearch(c, "")).toBe(true);
  expect(matchesSearch(c, "4321")).toBe(true);
  expect(matchesSearch(c, "#4321")).toBe(true);
  expect(matchesSearch(c, " #43 ")).toBe(true);
  expect(matchesSearch(c, "locked")).toBe(true);
  expect(matchesSearch(c, "LOGIN - locked")).toBe(true);
  expect(matchesSearch(c, "#99")).toBe(false);
  expect(matchesSearch(c, "expired")).toBe(false);
});

test("the search also finds a case by tag, module, automation status or its script's area, and two words narrow", () => {
  const c = {
    id: 4321,
    title: "Login - Locked Account",
    tags: "smoke; regression",
    module_value: "Payroll",
    automation_status: "Planned",
  };
  expect(matchesSearch(c, "regression")).toBe(true);
  expect(matchesSearch(c, "payroll")).toBe(true);
  expect(matchesSearch(c, "planned")).toBe(true);
  expect(matchesSearch(c, "leave requests", ["Leave requests"])).toBe(true);
  expect(matchesSearch(c, "leave requests")).toBe(false);
  expect(matchesSearch(c, "locked payroll")).toBe(true);
  expect(matchesSearch(c, "locked leave")).toBe(false);
  // A comma list still means those exact ids, and any search in it may match.
  expect(matchesSearch(c, "99, regression")).toBe(true);
  expect(matchesSearch(c, "99, 43")).toBe(false);
});

/** A setup view as the editor's command answers it. */
const view = () => ({
  fixture_name: "Draft cycle",
  account: "hr.admin",
  steps: ["Create cycle: name=A"],
  creates: ["cycle A"],
});

test("a collapsed card with a setup does not read it", async () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    return cmd === "auto_run_setup_view" ? view() : null;
  });
  const withSetup = { ...BARE, setup: { fixture: "Draft cycle" } } as CaseScript;
  renderCard({ open: false, script: withSetup });
  // A collapsed card reads nothing.
  expect(calls).not.toContain("auto_run_setup_view");
});

test("the setup line names the fixture and nothing about approval", async () => {
  const withSetup = { ...BARE, setup: { fixture: "Draft cycle" } } as CaseScript;
  mockIPC((cmd) => (cmd === "auto_run_setup_view" ? view() : null));
  renderCard({ open: true, script: withSetup });
  await waitFor(() => expect(screen.getByText("Setup:", { exact: false }).closest("p")).toHaveTextContent(/^Setup: Draft cycle$/));
});

test("a script with no setup shows no setup line and does not ask", async () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    return null;
  });
  renderCard({ open: true, script: BARE });
  expect(screen.queryByText("Setup:", { exact: false })).not.toBeInTheDocument();
  expect(calls).not.toContain("auto_run_setup_view");
});

const DIV: Divergence = {
  id: "d1",
  case_id: 7,
  step: 2,
  case_says: "exactly 4 levels",
  app_shows: "6 levels",
  kind: "data",
  reported_at: "1",
  status: "open",
};

test("a collapsed card says it needs a ruling", () => {
  renderCard({ divergences: [DIV] });
  expect(screen.getByRole("img", { name: "Needs a ruling: step 2" })).toBeInTheDocument();
});

test("a collapsed card whose case waits for a repair does not say it needs a ruling", () => {
  renderCard({ divergences: [{ ...DIV, status: "awaiting_repair" }] });
  expect(screen.getByRole("img", { name: "Waits for a repair: step 2" })).toBeInTheDocument();
  expect(screen.queryByRole("img", { name: /Needs a ruling/ })).not.toBeInTheDocument();
});

test("a case with no script still shows its divergence", () => {
  renderCard({ open: true, script: null, divergences: [DIV] });
  expect(screen.getByText("No script yet. An assistant writes it.")).toBeInTheDocument();
  expect(screen.getByText("Needs a ruling")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "The app is wrong for #7 step 2" })).toBeInTheDocument();
});

/** A script whose step 2 leaves one expectation unchecked, with a stand-in. */
const WITH_UNCHECKED = {
  case_id: 5002,
  title: "Open the cycle",
  steps: [
    { step_number: 1, actions: [] },
    {
      step_number: 2,
      actions: [],
      unchecked: [{ item: "no access-denied message", why: "the app never shows one", stand_in: "expect_hidden" }],
    },
  ],
} as CaseScript;

test("a passed case with an unchecked entry says 1 expectation unchecked", () => {
  renderCard({ c: { ...CASE, id: 5002 }, script: WITH_UNCHECKED, result: "Passed" });
  expect(screen.getByText("1 expectation unchecked")).toBeInTheDocument();
});

test("a failed case does not count its unchecked expectations", () => {
  renderCard({ c: { ...CASE, id: 5002 }, script: WITH_UNCHECKED, result: "Failed" });
  expect(screen.queryByText(/expectations? unchecked/)).not.toBeInTheDocument();
});

test("an open passed card says how many too, and a card whose script leaves nothing unchecked says nothing", () => {
  mockIPC(() => null);
  const { unmount } = renderCard({ c: { ...CASE, id: 5002 }, script: WITH_UNCHECKED, result: "Passed", open: true });
  expect(screen.getByText("1 expectation unchecked")).toBeInTheDocument();
  unmount();
  renderCard({ open: true, script: BARE, result: "Passed" });
  expect(screen.queryByText(/expectations? unchecked/)).not.toBeInTheDocument();
  expect(screen.queryByText("Not checked")).not.toBeInTheDocument();
});

test("the open card lists each unchecked expectation with why and its stand-in", () => {
  mockIPC(() => null);
  renderCard({ c: { ...CASE, id: 5002 }, script: WITH_UNCHECKED, result: "Failed", open: true });
  // The note is folded under its step (2): the row counts it, and opening the
  // step shows a tag word, then the text, with no separate list.
  const rows = within(screen.getByRole("list", { name: "Script steps of #5002" })).getAllByRole("listitem");
  expect(rows[1]).toHaveTextContent("1 unchecked");
  expect(rows[0]).not.toHaveTextContent("unchecked");
  expect(screen.queryByRole("group", { name: "Not checked in step 2" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Show step 2 of #5002" }));
  const group = screen.getByRole("group", { name: "Not checked in step 2" });
  expect(group).toHaveTextContent("Not checked");
  expect(group).toHaveTextContent("no access-denied message - the app never shows one (stand-in: expect_hidden)");
  expect(screen.queryByRole("list", { name: "Not checked expectations of #5002" })).not.toBeInTheDocument();
  expect(screen.queryByRole("group", { name: "Not checked in step 1" })).not.toBeInTheDocument();
  const items = within(screen.getByRole("list", { name: "Script steps of #5002" })).getAllByRole("listitem");
  expect(items[1]).toContainElement(group);
});

test("an unchecked expectation with no step of the list stays a short list after the steps", () => {
  mockIPC(() => null);
  const script = {
    ...BARE,
    steps: [
      { step_number: 0, actions: [], unchecked: "setup is not checked" },
      { step_number: 1, actions: [] },
    ],
  } as CaseScript;
  renderCard({ open: true, script });
  const list = screen.getByRole("list", { name: "Not checked expectations of #7" });
  expect(list).toHaveTextContent("step 0: the step's expected result - setup is not checked");
});

test("scriptFacts names each needed record with its reason", () => {
  const facts = actionFacts({ ...BARE, needs_records: [{ setup: "Rating scale", why: "the AUTOTEST scale" }] } as CaseScript);
  expect(facts).toContainEqual({ label: "Needs records", value: "Rating scale (the AUTOTEST scale)" });
});

test("the open card shows a clean-up guard that has never fired, whatever the case result, and nothing when none", () => {
  mockIPC(() => null);
  const unfired = {
    ...BARE,
    checked: true,
    unfired_guards: ['step 3: button "Activate"', 'step 3: css "#done"'],
  } as CaseScript;
  const { unmount } = renderCard({ open: true, script: unfired, result: "Passed" });
  expect(screen.getByText("Clean-up guard never fired")).toBeInTheDocument();
  const list = screen.getByRole("list", { name: `Clean-up guards that never fired of #${CASE.id}` });
  expect(list).toHaveTextContent('step 3: button "Activate"');
  expect(list).toHaveTextContent('step 3: css "#done"');
  unmount();
  renderCard({ open: true, script: BARE, result: "Passed" });
  expect(screen.queryByText("Clean-up guard never fired")).not.toBeInTheDocument();
});

test("the summary leaves out an account or area it does not have, and Must not save unless set", () => {
  expect(scriptSummary(FULL)).toEqual({ steps: "2 steps", account: "emp1", area: "Leave requests", noSave: true });
  expect(scriptSummary({ ...FULL, area: undefined } as CaseScript).area).toBeNull();
  expect(scriptSummary({ ...FULL, account: " ", area: "Self Service > Insights" } as CaseScript)).toMatchObject({
    account: null,
    area: "Self Service > Insights",
  });
  expect(scriptSummary(BARE)).toEqual({ steps: "1 step", account: null, area: null, noSave: false });
});

test("the action facts are the ones to act on, the assistant's notes the rest", () => {
  expect(actionFacts(FULL).map((f) => f.label)).toEqual(["Preconditions"]);
  expect(assistantNotes(FULL).map((f) => f.label)).toEqual(["Changes", "Needs unchanged", "Last repair"]);
});

test("assistant notes are a closed details with a count, and their items inside", () => {
  renderCard({ open: true });
  const summary = screen.getByText("Assistant notes (3)");
  const details = summary.closest("details") as HTMLDetailsElement;
  expect(details.open).toBe(false);
  expect(within(details).getByText("cycle published")).toBeInTheDocument();
  expect(within(details).getByText("The Save button moved into a menu")).toBeInTheDocument();
  expect(screen.getByText("Must not save").closest("details")).toBeNull();
  // At the bottom of the card, after the steps.
  const steps = screen.getByRole("list", { name: "Script steps of #7" });
  expect(steps.compareDocumentPosition(details) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});

/** The owner's example: steps 1-3 are Auto Test's own, step 6 leaves one check out. */
const AUTO_RUN_OPENING = {
  case_id: 7,
  title: "Apply for leave",
  steps: [
    { step_number: 1, actions: [], unchecked: "Auto Test launches the app" },
    { step_number: 2, actions: [], unchecked: "Auto Test signs in" },
    { step_number: 3, actions: [], unchecked: "Auto Test reaches the area" },
    { step_number: 4, actions: [{ kind: "navigate", url: "https://app.example/x" }] },
    { step_number: 5, actions: [{ kind: "navigate", url: "https://app.example/y" }] },
    {
      step_number: 6,
      actions: [{ kind: "navigate", url: "https://app.example/z" }],
      unchecked: [{ item: "the chart colours", why: "no locator reads a colour" }],
    },
  ],
} as CaseScript;
const SIX = {
  id: 7,
  title: "Apply for leave",
  steps: [1, 2, 3, 4, 5, 6].map((n) => ({ action: `Step ${n}`, expected: "" })),
};

test("steps Auto Test does show Done by Auto Test, not 0 actions", () => {
  renderCard({ open: true, c: SIX, script: AUTO_RUN_OPENING });
  const lines = within(screen.getByRole("list", { name: "Script steps of #7" })).getAllByRole("listitem");
  for (const i of [0, 1, 2]) {
    expect(lines[i]).toHaveTextContent("Done by Auto Test");
    expect(lines[i]).not.toHaveTextContent("0 actions");
  }
  expect(lines[3]).toHaveTextContent("1 action");
});

test("Not checked shows only under the step the script leaves out, and a passed case counts it", () => {
  renderCard({ open: true, c: SIX, script: AUTO_RUN_OPENING, result: "Passed" });
  // The note is under step 6 (open it to see it); steps 1-3 are Auto Test's
  // own and have none, so they have no chevron.
  expect(screen.queryByRole("button", { name: "Show step 1 of #7" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Show step 6 of #7" }));
  expect(screen.getAllByRole("group", { name: /Not checked in step/ })).toHaveLength(1);
  const group = screen.getByRole("group", { name: "Not checked in step 6" });
  expect(group).toHaveTextContent("the chart colours");
  expect(screen.getByText("1 expectation unchecked")).toBeInTheDocument();
});

const RULED: Divergence = {
  ...DIV,
  status: "ruled",
  ruling: "case_stands",
  ruled_at: "2",
  ruled_by: "assistant",
  said: "the app is wrong",
  note: "Four levels, as the case says",
  proposal: "6 levels",
} as Divergence;

/** Opens step 2 of the card (the one RULED sits on). */
const openStep2 = () => fireEvent.click(screen.getByRole("button", { name: "Show step 2 of #7" }));

test("a ruling sits on its step: one line of what was decided, Details and Undo", () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    return null;
  });
  renderCard({ open: true, divergences: [RULED] });
  // Folded into its step: no top list, and the line only shows once it opens.
  expect(screen.queryByText(/Rulings \(/)).not.toBeInTheDocument();
  expect(screen.queryByText("Four levels, as the case says")).not.toBeInTheDocument();
  openStep2();
  const group = screen.getByRole("group", { name: "Ruling in step 2" });
  expect(within(group).getByText("Ruling")).toBeInTheDocument();
  // The line is the assistant's note, cut on one line with the whole text in title.
  const line = screen.getByText("Four levels, as the case says");
  expect(line).toHaveClass("truncate");
  expect(line).toHaveAttribute("title", "Four levels, as the case says");
  // Details opens the full block; the full text is not there before.
  const details = screen.getByRole("button", { name: "Details of the ruling on #7 step 2" });
  expect(details).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByText(/Ruled by the assistant, on your words/)).not.toBeInTheDocument();
  fireEvent.click(details);
  expect(details).toHaveAttribute("aria-expanded", "true");
  const block = document.getElementById(details.getAttribute("aria-controls") ?? "") as HTMLElement;
  expect(block).toHaveTextContent('the case says "exactly 4 levels"; the app shows "6 levels".');
  expect(block).toHaveTextContent('Proposed: "6 levels".');
  expect(block).toHaveTextContent('Ruled by the assistant, on your words: "the app is wrong"');
  // Undo is the same command, under the same name.
  fireEvent.click(screen.getByRole("button", { name: "Undo the ruling on #7 step 2" }));
  expect(calls).toContain("auto_run_undo_divergence_ruling");
});

test("the summary counts the rulings in a chip, and has none without a ruling", () => {
  const { unmount } = renderCard({ open: true, divergences: [RULED, { ...RULED, id: "d2", step: 1 } as Divergence] });
  expect(within(screen.getByRole("group", { name: "Summary of #7" })).getByText("2 rulings")).toBeInTheDocument();
  unmount();
  renderCard({ open: true, divergences: [DIV] });
  expect(within(screen.getByRole("group", { name: "Summary of #7" })).queryByText(/ruling/)).not.toBeInTheDocument();
});

test("a ruling that waits for a repair keeps its badge and the /tcm:heal line inside the opened step", () => {
  renderCard({ open: true, divergences: [{ ...RULED, status: "awaiting_repair", ruling: "rewrite" } as Divergence] });
  // Closed, only the row tag says it; the badge and the heal line are inside.
  expect(screen.queryByRole("group", { name: "Ruling in step 2" })).not.toBeInTheDocument();
  openStep2();
  const group = screen.getByRole("group", { name: "Ruling in step 2" });
  expect(within(group).getByText("Waits for a repair")).toBeInTheDocument();
  expect(group).toHaveTextContent("/tcm:heal");
  expect(within(group).getByRole("button", { name: "Undo the ruling on #7 step 2" })).toBeInTheDocument();
});

test("a ruling on a step the list lacks stays a short list at the top, with its Undo", () => {
  renderCard({ open: true, divergences: [{ ...RULED, step: 9 } as Divergence] });
  expect(screen.getByText("Rulings (1)")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Undo the ruling on #7 step 9" })).toBeInTheDocument();
  // And it marks no step: nothing to open.
  expect(screen.queryByRole("button", { name: /Show step/ })).not.toBeInTheDocument();
});

test("a step's opened body shows its action count, and a step with nothing shows it on its row", () => {
  renderCard({ open: true, divergences: [RULED] });
  const items = within(screen.getByRole("list", { name: "Script steps of #7" })).getAllByRole("listitem");
  // Step 1 has nothing to fold: no chevron, no tags, its count on the row.
  expect(items[0]).toHaveTextContent("1 action");
  // Step 2 folds its count away until opened.
  expect(items[1]).not.toHaveTextContent("2 actions");
  openStep2();
  expect(items[1]).toHaveTextContent("2 actions");
});

test("steps start closed, Expand all opens each and Collapse all closes them", () => {
  mockIPC(() => null);
  renderCard({ c: SIX, script: AUTO_RUN_OPENING, open: true });
  // Steps 1-3 are Auto Test's own: only step 6 can open.
  expect(screen.getByRole("button", { name: "Show step 6 of #7" })).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByRole("group", { name: /Not checked in step/ })).not.toBeInTheDocument();
  const toggle = screen.getByRole("button", { name: "Expand all steps" });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(toggle);
  expect(screen.getByRole("group", { name: "Not checked in step 6" })).toBeInTheDocument();
  const collapse = screen.getByRole("button", { name: "Collapse all steps" });
  expect(collapse).toHaveAttribute("aria-expanded", "true");
  expect(collapse).toHaveAttribute("aria-controls", screen.getByRole("list", { name: "Script steps of #7" }).id);
  fireEvent.click(collapse);
  expect(screen.queryByRole("group", { name: /Not checked in step/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Expand all steps" })).toBeInTheDocument();
});

test("a card with one step opens it", () => {
  renderCard({
    open: true,
    script: { ...BARE, steps: [{ step_number: 1, actions: [], because: [{ value: "UTC", why: "no zone given" }] }] } as CaseScript,
  });
  expect(screen.getByRole("button", { name: "Hide step 1 of #7" })).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByRole("group", { name: "Assumed in step 1" })).toBeInTheDocument();
});

test("a script that arrives after the card mounts opens its one step once, and stays closed after the person closes it", () => {
  mockIPC(() => null);
  const one = { ...BARE, steps: [{ step_number: 1, actions: [], because: [{ value: "UTC", why: "no zone given" }] }] } as CaseScript;
  const qc = new QueryClient();
  const tree = (script: CaseScript | null) => (
    <QueryClientProvider client={qc}>
      <ul>
        <CaseCard c={CASE} org="acme" project="Web" script={script} result="Failed" selected={false} open onSelect={vi.fn()}
          onToggleOpen={vi.fn()} onEdit={vi.fn()} onRun={vi.fn()} confirmingClear={false} divergences={[]} onAskClear={vi.fn()} onClearDone={vi.fn()} />
      </ul>
    </QueryClientProvider>
  );
  const { rerender } = render(tree(null));
  rerender(tree(one));
  fireEvent.click(screen.getByRole("button", { name: "Hide step 1 of #7" }));
  rerender(tree({ ...one }));
  expect(screen.getByRole("button", { name: "Show step 1 of #7" })).toHaveAttribute("aria-expanded", "false");
});

test("a step whose ruling waits for a repair says so on its closed row", () => {
  renderCard({ open: true, divergences: [{ ...RULED, status: "awaiting_repair", ruling: "rewrite" } as Divergence] });
  const items = within(screen.getByRole("list", { name: "Script steps of #7" })).getAllByRole("listitem");
  expect(items[1]).toHaveTextContent("Waits for a repair");
  expect(items[1]).not.toHaveTextContent(/Ruled/);
});

test("a card with nothing to fold has no Expand all", () => {
  renderCard({ open: true, script: FULL });
  expect(screen.queryByRole("button", { name: /all steps/ })).not.toBeInTheDocument();
});

test("an invented value of a step the list lacks stays a fact of the script", () => {
  const script = {
    ...BARE,
    steps: [
      { step_number: 0, actions: [], because: [{ value: "USD", why: "free currency", provenance: "invented" }] },
      { step_number: 1, actions: [] },
    ],
  } as CaseScript;
  renderCard({ open: true, script });
  expect(screen.getByText("Invented values")).toBeInTheDocument();
  expect(screen.getByText("step 0: USD (free currency)")).toBeInTheDocument();
});

test("a ruling with no note reads as its proposed text, on its opened step", () => {
  renderCard({ open: true, divergences: [{ ...RULED, note: null } as Divergence] });
  openStep2();
  expect(screen.getByText("6 levels")).toHaveAttribute("title", "6 levels");
});

test("a step with a ruling shows the Ruled tag, and one without does not", () => {
  renderCard({ open: true, divergences: [RULED] });
  // The tag is on the folded row.
  const items = within(screen.getByRole("list", { name: "Script steps of #7" })).getAllByRole("listitem");
  expect(items[1]).toHaveTextContent("Ruled");
  expect(items[0]).not.toHaveTextContent("Ruled");
});

test("a divergence still open keeps its buttons and is no ruling row", () => {
  renderCard({ open: true, divergences: [DIV] });
  expect(screen.queryByText(/Rulings \(/)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "The app is wrong for #7 step 2" })).toBeInTheDocument();
  const items = within(screen.getByRole("list", { name: "Script steps of #7" })).getAllByRole("listitem");
  expect(items[1]).not.toHaveTextContent("Ruled");
});

test("a case with no script still shows a ruling and its Undo", () => {
  renderCard({ open: true, script: null, divergences: [RULED] });
  expect(screen.getByText("No script yet. An assistant writes it.")).toBeInTheDocument();
  expect(screen.getByText("Rulings (1)")).toBeInTheDocument();
  expect(screen.getByText("Four levels, as the case says")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Undo the ruling on #7 step 2" })).toBeInTheDocument();
});
