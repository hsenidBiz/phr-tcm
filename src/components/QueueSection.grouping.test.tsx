/**
 * The Queue's Group by area: a display-only switch that draws the queued
 * cases under nested area headings. The queue itself, its order and every
 * row's index are untouched - these tests drive the switch, the folds, the
 * group tick boxes and Shift-click ranges, and check the flat Queue is
 * exactly what it was while the switch is off.
 */
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
  tc("No area", ""),
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
    if (el.getAttribute("role") === "checkbox") continue;
    if (/^(Collapse|Expand) area /.test(label)) out.push(`# ${label.replace(/^(Collapse|Expand) area /, "")}`);
    const m = label.match(/^Collapse steps of (.*)$|^Expand steps of (.*)$/);
    if (m) out.push(m[1] ?? m[2]);
  }
  return out;
}

const groupSwitch = () => screen.getByRole("switch", { name: "Group by area" });
const box = (name: string) => screen.getByRole("checkbox", { name });

test("off by default: the Queue is flat, in queue order, with no group headings", () => {
  renderQueue();
  expect(groupSwitch()).toHaveAttribute("aria-checked", "false");
  expect(reading()).toEqual(QUEUE.map((c) => c.title));
  expect(screen.queryByRole("button", { name: /^(Collapse|Expand) area / })).toBeNull();
  expect(screen.queryByRole("checkbox", { name: /^Select all in / })).toBeNull();
});

test("a single case has nothing to group: no switch, unless grouping is already on", () => {
  const { unmount } = renderQueue([QUEUE[1]]);
  expect(screen.queryByRole("switch", { name: "Group by area" })).toBeNull();
  unmount();
  localStorage.setItem("tcm-v2-queue-group", "on");
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
  localStorage.setItem("tcm-v2-queue-group", "on");
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
    "No area",
  ]);
  // Each heading's count includes its nested cases.
  expect(screen.getByText("Zeta (2)")).toBeInTheDocument();
  expect(screen.getByText("Events (2)")).toBeInTheDocument();
  expect(screen.getByText("Create (2)")).toBeInTheDocument();
  expect(screen.getByText("Form (1)")).toBeInTheDocument();
  expect(screen.getByText("Ungrouped (1)")).toBeInTheDocument();
});

test("a folded group hides its cases and nested groups, and stays folded after a remount", () => {
  localStorage.setItem("tcm-v2-queue-group", "on");
  const { unmount } = renderQueue();
  fireEvent.click(screen.getByRole("button", { name: "Collapse area Events" }));
  expect(JSON.parse(localStorage.getItem("tcm-v2-queue-collapsed")!)).toEqual(["events"]);
  expect(reading()).toEqual(["# Zeta", "Zeta one", "Zeta two", "# Events", "# Alpha", "Alpha one", "# Ungrouped", "No area"]);
  unmount();
  renderQueue();
  expect(screen.getByRole("button", { name: "Expand area Events" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /steps of Form/ })).toBeNull();
});

test("a group's tick box selects and clears every case under it, nested included", () => {
  localStorage.setItem("tcm-v2-queue-group", "on");
  renderQueue();
  fireEvent.click(box("Select all in Events"));
  expect(box("Select Create")).toHaveAttribute("aria-checked", "true");
  expect(box("Select Form")).toHaveAttribute("aria-checked", "true");
  expect(box("Select Zeta one")).toHaveAttribute("aria-checked", "false");
  expect(screen.getByText("2 of 6 selected")).toBeInTheDocument();

  fireEvent.click(box("Select all in Events"));
  expect(box("Select Create")).toHaveAttribute("aria-checked", "false");
  expect(box("Select Form")).toHaveAttribute("aria-checked", "false");
});

test("a partly selected group shows the mixed state", () => {
  localStorage.setItem("tcm-v2-queue-group", "on");
  renderQueue();
  fireEvent.click(box("Select Form"));
  expect(box("Select all in Events")).toHaveAttribute("aria-checked", "mixed");
  expect(box("Select all in Events / Create / Form")).toHaveAttribute("aria-checked", "true");
});

test("grouped, a Shift-click range follows what is on screen, skipping folded groups", async () => {
  localStorage.setItem("tcm-v2-queue-group", "on");
  renderQueue();
  fireEvent.click(screen.getByRole("button", { name: "Collapse area Events / Create / Form" }));
  // On screen: Zeta one, Zeta two, Create, (Form folded), Alpha one, No area.
  await waitFor(() => expect(screen.queryByRole("button", { name: /steps of Form/ })).toBeNull());
  fireEvent.click(box("Select Zeta two"));
  fireEvent.click(box("Select Alpha one"), { shiftKey: true });
  await waitFor(() => expect(screen.getByText("3 of 6 selected")).toBeInTheDocument());
  for (const t of ["Zeta two", "Create", "Alpha one"]) expect(box(`Select ${t}`)).toHaveAttribute("aria-checked", "true");
  for (const t of ["Zeta one", "No area"]) expect(box(`Select ${t}`)).toHaveAttribute("aria-checked", "false");
  // Unfolding shows Form was not swept up by a range it was not visible in.
  fireEvent.click(screen.getByRole("button", { name: "Expand area Events / Create / Form" }));
  expect(box("Select Form")).toHaveAttribute("aria-checked", "false");
});

test("flat, a Shift-click range still runs over queue positions", () => {
  renderQueue();
  fireEvent.click(box("Select Zeta one"));
  fireEvent.click(box("Select Alpha one"), { shiftKey: true });
  expect(screen.getByText("3 of 6 selected")).toBeInTheDocument();
  for (const t of ["Zeta one", "Form", "Alpha one"]) expect(box(`Select ${t}`)).toHaveAttribute("aria-checked", "true");
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
