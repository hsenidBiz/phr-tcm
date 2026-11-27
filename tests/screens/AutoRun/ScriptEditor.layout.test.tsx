// The script window's layout: the settings row, each case step beside its
// actions, the summary line and the fold, and "Before it runs".

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ScriptEditor from "../../../src/screens/AutoRun/ScriptEditor";

vi.mock("../../../src/lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
  localStorage.clear();
});

const ACCOUNTS = [{ key: "hr.admin", label: "HR Admin", username: "kim", password: "p" }];

type CaseStep = { action: string; expected: string; shared?: number | null };

function mount(script: unknown, steps: CaseStep[], saved: unknown[] = []) {
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_load_script") return script;
    if (cmd === "auto_run_list_accounts") return ACCOUNTS;
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_save_script") {
      saved.push((args as { script: unknown }).script);
      return null;
    }
    return null;
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ScriptEditor caseId={7} title="Leave request" steps={steps} org="acme" project="Web" onClose={vi.fn()} />
    </QueryClientProvider>,
  );
}

/** A case step that a script step checks. */
const checked = (n: number) => ({ step_number: n, actions: [{ kind: "expect_visible", selector: "#ok" }] });
const caseSteps = (n: number): CaseStep[] =>
  Array.from({ length: n }, (_, i) => ({ action: `Do thing ${i + 1}`, expected: `Thing ${i + 1} is done` }));

test("the case id and title lead, with Edit script on the same line", async () => {
  mount({ case_id: 7, title: "t", steps: [checked(1)] }, caseSteps(1));
  const edit = await screen.findByRole("button", { name: "Edit script" });
  const heading = screen.getByRole("heading", { level: 2 });
  expect(heading).toHaveTextContent("#7 Leave request");
  expect(heading.parentElement).toContainElement(edit);
});

test("Runs as, Area and Must not save sit in one row, and the long note is a tooltip", async () => {
  mount({ case_id: 7, title: "t", steps: [checked(1)] }, caseSteps(1));
  const runsAs = await screen.findByRole("combobox", { name: "Runs as" });
  const area = screen.getByRole("combobox", { name: "Area" });
  const box = screen.getByRole("checkbox", { name: "Must not save" });
  const row = runsAs.closest("div.grid");
  expect(row).not.toBeNull();
  expect(row).toContainElement(area);
  expect(row).toContainElement(box);
  // The explanation is unchanged, now behind an (i): a title and an accessible description.
  // A button, so the keyboard reaches it too.
  const info = screen.getByRole("button", { name: "About Must not save" });
  const note = "any save the page tries to send is stopped before it reaches the server";
  expect(info.getAttribute("title")).toContain(note);
  const describedBy = info.getAttribute("aria-describedby") ?? "";
  expect(document.getElementById(describedBy)?.textContent).toContain(note);
});

test("each case step renders beside its actions, and its not-checked parts under them", async () => {
  const script = {
    case_id: 7,
    title: "t",
    steps: [
      checked(1),
      {
        step_number: 2,
        actions: [{ kind: "click", selector: "#open" }],
        unchecked: [
          { item: "the PDF text", why: "not in the accessibility tree" },
          { item: "the page count", why: "no locator" },
        ],
      },
    ],
  };
  mount(script, caseSteps(2));
  const one = await screen.findByRole("region", { name: "Step 1" });
  // One row: the step's words and expected result, and its action, together.
  expect(one).toHaveTextContent("Do thing 1");
  expect(one).toHaveTextContent("Thing 1 is done");
  expect(one).toHaveTextContent("#ok");
  const two = screen.getByRole("region", { name: "Step 2" });
  expect(two).toHaveTextContent("Do thing 2");
  expect(within(two).getAllByText("Not checked")).toHaveLength(2);
  expect(two).toHaveTextContent("the PDF text - not in the accessibility tree");
  expect(two).toHaveTextContent("the page count - no locator");
  // The rows are an ordered list.
  const ol = one.closest("ol");
  expect(ol).not.toBeNull();
  expect(within(ol as HTMLElement).getAllByRole("listitem").length).toBeGreaterThanOrEqual(2);
});

test("the Steps header sums up the checks, and the Checks block is gone when every step is checked", async () => {
  const script = {
    case_id: 7,
    title: "t",
    steps: [checked(1), { ...checked(2), unchecked: [{ item: "the footer", why: "decorative" }] }, checked(3)],
  };
  mount(script, caseSteps(3));
  await screen.findByRole("region", { name: "Step 1" });
  const view = screen.getByRole("region", { name: "Action script" });
  expect(within(view).getByRole("heading", { name: "Steps" }).parentElement).toHaveTextContent(
    "Steps · all 3 checked · 1 part not checked",
  );
  expect(screen.queryByText("Checks")).not.toBeInTheDocument();
  expect(screen.queryByText("Step 1: checked")).not.toBeInTheDocument();
});

test("with some steps not checked the header counts them and the Checks block stays", async () => {
  const script = {
    case_id: 7,
    title: "t",
    steps: [checked(1), { step_number: 2, actions: [{ kind: "click", selector: "#open" }] }],
  };
  mount(script, caseSteps(2));
  await screen.findByRole("region", { name: "Step 1" });
  const view = screen.getByRole("region", { name: "Action script" });
  expect(within(view).getByRole("heading", { name: "Steps" }).parentElement).toHaveTextContent(
    "Steps · 1 of 2 checked",
  );
  expect(screen.getByText("Checks")).toBeInTheDocument();
  expect(screen.getByText("Step 2: NOT CHECKED")).toBeInTheDocument();
});

test("past three steps the rest fold behind N more steps, and a step needing a look is never folded", async () => {
  // Six steps, all checked but step 5, which has no check: it must show.
  const script = {
    case_id: 7,
    title: "t",
    steps: [checked(1), checked(2), checked(3), checked(4), { step_number: 5, actions: [{ kind: "reload" }] }, checked(6)],
  };
  mount(script, caseSteps(6));
  await screen.findByRole("region", { name: "Step 1" });
  expect(screen.getByRole("region", { name: "Step 3" })).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Step 4" })).not.toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Step 5" })).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Step 6" })).not.toBeInTheDocument();
  // The fold names only the steps it holds: 5 is showing, so it is not listed.
  const fold = screen.getByRole("button", { name: "2 more steps: 4, 6" });
  expect(fold).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(fold);
  expect(fold).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByRole("region", { name: "Step 4" })).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Step 6" })).toBeInTheDocument();
  fireEvent.click(fold);
  expect(screen.queryByRole("region", { name: "Step 6" })).not.toBeInTheDocument();
});

test("three steps or fewer have no fold", async () => {
  mount({ case_id: 7, title: "t", steps: [checked(1), checked(2), checked(3)] }, caseSteps(3));
  await screen.findByRole("region", { name: "Step 3" });
  expect(screen.queryByRole("button", { name: /more steps?:/ })).not.toBeInTheDocument();
});

test("while the script is edited every case step shows, none folded", async () => {
  const steps = [1, 2, 3, 4, 5].map(checked);
  mount({ case_id: 7, title: "t", steps }, caseSteps(5));
  fireEvent.click(await screen.findByRole("button", { name: "Edit script" }));
  await screen.findByRole("textbox", { name: "Action script JSON" });
  for (let n = 1; n <= 5; n++) expect(screen.getByText(`Do thing ${n}`)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /more steps?:/ })).not.toBeInTheDocument();
});

test("Before it runs shows chips, and a + button opens its box and adds through the same save", async () => {
  const saved: unknown[] = [];
  mount(
    { case_id: 7, title: "t", steps: [checked(1)], needs_unchanged: ["00002773 90-degree goal plan not submitted"] },
    caseSteps(1),
    saved,
  );
  const chips = await screen.findByRole("list", { name: "Before it runs" });
  expect(chips).toHaveTextContent("Needs unchanged: 00002773 90-degree goal plan not submitted");
  expect(screen.getByRole("button", { name: "Remove needs unchanged 00002773 90-degree goal plan not submitted" })).toBeInTheDocument();
  // Nothing empty shows until a + button is pressed.
  expect(screen.queryByRole("textbox", { name: "Setting" })).not.toBeInTheDocument();
  const plus = screen.getByRole("button", { name: "Add a setting" });
  expect(plus).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(plus);
  expect(plus).toHaveAttribute("aria-expanded", "true");
  const name = screen.getByRole("textbox", { name: "Setting" });
  expect(name).toHaveFocus();
  fireEvent.change(name, { target: { value: "Rating scale" } });
  fireEvent.change(screen.getByRole("textbox", { name: "Value" }), { target: { value: "5 point" } });
  fireEvent.click(screen.getByRole("button", { name: "Add setting" }));
  expect(chips).toHaveTextContent("Setting: Rating scale = 5 point");
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));
  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0]).toEqual(expect.objectContaining({ needs_state: { "Rating scale": "5 point" } }));
});

test("with no entries, Before it runs shows only the three + buttons", async () => {
  mount({ case_id: 7, title: "t", steps: [checked(1)] }, caseSteps(1));
  await screen.findByRole("button", { name: "Save script" });
  expect(screen.queryByRole("list", { name: "Before it runs" })).not.toBeInTheDocument();
  for (const name of ["Add a change", "Add a needs-unchanged record", "Add a setting"]) {
    expect(screen.getByRole("button", { name })).toBeInTheDocument();
  }
  expect(screen.queryByRole("textbox", { name: "Add to Changes" })).not.toBeInTheDocument();
});

test("editing keeps the Steps summary line, which follows the JSON as it is typed", async () => {
  mount({ case_id: 7, title: "t", steps: [{ step_number: 1, actions: [{ kind: "reload" }] }] }, caseSteps(1));
  fireEvent.click(await screen.findByRole("button", { name: "Edit script" }));
  const box = await screen.findByRole("textbox", { name: "Action script JSON" });
  expect(screen.getByText("Steps", { selector: "span" }).parentElement).toHaveTextContent("Steps · 0 of 1 checked");
  fireEvent.change(box, { target: { value: JSON.stringify([checked(1)]) } });
  expect(screen.getByText("Steps", { selector: "span" }).parentElement).toHaveTextContent("Steps · all 1 checked");
  // All checked: no Checks block in the editor either.
  expect(screen.queryByText("Checks")).not.toBeInTheDocument();
});

test("the unchecked-parts count leaves out the clean-up, like the expectations shown elsewhere", async () => {
  const script = {
    case_id: 7,
    title: "t",
    steps: [
      { ...checked(1), unchecked: [{ item: "the footer", why: "decorative" }] },
      { step_number: 2, actions: [{ kind: "click", selector: "#x" }], unchecked: "removes the record", cleanup: true },
    ],
  };
  mount(script, caseSteps(1));
  await screen.findByRole("region", { name: "Step 1" });
  expect(screen.getByRole("heading", { name: "Steps" }).parentElement).toHaveTextContent("Steps · all 1 checked · 1 part not checked");
});

test("a step with an open divergence is never folded", async () => {
  mockIPC((cmd) => {
    if (cmd === "auto_run_load_script")
      return { case_id: 7, title: "t", steps: [1, 2, 3, 4, 5].map(checked) };
    if (cmd === "auto_run_list_accounts") return ACCOUNTS;
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_load_divergences")
      return [
        { id: "d1", case_id: 7, step: 5, case_says: "a", app_shows: "b", kind: "data", reported_at: "1", status: "open" },
        { id: "d2", case_id: 8, step: 4, case_says: "a", app_shows: "b", kind: "data", reported_at: "1", status: "open" },
      ];
    return null;
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ScriptEditor caseId={7} title="t" steps={caseSteps(5)} org="acme" project="Web" onClose={vi.fn()} />
    </QueryClientProvider>,
  );
  await screen.findByRole("region", { name: "Step 1" });
  await waitFor(() => expect(screen.getByRole("region", { name: "Step 5" })).toBeInTheDocument());
  expect(screen.queryByRole("region", { name: "Step 4" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "1 more step: 4" })).toBeInTheDocument();
});
