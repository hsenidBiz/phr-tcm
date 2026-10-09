/**
 * The Queue's Group by area: a display-only switch that draws the queued
 * cases under nested area headings. The queue itself, its order and every
 * row's index are untouched - these tests drive the switch and the folds,
 * and check the flat Queue is exactly what it was while the switch is off.
 *
 * Also how Queue cases are SELECTED, grouped or not: by clicking the row
 * itself, the way Update Test Cases works - a click selects one case,
 * Ctrl/Cmd toggles, Shift takes the range on screen, a group heading
 * takes its whole group - with no tick boxes on the rows.
 */
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { TestCase } from "../bindings";
import QueueSection from "./QueueSection";

vi.mock("../hooks/useOnScreen", () => ({ useOnScreen: () => [() => {}, true] }));
vi.mock("../lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

let viewArgs: Record<string, unknown> | null = null;

beforeEach(() => {
  viewArgs = null;
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "view_draft_html") {
      viewArgs = args as Record<string, unknown>;
      return null;
    }
    if (cmd.startsWith("list_") || cmd.endsWith("_values") || cmd === "pbi_test_cases") return [];
    return undefined;
  });
});

afterEach(() => {
  cleanup();
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

function tc(title: string, area: string): TestCase {
  return {
    title,
    steps: [{ action: "Open page", expected: "Page shown" }],
    tags: "",
    automation_status: "Not Automated",
    module_value: "",
    preconditions: "",
    update_id: null,
    spec_order: null,
    tester_order: null,
    area,
  };
}

// Queue order on purpose not A-Z, with a case that has no area first and
// one nested three levels deep.
const QUEUE = [
  tc("Loose case", ""),
  tc("Zeta one", "Zeta"),
  tc("Form", "Events / Create / Form"),
  tc("Alpha one", "Alpha"),
  tc("Create", "events / Create"),
  tc("Zeta two", "zeta "),
];

function Harness({ initial }: { initial: TestCase[] }) {
  const [queue, setQueue] = useState(initial);
  return <QueueSection org="acme" project="Web" pbiId={42} queue={queue} setQueue={setQueue} />;
}

function renderQueue(initial: TestCase[] = QUEUE) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Harness initial={initial} />
    </QueryClientProvider>,
  );
}

/** Row titles and group headings, top to bottom, as the user reads them. */
function reading(): string[] {
  const out: string[] = [];
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_ELEMENT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const el = n as HTMLElement;
    const label = el.getAttribute("aria-label") ?? "";
    if (/^(Collapse|Expand) area /.test(label)) out.push(`# ${label.replace(/^(Collapse|Expand) area /, "")}`);
    if (el.getAttribute("role") === "row") out.push(label);
  }
  return out;
}

const groupSwitch = () => screen.getByRole("switch", { name: "Group by area" });
const row = (title: string) => screen.getByRole("row", { name: title });
/** The titles of the selected rows, in the order they are on screen. */
const picked = () =>
  screen
    .queryAllByRole("row")
    .filter((r) => r.getAttribute("aria-selected") === "true")
    .map((r) => r.getAttribute("aria-label"));
const grouped = () => localStorage.setItem("tcm-v2-queue-group", "on");

test("off by default: the Queue is flat, in queue order, with no group headings", () => {
  renderQueue();
  expect(groupSwitch()).toHaveAttribute("aria-checked", "false");
  expect(reading()).toEqual(QUEUE.map((c) => c.title));
  expect(screen.queryByRole("button", { name: /^(Collapse|Expand) area / })).toBeNull();
});

test("a single case has nothing to group: no switch, unless grouping is already on", () => {
  const { unmount } = renderQueue([QUEUE[1]]);
  expect(screen.queryByRole("switch", { name: "Group by area" })).toBeNull();
  unmount();
  grouped();
  renderQueue([QUEUE[1]]);
  expect(groupSwitch()).toHaveAttribute("aria-checked", "true");
});

test("the switch is remembered on this machine", () => {
  const { unmount } = renderQueue();
  fireEvent.click(groupSwitch());
  expect(localStorage.getItem("tcm-v2-queue-group")).toBe("on");
  unmount();
  renderQueue();
  expect(groupSwitch()).toHaveAttribute("aria-checked", "true");
  fireEvent.click(groupSwitch());
  expect(localStorage.getItem("tcm-v2-queue-group")).toBe("off");
});

test("grouped: nested headings in queue order, case and spaces folded, counts nested, Ungrouped last", () => {
  grouped();
  renderQueue();
  expect(reading()).toEqual([
    "# Zeta",
    "Zeta one",
    "Zeta two",
    "# Events",
    "# Events / Create",
    "Create",
    "# Events / Create / Form",
    "Form",
    "# Alpha",
    "Alpha one",
    "# Ungrouped",
    "Loose case",
  ]);
  // Each heading's count includes its nested cases.
  expect(screen.getByText("Zeta (2)")).toBeInTheDocument();
  expect(screen.getByText("Events (2)")).toBeInTheDocument();
  expect(screen.getByText("Create (2)")).toBeInTheDocument();
  expect(screen.getByText("Form (1)")).toBeInTheDocument();
  expect(screen.getByText("Ungrouped (1)")).toBeInTheDocument();
});

test("a folded group hides its cases and nested groups, and stays folded after a remount", () => {
  grouped();
  const { unmount } = renderQueue();
  fireEvent.click(screen.getByRole("button", { name: "Collapse area Events" }));
  expect(JSON.parse(localStorage.getItem("tcm-v2-queue-collapsed")!)).toEqual(["events"]);
  expect(reading()).toEqual(["# Zeta", "Zeta one", "Zeta two", "# Events", "# Alpha", "Alpha one", "# Ungrouped", "Loose case"]);
  unmount();
  renderQueue();
  expect(screen.getByRole("button", { name: "Expand area Events" })).toBeInTheDocument();
  expect(screen.queryByRole("row", { name: "Form" })).toBeNull();
});

test("no tick boxes on the rows or the group headings; the bar's Select all stays", () => {
  grouped();
  renderQueue();
  expect(screen.getAllByRole("checkbox")).toHaveLength(1);
  expect(screen.getByRole("checkbox", { name: "Select all queued cases" })).toBeInTheDocument();
  // The rows sit in grids that say more than one row can be selected.
  const grids = screen.getAllByRole("grid");
  expect(grids.length).toBeGreaterThan(0);
  for (const g of grids) expect(g).toHaveAttribute("aria-multiselectable", "true");
});

test("clicking_a_row_selects_only_it_and_again_clears", () => {
  renderQueue();
  fireEvent.click(row("Zeta one"));
  expect(picked()).toEqual(["Zeta one"]);
  expect(screen.getByText("1 of 6 selected")).toBeInTheDocument();
  // The same accent border and tint Update Test Cases gives a selected case.
  expect(row("Zeta one")).toHaveClass("border-accent", "bg-accent-soft");
  expect(row("Form")).not.toHaveClass("bg-accent-soft");

  fireEvent.click(row("Alpha one"));
  expect(picked()).toEqual(["Alpha one"]);
  fireEvent.click(row("Alpha one"));
  expect(picked()).toEqual([]);
  expect(screen.getByText("Select cases for bulk actions")).toBeInTheDocument();
});

test("ctrl_click_toggles_and_shift_click_selects_the_visible_range", async () => {
  grouped();
  renderQueue();
  fireEvent.click(screen.getByRole("button", { name: "Collapse area Events / Create / Form" }));
  // On screen: Zeta one, Zeta two, Create, (Form folded), Alpha one, Loose case.
  await waitFor(() => expect(screen.queryByRole("row", { name: "Form" })).toBeNull());

  fireEvent.click(row("Zeta two"));
  fireEvent.click(row("Alpha one"), { shiftKey: true });
  expect(picked()).toEqual(["Zeta two", "Create", "Alpha one"]);

  // Ctrl (or Cmd) takes one case out, and puts one in.
  fireEvent.click(row("Create"), { ctrlKey: true });
  expect(picked()).toEqual(["Zeta two", "Alpha one"]);
  fireEvent.click(row("Zeta one"), { metaKey: true });
  expect(picked()).toEqual(["Zeta one", "Zeta two", "Alpha one"]);

  // Ctrl+Shift ADDS the range from the last click to what is already
  // selected; Shift alone replaces it.
  fireEvent.click(row("Create"), { ctrlKey: true });
  fireEvent.click(row("Loose case"), { ctrlKey: true, shiftKey: true });
  expect(picked()).toEqual(["Zeta one", "Zeta two", "Create", "Alpha one", "Loose case"]);
  fireEvent.click(row("Alpha one"), { shiftKey: true });
  expect(picked()).toEqual(["Create", "Alpha one"]);

  // Unfolding shows Form was never swept up by a range it was hidden from.
  fireEvent.click(screen.getByRole("button", { name: "Expand area Events / Create / Form" }));
  expect(row("Form")).toHaveAttribute("aria-selected", "false");
});

test("flat, a Shift-click range runs over queue positions", () => {
  renderQueue();
  fireEvent.click(row("Zeta one"));
  fireEvent.click(row("Alpha one"), { shiftKey: true });
  expect(picked()).toEqual(["Zeta one", "Form", "Alpha one"]);
});

test("a Shift-click does not start a text selection across the rows", () => {
  renderQueue();
  // fireEvent returns false when a handler called preventDefault.
  expect(fireEvent.mouseDown(row("Alpha one"), { shiftKey: true })).toBe(false);
  expect(fireEvent.mouseDown(row("Alpha one"))).toBe(true);
});

test("clicking_a_rows_buttons_does_not_change_the_selection", async () => {
  renderQueue();
  fireEvent.click(row("Zeta one"));
  fireEvent.click(screen.getByRole("button", { name: "Expand steps of Alpha one" }));
  fireEvent.click(screen.getByRole("button", { name: "Collapse steps of Alpha one" }));
  fireEvent.click(screen.getByRole("button", { name: "Edit Alpha one" }), { shiftKey: true });
  expect(picked()).toEqual(["Zeta one"]);
  // Inside the open editor: its fields take clicks and keys for themselves,
  // and so does the editor's own background.
  const save = await screen.findByRole("button", { name: "Save to queue" });
  const field = within(row("Alpha one")).getAllByRole("textbox")[0];
  fireEvent.click(field);
  fireEvent.keyDown(field, { key: " " });
  fireEvent.click(save.parentElement!);
  expect(picked()).toEqual(["Zeta one"]);
  fireEvent.click(screen.getByRole("button", { name: "Close the editor for Alpha one" }));
  expect(picked()).toEqual(["Zeta one"]);
});

test("clicking_a_group_heading_selects_its_cases", () => {
  grouped();
  renderQueue();
  const events = screen.getByRole("button", { name: "Events (2)" });
  fireEvent.click(events);
  expect(picked()).toEqual(["Create", "Form"]);
  expect(events).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByText("2 of 6 selected")).toBeInTheDocument();

  // Partly selected says so; a click then takes the rest.
  fireEvent.click(row("Form"), { ctrlKey: true });
  expect(events).toHaveAttribute("aria-pressed", "mixed");
  fireEvent.click(events);
  expect(picked()).toEqual(["Create", "Form"]);
  // Again clears the group, and leaves other cases alone.
  fireEvent.click(row("Zeta one"), { ctrlKey: true });
  fireEvent.click(events);
  expect(picked()).toEqual(["Zeta one"]);

  // The chevron folds; it never selects.
  fireEvent.click(screen.getByRole("button", { name: "Collapse area Events" }));
  expect(picked()).toEqual(["Zeta one"]);
});

test("space_selects_the_focused_row", () => {
  renderQueue();
  const zeta = row("Zeta one");
  expect(zeta).toHaveAttribute("tabindex", "0");
  zeta.focus();
  fireEvent.keyDown(zeta, { key: " " });
  expect(picked()).toEqual(["Zeta one"]);
  fireEvent.keyDown(row("Alpha one"), { key: " ", shiftKey: true });
  expect(picked()).toEqual(["Zeta one", "Form", "Alpha one"]);
  fireEvent.keyDown(row("Form"), { key: " ", ctrlKey: true });
  expect(picked()).toEqual(["Zeta one", "Alpha one"]);
  fireEvent.keyDown(row("Create"), { key: "Enter" });
  expect(picked()).toEqual(["Create"]);
});

test("View in browser asks for the grouped page only while the Queue is grouped", async () => {
  renderQueue();
  fireEvent.click(screen.getByRole("button", { name: "View in browser" }));
  await waitFor(() => expect(viewArgs).not.toBeNull());
  expect(viewArgs!.grouped).toBe(false);

  viewArgs = null;
  fireEvent.click(groupSwitch());
  fireEvent.click(screen.getByRole("button", { name: "View in browser" }));
  await waitFor(() => expect(viewArgs).not.toBeNull());
  expect(viewArgs!.grouped).toBe(true);
});
