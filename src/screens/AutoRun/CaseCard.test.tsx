// One Auto Run case card on its own: a single line while collapsed, and
// the script's summary, steps and files once open. Which buttons show on a
// collapsed card, and which sections an open one leaves out, are what is
// pinned here; how the card looks is not something jsdom can see.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { CaseScript } from "../../bindings";
import CaseCard, { scriptFacts, scriptFiles } from "./CaseCard";
import { matchesSearch } from "./CaseSearch";

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

function renderCard(props: Partial<Parameters<typeof CaseCard>[0]> = {}) {
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
  for (const name of ["Edit script for #7", "Add script for #7", "Run #7"]) {
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
});

test("an open card shows the Script, Steps and Files sections, then Script and Run", () => {
  mockIPC(() => null);
  const { onEdit, onRun } = renderCard({ open: true });
  expect(screen.getByRole("button", { name: "Hide details for #7" })).toHaveAttribute("aria-expanded", "true");

  // Script: every fact this script has.
  expect(screen.getByText("2 steps")).toBeInTheDocument();
  expect(screen.getByText("emp1")).toBeInTheDocument();
  expect(screen.getByText("Leave requests")).toBeInTheDocument();
  expect(screen.getByText("Yes, saves are stopped while it runs")).toBeInTheDocument();
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

  fireEvent.click(screen.getByRole("button", { name: "Edit script for #7" }));
  fireEvent.click(screen.getByRole("button", { name: "Run #7" }));
  expect(onEdit).toHaveBeenCalledTimes(1);
  expect(onRun).toHaveBeenCalledTimes(1);
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
  expect(screen.getByText("1 step")).toBeInTheDocument();
  for (const label of [
    "Runs as", "Area", "Must not save", "Page errors", "Ignored page errors", "Unexpected dialogs",
    "Preconditions", "Changes", "Needs unchanged", "Last repair",
  ]) {
    expect(screen.queryByText(label)).not.toBeInTheDocument();
  }
  expect(screen.queryByText("Files")).not.toBeInTheDocument();
  expect(screen.getByText("Steps")).toBeInTheDocument();
});

test("an open card with no script says so and offers Add script, with no Script or Steps sections", () => {
  const { onEdit } = renderCard({ open: true, script: null });
  expect(screen.getByText("No script yet")).toBeInTheDocument();
  expect(screen.queryByText("Steps")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Run #7" })).not.toBeInTheDocument();
  expect(screen.getByRole("checkbox", { name: "Select #7" })).toHaveClass("invisible");
  fireEvent.click(screen.getByRole("button", { name: "Add script for #7" }));
  expect(onEdit).toHaveBeenCalledTimes(1);
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

test("scriptFacts and scriptFiles skip what is not there", () => {
  expect(scriptFacts(BARE)).toEqual([{ label: "Step count", value: "1 step" }]);
  expect(scriptFiles(BARE)).toEqual({ uploads: [], downloads: [] });
  expect(scriptFiles(FULL)).toEqual({ uploads: ["note.pdf"], downloads: ["leave-*.xlsx"] });
  // Blank text is no value either.
  expect(scriptFacts({ ...BARE, account: "  ", changes: [] } as CaseScript)).toHaveLength(1);
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

/** A setup view as the editor's command answers it. */
const view = (approval: string) => ({
  fixture_name: "Draft cycle",
  account: "hr.admin",
  steps: ["Create cycle: name=A"],
  creates: ["cycle A"],
  approval,
  approved_at: approval === "approved" ? "2026-10-06 09:00:00" : null,
  fingerprint: "f1",
});

test("a collapsed card with a setup does not read its approval", async () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    return cmd === "auto_run_setup_view" ? view("changed") : null;
  });
  const withSetup = { ...BARE, setup: { fixture: "Draft cycle" } } as CaseScript;
  renderCard({ open: false, script: withSetup });
  // A collapsed card reads nothing.
  expect(calls).not.toContain("auto_run_setup_view");
});

test("the setup line follows the approval: approved, not approved, changed since approved", async () => {
  const withSetup = { ...BARE, setup: { fixture: "Draft cycle" } } as CaseScript;
  for (const [approval, words] of [
    ["approved", "approved"],
    ["none", "not approved"],
    ["changed", "changed since approved"],
  ] as const) {
    mockIPC((cmd) => (cmd === "auto_run_setup_view" ? view(approval) : null));
    const { unmount } = renderCard({ open: true, script: withSetup });
    await waitFor(() => expect(screen.getByText("Setup:", { exact: false }).closest("p")).toHaveTextContent(`Setup: Draft cycle ${words}`));
    unmount();
  }
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
