import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "../../lib/toast";
import type { SuiteCase } from "../../lib/suiteOrder";
import SuiteCases from "./SuiteCases";
import { caseRow } from "./testSupport";

vi.mock("../../lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));
const { openDialog } = vi.hoisted(() => ({ openDialog: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openDialog }));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

const point = (id: number, name: string, config = "Windows 10") => ({
  point_id: id * 10, test_case_id: id, test_case_name: name, config_name: config,
  tester: "", last_outcome: "none", last_run_id: null, last_result_id: null,
});

function Harness({ onToggle }: { onToggle?: (cases: SuiteCase[], on: boolean) => void }) {
  const [selected, setSelected] = useState<Set<number>>(new Set());
  return (
    <SuiteCases
      org="acme"
      project="Web"
      planId={9}
      suiteId={91}
      suiteName="Regression"
      selected={selected}
      onToggle={(cases, on) => {
        onToggle?.(cases, on);
        setSelected((s) => {
          const next = new Set(s);
          for (const c of cases) on ? next.add(c.id) : next.delete(c.id);
          return next;
        });
      }}
    />
  );
}

const DEFAULT_POINTS = [
  point(201, "Valid login"),
  point(201, "Valid login", "Windows 11"),
  point(202, "Bad password"),
  point(203, "Locked out"),
];

/** The suite's entries, built from whichever points a test supplies: one
 * "suite" header plus one "testCase" entry per distinct test_case_id, in
 * the order it first appears - so a test naming ids the default fixture
 * never used (as the A-Z groups test does) still gets a matching suite. */
function entriesFor(points: ReturnType<typeof point>[]) {
  const seen = new Set<number>();
  const ids: number[] = [];
  for (const p of points) {
    if (!seen.has(p.test_case_id)) {
      seen.add(p.test_case_id);
      ids.push(p.test_case_id);
    }
  }
  return [
    { id: 95, sequence_number: 0, entry_type: "suite" },
    ...ids.map((id, i) => ({ id, sequence_number: i + 1, entry_type: "testCase" })),
  ];
}

function mount(
  extra: (cmd: string, args: unknown) => unknown = () => undefined,
  onToggle?: (c: SuiteCase[], on: boolean) => void,
  points = DEFAULT_POINTS,
) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "list_suite_entries") return entriesFor(points);
    if (cmd === "list_test_points") return points;
    const answered = extra(cmd, args);
    if (answered !== undefined) return answered;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { unmount } = render(
    <QueryClientProvider client={qc}>
      <Harness onToggle={onToggle} />
    </QueryClientProvider>,
  );
  return { calls, unmount };
}

/// A hand-driven IntersectionObserver: jsdom's never reports anything, so
/// every element reads as on screen until a test says otherwise.
function stubScroll() {
  const real = globalThis.IntersectionObserver;
  const watched: { el: Element; cb: (e: { isIntersecting: boolean }[]) => void }[] = [];
  globalThis.IntersectionObserver = class {
    cb: (e: { isIntersecting: boolean }[]) => void;
    constructor(cb: (e: { isIntersecting: boolean }[]) => void) {
      this.cb = cb;
    }
    observe(el: Element) {
      watched.push({ el, cb: this.cb });
    }
    unobserve() {}
    disconnect() {
      for (let i = watched.length - 1; i >= 0; i--) if (watched[i].cb === this.cb) watched.splice(i, 1);
    }
    takeRecords() {
      return [];
    }
  } as unknown as typeof IntersectionObserver;
  const report = (match: (el: Element) => boolean, isIntersecting: boolean) =>
    act(() => {
      for (const w of watched.filter((x) => match(x.el))) w.cb([{ isIntersecting }]);
    });
  return {
    /** The suite's own order-actions row (the ActionDock's in-place row,
     * which ActionDock and this screen both watch) scrolls out of view, or
     * back into it. Matched by "Apply order" rather than the "Group by
     * title" switch: the view controls now sit outside the dock, so the
     * switch is no longer inside the watched element. */
    toolbar: (onScreen: boolean) =>
      report((el) => within(el as HTMLElement).queryByRole("button", { name: "Apply order" }) != null, onScreen),
    /** The suite's case list scrolls out of view, or back into it. */
    list: (l: HTMLElement, onScreen: boolean) => report((el) => el.contains(l), onScreen),
    restore: () => {
      globalThis.IntersectionObserver = real;
    },
  };
}

/** The floating copy is always in the DOM once mounted (fix round 1,
 * Important 2 - it is always `aria-hidden`, so `getByRole` can never find
 * it), and only visually hidden the rest of the time. "Showing" means
 * found by its `data-sticky-action` + `aria-label`, AND not carrying the
 * hidden state's `opacity-0`. */
const floatingBar = () => {
  const el = [...document.querySelectorAll("[data-sticky-action]")].find(
    (e) => e.getAttribute("aria-label") === "Order actions for Regression",
  ) as HTMLElement | undefined;
  return el && !el.className.includes("opacity-0") ? el : null;
};

async function list() {
  const l = await screen.findByRole("list", { name: "Test cases in Regression" });
  // Let the passive effects that follow the first paint (order mirror,
  // mutation option sync) settle before a caller fires an action.
  await new Promise((r) => setTimeout(r, 0));
  return l;
}

test("lists the suite's cases once each in entry order; nothing to apply at first", async () => {
  mount();
  const l = await list();
  const rows = within(l).getAllByRole("listitem");
  expect(rows).toHaveLength(3);
  expect(rows[0]).toHaveTextContent("#201");
  expect(rows[0]).toHaveTextContent("Valid login");
  expect(rows[2]).toHaveTextContent("#203");
  expect(within(l).queryByText(/#95/)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Apply order" })).toBeDisabled();
});

test("drag re-orders; Apply order sends the ids, disables at once, and reloads", async () => {
  const { calls } = mount((cmd) => {
    if (cmd === "reorder_suite_cases") return [203, 201, 202];
  });
  const l = await list();
  const rows = within(l).getAllByRole("listitem");
  fireEvent.dragStart(rows[2]);
  fireEvent.dragOver(rows[0]);
  fireEvent.drop(rows[0]);
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#203");
  // Dirty now, but the toolbar is on screen, so its button is the only one.
  const apply = screen.getByRole("button", { name: "Apply order" });
  expect(apply).toBeEnabled();
  fireEvent.click(apply);
  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "reorder_suite_cases");
    expect(call?.args).toEqual({ organization: "acme", project: "Web", suiteId: 91, caseIds: [203, 201, 202] });
  });
  await waitFor(() => expect(apply).toBeDisabled());
  await waitFor(() => expect(calls.filter((c) => c.cmd === "list_suite_entries").length).toBeGreaterThan(1));
  expect(toast.success).toHaveBeenCalledWith("Order saved.");
});

test("Move up and down step a row; Reset restores the server order", async () => {
  mount();
  const l = await list();
  fireEvent.click(within(l).getByRole("button", { name: "Move #202 up" }));
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#202");
  fireEvent.click(within(l).getByRole("button", { name: "Move #202 down" }));
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#201");
  fireEvent.click(within(l).getByRole("button", { name: "Move #201 down" }));
  expect(screen.getByRole("button", { name: "Apply order" })).toBeEnabled();
  fireEvent.click(screen.getByRole("button", { name: "Reset" }));
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#201");
  expect(screen.getByRole("button", { name: "Apply order" })).toBeDisabled();
});

/// Tick several rows and the arrows move them together, keeping their
/// order - the keyboard's version of dragging the block.
test("Move up on a ticked row moves the whole selection as a block", async () => {
  mount();
  const l = await list();
  fireEvent.click(caseRow(l, 202), { ctrlKey: true });
  fireEvent.click(caseRow(l, 203), { ctrlKey: true });
  fireEvent.click(within(l).getByRole("button", { name: "Move #203 up" }));
  const rows = () => within(l).getAllByRole("listitem").map((r) => r.textContent?.match(/#\d+/)?.[0]);
  expect(rows()).toEqual(["#202", "#203", "#201"]);
  // An unticked row still moves alone.
  fireEvent.click(within(l).getByRole("button", { name: "Move #201 up" }));
  expect(rows()).toEqual(["#202", "#201", "#203"]);
});

/// The same through the mouse: dragging any ticked row carries the block.
test("dragging a ticked row drops the whole selection at the target", async () => {
  mount();
  const l = await list();
  fireEvent.click(caseRow(l, 201), { ctrlKey: true });
  fireEvent.click(caseRow(l, 202), { ctrlKey: true });
  const [r201, , r203] = within(l).getAllByRole("listitem");
  fireEvent.dragStart(r201);
  fireEvent.dragOver(r203);
  fireEvent.drop(r203);
  expect(within(l).getAllByRole("listitem").map((r) => r.textContent?.match(/#\d+/)?.[0])).toEqual(["#203", "#201", "#202"]);
});

/// Field report: with an unsaved order, the floating bar showed alongside
/// the suite's own toolbar - two Apply orders, two Resets. It stands in for
/// the toolbar, so it never shows while the toolbar is on screen.
test("an unsaved order does not float a second bar while the toolbar is on screen", async () => {
  const scroll = stubScroll();
  try {
    mount();
    const l = await list();
    fireEvent.click(within(l).getByRole("button", { name: "Move #202 up" }));
    expect(screen.getAllByRole("button", { name: "Apply order" })).toHaveLength(1);
    expect(floatingBar()).not.toBeInTheDocument();

    // Scroll past the toolbar: the bar takes over, naming its suite.
    scroll.toolbar(false);
    const bar = await waitFor(() => {
      expect(floatingBar()).not.toBeNull();
      return floatingBar()!;
    });
    // Its buttons are always aria-hidden (fix round 1, Important 2), so
    // `hidden: true` reinstates them into the search - they are not
    // themselves aria-hidden, only their ancestor is.
    expect(within(bar).getByRole("button", { name: "Apply order", hidden: true })).toBeEnabled();
    expect(within(bar).getByRole("button", { name: "Reset", hidden: true })).toBeEnabled();

    // Past the whole suite: the unsaved order keeps it up.
    scroll.list(l, false);
    expect(floatingBar()).toBeInTheDocument();

    // Back up to the toolbar: one set of buttons again.
    scroll.toolbar(true);
    await waitFor(() => expect(floatingBar()).not.toBeInTheDocument());
    expect(screen.getAllByRole("button", { name: "Apply order" })).toHaveLength(1);

    // A clean suite scrolled right away has no bar at all.
    scroll.toolbar(false);
    await waitFor(() => expect(floatingBar()).not.toBeNull());
    fireEvent.click(within(floatingBar()!).getByRole("button", { name: "Reset", hidden: true }));
    await waitFor(() => expect(floatingBar()).not.toBeInTheDocument());
  } finally {
    scroll.restore();
  }
});

/// Several files, arranged in the dialog, each a block in the file's ROW
/// order - tester_order is ignored, it is what jumbled a suite once.
test("Apply order from files arranges the list by the files' row order and the dialog's file order", async () => {
  openDialog.mockResolvedValue(["C:\\drafts\\a.json", "C:\\drafts\\b.json"]);
  const base = { steps: [], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "" };
  mount((cmd, args) => {
    if (cmd === "parse_import_file") {
      const path = (args as { path: string }).path;
      return path.endsWith("a.json")
        ? { cases: [{ ...base, title: "c", update_id: 203, tester_order: 2 }, { ...base, title: "a", update_id: 201, tester_order: 1 }], warnings: [] }
        : { cases: [{ ...base, title: "b", update_id: 202, tester_order: 1 }], warnings: [] };
    }
  });
  const l = await list();
  fireEvent.click(screen.getByRole("button", { name: "Apply order from files" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getAllByRole("listitem")[0]).toHaveTextContent("a.json");
  // b.json's block first, then a.json's: 202, then 203 and 201 in a.json's ROW order.
  fireEvent.click(within(dialog).getByRole("button", { name: "Move b.json up" }));
  fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  expect(within(l).getAllByRole("listitem").map((r) => r.textContent?.match(/#\d+/)?.[0])).toEqual(["#202", "#203", "#201"]);
  expect(toast.info).toHaveBeenCalledWith("Placed 3 of 3 test cases from 2 files. Apply order to save.");
  expect(screen.getAllByRole("button", { name: "Apply order" })[0]).toBeEnabled();
});

test("cancelling the picker opens nothing, and a file naming none of the suite's cases is flagged in the dialog", async () => {
  openDialog.mockResolvedValueOnce(null).mockResolvedValueOnce(["C:\\drafts\\other.json"]);
  const { calls } = mount((cmd) => {
    if (cmd === "parse_import_file")
      return { cases: [{ title: "x", steps: [], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "", update_id: null, tester_order: 1 }], warnings: [] };
  });
  await list();
  fireEvent.click(screen.getByRole("button", { name: "Apply order from files" }));
  await waitFor(() => expect(openDialog).toHaveBeenCalledTimes(1));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(calls.filter((c) => c.cmd === "parse_import_file")).toHaveLength(0);

  fireEvent.click(screen.getByRole("button", { name: "Apply order from files" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText("places nothing from this suite")).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: "Apply" })).toBeDisabled();
});

test("selecting a row reports the case to the parent; deselecting reports it back", async () => {
  const toggles: Array<[number[], boolean]> = [];
  mount(undefined, (cases, on) => toggles.push([cases.map((c) => c.id), on]));
  const l = await list();
  fireEvent.click(caseRow(l, 202), { ctrlKey: true });
  expect(toggles[toggles.length - 1]).toEqual([[202], true]);
  // "Select all" is in CaseOrderList's header row, a sibling of the <ol>
  // rather than one of its <li>s, so it is outside `l`'s scope.
  fireEvent.click(screen.getByRole("button", { name: "Select all" }));
  expect(toggles[toggles.length - 1]).toEqual([[201, 203], true]);
  fireEvent.click(caseRow(l, 202), { ctrlKey: true });
  expect(toggles[toggles.length - 1]).toEqual([[202], false]);
});

test("a suite already read once paints from the cache instead of loading again", async () => {
  // First visit: the two reads happen and the rows appear.
  const first = mount();
  expect(await screen.findByText(/Case 201|#201/)).toBeInTheDocument();
  const readsFirst = first.calls.filter((c) => c.cmd === "list_suite_entries").length;
  expect(readsFirst).toBe(1);
  first.unmount();

  // Second visit, same suite: rows are there in the first paint, with no
  // "Loading test cases" and no fresh read.
  const second = mount();
  expect(screen.getByText(/Case 201|#201/)).toBeInTheDocument();
  expect(screen.queryByText("Loading test cases")).not.toBeInTheDocument();
  expect(second.calls.filter((c) => c.cmd === "list_suite_entries")).toHaveLength(0);
});

test("an empty suite says so", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_suite_entries") return [];
    if (cmd === "list_test_points") return [];
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Harness />
    </QueryClientProvider>,
  );
  expect(await screen.findByText("No test cases in this suite.")).toBeInTheDocument();
});

// "Filter Card" needs a second member: the app's existing title-grouping
// (src/lib/grouping.ts, shared with Run Tests / View Test Cases) only
// turns a delimiter prefix into a named group at 2+ cases - a lone case
// falls into "" (Ungrouped) same as it does everywhere else in the app.
const GROUPED_POINTS = [
  point(201, "Alerts | ring once"),
  point(202, "Filter Card | collapses"),
  point(203, "Alerts | ring twice"),
  point(204, "Filter Card | expands"),
];

/// Turning Group by title on arranges the list so each group is together
/// (in order of first appearance) and shows a header per group; that is a
/// real reorder, so Apply order lights up. Off hides the headers only.
test("Group by title arranges the list into sections and remembers the switch", async () => {
  mount(undefined, undefined, GROUPED_POINTS);
  const l = await list();
  const rows = () => within(l).getAllByRole("listitem").map((r) => r.textContent?.match(/#\d+/)?.[0]);
  expect(screen.getAllByRole("button", { name: "Apply order" })[0]).toBeDisabled();

  fireEvent.click(screen.getByRole("switch", { name: "Group by title" }));
  expect(rows()).toEqual(["#201", "#203", "#202", "#204"]);
  expect(within(l).getByText("Alerts")).toBeInTheDocument();
  expect(within(l).getByText("Filter Card")).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: "Apply order" })[0]).toBeEnabled();
  expect(localStorage.getItem("tcm-v2-group-manage")).toBe("on");

  fireEvent.click(screen.getByRole("switch", { name: "Group by title" }));
  expect(within(l).queryByText("Alerts")).not.toBeInTheDocument();
  expect(rows()).toEqual(["#201", "#203", "#202", "#204"]);
});

/// A suite opened with the switch already on is NOT rearranged - nothing
/// the user did not ask for may make the list dirty. Its headers show the
/// order as it is, split groups and all.
test("a remembered switch shows sections without reordering", async () => {
  localStorage.setItem("tcm-v2-group-manage", "on");
  mount(undefined, undefined, GROUPED_POINTS);
  const l = await list();
  expect(within(l).getAllByRole("listitem").map((r) => r.textContent?.match(/#\d+/)?.[0])).toEqual(["#201", "#202", "#203", "#204"]);
  expect(within(l).getAllByText("Alerts")).toHaveLength(2);
  expect(screen.getAllByRole("button", { name: "Apply order" })[0]).toBeDisabled();
});

test("A-Z groups sorts the groups by name; a header selects its cases and moves as a block", async () => {
  mount(undefined, undefined, [
    point(201, "Zeta | one"),
    point(202, "Alpha | one"),
    point(203, "Zeta | two"),
    point(204, "Alpha | two"),
  ]);
  const l = await list();
  const rows = () => within(l).getAllByRole("listitem").map((r) => r.textContent?.match(/#\d+/)?.[0]);
  fireEvent.click(screen.getByRole("switch", { name: "Group by title" }));
  expect(rows()).toEqual(["#201", "#203", "#202", "#204"]);
  fireEvent.click(screen.getByRole("button", { name: "A-Z groups" }));
  expect(rows()).toEqual(["#202", "#204", "#201", "#203"]);

  // Ctrl+click on a header selects its whole group.
  fireEvent.click(within(l).getByText("Zeta").closest("li")!, { ctrlKey: true });
  expect(caseRow(l, 201)).toHaveAttribute("data-selected", "true");
  expect(caseRow(l, 203)).toHaveAttribute("data-selected", "true");

  fireEvent.click(within(l).getByRole("button", { name: "Move group Zeta up" }));
  expect(rows()).toEqual(["#201", "#203", "#202", "#204"]);
});

/// A header is a section BOUNDARY, not a row: dropping onto it must land
/// the block before that section whichever direction the drag came from.
/// A drop from above used to land after the section's first case -
/// inside the section - because the header reused the row drop rule.
test("dropping a block onto a section header from above lands it before the section, not inside it", async () => {
  mount(undefined, undefined, [
    point(201, "Zeta | one"),
    point(202, "Alpha | one"),
    point(203, "Zeta | two"),
    point(204, "Alpha | two"),
  ]);
  const l = await list();
  const rows = () => within(l).getAllByRole("listitem").map((r) => r.textContent?.match(/#\d+/)?.[0]);
  fireEvent.click(screen.getByRole("switch", { name: "Group by title" }));
  expect(rows()).toEqual(["#201", "#203", "#202", "#204"]);

  // #201 sits ABOVE the Alpha section; dropping it on Alpha's header must
  // land it right before Alpha, not as a new first row inside it.
  const [r201] = within(l).getAllByRole("listitem");
  const alphaHeader = within(l).getByText("Alpha").closest("li")!;
  fireEvent.dragStart(r201);
  fireEvent.dragOver(alphaHeader);
  fireEvent.drop(alphaHeader);
  expect(rows()).toEqual(["#203", "#201", "#202", "#204"]);

  // Mirror, from BELOW: #204 dropped on the Zeta header lands ahead of
  // Zeta's cases (this direction already worked before the fix).
  const zetaHeader = within(l).getByText("Zeta").closest("li")!;
  const rowEls = within(l).getAllByRole("listitem");
  const last = rowEls[rowEls.length - 1];
  fireEvent.dragStart(last);
  fireEvent.dragOver(zetaHeader);
  fireEvent.drop(zetaHeader);
  expect(rows()).toEqual(["#204", "#203", "#201", "#202"]);
});

// ---- Selection by click, folding groups, the bar that stays put ---------

const selectedRows = (l: HTMLElement) =>
  within(l)
    .getAllByRole("listitem")
    .filter((r) => r.getAttribute("data-selected") === "true")
    .map((r) => r.textContent?.match(/#\d+/)?.[0]);

/// Same as View Test Cases and Update Test Cases: no checkboxes. A click
/// selects one row, Ctrl+click adds or removes, Shift+click takes a range.
test("rows select by click, Ctrl+click and Shift+click, with no checkboxes", async () => {
  mount();
  const l = await list();
  expect(screen.queryAllByRole("checkbox")).toHaveLength(0);

  fireEvent.click(caseRow(l, 201));
  expect(selectedRows(l)).toEqual(["#201"]);
  fireEvent.click(caseRow(l, 203));
  expect(selectedRows(l)).toEqual(["#203"]); // a plain click replaces
  fireEvent.click(caseRow(l, 201), { ctrlKey: true });
  expect(selectedRows(l)).toEqual(["#201", "#203"]); // Ctrl adds
  fireEvent.click(caseRow(l, 203), { ctrlKey: true });
  expect(selectedRows(l)).toEqual(["#201"]); // and removes

  // A range runs from the last row clicked without Shift.
  fireEvent.click(caseRow(l, 201));
  fireEvent.click(caseRow(l, 203), { shiftKey: true });
  expect(selectedRows(l)).toEqual(["#201", "#202", "#203"]);

  // The arrow buttons move rows; they do not change the selection.
  fireEvent.click(within(l).getByRole("button", { name: "Move #203 up" }));
  expect(selectedRows(l)).toEqual(["#201", "#202", "#203"]);

  fireEvent.click(screen.getByRole("button", { name: "Clear" }));
  expect(selectedRows(l)).toEqual([]);
});

/// Groups fold like every other grouped list in the app, and the fold is
/// remembered. A folded group still selects and moves as a block.
test("a group folds from its header, stays folded, and still selects as a block", async () => {
  localStorage.setItem("tcm-v2-group-manage", "on");
  mount(undefined, undefined, GROUPED_POINTS);
  const l = await list();
  const rows = () => within(l).queryAllByRole("listitem").map((r) => r.textContent?.match(/#\d+/)?.[0]);
  expect(rows()).toEqual(["#201", "#202", "#203", "#204"]);

  // Sections are runs in the CURRENT order - Alerts, Filter Card, Alerts,
  // Filter Card - and a fold goes by name, so both Alerts runs fold.
  fireEvent.click(within(l).getAllByRole("button", { name: "Collapse group Alerts" })[0]);
  expect(rows()).toEqual(["#202", "#204"]);
  expect(JSON.parse(localStorage.getItem("tcm-v2-manage-collapsed-groups") ?? "[]")).toEqual(["Alerts"]);

  // Ctrl+click on a folded header selects what is inside that run.
  const alertsHeader = within(l).getAllByText("Alerts")[0].closest("li")!;
  fireEvent.click(alertsHeader, { ctrlKey: true });
  expect(screen.getByText("1 of 4 selected")).toBeInTheDocument();

  // A plain click on the header unfolds it again.
  fireEvent.click(alertsHeader);
  expect(rows()).toEqual(["#201", "#202", "#203", "#204"]);
  expect(selectedRows(l)).toEqual(["#201"]);

  fireEvent.click(screen.getByRole("button", { name: "Collapse groups" }));
  expect(rows()).toEqual([]);
  fireEvent.click(screen.getByRole("button", { name: "Expand groups" }));
  expect(rows()).toEqual(["#201", "#202", "#203", "#204"]);
});

/// Field report: the bar scrolled away with the page. This screen renders
/// inside an animated wrapper whose transform makes `fixed` mean the scroll
/// region, so the bar has to live directly under <body>, like the Import
/// tab's floating Review button.
test("the floating order bar is pinned to the app window, not the page", async () => {
  const scroll = stubScroll();
  try {
    mount();
    await list();
    scroll.toolbar(false);
    const bar = await waitFor(() => {
      expect(floatingBar()).not.toBeNull();
      return floatingBar()!;
    });
    expect(bar.parentElement).toBe(document.body);
    expect(bar.className).toMatch(/\bfixed\b/);
    expect(bar.className).toMatch(/\bright-6\b/);
    // Readable over a scrolling list of cases (fix round 1, Important 1) -
    // the pill this bar wore before ActionDock existed.
    expect(bar.className).toContain("rounded-full");
    expect(bar.className).toContain("bg-surface");
  } finally {
    scroll.restore();
  }
});

/// Field request: the toolbar - Apply order, Reset and Apply order from
/// files - follows the user once they scroll past it, like the Import tab's
/// floating Review button, not only while the order is unsaved.
test("scrolling past the toolbar while the cases are in view floats all three actions", async () => {
  const scroll = stubScroll();
  try {
    mount();
    const l = await list();

    // Everything on screen: no floating copy.
    expect(floatingBar()).not.toBeInTheDocument();

    // The toolbar scrolls off the top while the cases are still in view.
    scroll.toolbar(false);
    const floating = await waitFor(() => {
      expect(floatingBar()).not.toBeNull();
      return floatingBar()!;
    });
    expect(within(floating).getByRole("button", { name: "Apply order", hidden: true })).toBeDisabled(); // nothing to save yet
    expect(within(floating).getByRole("button", { name: "Reset", hidden: true })).toBeDisabled();
    expect(within(floating).getByRole("button", { name: "Apply order from files", hidden: true })).toBeEnabled();

    // Scrolling back up to the toolbar sends it away again.
    scroll.toolbar(true);
    await waitFor(() => expect(floatingBar()).not.toBeInTheDocument());

    // Scrolled past the whole suite: nothing of this suite is on screen, so
    // a clean suite has no bar.
    scroll.toolbar(false);
    scroll.list(l, false);
    await waitFor(() => expect(floatingBar()).not.toBeInTheDocument());
  } finally {
    scroll.restore();
  }
});

/// The owner's standing rule: actions on the thing (the order) live in the
/// right-aligned dock; view controls (grouping) stay out of it, on the left.
test("the order actions sit in a right-aligned dock, separate from the view controls", async () => {
  localStorage.setItem("tcm-v2-group-manage", "on");
  mount();
  await list();

  const dock = screen.getByRole("button", { name: "Apply order" }).closest(".justify-end") as HTMLElement;
  expect(dock).not.toBeNull();
  expect(within(dock).getByRole("button", { name: "Reset" })).toBeInTheDocument();
  expect(within(dock).getByRole("button", { name: "Apply order from files" })).toBeInTheDocument();

  // The view controls - grouping, A-Z, expand/collapse - are outside it,
  // but still on the page.
  expect(within(dock).queryByRole("switch", { name: "Group by title" })).toBeNull();
  expect(within(dock).queryByRole("button", { name: "A-Z groups" })).toBeNull();
  expect(screen.getByRole("switch", { name: "Group by title" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "A-Z groups" })).toBeInTheDocument();
});

// ---- Run order (execution-order-modal design §6) ----

test("Suite Management shows only the Azure DevOps order editor and never reads the run order", async () => {
  const { calls } = mount();
  await list();
  expect(screen.queryByRole("heading", { name: "Suggested run order" })).not.toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "Order in Azure DevOps" })).not.toBeInTheDocument();
  expect(screen.queryByRole("list", { name: /^Suggested run order/ })).not.toBeInTheDocument();
  expect(calls.some((c) => c.cmd === "get_run_order")).toBe(false);
});
