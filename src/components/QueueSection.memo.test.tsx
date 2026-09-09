import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { memo, useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import type { TestCase } from "../bindings";
import QueueSection from "./QueueSection";

/** Every row render goes through this counter. The real row is rendered
 * underneath, so the assertions below are about WHEN rows render, not
 * what they show. */
let rowRenders = 0;
vi.mock("./QueueRow", async (importOriginal) => {
  const mod = await importOriginal<typeof import("./QueueRow")>();
  const Counting = memo((props: import("./QueueRow").QueueRowProps) => {
    rowRenders++;
    return mod.QueueRowInner(props);
  });
  return { ...mod, default: Counting };
});
vi.mock("../hooks/useOnScreen", () => ({ useOnScreen: () => [() => {}, true] }));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  rowRenders = 0;
});

function makeCase(n: number): TestCase {
  return {
    title: `Case ${n}`,
    steps: [{ action: "Open page", expected: "Page shown" }],
    tags: "smoke",
    automation_status: "Not Automated",
    module_value: "",
    preconditions: "",
    update_id: null,
    spec_order: null,
    tester_order: null,
  };
}

/** Owns the queue like ManualEntry / ImportFile, plus a knob that
 * re-renders the owner WITHOUT touching the queue - the tab-switch and
 * progress-tick situation the memoisation exists for. */
function Harness({ initial }: { initial: TestCase[] }) {
  const [queue, setQueue] = useState<TestCase[]>(initial);
  const [tick, setTick] = useState(0);
  return (
    <>
      <button onClick={() => setTick((t) => t + 1)}>tick {tick}</button>
      <QueueSection org="acme" project="Web" pbiId={42} queue={queue} setQueue={setQueue} />
    </>
  );
}

function renderQueue(initial: TestCase[]) {
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Harness initial={initial} />
    </QueryClientProvider>,
  );
}

/** Six full renders of QueueSection happened on every tab switch, and each
 * rebuilt every row - 104 rows made switching to or from Import File take
 * a quarter of a second. Rows must render once on mount and then only
 * when their own inputs change. */
test("a re-render of the queue's owner does not re-render the rows", async () => {
  renderQueue([makeCase(1), makeCase(2), makeCase(3)]);
  await screen.findByText("Case 3");
  // Let the mount-time effects and queries settle - those renders are
  // the ones that used to cost the most, and they must not reach the rows.
  await new Promise((r) => setTimeout(r, 50));
  const afterMount = rowRenders;
  expect(afterMount).toBeGreaterThanOrEqual(3);

  fireEvent.click(screen.getByText(/^tick/));
  fireEvent.click(screen.getByText(/^tick/));
  expect(rowRenders).toBe(afterMount);
});

test("expanding one row's steps re-renders that row only", async () => {
  renderQueue([makeCase(1), makeCase(2), makeCase(3)]);
  await screen.findByText("Case 3");
  await new Promise((r) => setTimeout(r, 50));
  const before = rowRenders;

  fireEvent.click(screen.getByRole("button", { name: "Expand steps of Case 2" }));
  expect(await screen.findByRole("button", { name: "Collapse steps of Case 2" })).toBeInTheDocument();
  expect(rowRenders).toBe(before + 1);
});

test("selecting a row re-renders that row only", async () => {
  renderQueue([makeCase(1), makeCase(2), makeCase(3)]);
  await screen.findByText("Case 3");
  await new Promise((r) => setTimeout(r, 50));
  const before = rowRenders;

  fireEvent.click(screen.getByRole("checkbox", { name: "Select Case 1" }));
  expect(screen.getByRole("checkbox", { name: "Select Case 1" })).toHaveAttribute("aria-checked", "true");
  expect(rowRenders).toBe(before + 1);
});
