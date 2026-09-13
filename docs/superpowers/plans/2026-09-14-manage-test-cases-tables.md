# Manage Test Cases: Plan Tables Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the plan/suite dropdown picker on the Manage Test Cases screen with one table per test plan: every suite of the plan as a collapsible block with its test cases underneath. A PBI picked in the context bar preselects the plan that holds its suite (with a "Show all plans" switch); with no PBI, every plan is shown. Users select cases across a plan's suites, copy them into another suite of that plan, or create a new static suite (optionally with the selected cases in it). Per-suite re-ordering (drag and drop, arrow buttons, Apply order, Reset, "Apply tester order from file") stays. "Move to PBI" leaves this screen (it still exists on Update Test Cases).

**Architecture:** The screen (`index.tsx`) owns the plan list query and one selection (`{ planId, cases }`, cases from one plan at a time). `PlanTable.tsx` renders one plan: a header with the plan's copy-to-suite and new-suite controls, then a suite block per suite in tree order. `SuiteCases.tsx` is the expanded body of one suite: it owns that suite's cases query, working order, Apply/Reset/file actions, and renders the existing `CaseOrderList`. `NewFolderDialog.tsx` is renamed `NewSuiteDialog.tsx` with "test suite" wording. `SuitePicker.tsx` and its test are deleted. No Rust changes.

**Tech Stack:** React 19 + TypeScript, TanStack Query, vitest + Testing Library. Existing commands: `listPlansWithSuites`, `listSuiteEntries`, `listTestPoints`, `reorderSuiteCases`, `parseImportFile`, `createStaticSuite`, `addCasesToSuite`.

## Global Constraints

- Development builds only: `MANAGE_CASES_ENABLED` in `src/components/Sidebar.tsx` gates the section; nothing in this plan changes that.
- Run one build or test command at a time on this shared machine. Gates: `npx tsc --noEmit`, `npx vitest run` (repo root). No Rust changes, so `cargo test` is not needed.
- Colours via tokens only (`text-muted`, `text-faint`, `bg-surface`, `bg-surface-2`, `border-border`, `border-accent`, `bg-accent-soft`, `text-accent`, `text-warning`, `text-danger`); never a palette colour or hex.
- Button icons from `src/lib/actionIcons.ts` as `<IconX aria-hidden />`, no `size` prop. Icons outside buttons (chevrons, folder glyphs) may come from `lucide-react` with `size={14}` as `src/screens/Suites.tsx` does. Icon-only interactive elements carry `aria-label`. `src/ui-consistency.test.ts` enforces this and must not be weakened.
- Dropdowns are the themed `Select` from `src/components/ui/select.tsx` (`value`, `onChange` receiving `{ target: { value } }`, `<option>` children, `aria-label`; renders a `combobox` trigger and `option` roles while open). Never a raw `<select>`.
- No em dashes in any text a user reads.
- Every Azure DevOps write stays a copy or a re-order: adding cases to a suite never removes them anywhere. No DELETE.
- Commits use a Bash heredoc `git commit -q -F - <<'EOF' … EOF` ending with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Tests drive real behaviour through roles and accessible names, never mock internals.

---

## File map

| File | Responsibility |
| --- | --- |
| `src/screens/ManageCases/suiteCases.ts` (create) | `loadSuiteCases` (moved out of `index.tsx`) and the query key helper. |
| `src/screens/ManageCases/selection.ts` (create) + `.test.ts` | The one-plan selection model: `Selection`, `toggleSelection`, `selectedIdsIn`. |
| `src/screens/ManageCases/SuiteCases.tsx` (create) + `.test.tsx` | Expanded suite body: cases query, order, Apply / Reset / file, `CaseOrderList`. |
| `src/screens/ManageCases/PlanTable.tsx` (create) | One plan: header (copy to suite, new suite), suite blocks in tree order, expand state. |
| `src/screens/ManageCases/NewSuiteDialog.tsx` (rename from `NewFolderDialog.tsx`) | Same dialog, "test suite" wording. |
| `src/screens/ManageCases/index.tsx` (rewrite) | Plans query, PBI plan preselection, "Show all plans", selection state, one `PlanTable` per plan. |
| `src/screens/ManageCases/SuitePicker.tsx`, `SuitePicker.test.tsx` (delete) | Gone. |
| `src/screens/ManageCases/testSupport.tsx` (rewrite), `index.test.tsx` (rewrite), `actions.test.tsx` (rewrite), `folders.test.tsx` → `suites.test.tsx` (rewrite) | Tests follow the new screen. |
| `src/App.tsx` (modify) | Passes `pbi` to `ManageCases`. |
| `src/lib/actionIcons.ts` (modify) | `IconNewFolder` → also usable as the new-suite icon (keep the name; add `IconCopyToSuite` = `FolderInput`, replacing `IconAddToFolder`). |

---

### Task 1: Suite body component with its own query, order and actions

**Files:**
- Create: `src/screens/ManageCases/suiteCases.ts`
- Create: `src/screens/ManageCases/selection.ts`, `src/screens/ManageCases/selection.test.ts`
- Create: `src/screens/ManageCases/SuiteCases.tsx`, `src/screens/ManageCases/SuiteCases.test.tsx`

**Interfaces:**
- Consumes: `commands.listSuiteEntries`, `commands.listTestPoints`, `commands.reorderSuiteCases`, `commands.parseImportFile`; `CaseOrderList` (`cases`, `selected: Set<number>`, `onChange`, `onSelect: (next: Set<number>) => void`, `disabled`); `orderFromFile`, `sameOrder`, `SuiteCase` from `src/lib/suiteOrder`; `open` from `@tauri-apps/plugin-dialog`; `unwrap`, `unwrapStr` from `src/lib/ipc`.
- Produces:
  - `suiteCases.ts`: `loadSuiteCases(org, project, planId, suiteId): Promise<SuiteCase[]>`, `suiteCasesKey(org, project, planId, suiteId): readonly ["suite-cases", string, string, number, number]`.
  - `selection.ts`: `type Selection = { planId: number; cases: Map<number, SuiteCase> }`; `toggleSelection(sel: Selection | null, planId: number, cases: SuiteCase[], on: boolean): Selection | null`; `selectedIdsIn(sel: Selection | null, planId: number): Set<number>`.
  - `SuiteCases` default export, props `{ org: string; project: string; planId: number; suiteId: number; selected: Set<number>; onToggle: (cases: SuiteCase[], on: boolean) => void }`.

- [ ] **Step 1: Write the failing selection test**

Create `src/screens/ManageCases/selection.test.ts`:

```ts
import { describe, expect, test } from "vitest";
import { selectedIdsIn, toggleSelection } from "./selection";

const c = (id: number) => ({ id, title: `T${id}` });

describe("toggleSelection", () => {
  test("adds and removes cases within one plan", () => {
    let sel = toggleSelection(null, 9, [c(1), c(2)], true);
    expect(sel?.planId).toBe(9);
    expect([...sel!.cases.keys()]).toEqual([1, 2]);
    sel = toggleSelection(sel, 9, [c(1)], false);
    expect([...sel!.cases.keys()]).toEqual([2]);
  });
  test("selecting in another plan replaces the selection", () => {
    const sel = toggleSelection(toggleSelection(null, 9, [c(1)], true), 10, [c(5)], true);
    expect(sel?.planId).toBe(10);
    expect([...sel!.cases.keys()]).toEqual([5]);
  });
  test("removing the last case clears the selection", () => {
    const sel = toggleSelection(toggleSelection(null, 9, [c(1)], true), 9, [c(1)], false);
    expect(sel).toBeNull();
  });
  test("removing from another plan is a no-op", () => {
    const before = toggleSelection(null, 9, [c(1)], true);
    expect(toggleSelection(before, 10, [c(1)], false)).toBe(before);
  });
});

describe("selectedIdsIn", () => {
  test("is the plan's ids, or empty for another plan or no selection", () => {
    const sel = toggleSelection(null, 9, [c(1), c(2)], true);
    expect([...selectedIdsIn(sel, 9)]).toEqual([1, 2]);
    expect(selectedIdsIn(sel, 10).size).toBe(0);
    expect(selectedIdsIn(null, 9).size).toBe(0);
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

`npx vitest run src/screens/ManageCases/selection.test.ts`
Expected: FAIL, cannot resolve `./selection`.

- [ ] **Step 3: Write the selection model and the query helpers**

Create `src/screens/ManageCases/selection.ts`:

```ts
import type { SuiteCase } from "../../lib/suiteOrder";

/** The cases picked for a bulk action. One plan at a time: a copy or a
 * new suite happens inside a plan, so picking a case in another plan
 * starts over rather than building a selection nothing could act on. */
export type Selection = { planId: number; cases: Map<number, SuiteCase> };

export function toggleSelection(
  sel: Selection | null,
  planId: number,
  cases: SuiteCase[],
  on: boolean,
): Selection | null {
  if (on) {
    const base = sel && sel.planId === planId ? sel.cases : new Map<number, SuiteCase>();
    const next = new Map(base);
    for (const c of cases) next.set(c.id, c);
    return { planId, cases: next };
  }
  if (!sel || sel.planId !== planId) return sel;
  const next = new Map(sel.cases);
  for (const c of cases) next.delete(c.id);
  return next.size === 0 ? null : { planId, cases: next };
}

export function selectedIdsIn(sel: Selection | null, planId: number): Set<number> {
  return sel && sel.planId === planId ? new Set(sel.cases.keys()) : new Set();
}
```

Create `src/screens/ManageCases/suiteCases.ts`:

```ts
import { commands } from "../../bindings";
import { unwrap } from "../../lib/ipc";
import type { SuiteCase } from "../../lib/suiteOrder";

export const suiteCasesKey = (org: string, project: string, planId: number, suiteId: number) =>
  ["suite-cases", org, project, planId, suiteId] as const;

/** The suite's cases in Azure DevOps' own order. The entries carry the
 * order and the points carry the names; a case with several
 * configurations has several points and one row. */
export async function loadSuiteCases(
  org: string,
  project: string,
  planId: number,
  suiteId: number,
): Promise<SuiteCase[]> {
  const [entries, points] = await Promise.all([
    unwrap(commands.listSuiteEntries(org, project, suiteId)),
    unwrap(commands.listTestPoints(org, project, planId, suiteId)),
  ]);
  const names = new Map<number, string>();
  for (const p of points) {
    if (p.test_case_id != null && !names.has(p.test_case_id)) names.set(p.test_case_id, p.test_case_name);
  }
  return entries
    .filter((e) => e.entry_type === "testCase")
    .map((e) => ({ id: e.id, title: names.get(e.id) ?? `Test case ${e.id}` }));
}
```

Run `npx vitest run src/screens/ManageCases/selection.test.ts`. Expected: pass.

- [ ] **Step 4: Write the failing SuiteCases test**

Create `src/screens/ManageCases/SuiteCases.test.tsx`:

```tsx
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "sonner";
import type { SuiteCase } from "../../lib/suiteOrder";
import SuiteCases from "./SuiteCases";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));
const { openDialog } = vi.hoisted(() => ({ openDialog: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openDialog }));

afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

const ENTRIES = [
  { id: 95, sequence_number: 0, entry_type: "suite" },
  { id: 201, sequence_number: 1, entry_type: "testCase" },
  { id: 202, sequence_number: 2, entry_type: "testCase" },
  { id: 203, sequence_number: 3, entry_type: "testCase" },
];
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

function mount(extra: (cmd: string, args: unknown) => unknown = () => undefined, onToggle?: (c: SuiteCase[], on: boolean) => void) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "list_suite_entries") return ENTRIES;
    if (cmd === "list_test_points")
      return [point(201, "Valid login"), point(201, "Valid login", "Windows 11"), point(202, "Bad password"), point(203, "Locked out")];
    return extra(cmd, args);
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Harness onToggle={onToggle} />
    </QueryClientProvider>,
  );
  return { calls };
}

async function list() {
  const l = await screen.findByRole("list", { name: "Test cases in order" });
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

test("Apply tester order from file re-orders from the file's tester_order", async () => {
  openDialog.mockResolvedValue("C:\\drafts\\auth.json");
  const base = { steps: [], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "" };
  mount((cmd) => {
    if (cmd === "parse_import_file")
      return { cases: [{ ...base, title: "c", update_id: 203, tester_order: 1 }, { ...base, title: "a", update_id: 201, tester_order: 2 }], warnings: [] };
  });
  const l = await list();
  fireEvent.click(screen.getByRole("button", { name: "Apply tester order from file" }));
  await waitFor(() => expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#203"));
  expect(within(l).getAllByRole("listitem")[1]).toHaveTextContent("#201");
  expect(toast.info).toHaveBeenCalledWith("Placed 2 of 3 test cases from the file. Apply order to save.");
  expect(screen.getByRole("button", { name: "Apply order" })).toBeEnabled();
});

test("a file naming none of the suite's cases changes nothing; cancelling the picker does nothing", async () => {
  openDialog.mockResolvedValueOnce("C:\\drafts\\other.json").mockResolvedValueOnce(null);
  const { calls } = mount((cmd) => {
    if (cmd === "parse_import_file")
      return { cases: [{ title: "x", steps: [], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "", update_id: null, tester_order: 1 }], warnings: [] };
  });
  const l = await list();
  fireEvent.click(screen.getByRole("button", { name: "Apply tester order from file" }));
  await waitFor(() =>
    expect(toast.warning).toHaveBeenCalledWith("No test case in that file is in this suite. The file needs ids from an upload."),
  );
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#201");
  const parses = calls.filter((c) => c.cmd === "parse_import_file").length;
  fireEvent.click(screen.getByRole("button", { name: "Apply tester order from file" }));
  await waitFor(() => expect(openDialog).toHaveBeenCalledTimes(2));
  expect(calls.filter((c) => c.cmd === "parse_import_file").length).toBe(parses);
});

test("checking a row reports the case to the parent; unchecking reports it back", async () => {
  const toggles: Array<[number[], boolean]> = [];
  mount(undefined, (cases, on) => toggles.push([cases.map((c) => c.id), on]));
  const l = await list();
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #202" }));
  expect(toggles.at(-1)).toEqual([[202], true]);
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select all test cases" }));
  expect(toggles.at(-1)).toEqual([[201, 203], true]);
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #202" }));
  expect(toggles.at(-1)).toEqual([[202], false]);
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
```

- [ ] **Step 5: Run it to verify it fails**

`npx vitest run src/screens/ManageCases/SuiteCases.test.tsx`
Expected: FAIL, cannot resolve `./SuiteCases`.

- [ ] **Step 6: Write SuiteCases**

Create `src/screens/ManageCases/SuiteCases.tsx` (the order/Apply/Reset/file logic moves here from the old `index.tsx` unchanged in behaviour):

```tsx
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { open } from "@tauri-apps/plugin-dialog";
import { commands } from "../../bindings";
import ScanProgress from "../../components/ScanProgress";
import { Button } from "../../components/ui/button";
import { IconConfirm, IconImport, IconUndo } from "../../lib/actionIcons";
import { unwrap, unwrapStr } from "../../lib/ipc";
import { orderFromFile, sameOrder, type SuiteCase } from "../../lib/suiteOrder";
import CaseOrderList from "./CaseOrderList";
import { loadSuiteCases, suiteCasesKey } from "./suiteCases";

/** One expanded suite: its cases in Azure DevOps' order, re-orderable and
 * selectable. Order lives here (Apply saves it, Reset drops it); which
 * rows are selected lives in the screen, because a selection can span
 * the suites of a plan. */
export default function SuiteCases({
  org,
  project,
  planId,
  suiteId,
  selected,
  onToggle,
}: {
  org: string;
  project: string;
  planId: number;
  suiteId: number;
  /** Ids of this suite's cases the screen holds selected. */
  selected: Set<number>;
  /** Cases the user just checked (on) or unchecked (off). */
  onToggle: (cases: SuiteCase[], on: boolean) => void;
}) {
  const qc = useQueryClient();
  const key = suiteCasesKey(org, project, planId, suiteId);
  const cases = useQuery({
    queryKey: key,
    queryFn: () => loadSuiteCases(org, project, planId, suiteId),
    retry: false,
  });

  // The order on screen. Starts as the server's and drifts as the user
  // drags; Apply sends it, Reset throws it away. A fresh read replaces it.
  const [order, setOrder] = useState<SuiteCase[]>([]);
  useEffect(() => {
    if (cases.data) setOrder(cases.data);
  }, [cases.data]);
  const dirty = cases.data ? !sameOrder(order, cases.data) : false;

  const apply = useMutation({
    mutationFn: () => unwrap(commands.reorderSuiteCases(org, project, suiteId, order.map((c) => c.id))),
    onSuccess: (serverIds) => {
      toast.success("Order saved.");
      // Write the server's own order into the cache first so `dirty`
      // reads false at once, instead of Apply flipping back on for the
      // beat before the refetch lands.
      const byId = new Map(order.map((c) => [c.id, c]));
      const next = serverIds.map((id) => byId.get(id)).filter((c): c is SuiteCase => c != null);
      qc.setQueryData(key, next);
      qc.invalidateQueries({ queryKey: key });
    },
    onError: (e) => toast.error(`Could not save the order: ${e.message}`),
  });

  /** A draft .json carries each uploaded case's id and, after the
   * optimizer's grouping pass, its tester_order. The file only proposes:
   * the list re-orders on screen and Apply order is what saves it. */
  const fromFile = useMutation({
    mutationFn: async () => {
      const path = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "Test case files", extensions: ["json"] }],
      });
      if (typeof path !== "string") return null;
      const parsed = await unwrapStr(commands.parseImportFile(path));
      return orderFromFile(order, parsed.cases);
    },
    onSuccess: (result) => {
      if (!result) return;
      if (result.matched === 0) {
        toast.warning("No test case in that file is in this suite. The file needs ids from an upload.");
        return;
      }
      setOrder(result.order);
      toast.info(`Placed ${result.matched} of ${order.length} test cases from the file. Apply order to save.`);
    },
    onError: (e) => toast.error(`Could not read the file: ${e.message ?? e}`),
  });

  const busy = apply.isPending || fromFile.isPending;

  if (cases.isLoading) return <ScanProgress label="Loading test cases" className="my-2" />;
  if (cases.isError) return <p className="my-2 text-sm text-danger">{cases.error.message}</p>;
  if (!cases.data || cases.data.length === 0)
    return <p className="my-2 text-sm text-muted">No test cases in this suite.</p>;

  return (
    <div className="my-2 space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" disabled={!dirty || busy} onClick={() => apply.mutate()}>
          <IconConfirm aria-hidden />
          {apply.isPending ? "Saving" : "Apply order"}
        </Button>
        <Button size="sm" variant="ghost" disabled={!dirty || busy} onClick={() => cases.data && setOrder(cases.data)}>
          <IconUndo aria-hidden />
          Reset
        </Button>
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => fromFile.mutate()}>
          <IconImport aria-hidden />
          {fromFile.isPending ? "Reading file" : "Apply tester order from file"}
        </Button>
      </div>
      <CaseOrderList
        cases={order}
        selected={selected}
        onChange={setOrder}
        onSelect={(next) => {
          const added = order.filter((c) => next.has(c.id) && !selected.has(c.id));
          const removed = order.filter((c) => !next.has(c.id) && selected.has(c.id));
          if (added.length) onToggle(added, true);
          if (removed.length) onToggle(removed, false);
        }}
        disabled={busy}
      />
    </div>
  );
}
```

Check `ScanProgress` accepts `className` (its props list at `src/components/ScanProgress.tsx:11-20` includes it).

- [ ] **Step 7: Run the tests**

`npx vitest run src/screens/ManageCases/SuiteCases.test.tsx src/screens/ManageCases/selection.test.ts src/ui-consistency.test.ts`
Expected: all pass. (The old `index.test.tsx`, `actions.test.tsx`, `folders.test.tsx` still pass because `index.tsx` is untouched in this task.)

- [ ] **Step 8: Commit**

```bash
git add src/screens/ManageCases/suiteCases.ts src/screens/ManageCases/selection.ts src/screens/ManageCases/selection.test.ts src/screens/ManageCases/SuiteCases.tsx src/screens/ManageCases/SuiteCases.test.tsx
git commit -q -F - <<'EOF'
feat(v2): Manage Test Cases suite body owns its order, actions and selection reporting

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 2: Plan tables, PBI preselection, copy to suite, new suite; the screen rewrite

**Files:**
- Modify: `src/lib/actionIcons.ts` (replace `FolderInput as IconAddToFolder` with `FolderInput as IconCopyToSuite`)
- Rename: `src/screens/ManageCases/NewFolderDialog.tsx` → `NewSuiteDialog.tsx` (`git mv`), wording changes
- Create: `src/screens/ManageCases/PlanTable.tsx`
- Rewrite: `src/screens/ManageCases/index.tsx`
- Delete: `src/screens/ManageCases/SuitePicker.tsx`, `src/screens/ManageCases/SuitePicker.test.tsx`, `src/screens/ManageCases/actions.test.tsx` (its coverage moved to `SuiteCases.test.tsx` in Task 1)
- Rewrite: `src/screens/ManageCases/testSupport.tsx`, `src/screens/ManageCases/index.test.tsx`; rename `folders.test.tsx` → `suites.test.tsx` and rewrite
- Modify: `src/App.tsx` (the `<ManageCases org={org} project={project} />` route gains `pbi={pbi}`)

**Interfaces:**
- Consumes: Task 1's `SuiteCases`, `Selection`, `toggleSelection`, `selectedIdsIn`, `suiteCasesKey`; `buildTree`, `flattenTree`, `indented` from `src/lib/suiteTree`; `commands.listPlansWithSuites` with `persistentQuery` + `CACHE.structure` (as `src/screens/Suites.tsx:153-163` does); `commands.addCasesToSuite`, `commands.createStaticSuite`; `PbiHit`, `PlanWithSuites`, `SuiteRef` from bindings; `Badge`, `Select`, `Button`, `Input`, `Modal`.
- Produces:
  - `ManageCases` props `{ org: string; project: string; pbi: PbiHit | null }`.
  - `PlanTable` props `{ org: string; project: string; plan: PlanWithSuites["plan"]; suites: SuiteRef[]; initiallyExpanded: number[]; selection: Selection | null; onToggle: (planId: number, cases: SuiteCase[], on: boolean) => void; onClearSelection: () => void }`.
  - `NewSuiteDialog` props unchanged from `NewFolderDialog` (`org, project, planId, parents, defaultParentId, caseIds, sourceName, onClose, onCreated`), except `sourceName` becomes `sourceLabel: string` (a sentence fragment such as `their suites`).

- [ ] **Step 1: Icons and the dialog rename**

In `src/lib/actionIcons.ts` change `FolderInput as IconAddToFolder,` to:

```ts
  // Copying selected cases into another suite: a folder with an arrow in.
  FolderInput as IconCopyToSuite,
```

`git mv src/screens/ManageCases/NewFolderDialog.tsx src/screens/ManageCases/NewSuiteDialog.tsx`, rename the function to `NewSuiteDialog`, the prop `sourceName` to `sourceLabel`, and change the user-facing strings to:

- heading: `New test suite`
- intro: `A static test suite. It can sit under the plan root or under another static suite.`
- input label and aria-label: `Suite name`; placeholder `Smoke`
- warning toast: `` `Created suite "${suite.name}", but the test cases could not be added: ${addError}` ``
- success (no cases): `` `Created suite "${suite.name}".` ``
- success (with cases): `` `Created suite "${suite.name}" and added ${added!.length} test case${added!.length === 1 ? "" : "s"}. They stay in ${sourceLabel} too.` ``
- error: `` `Could not create the suite: ${e.message}` ``
- button: `Creating` / `` `Create suite and add ${n} test case${n === 1 ? "" : "s"}` `` / `Create suite`

The doc comment: `/** A static test suite, created under a static suite or the plan root (the only parents Azure DevOps allows). With cases selected the same click copies them in afterwards: they stay where they were, a case can live in many suites. */`

- [ ] **Step 2: Write the failing screen tests**

Rewrite `src/screens/ManageCases/testSupport.tsx`:

```tsx
import { mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import type { PbiHit } from "../../bindings";
import ManageCases from "./index";

export const PLANS = [
  {
    plan: { id: 9, name: "Auth - Test Plan", area_path: "Proj\\Auth", root_suite_id: 90 },
    suites: [
      { id: 91, name: "Regression", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
      { id: 92, name: "Smoke", suite_type: "staticTestSuite", requirement_id: null, parent_id: 91 },
      { id: 93, name: "PBI 42 suite", suite_type: "requirementTestSuite", requirement_id: 42, parent_id: null },
    ],
  },
  {
    plan: { id: 10, name: "Billing - Test Plan", area_path: "Proj\\Billing", root_suite_id: 100 },
    suites: [
      { id: 101, name: "Invoices", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
    ],
  },
];

export const ENTRIES = [
  { id: 95, sequence_number: 0, entry_type: "suite" },
  { id: 201, sequence_number: 1, entry_type: "testCase" },
  { id: 202, sequence_number: 2, entry_type: "testCase" },
  { id: 203, sequence_number: 3, entry_type: "testCase" },
];

export const point = (id: number, name: string, config = "Windows 10") => ({
  point_id: id * 10,
  test_case_id: id,
  test_case_name: name,
  config_name: config,
  tester: "",
  last_outcome: "none",
  last_run_id: null,
  last_result_id: null,
});

/** Mount the screen over two plans. Every suite answers with cases 201,
 * 202, 203 (the fixture does not vary per suite). `extra` answers any
 * other command. Returns every IPC call for assertions. */
export function mountScreen(
  extra: (cmd: string, args: unknown) => unknown = () => undefined,
  pbi: PbiHit | null = null,
) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_plans_with_suites") return PLANS;
    if (cmd === "list_suite_entries") return ENTRIES;
    if (cmd === "list_test_points")
      return [point(201, "Valid login"), point(201, "Valid login", "Windows 11"), point(202, "Bad password"), point(203, "Locked out")];
    return extra(cmd, args);
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <ManageCases org="acme" project="Web" pbi={pbi} />
    </QueryClientProvider>,
  );
  return { calls };
}

/** Expand a suite block by name and resolve to its case list. */
export async function expandSuite(name: string) {
  fireEvent.click(await screen.findByRole("button", { name: `Expand ${name}` }));
  const l = await screen.findByRole("list", { name: `Test cases in ${name}` });
  await new Promise((r) => setTimeout(r, 0));
  return l;
}
```

Note: the case list's accessible name changes from the fixed `Test cases in order` to `` `Test cases in ${suiteName}` ``, because several lists can be open at once. `CaseOrderList` gains an `ariaLabel: string` prop (Task 2 step 4) and `SuiteCases` gains `suiteName: string` and passes `` `Test cases in ${suiteName}` ``; update `SuiteCases.test.tsx`'s Harness (pass `suiteName="Regression"`) and its `list()` helper to look for `Test cases in Regression`.

Rewrite `src/screens/ManageCases/index.test.tsx`:

```tsx
import { clearMocks } from "@tauri-apps/api/mocks";
import { fireEvent, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { expandSuite, mountScreen } from "./testSupport";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

test("with no PBI every plan is a table of its suites, collapsed, in tree order", async () => {
  const { calls } = mountScreen();
  expect(await screen.findByRole("region", { name: "Auth - Test Plan" })).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Billing - Test Plan" })).toBeInTheDocument();
  const auth = screen.getByRole("region", { name: "Auth - Test Plan" });
  const rows = within(auth).getAllByRole("button", { name: /^Expand / });
  expect(rows.map((r) => r.getAttribute("aria-label"))).toEqual(["Expand Regression", "Expand Smoke", "Expand PBI 42 suite"]);
  // Smoke sits under Regression: one level deeper.
  expect(rows[1].style.paddingLeft).not.toBe(rows[0].style.paddingLeft);
  expect(within(auth).getByText("PBI 42")).toBeInTheDocument();
  // Nothing is expanded, so no suite has been read yet.
  expect(calls.some((c) => c.cmd === "list_suite_entries")).toBe(false);
  expect(screen.queryByRole("button", { name: "Show all plans" })).not.toBeInTheDocument();
});

test("expanding a suite loads its cases; collapsing hides them", async () => {
  const { calls } = mountScreen();
  const l = await expandSuite("Regression");
  expect(within(l).getAllByRole("listitem")).toHaveLength(3);
  expect(calls.filter((c) => c.cmd === "list_suite_entries").map((c) => (c.args as { suiteId: number }).suiteId)).toEqual([91]);
  fireEvent.click(screen.getByRole("button", { name: "Collapse Regression" }));
  expect(screen.queryByRole("list", { name: "Test cases in Regression" })).not.toBeInTheDocument();
});

test("a picked PBI shows only its plan with its suite open; Show all plans widens it", async () => {
  mountScreen(undefined, { id: 42, title: "Login", work_item_type: "Product Backlog Item" });
  expect(await screen.findByRole("region", { name: "Auth - Test Plan" })).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Billing - Test Plan" })).not.toBeInTheDocument();
  expect(await screen.findByRole("list", { name: "Test cases in PBI 42 suite" })).toBeInTheDocument();
  expect(screen.getByText("Showing the plan that holds PBI #42.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Show all plans" }));
  expect(screen.getByRole("region", { name: "Billing - Test Plan" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Show only this PBI's plan" }));
  expect(screen.queryByRole("region", { name: "Billing - Test Plan" })).not.toBeInTheDocument();
});

test("a picked PBI with no suite in any plan falls back to every plan and says so", async () => {
  mountScreen(undefined, { id: 77, title: "Orphan", work_item_type: "Product Backlog Item" });
  expect(await screen.findByRole("region", { name: "Billing - Test Plan" })).toBeInTheDocument();
  expect(screen.getByText("PBI #77 has no test suite yet. Showing every plan.")).toBeInTheDocument();
});

test("no plans yet", async () => {
  mountScreen((cmd) => (cmd === "list_plans_with_suites" ? [] : undefined));
  const { clearMocks: _ } = await import("@tauri-apps/api/mocks");
  expect(await screen.findByText("No test plans with test suites in this project yet.")).toBeInTheDocument();
});
```

The last test's `await import` line is unnecessary; write it without it (the `mockIPC` in `mountScreen` answers `list_plans_with_suites` before `extra`, so the `extra` override never fires). Make `mountScreen` consult `extra` FIRST and fall back to the fixtures only when `extra` returns `undefined`, so a test can override any command:

```ts
    const answered = extra(cmd, args);
    if (answered !== undefined) return answered;
    if (cmd === "list_plans_with_suites") return PLANS;
    …
```

and write the test as:

```tsx
test("no plans yet", async () => {
  mountScreen((cmd) => (cmd === "list_plans_with_suites" ? [] : undefined));
  expect(await screen.findByText("No test plans with test suites in this project yet.")).toBeInTheDocument();
});
```

Create `src/screens/ManageCases/suites.test.tsx` (replaces `folders.test.tsx`):

```tsx
import { clearMocks } from "@tauri-apps/api/mocks";
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "sonner";
import { expandSuite, mountScreen } from "./testSupport";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

async function openSelect(name: string) {
  const trigger = screen.getByRole("combobox", { name });
  fireEvent.click(trigger);
  return trigger;
}

test("selecting cases in a plan enables Copy to suite for that plan only; copying adds them and keeps them", async () => {
  const { calls } = mountScreen((cmd) => (cmd === "add_cases_to_suite" ? [201, 203] : undefined));
  const l = await expandSuite("PBI 42 suite");
  const auth = screen.getByRole("region", { name: "Auth - Test Plan" });
  const billing = screen.getByRole("region", { name: "Billing - Test Plan" });
  expect(within(auth).getByRole("button", { name: "Copy to suite" })).toBeDisabled();

  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #201" }));
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #203" }));
  expect(within(auth).getByText("2 selected")).toBeInTheDocument();
  expect(within(billing).queryByText(/selected/)).not.toBeInTheDocument();
  // A target is needed: static suites of this plan (root included), never the PBI suite.
  await openSelect("Copy to");
  const labels = screen.getAllByRole("option").map((o) => o.textContent);
  expect(labels).toEqual(["Pick a suite", "Plan root", "Regression", "\u00a0\u00a0\u00a0\u00a0Smoke"]);
  fireEvent.click(screen.getByRole("option", { name: "Regression" }));
  fireEvent.click(within(auth).getByRole("button", { name: "Copy to suite" }));

  await waitFor(() => {
    const add = calls.find((c) => c.cmd === "add_cases_to_suite");
    expect(add?.args).toEqual({ organization: "acme", project: "Web", planId: 9, suiteId: 91, caseIds: [201, 203] });
  });
  expect(toast.success).toHaveBeenCalledWith("Copied 2 test cases to Regression. They stay where they were.");
  // The selection is spent, and the target suite is read again if it is open.
  await waitFor(() => expect(within(auth).queryByText(/selected/)).not.toBeInTheDocument());
});

test("selecting in a second plan starts a new selection", async () => {
  mountScreen();
  const l1 = await expandSuite("Regression");
  fireEvent.click(within(l1).getByRole("checkbox", { name: "Select #201" }));
  const l2 = await expandSuite("Invoices");
  fireEvent.click(within(l2).getByRole("checkbox", { name: "Select #202" }));
  const auth = screen.getByRole("region", { name: "Auth - Test Plan" });
  const billing = screen.getByRole("region", { name: "Billing - Test Plan" });
  expect(within(billing).getByText("1 selected")).toBeInTheDocument();
  expect(within(auth).queryByText(/selected/)).not.toBeInTheDocument();
  expect(within(l1).getByRole("checkbox", { name: "Select #201" })).not.toBeChecked();
});

test("New test suite offers root and static suites as parents; with cases selected it creates and copies", async () => {
  const { calls } = mountScreen((cmd, args) => {
    if (cmd === "create_static_suite")
      return { id: 94, name: (args as { name: string }).name, suite_type: "staticTestSuite", requirement_id: null, parent_id: 90 };
    if (cmd === "add_cases_to_suite") return [202];
    return undefined;
  });
  const l = await expandSuite("Regression");
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #202" }));
  const auth = screen.getByRole("region", { name: "Auth - Test Plan" });
  fireEvent.click(within(auth).getByRole("button", { name: "New test suite" }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.click(within(dialog).getByRole("combobox", { name: "Create inside" }));
  expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual([
    "Plan root (Auth - Test Plan)",
    "Regression",
    "\u00a0\u00a0\u00a0\u00a0Smoke",
  ]);
  fireEvent.click(screen.getByRole("option", { name: "Plan root (Auth - Test Plan)" }));
  fireEvent.change(within(dialog).getByLabelText("Suite name"), { target: { value: "  Nightly  " } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Create suite and add 1 test case" }));
  await waitFor(() => {
    const create = calls.find((c) => c.cmd === "create_static_suite");
    expect(create?.args).toEqual({ organization: "acme", project: "Web", planId: 9, parentSuiteId: 90, name: "Nightly" });
  });
  await waitFor(() => {
    const add = calls.find((c) => c.cmd === "add_cases_to_suite");
    expect(add?.args).toEqual({ organization: "acme", project: "Web", planId: 9, suiteId: 94, caseIds: [202] });
  });
  expect(toast.success).toHaveBeenCalledWith('Created suite "Nightly" and added 1 test case. They stay in their suites too.');
  await waitFor(() => expect(calls.filter((c) => c.cmd === "list_plans_with_suites").length).toBeGreaterThan(1));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test("New test suite with nothing selected only creates; an empty name is refused", async () => {
  const { calls } = mountScreen((cmd) =>
    cmd === "create_static_suite"
      ? { id: 94, name: "Nightly", suite_type: "staticTestSuite", requirement_id: null, parent_id: 100 }
      : undefined,
  );
  const billing = await screen.findByRole("region", { name: "Billing - Test Plan" });
  fireEvent.click(within(billing).getByRole("button", { name: "New test suite" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByRole("button", { name: "Create suite" })).toBeDisabled();
  fireEvent.change(within(dialog).getByLabelText("Suite name"), { target: { value: "   " } });
  expect(within(dialog).getByRole("button", { name: "Create suite" })).toBeDisabled();
  fireEvent.change(within(dialog).getByLabelText("Suite name"), { target: { value: "Nightly" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Create suite" }));
  await waitFor(() => {
    const create = calls.find((c) => c.cmd === "create_static_suite");
    expect(create?.args).toEqual({ organization: "acme", project: "Web", planId: 10, parentSuiteId: 100, name: "Nightly" });
  });
  expect(calls.some((c) => c.cmd === "add_cases_to_suite")).toBe(false);
  expect(toast.success).toHaveBeenCalledWith('Created suite "Nightly".');
});

test("the suite is created but the copy fails: the suite still shows up and the toast says what happened", async () => {
  const { calls } = mountScreen((cmd) => {
    if (cmd === "create_static_suite")
      return { id: 94, name: "Nightly", suite_type: "staticTestSuite", requirement_id: null, parent_id: 90 };
    if (cmd === "add_cases_to_suite") throw new Error("boom");
    return undefined;
  });
  const l = await expandSuite("Regression");
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #202" }));
  const auth = screen.getByRole("region", { name: "Auth - Test Plan" });
  fireEvent.click(within(auth).getByRole("button", { name: "New test suite" }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.change(within(dialog).getByLabelText("Suite name"), { target: { value: "Nightly" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Create suite and add 1 test case" }));
  await waitFor(() => expect(toast.warning).toHaveBeenCalledWith(expect.stringContaining('Created suite "Nightly", but the test cases could not be added'), { duration: 20000 }));
  await waitFor(() => expect(calls.filter((c) => c.cmd === "list_plans_with_suites").length).toBeGreaterThan(1));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});
```

Delete `src/screens/ManageCases/SuitePicker.test.tsx`, `src/screens/ManageCases/actions.test.tsx`, `src/screens/ManageCases/folders.test.tsx` (`git rm`).

- [ ] **Step 3: Run the new tests to verify they fail**

`npx vitest run src/screens/ManageCases/index.test.tsx src/screens/ManageCases/suites.test.tsx`
Expected: FAIL (no `region` named after a plan; `ManageCases` has no `pbi` prop yet).

- [ ] **Step 4: `CaseOrderList` accessible name and `SuiteCases` suite name**

In `src/screens/ManageCases/CaseOrderList.tsx` add a required prop `ariaLabel: string` and use it: `<ol aria-label={ariaLabel} …>`. In `SuiteCases.tsx` add the prop `suiteName: string` and pass `ariaLabel={`Test cases in ${suiteName}`}`. Update `SuiteCases.test.tsx` (Harness passes `suiteName="Regression"`, `list()` finds `Test cases in Regression`).

- [ ] **Step 5: Write PlanTable**

Create `src/screens/ManageCases/PlanTable.tsx`:

```tsx
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, FolderTree } from "lucide-react";
import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { toast } from "sonner";
import { commands, type PlanWithSuites, type SuiteRef } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Select } from "../../components/ui/select";
import { IconCopyToSuite, IconNewFolder } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { unwrap } from "../../lib/ipc";
import type { SuiteCase } from "../../lib/suiteOrder";
import { buildTree, flattenTree, indented } from "../../lib/suiteTree";
import NewSuiteDialog from "./NewSuiteDialog";
import { selectedIdsIn, type Selection } from "./selection";
import SuiteCases from "./SuiteCases";
import { suiteCasesKey } from "./suiteCases";

/** One test plan as a table: its suites in tree order, each a block that
 * opens to show its cases. The header carries the plan's bulk actions,
 * which act on the cases selected in THIS plan: copy them into another
 * suite of the plan, or make a new static suite (with them in it). */
export default function PlanTable({
  org,
  project,
  plan,
  suites,
  initiallyExpanded,
  selection,
  onToggle,
  onClearSelection,
}: {
  org: string;
  project: string;
  plan: PlanWithSuites["plan"];
  suites: SuiteRef[];
  /** Suite ids to open on first render (a picked PBI's own suite). */
  initiallyExpanded: number[];
  selection: Selection | null;
  onToggle: (planId: number, cases: SuiteCase[], on: boolean) => void;
  onClearSelection: () => void;
}) {
  const qc = useQueryClient();
  const [expanded, setExpanded] = useState<Set<number>>(() => new Set(initiallyExpanded));
  const [target, setTarget] = useState("");
  const [newOpen, setNewOpen] = useState(false);

  const rows = useMemo(() => flattenTree(buildTree(suites)), [suites]);
  const selectedIds = selectedIdsIn(selection, plan.id);
  const selectedCases = selection && selection.planId === plan.id ? [...selection.cases.values()] : [];

  /** Static suites, root first: where cases can be copied and suites created. */
  const staticTargets = useMemo(
    () => [
      ...(plan.root_suite_id != null ? [{ id: plan.root_suite_id, label: "Plan root", name: "Plan root" }] : []),
      ...rows
        .filter(({ suite }) => suite.suite_type === "staticTestSuite")
        .map(({ suite, depth }) => ({ id: suite.id, label: indented(suite.name, depth), name: suite.name })),
    ],
    [plan.root_suite_id, rows],
  );
  useEffect(() => {
    if (target && !staticTargets.some((t) => String(t.id) === target)) setTarget("");
  }, [target, staticTargets]);

  const copy = useMutation({
    mutationFn: () =>
      unwrap(commands.addCasesToSuite(org, project, plan.id, Number(target), selectedCases.map((c) => c.id))),
    onSuccess: (added) => {
      const name = staticTargets.find((t) => String(t.id) === target)?.name ?? "the suite";
      toast.success(
        `Copied ${added.length} test case${added.length === 1 ? "" : "s"} to ${name}. ${added.length === 1 ? "It stays" : "They stay"} where ${added.length === 1 ? "it was" : "they were"}.`,
      );
      qc.invalidateQueries({ queryKey: suiteCasesKey(org, project, plan.id, Number(target)) });
      onClearSelection();
    },
    onError: (e) => toast.error(`Could not copy the cases: ${e.message}`),
  });

  const toggle = (id: number) =>
    setExpanded((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const busy = copy.isPending;

  return (
    <section aria-label={plan.name} className="rounded-md border border-border bg-surface">
      <header className="flex flex-wrap items-center gap-2 border-b border-border px-3 py-2 text-sm">
        <FolderTree size={14} className="text-accent" aria-hidden />
        <span className="font-medium text-text">{plan.name}</span>
        <span className="text-xs text-faint">{plan.area_path}</span>
        <span className="flex-1" />
        {selectedCases.length > 0 && (
          <span className="text-xs text-muted">{selectedCases.length} selected</span>
        )}
        <Select
          aria-label="Copy to"
          triggerClassName="py-1.5"
          value={target}
          disabled={busy || staticTargets.length === 0}
          onChange={(e) => setTarget(e.target.value)}
        >
          <option value="">Pick a suite</option>
          {staticTargets.map((t) => (
            <option key={t.id} value={t.id}>
              {t.label}
            </option>
          ))}
        </Select>
        <Button
          size="sm"
          variant="ghost"
          disabled={busy || !target || selectedCases.length === 0}
          onClick={() => copy.mutate()}
        >
          <IconCopyToSuite aria-hidden />
          {copy.isPending ? "Copying" : "Copy to suite"}
        </Button>
        <Button size="sm" variant="ghost" disabled={busy || staticTargets.length === 0} onClick={() => setNewOpen(true)}>
          <IconNewFolder aria-hidden />
          New test suite
        </Button>
      </header>
      <ul className="p-1">
        {rows.map(({ suite, depth }) => {
          const open = expanded.has(suite.id);
          return (
            <li key={suite.id} className="cv-row" style={{ "--cv-size": "32px" } as CSSProperties}>
              <button
                type="button"
                aria-label={`${open ? "Collapse" : "Expand"} ${suite.name}`}
                aria-expanded={open}
                className={cn(
                  "flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm text-text hover:bg-accent-soft",
                  open && "bg-surface-2",
                )}
                style={{ paddingLeft: 8 + depth * 18 }}
                onClick={() => toggle(suite.id)}
              >
                {open ? <ChevronDown size={14} aria-hidden /> : <ChevronRight size={14} aria-hidden />}
                {suite.suite_type === "requirementTestSuite" && suite.requirement_id != null && (
                  <Badge className="shrink-0 bg-accent-soft text-accent">PBI {suite.requirement_id}</Badge>
                )}
                <span className="min-w-0 flex-1 break-words">{suite.name}</span>
                {selectedIds.size > 0 && open && (
                  <span className="text-xs text-faint">{suite.suite_type === "requirementTestSuite" ? "PBI suite" : "static"}</span>
                )}
              </button>
              {open && (
                <div style={{ paddingLeft: 8 + depth * 18 + 20 }}>
                  <SuiteCases
                    org={org}
                    project={project}
                    planId={plan.id}
                    suiteId={suite.id}
                    suiteName={suite.name}
                    selected={selectedIds}
                    onToggle={(cases, on) => onToggle(plan.id, cases, on)}
                  />
                </div>
              )}
            </li>
          );
        })}
      </ul>
      {newOpen && staticTargets.length > 0 && (
        <NewSuiteDialog
          org={org}
          project={project}
          planId={plan.id}
          parents={staticTargets.map((t) => ({ id: t.id, label: t.id === plan.root_suite_id ? `Plan root (${plan.name})` : t.label }))}
          defaultParentId={staticTargets[0].id}
          caseIds={selectedCases.map((c) => c.id)}
          sourceLabel="their suites"
          onClose={() => setNewOpen(false)}
          onCreated={() => {
            qc.invalidateQueries({ queryKey: ["plans-suites", org, project] });
            onClearSelection();
          }}
        />
      )}
    </section>
  );
}
```

Remove the `{selectedIds.size > 0 && open && (…"static")}` span: it was a leftover idea and the test does not want it. The copy toast for one case must read `Copied 1 test case to X. It stays where it was.` and for several `Copied N test cases to X. They stay where they were.` (the template above produces exactly that).

The `"Copy to"` select shows `Pick a suite`, `Plan root`, then static suites indented; `suites.test.tsx` asserts that list.

- [ ] **Step 6: Rewrite the screen**

Replace `src/screens/ManageCases/index.tsx` with:

```tsx
import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { commands, type PbiHit } from "../../bindings";
import ScanProgress from "../../components/ScanProgress";
import { Button } from "../../components/ui/button";
import { CACHE, persistentQuery } from "../../lib/persistentQuery";
import { unwrap } from "../../lib/ipc";
import type { SuiteCase } from "../../lib/suiteOrder";
import PlanTable from "./PlanTable";
import { toggleSelection, type Selection } from "./selection";

/** Bulk work on test cases, plan by plan: every suite of a plan with its
 * cases underneath. Cases are selected across a plan's suites and copied
 * into another suite or a new one; each suite's order can be changed and
 * saved. A PBI picked in the bar narrows the view to the plan that holds
 * its suite, with that suite already open. */
export default function ManageCases({ org, project, pbi }: { org: string; project: string; pbi: PbiHit | null }) {
  const plans = useQuery({
    queryKey: ["plans-suites", org, project],
    ...persistentQuery({
      key: `plans-suites:${org}/${project}`,
      fetcher: () => unwrap(commands.listPlansWithSuites(org, project)),
      ...CACHE.structure,
    }),
    enabled: Boolean(org && project),
    gcTime: 60 * 60_000,
    retry: false,
  });

  const [selection, setSelection] = useState<Selection | null>(null);
  const [showAll, setShowAll] = useState(false);
  // A new PBI in the bar narrows the view again.
  useEffect(() => setShowAll(false), [pbi?.id]);

  /** The plan holding the PBI's requirement suite, and that suite. */
  const pbiPlan = useMemo(() => {
    if (!pbi || !plans.data) return null;
    for (const p of plans.data) {
      const suite = p.suites.find((s) => s.suite_type === "requirementTestSuite" && s.requirement_id === pbi.id);
      if (suite) return { plan: p, suiteId: suite.id };
    }
    return null;
  }, [pbi, plans.data]);

  const visible = pbiPlan && !showAll ? [pbiPlan.plan] : (plans.data ?? []);

  const onToggle = (planId: number, cases: SuiteCase[], on: boolean) =>
    setSelection((s) => toggleSelection(s, planId, cases, on));

  if (!org || !project) {
    return (
      <p className="text-sm text-muted">
        Pick an organization and project in the bar above to manage test cases.
      </p>
    );
  }

  return (
    <div className="space-y-3">
      {pbi && plans.data && (
        <div className="flex flex-wrap items-center gap-2 text-sm text-muted">
          <span>
            {pbiPlan
              ? `Showing the plan that holds PBI #${pbi.id}.`
              : `PBI #${pbi.id} has no test suite yet. Showing every plan.`}
          </span>
          {pbiPlan && (
            <Button size="sm" variant="ghost" onClick={() => setShowAll((v) => !v)}>
              {showAll ? "Show only this PBI's plan" : "Show all plans"}
            </Button>
          )}
        </div>
      )}
      {plans.isFetching && !plans.data && <ScanProgress label="Loading test plans" />}
      {plans.isError && <p className="text-sm text-danger">{plans.error.message}</p>}
      {plans.data && plans.data.length === 0 && (
        <p className="text-sm text-muted">No test plans with test suites in this project yet.</p>
      )}
      {visible.map(({ plan, suites }) => (
        <PlanTable
          key={plan.id}
          org={org}
          project={project}
          plan={plan}
          suites={suites}
          initiallyExpanded={pbiPlan && pbiPlan.plan.plan.id === plan.id ? [pbiPlan.suiteId] : []}
          selection={selection}
          onToggle={onToggle}
          onClearSelection={() => setSelection(null)}
        />
      ))}
    </div>
  );
}
```

`git rm src/screens/ManageCases/SuitePicker.tsx`. In `src/App.tsx` the route becomes `<ManageCases org={org} project={project} pbi={pbi} />`.

The `Show all plans` toggle keeps `PlanTable` instances mounted (React keys by plan id), so expanded state and open lists survive the switch.

- [ ] **Step 7: Run the tests, then the gates**

`npx vitest run src/screens/ManageCases src/ui-consistency.test.ts src/App.test.tsx`
Expected: all pass. Then `npx tsc --noEmit` (clean; if `IconAddToFolder` is referenced anywhere else, `grep -rn IconAddToFolder src` and fix), then the full `npx vitest run`.

- [ ] **Step 8: Commit**

```bash
git add -A src/screens/ManageCases src/lib/actionIcons.ts src/App.tsx
git commit -q -F - <<'EOF'
feat(v2): Manage Test Cases shows each plan as a table of suites and cases

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```
