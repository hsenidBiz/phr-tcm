# Execution Order Modal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Run Tests' Order select, row drag, row/group Move buttons and Reset, and Suite Management's Suggested run order section, with one **Set execution order** button in Run Tests that opens an `Execution order` modal. The modal saves the chosen order for this machine (**Use this order**) or for every tester (**Save for everyone**).

**Architecture:** `useRunOrder` keeps owning the list's order (view + reconcile) and gains two small entry points the modal calls: `changeView(v)` (already there) and `saveMine(ids)`. It also exposes what the modal reads (`file`, `loading`, `myOrder`, `specCases`, `note`). A new `ExecutionOrderModal` holds the Start from choice, a `CaseOrderList` and the two save paths. Save for everyone calls the existing `saveRunOrder` command and writes the result into the shared run-order query entry. The save payload's group rule moves out of `SuggestedOrder` into a pure `runOrderPayload` in `src/lib/runOrder.ts`, so it exists once. Then `SuggestedOrder` is deleted.

**Tech Stack:** React 19 + TypeScript, TanStack Query v5, Tailwind tokens, vitest + Testing Library (jsdom), `@tauri-apps/api/mocks`.

**Spec:** docs/superpowers/specs/2026-09-23-execution-order-modal-design.md

## Global Constraints

- Run every command from the repository root: `C:\Users\Admin\Desktop\Main Menu\Software\Personal Projects\Automated Test Cases Creator` (in Bash: `cd "/c/Users/Admin/Desktop/Main Menu/Software/Personal Projects/Automated Test Cases Creator"`). The `v2/` folder is not the repo root.
- Colours come only from Tailwind theme tokens (`text-text`, `text-muted`, `text-warning`, `bg-surface-2`, `border-border`...). No hex, no default palette, no `text-white`/`bg-black`.
- Icons come from `src/lib/actionIcons.ts` and are written as `<IconX aria-hidden />` with no `size` or className. Never import a lucide icon straight into a screen for a button.
- Dropdowns use the themed `Select` (`src/components/ui/select.tsx`). Dialogs use the shared `Modal` (`src/components/ui/modal.tsx`).
- No em dashes in user-facing text.
- Never weaken `src/ui-consistency.test.ts` or `src/a11y.test.tsx`.
- `src/bindings.ts` stays unchanged. There are no Rust changes.
- No Azure DevOps call beyond the existing `getRunOrder` / `saveRunOrder` and the suite-cases loader (`loadSuiteCases`).
- `src/tour/tourBackend.ts` stays read-only: no `saveRunOrder` stand-in.
- One cache implementation: `src/lib/cache.ts` (`persistentQuery`, `setQueryData` + `invalidateQueries`). Add no private cache.
- Frontend tests: `npx vitest run <paths> --exclude "**/.claude/**"`. Typecheck: `npx tsc --noEmit`. Run one suite at a time; the machine is shared.
- Commits go through a Bash heredoc (`git commit -F - <<'EOF' … EOF`) and end with a `Co-Authored-By:` trailer naming the model that makes the commit. The examples below use `Claude Opus 5.5`; a different executing model writes its own name. Confirm with `git log -1`.
- Never write source files through Bash heredocs. Use the Write/Edit tools.
- Source files are CRLF. Keep them CRLF when editing.
- `src/screens/RunPanel/index.tsx` is large. Use the exact old/new snippets below with Edit; do not re-read the whole file repeatedly.

---

### Task 1: The Execution order modal (with `saveMine` and the shared save payload)

**Files:**
- Modify: `src/lib/runOrder.ts` (add `runOrderPayload`; import `RunOrderCase` type)
- Modify: `src/lib/runOrder.test.ts` (import + 3 tests at the end)
- Modify: `src/screens/ManageCases/SuggestedOrder.tsx` (use `runOrderPayload` in its save; lines 4, 12, 157-170)
- Modify: `src/screens/RunPanel/useRunOrder.ts` (add `specCases`, `saveMine`; `commit` goes through `saveMine`; return `file`, `loading`, `myOrder`, `specCases`, `saveMine`)
- Create: `src/screens/RunPanel/useRunOrder.test.tsx`
- Create: `src/screens/RunPanel/ExecutionOrderModal.tsx`
- Create: `src/screens/RunPanel/ExecutionOrderModal.test.tsx`

**Interfaces:**
- Consumes:
  - `reconcile(order: readonly number[], spec: readonly number[]): number[]`, `saveMyOrder`, `loadMyOrder`, `saveOrderView`, `type OrderView`, `type OrderKey` from `src/lib/runOrder.ts`
  - `testerOrderSources(watches: readonly WatchedFile[], suiteIds: readonly number[]): TesterOrderSource[]` from `src/lib/testerOrderStart.ts` (`TesterOrderSource = { path; label; ids: number[]; groups: Map<number, string> }`)
  - `loadWatches(org: string, pbiId: number): WatchedFile[]` from `src/lib/fileSync.ts`
  - `sameOrder(a: SuiteCase[], b: SuiteCase[]): boolean`, `type SuiteCase = { id: number; title: string }` from `src/lib/suiteOrder.ts`
  - `CaseOrderList` (default export of `src/screens/ManageCases/CaseOrderList.tsx`): props `{ cases; selected; onChange; onSelect; ariaLabel; disabled? }`
  - `commands.saveRunOrder(organization: string, project: string, pbiId: number, cases: RunOrderCase[])` and the types `RunOrderCase`, `RunOrderFile` from `src/bindings.ts`
  - `ORDER_LABELS: Record<OrderView, string>` and `runOrderQueryOptions(org, project, pbiId)` (its `.queryKey` is `["run-order", org, project, pbiId]`) from `src/screens/RunPanel/useRunOrder.ts`
- Produces:
  - `runOrderPayload(ids: readonly number[], saved: readonly { id: number; group?: string | null }[] | null, fileGroups?: ReadonlyMap<number, string>): RunOrderCase[]` in `src/lib/runOrder.ts`
  - `useRunOrder(...)` additionally returns `file: RunOrderFile | null`, `loading: boolean`, `myOrder: number[] | null`, `specCases: SuiteCase[]`, `saveMine: (ids: readonly number[]) => boolean`. It still returns `view`, `changeView: (v: OrderView) => void` and `note: string | null`, and it keeps every existing field until Task 2.
  - `export type ExecutionOrderModalProps` and `export default function ExecutionOrderModal(props: ExecutionOrderModalProps)` in `src/screens/RunPanel/ExecutionOrderModal.tsx`:
    ```ts
    export type ExecutionOrderModalProps = {
      org: string;
      project: string;
      /** 0 when the suite has no PBI: Save for everyone is then absent. */
      pbiId: number;
      /** The cases Run Tests lists, in spec order, with their titles. */
      cases: SuiteCase[];
      /** The list's active order: where Start from begins. */
      view: OrderView;
      file: RunOrderFile | null;
      /** `noteFor(reason)` when the suggested run order could not be read, else null. */
      unreadableNote: string | null;
      /** The run-order read has not settled yet. */
      loading: boolean;
      myOrder: readonly number[] | null;
      /** Use this order on a stored order left exactly as it is: switch the view only. */
      onUseView: (v: OrderView) => void;
      /** Use this order on a list of the tester's own: store it as My order. False when it could not be saved. */
      onUseMine: (ids: number[]) => boolean;
      onClose: () => void;
    };
    ```

> Deviation from spec: §5 says the modal lists "the suite's cases (`{id, title}`)". The modal lists the cases Run Tests lists instead (`useRunOrder`'s `specCases`): every case that has a test point, in the suite's spec order, titled from its points. A suite entry with no test point cannot be run and Run Tests never shows it. This needs no second read of the suite, and the modal's list matches the list it orders.

- [ ] **Step 1: Write the failing tests for `runOrderPayload`**

In `src/lib/runOrder.test.ts`, add `runOrderPayload` to the import. Old:

```ts
  resortUpcoming,
  saveMyOrder,
```

New:

```ts
  resortUpcoming,
  runOrderPayload,
  saveMyOrder,
```

Append at the end of the file:

```ts

// --- runOrderPayload -------------------------------------------------------

test("runOrderPayload: list order; a tester-order file's area first, then the saved file's group, else none", () => {
  expect(
    runOrderPayload(
      [203, 201, 202, 204],
      [{ id: 201, group: "Auth" }, { id: 202, group: "Old" }, { id: 203 }],
      new Map([[203, "Auth / Lockout"]]),
    ),
  ).toEqual([
    { id: 203, group: "Auth / Lockout" },
    { id: 201, group: "Auth" },
    { id: 202, group: "Old" },
    { id: 204 },
  ]);
});

test("runOrderPayload: no saved file and no tester-order file means no groups", () => {
  expect(runOrderPayload([2, 1], null)).toEqual([{ id: 2 }, { id: 1 }]);
});

test("runOrderPayload: an empty or null group is not written", () => {
  expect(runOrderPayload([1, 2], [{ id: 1, group: "" }, { id: 2, group: null }])).toEqual([{ id: 1 }, { id: 2 }]);
});
```

- [ ] **Step 2: Write the failing hook tests**

Create `src/screens/RunPanel/useRunOrder.test.tsx`:

```tsx
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { toast } from "sonner";
import { afterEach, expect, test, vi } from "vitest";
import type { TestPoint } from "../../bindings";
import { useRunOrder } from "./useRunOrder";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

const point = (point_id: number, test_case_id: number, test_case_name: string): TestPoint => ({
  point_id,
  test_case_id,
  test_case_name,
  config_name: "Windows 10",
  tester: "",
  last_outcome: "",
  last_run_id: null,
  last_result_id: null,
});

// Constants, so the hook's memos see the same arrays on every render.
const POINTS = [point(1, 301, "Alpha check"), point(2, 302, "Bravo check"), point(3, 303, "Charlie check")];
const SUITE = { plan_id: 9, suite_id: 91 };
const MY_KEY = "tcm-v2-run-order:acme/9/91";
const VIEW_KEY = "tcm-v2-run-order-view:acme/9/91";

function mountHook(runOrder: unknown = { state: "none" }, entries: number[] = [303, 301, 302]) {
  mockIPC((cmd) => {
    switch (cmd) {
      case "get_run_order":
        return runOrder;
      case "list_suite_entries":
        return entries.map((id, i) => ({ id, sequence_number: i + 1, entry_type: "testCase" }));
      case "list_test_points":
        // The suite-cases loader's own read: no names here, so a title in
        // specCases can only have come from the points the hook was given.
        return [];
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  );
  return renderHook(
    () => useRunOrder({ org: "acme", project: "Web", pbiId: 42, suite: SUITE, points: POINTS, grouped: false }),
    { wrapper },
  );
}

test("specCases is the suite's spec order, each case titled from its points", async () => {
  const { result } = mountHook();
  await waitFor(() =>
    expect(result.current.specCases).toEqual([
      { id: 303, title: "Charlie check" },
      { id: 301, title: "Alpha check" },
      { id: 302, title: "Bravo check" },
    ]),
  );
});

test("exposes the found file once the read settles", async () => {
  const FILE = {
    format: "tcm-run-order",
    version: 1,
    saved_by: "lead@example.com",
    saved_at: "2026-09-23T10:15:00Z",
    cases: [{ id: 302 }, { id: 301 }, { id: 303 }],
  };
  const { result } = mountHook({ state: "found", file: FILE });
  await waitFor(() => expect(result.current.loading).toBe(false));
  expect(result.current.file).toEqual(FILE);
  expect(result.current.view).toBe("suggested");
  expect(result.current.note).toBeNull();
});

test("saveMine stores the list as My order on this machine and makes it the list's order", async () => {
  const { result } = mountHook();
  await waitFor(() => expect(result.current.specCases.map((c) => c.id)).toEqual([303, 301, 302]));

  let ok = false;
  act(() => {
    ok = result.current.saveMine([302, 303, 301]);
  });

  expect(ok).toBe(true);
  expect(JSON.parse(localStorage.getItem(MY_KEY) as string)).toEqual([302, 303, 301]);
  expect(localStorage.getItem(VIEW_KEY)).toBe("mine");
  expect(result.current.view).toBe("mine");
  expect(result.current.myOrder).toEqual([302, 303, 301]);
  expect(result.current.ordered.map((p) => p.test_case_id)).toEqual([302, 303, 301]);
  expect(toast.info).not.toHaveBeenCalled();
});

test("saveMine with storage unavailable says so and changes nothing", async () => {
  const { result } = mountHook();
  await waitFor(() => expect(result.current.loading).toBe(false));

  const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("QuotaExceededError");
  });
  let ok = true;
  try {
    act(() => {
      ok = result.current.saveMine([302, 303, 301]);
    });
  } finally {
    setItem.mockRestore();
  }

  expect(ok).toBe(false);
  expect(toast.error).toHaveBeenCalledWith("Your own order could not be saved on this machine.");
  expect(result.current.view).toBe("spec");
  expect(result.current.myOrder).toBeNull();
});

test("changeView switches the view and leaves My order stored", async () => {
  localStorage.setItem(MY_KEY, JSON.stringify([301, 302, 303]));
  localStorage.setItem(VIEW_KEY, "mine");
  const { result } = mountHook();
  await waitFor(() => expect(result.current.view).toBe("mine"));

  act(() => result.current.changeView("spec"));

  expect(result.current.view).toBe("spec");
  expect(result.current.myOrder).toEqual([301, 302, 303]);
  expect(JSON.parse(localStorage.getItem(MY_KEY) as string)).toEqual([301, 302, 303]);
});
```

- [ ] **Step 3: Write the failing modal tests**

Create `src/screens/RunPanel/ExecutionOrderModal.test.tsx`:

```tsx
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import type { ComponentProps } from "react";
import { toast } from "sonner";
import { afterEach, expect, test, vi } from "vitest";
import type { SuiteCase } from "../../lib/suiteOrder";
import ExecutionOrderModal from "./ExecutionOrderModal";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

const CASES: SuiteCase[] = [
  { id: 201, title: "Valid login" },
  { id: 202, title: "Bad password" },
  { id: 203, title: "Locked out" },
];

/** Whatever locale this run uses - matches the component's own
 * `toLocaleDateString()` call exactly. */
const localDate = (iso: string) => new Date(iso).toLocaleDateString();

const FILE = (cases: Array<{ id: number; group?: string }>) => ({
  format: "tcm-run-order",
  version: 1,
  saved_by: "lead@example.com",
  saved_at: "2026-09-23T10:15:00Z",
  cases,
});

const CONFIRM = "Every tester will see this as the suggested run order for this PBI.";

type Props = ComponentProps<typeof ExecutionOrderModal>;

function mount(props: Partial<Props> = {}, backend: (cmd: string, args: unknown) => unknown = () => undefined) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    return backend(cmd, args);
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const handlers = {
    onUseView: vi.fn(),
    onUseMine: vi.fn((_ids: number[]) => true),
    onClose: vi.fn(),
  };
  render(
    <QueryClientProvider client={qc}>
      <ExecutionOrderModal
        org="acme"
        project="Web"
        pbiId={42}
        cases={CASES}
        view="spec"
        file={null}
        unreadableNote={null}
        loading={false}
        myOrder={null}
        {...handlers}
        {...props}
      />
    </QueryClientProvider>,
  );
  return { calls, qc, ...handlers };
}

const startFrom = () => screen.getByRole("combobox", { name: "Start from" });
const orderList = () => screen.getByRole("list", { name: "Execution order" });
/** The list's case ids top to bottom. */
const idsOnScreen = () =>
  within(orderList())
    .getAllByRole("listitem")
    .map((li) => Number(/#(\d+)/.exec(li.textContent ?? "")?.[1]))
    .filter((n) => Number.isFinite(n));
const optionLabels = () => screen.getAllByRole("option").map((o) => o.textContent);
const pick = (name: string) => {
  fireEvent.click(startFrom());
  fireEvent.click(screen.getByRole("option", { name }));
};

/** Remembers a watched draft for PBI 42 the way the Import tab does, with
 * each case stamped with its work item id after upload. */
function watchDraft(path: string, cases: Array<{ id: number | null; order: number | null; area?: string }>) {
  const snapshot = cases.map((c, i) => ({
    title: `Case ${i}`,
    steps: [],
    tags: "",
    automation_status: "Not Automated",
    module_value: "",
    preconditions: "",
    update_id: c.id,
    tester_order: c.order,
    area: c.area ?? "",
  }));
  localStorage.setItem("tcm-v2-watch:acme/42", JSON.stringify([{ path, stamp: "s1", snapshot }]));
}

// ---- Start from ----

test("a found suggested order is offered first, with who saved it and when", () => {
  mount({ file: FILE([{ id: 203 }, { id: 201 }, { id: 202 }]), view: "suggested" });
  expect(screen.getByRole("heading", { name: "Execution order" })).toBeInTheDocument();
  expect(startFrom()).toHaveTextContent("Suggested run order");
  expect(idsOnScreen()).toEqual([203, 201, 202]);
  expect(screen.getByText(`Saved by lead@example.com on ${localDate("2026-09-23T10:15:00Z")}`)).toBeInTheDocument();
  fireEvent.click(startFrom());
  expect(optionLabels()).toEqual(["Suggested run order", "Spec order"]);
});

test("with no suggested order, only Spec order is offered and the note says so", () => {
  mount();
  expect(startFrom()).toHaveTextContent("Spec order");
  expect(idsOnScreen()).toEqual([201, 202, 203]);
  expect(screen.getByText("No suggested run order yet.")).toBeInTheDocument();
  fireEvent.click(startFrom());
  expect(optionLabels()).toEqual(["Spec order"]);
});

test("while the read is pending there is no note yet", () => {
  mount({ loading: true });
  expect(screen.queryByText("No suggested run order yet.")).not.toBeInTheDocument();
});

test("an unreadable file lists Suggested greyed out, with the reason", () => {
  const note = "The suggested run order could not be read: the run-order file is damaged. See Settings → Logs.";
  mount({ unreadableNote: note });
  expect(screen.getByText(note)).toBeInTheDocument();
  fireEvent.click(startFrom());
  const suggested = screen.getByRole("option", { name: "Suggested run order" });
  expect(suggested).toBeDisabled();
  fireEvent.click(suggested);
  expect(startFrom()).toHaveTextContent("Spec order");
});

test("My order is offered when this machine has one, reconciled against the cases", () => {
  mount({ myOrder: [203, 999, 201] });
  fireEvent.click(startFrom());
  expect(optionLabels()).toEqual(["Spec order", "My order"]);
  fireEvent.click(screen.getByRole("option", { name: "My order" }));
  // 999 is not one of the cases and drops out; 202 lands at the end.
  expect(idsOnScreen()).toEqual([203, 201, 202]);
});

test("an uploaded optimized draft offers its tester order", () => {
  watchDraft("C:/work/login.json", [
    { id: 201, order: 2 },
    { id: 203, order: 1 },
  ]);
  mount();
  pick("Tester order from login.json");
  expect(idsOnScreen()).toEqual([203, 201, 202]);
});

test("changing Start from replaces the list, edits included", () => {
  mount({ file: FILE([{ id: 203 }, { id: 201 }, { id: 202 }]), view: "suggested" });
  fireEvent.click(within(orderList()).getByRole("button", { name: "Move #201 up" }));
  expect(idsOnScreen()).toEqual([201, 203, 202]);
  pick("Spec order");
  expect(idsOnScreen()).toEqual([201, 202, 203]);
});

// ---- Use this order ----

test("Use this order on an unchanged Suggested run order switches the view only", () => {
  const { onUseView, onUseMine, onClose } = mount({
    file: FILE([{ id: 203 }, { id: 201 }, { id: 202 }]),
    view: "suggested",
    myOrder: [202, 201, 203],
  });
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));
  expect(onUseView).toHaveBeenCalledWith("suggested");
  expect(onUseMine).not.toHaveBeenCalled();
  expect(onClose).toHaveBeenCalled();
});

test("Use this order on an unchanged Spec order switches the view only", () => {
  const { onUseView, onUseMine } = mount({ file: FILE([{ id: 203 }, { id: 201 }, { id: 202 }]), view: "suggested" });
  pick("Spec order");
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));
  expect(onUseView).toHaveBeenCalledWith("spec");
  expect(onUseMine).not.toHaveBeenCalled();
});

test("Use this order on an unchanged My order just makes it the view", () => {
  const { onUseView, onUseMine } = mount({ myOrder: [203, 201, 202], view: "mine" });
  expect(idsOnScreen()).toEqual([203, 201, 202]);
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));
  expect(onUseView).toHaveBeenCalledWith("mine");
  expect(onUseMine).not.toHaveBeenCalled();
});

test("Use this order after a reorder saves the list as My order", () => {
  const { onUseView, onUseMine, onClose } = mount({
    file: FILE([{ id: 203 }, { id: 201 }, { id: 202 }]),
    view: "suggested",
  });
  fireEvent.click(within(orderList()).getByRole("button", { name: "Move #201 up" }));
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));
  expect(onUseMine).toHaveBeenCalledWith([201, 203, 202]);
  expect(onUseView).not.toHaveBeenCalled();
  expect(onClose).toHaveBeenCalled();
});

test("Use this order on a tester-order start saves My order", () => {
  watchDraft("C:/work/login.json", [
    { id: 201, order: 2 },
    { id: 203, order: 1 },
  ]);
  const { onUseMine } = mount();
  pick("Tester order from login.json");
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));
  expect(onUseMine).toHaveBeenCalledWith([203, 201, 202]);
});

test("when My order could not be saved the modal stays open", () => {
  const onUseMine = vi.fn(() => false);
  const { onClose } = mount({ onUseMine });
  fireEvent.click(within(orderList()).getByRole("button", { name: "Move #202 up" }));
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));
  expect(onUseMine).toHaveBeenCalledWith([202, 201, 203]);
  expect(onClose).not.toHaveBeenCalled();
  expect(screen.getByRole("dialog")).toBeInTheDocument();
});

test("Cancel changes nothing", () => {
  const { onUseView, onUseMine, onClose, calls } = mount();
  fireEvent.click(within(orderList()).getByRole("button", { name: "Move #202 up" }));
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(onClose).toHaveBeenCalled();
  expect(onUseView).not.toHaveBeenCalled();
  expect(onUseMine).not.toHaveBeenCalled();
  expect(calls).toEqual([]);
});

// ---- Save for everyone ----

test("Save for everyone asks first; Cancel sends nothing and goes back", () => {
  const { calls, onClose } = mount();
  fireEvent.click(screen.getByRole("button", { name: "Save for everyone" }));
  expect(screen.getByText(CONFIRM)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Use this order" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.queryByText(CONFIRM)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Use this order" })).toBeInTheDocument();
  expect(onClose).not.toHaveBeenCalled();
  expect(calls.some((c) => c.cmd === "save_run_order")).toBe(false);
});

test("Save sends the list, groups from the tester-order file then the saved file, and switches to Suggested", async () => {
  watchDraft("C:/work/login.json", [
    { id: 203, order: 1, area: "Auth / Lockout" },
    { id: 201, order: 2, area: "Auth" },
    { id: 202, order: 3 },
  ]);
  const NEW = { ...FILE([{ id: 203 }, { id: 201 }, { id: 202 }]), saved_by: "me@example.com" };
  const { calls, qc, onUseView, onClose } = mount(
    { file: FILE([{ id: 201 }, { id: 202, group: "Old" }, { id: 203 }]), view: "suggested" },
    (cmd) => (cmd === "save_run_order" ? NEW : undefined),
  );
  pick("Tester order from login.json");
  fireEvent.click(screen.getByRole("button", { name: "Save for everyone" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

  await vi.waitFor(() => expect(toast.success).toHaveBeenCalledWith("Suggested run order saved."));
  expect(calls.find((c) => c.cmd === "save_run_order")?.args).toEqual({
    organization: "acme",
    project: "Web",
    pbiId: 42,
    cases: [
      { id: 203, group: "Auth / Lockout" },
      { id: 201, group: "Auth" },
      { id: 202, group: "Old" },
    ],
  });
  // The shared entry Run Tests reads carries the new file at once.
  expect(qc.getQueryData(["run-order", "acme", "Web", 42])).toEqual({ state: "found", file: NEW });
  expect(onUseView).toHaveBeenCalledWith("suggested");
  expect(onClose).toHaveBeenCalled();
});

test("started from a stored order, each case keeps the group it had in the saved file", async () => {
  const { calls } = mount(
    { file: FILE([{ id: 201, group: "Auth" }, { id: 202 }, { id: 203 }]), view: "spec" },
    (cmd) => (cmd === "save_run_order" ? FILE([{ id: 201, group: "Auth" }, { id: 203 }, { id: 202 }]) : undefined),
  );
  fireEvent.click(within(orderList()).getByRole("button", { name: "Move #203 up" }));
  fireEvent.click(screen.getByRole("button", { name: "Save for everyone" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(calls.some((c) => c.cmd === "save_run_order")).toBe(true));
  expect(calls.find((c) => c.cmd === "save_run_order")?.args).toEqual({
    organization: "acme",
    project: "Web",
    pbiId: 42,
    cases: [{ id: 201, group: "Auth" }, { id: 203 }, { id: 202 }],
  });
});

test("a failed save says why and keeps the modal open", async () => {
  const { onClose, onUseView } = mount({}, (cmd) =>
    cmd === "save_run_order" ? Promise.reject({ kind: "Network", detail: "Could not reach Azure DevOps." }) : undefined,
  );
  fireEvent.click(screen.getByRole("button", { name: "Save for everyone" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() =>
    expect(toast.error).toHaveBeenCalledWith("Could not save the suggested run order: Could not reach Azure DevOps."),
  );
  expect(onClose).not.toHaveBeenCalled();
  expect(onUseView).not.toHaveBeenCalled();
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Save for everyone" })).toBeInTheDocument();
});

test("without a PBI there is no Save for everyone", () => {
  mount({ pbiId: 0 });
  expect(screen.queryByRole("button", { name: "Save for everyone" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Use this order" })).toBeInTheDocument();
});
```

> Deviation from spec: §7 lists "absent for a suite without a PBI" under Run Tests. Run Tests always has a PBI (`RunPanel` takes a `pbiId`), so the case is pinned here, on the modal's `pbiId` prop.

- [ ] **Step 4: Run the new tests to see them fail**

Run: `npx vitest run src/lib/runOrder.test.ts src/screens/RunPanel/useRunOrder.test.tsx src/screens/RunPanel/ExecutionOrderModal.test.tsx --exclude "**/.claude/**"`
Expected: FAIL. `runOrderPayload` is not exported, `specCases`/`saveMine`/`file`/`loading` are undefined, and `./ExecutionOrderModal` cannot be resolved.

- [ ] **Step 5: Add `runOrderPayload` to `src/lib/runOrder.ts`**

Edit the imports. Old:

```ts
import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
```

New:

```ts
import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { RunOrderCase } from "../bindings";
```

Insert after the end of `resortUpcoming` (just before `const orderKey = (k: OrderKey) =>`):

```ts
/** The cases a suggested run order is saved with, in `ids` order. A case's
 * group is the started-from draft's area when the start was a tester-order
 * file (`fileGroups`), else the group it had in the saved file (`saved`),
 * else none - an empty group is never written (design doc §4.2). */
export function runOrderPayload(
  ids: readonly number[],
  saved: readonly { id: number; group?: string | null }[] | null,
  fileGroups?: ReadonlyMap<number, string>,
): RunOrderCase[] {
  const savedGroup = new Map(saved?.map((c) => [c.id, c.group]) ?? []);
  return ids.map((id) => {
    const group = fileGroups?.get(id) ?? savedGroup.get(id);
    return group ? { id, group } : { id };
  });
}

```

- [ ] **Step 6: Make `SuggestedOrder` use it (no second copy of the rule)**

In `src/screens/ManageCases/SuggestedOrder.tsx`, edit line 4. Old:

```ts
import { commands, type RunOrderCase } from "../../bindings";
```

New:

```ts
import { commands } from "../../bindings";
```

Edit line 12. Old:

```ts
import { loadMyOrder, onMyOrderChanged, reconcile, type OrderKey } from "../../lib/runOrder";
```

New:

```ts
import { loadMyOrder, onMyOrderChanged, reconcile, runOrderPayload, type OrderKey } from "../../lib/runOrder";
```

Edit the mutation. Old:

```ts
    mutationFn: () => {
      // Each case keeps the group it had in the saved file; a case with no
      // prior group (or no saved file at all) gets none. Started from a
      // draft's tester order, the draft's areas come first - the groups an
      // upload of that file would have written.
      const groupOf = new Map(file?.cases.map((c) => [c.id, c.group]) ?? []);
      const fileGroups = testerSourceFor(startFrom)?.groups;
      const payload: RunOrderCase[] = order.map((c) => {
        const group = fileGroups?.get(c.id) ?? groupOf.get(c.id);
        return group ? { id: c.id, group } : { id: c.id };
      });
      return unwrap(commands.saveRunOrder(org, project, pbiId, payload));
    },
```

New:

```ts
    mutationFn: () =>
      unwrap(
        commands.saveRunOrder(
          org,
          project,
          pbiId,
          runOrderPayload(
            order.map((c) => c.id),
            file?.cases ?? null,
            testerSourceFor(startFrom)?.groups,
          ),
        ),
      ),
```

- [ ] **Step 7: Extend `useRunOrder`**

In `src/screens/RunPanel/useRunOrder.ts`, add `specCases` after `specIds`. Old:

```ts
  const specIds = useMemo(
    () => specOrderOf(points, suiteCases.data?.map((c) => c.id)),
    [points, suiteCases.data],
  );
```

New:

```ts
  const specIds = useMemo(
    () => specOrderOf(points, suiteCases.data?.map((c) => c.id)),
    [points, suiteCases.data],
  );
  // The same cases with their titles, for the Execution order modal's list:
  // a case is titled by its first point's name.
  const specCases = useMemo<SuiteCase[]>(() => {
    const titles = new Map<number, string>();
    for (const p of points) {
      if (p.test_case_id != null && !titles.has(p.test_case_id)) titles.set(p.test_case_id, p.test_case_name);
    }
    return specIds.map((id) => ({ id, title: titles.get(id) ?? `Test case ${id}` }));
  }, [points, specIds]);
```

Replace `commit` with `saveMine` plus a `commit` that goes through it. Old:

```ts
  const commit = (next: SuiteCase[]) => {
    if (!key) return;
    const ids = next.map((c) => c.id);
    saveMyOrder(key, ids);
    // Read it back: with storage unavailable the save is silently dropped,
    // and switching to a My order that is not there would mislead.
    const saved = loadMyOrder(key);
    if (!saved || saved.length !== ids.length || saved.some((id, i) => id !== ids[i])) {
      toast.error("Your own order could not be saved on this machine.");
      return;
    }
    if (view !== "mine") {
      saveOrderView(key, "mine");
      toast.info("Now using your own order, on this machine.");
    }
    bump();
  };
```

New:

```ts
  /** `ids` as My order on this machine, and My order as the list's order:
   * the Execution order modal's Use this order on a list of the tester's
   * own. False (after saying so) when storage dropped the save - switching
   * to a My order that is not there would mislead. */
  const saveMine = (ids: readonly number[]): boolean => {
    if (!key) return false;
    saveMyOrder(key, ids);
    // Read it back: with storage unavailable the save is silently dropped.
    const saved = loadMyOrder(key);
    if (!saved || saved.length !== ids.length || saved.some((id, i) => id !== ids[i])) {
      toast.error("Your own order could not be saved on this machine.");
      return false;
    }
    saveOrderView(key, "mine");
    bump();
    return true;
  };
  const commit = (next: SuiteCase[]) => {
    const wasMine = view === "mine";
    if (saveMine(next.map((c) => c.id)) && !wasMine) toast.info("Now using your own order, on this machine.");
  };
```

Extend the return. Old:

```ts
  return {
    view,
    options,
    changeView,
    note,
```

New:

```ts
  return {
    view,
    options,
    changeView,
    saveMine,
    note,
    file,
    loading: runOrder.isLoading,
    myOrder,
    specCases,
```

- [ ] **Step 8: Write the modal**

Create `src/screens/RunPanel/ExecutionOrderModal.tsx`:

```tsx
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { toast } from "sonner";
import { commands, type RunOrderFile } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import { IconCancel, IconConfirm, IconShare } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { loadWatches } from "../../lib/fileSync";
import { unwrap } from "../../lib/ipc";
import { reconcile, runOrderPayload, type OrderView } from "../../lib/runOrder";
import { sameOrder, type SuiteCase } from "../../lib/suiteOrder";
import { testerOrderSources } from "../../lib/testerOrderStart";
import CaseOrderList from "../ManageCases/CaseOrderList";
import { ORDER_LABELS, runOrderQueryOptions } from "./useRunOrder";

/** A stored order, or `file:<path>` for one watched draft's tester order. */
type StartFrom = OrderView | `file:${string}`;

const isStored = (from: StartFrom): from is OrderView => from === "suggested" || from === "spec" || from === "mine";

export type ExecutionOrderModalProps = {
  org: string;
  project: string;
  /** 0 when the suite has no PBI: Save for everyone is then absent. */
  pbiId: number;
  /** The cases Run Tests lists, in spec order, with their titles. */
  cases: SuiteCase[];
  /** The list's active order: where Start from begins. */
  view: OrderView;
  file: RunOrderFile | null;
  /** `noteFor(reason)` when the suggested run order could not be read, else null. */
  unreadableNote: string | null;
  /** The run-order read has not settled yet. */
  loading: boolean;
  myOrder: readonly number[] | null;
  /** Use this order on a stored order left exactly as it is: switch the view only. */
  onUseView: (v: OrderView) => void;
  /** Use this order on a list of the tester's own: store it as My order. False when it could not be saved. */
  onUseMine: (ids: number[]) => boolean;
  onClose: () => void;
};

/**
 * Run Tests' one place to set the execution order (execution-order-modal
 * design §5). Start from a stored order or an uploaded draft's tester
 * order, arrange the cases, then either use the list on this machine or
 * save it as the PBI's suggested run order for every tester.
 *
 * Reordering happens only here: the Run Tests list shows the chosen order
 * and never edits it. Nothing here calls `reorderSuiteCases` - the order in
 * Azure DevOps stays Suite Management's alone.
 */
export default function ExecutionOrderModal({
  org,
  project,
  pbiId,
  cases,
  view,
  file,
  unreadableNote,
  loading,
  myOrder,
  onUseView,
  onUseMine,
  onClose,
}: ExecutionOrderModalProps) {
  const qc = useQueryClient();
  const specIds = useMemo(() => cases.map((c) => c.id), [cases]);
  const byId = useMemo(() => new Map(cases.map((c) => [c.id, c])), [cases]);
  const toCases = (ids: readonly number[]): SuiteCase[] =>
    ids.map((id) => byId.get(id) ?? { id, title: `Test case ${id}` });

  // An optimized draft whose cases were all uploaded earlier: the upload
  // saved no suggested order for it, but the Import tab still remembers the
  // file with its ids and tester order, so it can be started from here.
  const testerSources = useMemo(
    () => testerOrderSources(loadWatches(org, pbiId), specIds),
    [org, pbiId, specIds],
  );
  const testerSourceFor = (from: StartFrom) => testerSources.find((s) => `file:${s.path}` === from);

  /** The list a start gives, reconciled against the cases (design doc §4.4). */
  const idsFor = (from: StartFrom): number[] => {
    if (from === "suggested" && file) return reconcile(file.cases.map((c) => c.id), specIds);
    if (from === "mine" && myOrder) return reconcile(myOrder, specIds);
    const tester = testerSourceFor(from);
    return tester ? reconcile(tester.ids, specIds) : specIds;
  };

  const [startFrom, setStartFrom] = useState<StartFrom>(view);
  const [order, setOrder] = useState<SuiteCase[]>(() => toCases(idsFor(view)));
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [confirming, setConfirming] = useState(false);

  // An unreadable file still lists Suggested, greyed out: the tester sees
  // there is one, and the note says why it cannot be used.
  const startOptions: { value: StartFrom; label: string; disabled: boolean }[] = [
    ...(file || unreadableNote != null
      ? [{ value: "suggested" as const, label: ORDER_LABELS.suggested, disabled: !file }]
      : []),
    { value: "spec", label: ORDER_LABELS.spec, disabled: false },
    ...(myOrder ? [{ value: "mine" as const, label: ORDER_LABELS.mine, disabled: false }] : []),
    ...testerSources.map((s) => ({ value: `file:${s.path}` as const, label: s.label, disabled: false })),
  ];

  const changeStartFrom = (from: StartFrom) => {
    setStartFrom(from);
    setOrder(toCases(idsFor(from)));
    setSelected(new Set());
  };

  // A stored order left exactly as it is changes only which order the list
  // shows, so a stored My order survives a switch to Suggested or Spec.
  // Anything else is the tester's own list.
  const applyOrder = () => {
    if (isStored(startFrom) && sameOrder(order, toCases(idsFor(startFrom)))) {
      onUseView(startFrom);
      onClose();
    } else if (onUseMine(order.map((c) => c.id))) {
      onClose();
    }
  };

  const runOrderKey = runOrderQueryOptions(org, project, pbiId).queryKey;
  const save = useMutation({
    mutationFn: () =>
      unwrap(
        commands.saveRunOrder(
          org,
          project,
          pbiId,
          runOrderPayload(
            order.map((c) => c.id),
            file?.cases ?? null,
            testerSourceFor(startFrom)?.groups,
          ),
        ),
      ),
    onSuccess: (newFile) => {
      toast.success("Suggested run order saved.");
      // Run Tests sees the new file at once...
      qc.setQueryData(runOrderKey, { state: "found" as const, file: newFile });
      // ...and the disk copy is rewritten too, or the next launch would
      // paint the old order from disk before asking Azure DevOps again.
      qc.invalidateQueries({ queryKey: runOrderKey });
      onUseView("suggested");
      onClose();
    },
    onError: (e) => {
      setConfirming(false);
      toast.error(`Could not save the suggested run order: ${e.message}`);
    },
  });

  // Not while a save is on its way: its answer has to land on this modal.
  const close = () => {
    if (!save.isPending) onClose();
  };

  const note =
    unreadableNote ??
    (file
      ? `Saved by ${file.saved_by} on ${new Date(file.saved_at).toLocaleDateString()}`
      : loading
        ? null
        : "No suggested run order yet.");

  return (
    <Modal onClose={close} className="flex max-h-[85vh] w-[640px] max-w-full flex-col gap-3 p-4">
      <h2 className="text-sm font-semibold text-text">Execution order</h2>
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-xs text-muted">Start from</span>
        <Select
          aria-label="Start from"
          className="w-72"
          triggerClassName="px-2 py-1.5"
          value={startFrom}
          disabled={save.isPending}
          onChange={(e) => changeStartFrom(e.target.value as StartFrom)}
        >
          {startOptions.map((o) => (
            <option key={o.value} value={o.value} disabled={o.disabled}>
              {o.label}
            </option>
          ))}
        </Select>
      </div>
      {note && <p className={cn("text-xs", unreadableNote ? "text-warning" : "text-muted")}>{note}</p>}
      <div className="min-h-0 flex-1 overflow-y-auto">
        <CaseOrderList
          cases={order}
          selected={selected}
          onChange={setOrder}
          onSelect={setSelected}
          ariaLabel="Execution order"
          disabled={save.isPending}
        />
      </div>
      {confirming ? (
        <div className="space-y-3 rounded-md border border-border bg-surface-2 p-3">
          <p className="text-sm text-text">Every tester will see this as the suggested run order for this PBI.</p>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" size="sm" disabled={save.isPending} onClick={() => setConfirming(false)}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button size="sm" disabled={save.isPending} onClick={() => save.mutate()}>
              <IconConfirm aria-hidden />
              {save.isPending ? "Saving" : "Save"}
            </Button>
          </div>
        </div>
      ) : (
        <div className="flex items-center justify-end gap-2">
          <Button variant="ghost" size="sm" onClick={close}>
            <IconCancel aria-hidden />
            Cancel
          </Button>
          {pbiId > 0 && (
            <Button
              variant="outline"
              size="sm"
              title="Save as the suggested run order every tester starts from"
              onClick={() => setConfirming(true)}
            >
              <IconShare aria-hidden />
              Save for everyone
            </Button>
          )}
          <Button size="sm" onClick={applyOrder}>
            <IconConfirm aria-hidden />
            Use this order
          </Button>
        </div>
      )}
    </Modal>
  );
}
```

- [ ] **Step 9: Run the tests to see them pass, plus the files the change touched**

Run: `npx vitest run src/lib/runOrder.test.ts src/screens/RunPanel/useRunOrder.test.tsx src/screens/RunPanel/ExecutionOrderModal.test.tsx src/screens/ManageCases/SuggestedOrder.test.tsx src/screens/RunPanel.test.tsx src/ui-consistency.test.ts --exclude "**/.claude/**"`
Expected: PASS. `SuggestedOrder.test.tsx` passing proves the extracted payload keeps its group rule. `RunPanel.test.tsx` passing proves `commit` still behaves the same (move → My order + one info toast; storage failure → error toast).

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 10: Commit**

```bash
git add src/lib/runOrder.ts src/lib/runOrder.test.ts src/screens/ManageCases/SuggestedOrder.tsx src/screens/RunPanel/useRunOrder.ts src/screens/RunPanel/useRunOrder.test.tsx src/screens/RunPanel/ExecutionOrderModal.tsx src/screens/RunPanel/ExecutionOrderModal.test.tsx
git commit -F - <<'EOF'
feat(v2): an Execution order modal that sets my order or everyone's

Start from the suggested run order, spec order, my order or an uploaded
draft's tester order; arrange the cases; then Use this order (this
machine) or Save for everyone (the PBI's suggested run order, after a
confirm step). The save payload's group rule now lives once, in
runOrderPayload. Not yet wired into Run Tests.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1 --stat
```

---

### Task 2: Run Tests sets its order only through the modal

**Files:**
- Modify: `src/lib/actionIcons.ts` (add `IconSetOrder`, after `ArrowDown as IconMoveDown,` at line 45)
- Modify: `src/screens/RunPanel/index.tsx` (imports lines 2 and 29-38; drag state lines 286-289; toolbar lines 464-490; group header lines 606-630; row lines 646-668, 671, 690-720)
- Replace: `src/screens/RunPanel/useRunOrder.ts` (whole file: drop the list's own reordering)
- Modify: `src/screens/RunPanel.test.tsx` (import line 3; `OrderMock`/`mockOrder` lines 562-614; everything from line 622 to the end)

**Interfaces:**
- Consumes: `ExecutionOrderModal` and `ExecutionOrderModalProps` (Task 1); from `useRunOrder`: `view`, `changeView(v: OrderView): void`, `saveMine(ids: readonly number[]): boolean`, `note`, `file`, `loading`, `myOrder`, `specCases`, `ordered`, `sections`.
- Produces: `useRunOrder(...)` returns exactly `{ view, changeView, saveMine, note, file, loading, myOrder, specCases, ordered, sections }`. Removed: `options`, `canMove`, `moveCase`, `dropCase`, `canMoveGroup`, `moveGroup`, `resetLabel`, `reset`, `displayOrder`. `IconSetOrder` is added to the icon vocabulary.

- [ ] **Step 1: Rewrite the Run Tests order tests (they fail against the current screen)**

In `src/screens/RunPanel.test.tsx`, line 3. Old:

```ts
import { fireEvent, render, screen } from "@testing-library/react";
```

New:

```ts
import { fireEvent, render, screen, within } from "@testing-library/react";
```

Replace the `OrderMock` type and `mockOrder` function (lines 562-614). Old: everything from `type OrderMock = {` through the closing `}` of `mockOrder` (the line after `}, { shouldMockEvents: true });`). New:

```tsx
type OrderMock = {
  points: Array<{ point_id: number; test_case_id: number; name: string; config?: string }>;
  entries?: number[];
  /** What get_run_order answers. A function is asked on every read, so a
   * save can change what the next read sees, as the real backend does. */
  runOrder?: unknown;
  /** get_run_order fails with this AdoError instead of answering. */
  runOrderError?: unknown;
  /** Answers save_run_order, given its arguments. */
  onSave?: (args: unknown) => unknown;
  calls?: string[];
};

const RUN_ORDER_FILE = (cases: Array<{ id: number; group?: string }>) => ({
  state: "found",
  file: {
    format: "tcm-run-order",
    version: 1,
    saved_by: "lead@example.com",
    saved_at: "2026-09-23T10:15:00Z",
    cases,
  },
});

// Events are mocked for real here (shouldMockEvents), so an emit reaches
// the screen's listeners the way another window's save would.
function mockOrder({ points, entries, runOrder, runOrderError, onSave, calls }: OrderMock) {
  mockIPC((cmd, args) => {
    calls?.push(cmd);
    switch (cmd) {
      case "run_history":
        return [];
      case "ensure_pbi_suite":
        return { plan_id: 9, plan_name: "Auth - Test Plan", suite_id: 91 };
      case "list_test_points":
        return points.map((p) => ({
          point_id: p.point_id,
          test_case_id: p.test_case_id,
          test_case_name: p.name,
          config_name: p.config ?? "Windows 10",
          tester: "",
          last_outcome: "",
          last_run_id: null,
          last_result_id: null,
        }));
      case "list_suite_entries":
        return (entries ?? points.map((p) => p.test_case_id)).map((id, i) => ({
          id,
          sequence_number: i + 1,
          entry_type: "testCase",
        }));
      case "get_run_order":
        if (runOrderError) return Promise.reject(runOrderError);
        return (typeof runOrder === "function" ? runOrder() : runOrder) ?? { state: "none" };
      case "save_run_order":
        return onSave?.(args);
    }
  }, { shouldMockEvents: true });
}
```

(The old `RUN_ORDER_FILE` constant sat between `OrderMock` and `mockOrder`. It is part of the replaced span and appears once, above.)

Keep the `ABC` constant (lines 616-620) as it is. Replace everything from the line `/** The rows' titles top to bottom, read off each row's Move up button. */` to the end of the file with:

```tsx
/** The rows' titles top to bottom, as the table shows them. A fold's
 * closing copy (lib/exitGhost, e.g. after Group by title regroups the
 * list) is a picture, not rows, so it is skipped. */
const rowNames = () =>
  Array.from(document.querySelectorAll("tbody tr .id-mono"))
    .filter((el) => !el.closest("[data-exit-ghost]"))
    .map((el) => (el.parentElement?.textContent ?? "").replace(/^#\d+\s*/, ""));

/** The muted line beside Set execution order that names the list's order. */
const activeOrder = () => screen.getByRole("status");
const openOrderModal = () => fireEvent.click(screen.getByRole("button", { name: "Set execution order" }));
const startFrom = () => screen.getByRole("combobox", { name: "Start from" });
const modalList = () => screen.getByRole("list", { name: "Execution order" });
const rowOf = (title: string) => screen.getByText(title).closest("tr")!;
const MY_KEY = "tcm-v2-run-order:acme/9/91";
const VIEW_KEY = "tcm-v2-run-order-view:acme/9/91";
const CONFIRM = "Every tester will see this as the suggested run order for this PBI.";

/** Remembers a watched draft for PBI 42 the way the Import tab does. */
function watchDraft(path: string, cases: Array<{ id: number | null; order: number | null; area?: string }>) {
  const snapshot = cases.map((c, i) => ({
    title: `Case ${i}`,
    steps: [],
    tags: "",
    automation_status: "Not Automated",
    module_value: "",
    preconditions: "",
    update_id: c.id,
    tester_order: c.order,
    area: c.area ?? "",
  }));
  localStorage.setItem("tcm-v2-watch:acme/42", JSON.stringify([{ path, stamp: "s1", snapshot }]));
}

test("opens in the suggested run order when the PBI has one, not the points' order", async () => {
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 302 }, { id: 303 }, { id: 301 }]) });
  renderPanel();
  await screen.findByText("Alpha check");
  await vi.waitFor(() => expect(activeOrder()).toHaveTextContent("Suggested run order"));
  await vi.waitFor(() => expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]));
});

test("the list has no order controls of its own; one button opens the modal", async () => {
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 302 }, { id: 303 }, { id: 301 }]) });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]));

  expect(screen.queryByRole("combobox", { name: "Order" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^Move / })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^Reset to/ })).not.toBeInTheDocument();
  expect(rowOf("Alpha check")).not.toHaveAttribute("draggable");

  openOrderModal();
  expect(screen.getByRole("heading", { name: "Execution order" })).toBeInTheDocument();
  expect(startFrom()).toHaveTextContent("Suggested run order");
});

test("with no suggested order the list follows spec order and the modal offers only Spec order", async () => {
  mockOrder({ points: ABC, entries: [303, 301, 302] });
  renderPanel();
  await screen.findByText("Alpha check");
  await vi.waitFor(() => expect(rowNames()).toEqual(["Charlie check", "Alpha check", "Bravo check"]));
  expect(activeOrder()).toHaveTextContent("Spec order");

  openOrderModal();
  expect(await within(screen.getByRole("dialog")).findByText("No suggested run order yet.")).toBeInTheDocument();
  fireEvent.click(startFrom());
  expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["Spec order"]);
});

/// The disk seed (design doc §4.4's reconciled order) is fresh the moment
/// the screen mounts, so a plain Refresh must still force a real re-read
/// of both the run-order file and the suite's cases - otherwise Refresh
/// looks like it worked but keeps serving what was cached before an
/// upload or a save for everyone landed.
test("Refresh outcomes also re-reads the suggested run order and the suite's cases", async () => {
  const calls: string[] = [];
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 302 }, { id: 303 }, { id: 301 }]), calls });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]));

  const before = {
    order: calls.filter((c) => c === "get_run_order").length,
    entries: calls.filter((c) => c === "list_suite_entries").length,
  };

  fireEvent.click(screen.getByRole("button", { name: "Refresh outcomes" }));

  await vi.waitFor(() => {
    expect(calls.filter((c) => c === "get_run_order").length).toBeGreaterThan(before.order);
    expect(calls.filter((c) => c === "list_suite_entries").length).toBeGreaterThan(before.entries);
  });
});

test("an unreadable run-order file says why, and the modal greys out Suggested", async () => {
  mockOrder({ points: ABC, runOrder: { state: "unreadable", reason: "the run-order file is damaged" } });
  renderPanel();
  const note = "The suggested run order could not be read: the run-order file is damaged. See Settings → Logs.";
  expect(await screen.findByText(note)).toBeInTheDocument();
  expect(activeOrder()).toHaveTextContent("Spec order");

  openOrderModal();
  expect(within(screen.getByRole("dialog")).getByText(note)).toBeInTheDocument();
  fireEvent.click(startFrom());
  const suggested = screen.getByRole("option", { name: "Suggested run order" });
  expect(suggested).toBeDisabled();
  fireEvent.click(suggested);
  expect(startFrom()).toHaveTextContent("Spec order");
});

test("a reason that already points at the logs is not told to go there twice", async () => {
  mockOrder({
    points: ABC,
    runOrder: { state: "unreadable", reason: "the file is damaged; Settings → Logs has the details." },
  });
  renderPanel();
  expect(
    await screen.findByText(
      "The suggested run order could not be read: the file is damaged; Settings → Logs has the details.",
    ),
  ).toBeInTheDocument();
  expect(screen.queryByText(/See Settings/)).not.toBeInTheDocument();
});

test("a failed read of the run order shows the note, and the modal greys out Suggested", async () => {
  mockOrder({ points: ABC, runOrderError: { kind: "Forbidden" } });
  renderPanel();
  expect(
    await screen.findByText(
      "The suggested run order could not be read: You don't have permission for this resource. See Settings → Logs.",
    ),
  ).toBeInTheDocument();
  expect(activeOrder()).toHaveTextContent("Spec order");
  openOrderModal();
  fireEvent.click(startFrom());
  expect(screen.getByRole("option", { name: "Suggested run order" })).toBeDisabled();
});

test("Start from lists My order and an uploaded draft's tester order", async () => {
  localStorage.setItem(MY_KEY, JSON.stringify([303, 301, 302]));
  watchDraft("C:/work/login.json", [
    { id: 301, order: 2 },
    { id: 303, order: 1 },
  ]);
  mockOrder({ points: ABC });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));
  openOrderModal();
  fireEvent.click(startFrom());
  expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual([
    "Spec order",
    "My order",
    "Tester order from login.json",
  ]);
});

test("Use this order on an unchanged Suggested run order sets the view only; My order is kept", async () => {
  localStorage.setItem(MY_KEY, JSON.stringify([303, 301, 302]));
  localStorage.setItem(VIEW_KEY, "spec");
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 302 }, { id: 303 }, { id: 301 }]) });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));
  expect(activeOrder()).toHaveTextContent("Spec order");

  openOrderModal();
  fireEvent.click(startFrom());
  fireEvent.click(screen.getByRole("option", { name: "Suggested run order" }));
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));

  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(activeOrder()).toHaveTextContent("Suggested run order");
  expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]);
  expect(localStorage.getItem(VIEW_KEY)).toBe("suggested");
  expect(JSON.parse(localStorage.getItem(MY_KEY) as string)).toEqual([303, 301, 302]);
});

test("a reorder in the modal becomes My order on this machine and never writes a shared order", async () => {
  const calls: string[] = [];
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 302 }, { id: 303 }, { id: 301 }]), calls });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]));

  openOrderModal();
  fireEvent.click(within(modalList()).getByRole("button", { name: "Move #302 down" }));
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));

  expect(activeOrder()).toHaveTextContent("My order");
  expect(rowNames()).toEqual(["Charlie check", "Bravo check", "Alpha check"]);
  expect(JSON.parse(localStorage.getItem(MY_KEY) as string)).toEqual([303, 302, 301]);
  expect(localStorage.getItem(VIEW_KEY)).toBe("mine");
  expect(calls).not.toContain("reorder_suite_cases");
  expect(calls).not.toContain("save_run_order");
});

test("Use this order on a tester-order start saves it as My order", async () => {
  watchDraft("C:/work/login.json", [
    { id: 301, order: 2 },
    { id: 303, order: 1 },
  ]);
  mockOrder({ points: ABC });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));

  openOrderModal();
  fireEvent.click(startFrom());
  fireEvent.click(screen.getByRole("option", { name: "Tester order from login.json" }));
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));

  expect(JSON.parse(localStorage.getItem(MY_KEY) as string)).toEqual([303, 301, 302]);
  expect(activeOrder()).toHaveTextContent("My order");
  expect(rowNames()).toEqual(["Charlie check", "Alpha check", "Bravo check"]);
});

test("Save for everyone asks first; Cancel sends nothing", async () => {
  const calls: string[] = [];
  mockOrder({ points: ABC, calls });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));

  openOrderModal();
  fireEvent.click(screen.getByRole("button", { name: "Save for everyone" }));
  expect(screen.getByText(CONFIRM)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

  expect(screen.queryByText(CONFIRM)).not.toBeInTheDocument();
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  expect(calls).not.toContain("save_run_order");
});

test("Save for everyone sends the list with the saved groups, toasts, and switches the list to Suggested", async () => {
  const NEW = RUN_ORDER_FILE([{ id: 301, group: "Auth" }, { id: 303 }, { id: 302 }]);
  let current: unknown = RUN_ORDER_FILE([{ id: 301, group: "Auth" }, { id: 302 }, { id: 303 }]);
  const saved: unknown[] = [];
  localStorage.setItem(VIEW_KEY, "spec");
  mockOrder({
    points: ABC,
    runOrder: () => current,
    onSave: (args) => {
      saved.push(args);
      current = NEW;
      return NEW.file;
    },
  });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));
  expect(activeOrder()).toHaveTextContent("Spec order");

  openOrderModal();
  expect(startFrom()).toHaveTextContent("Spec order");
  fireEvent.click(within(modalList()).getByRole("button", { name: "Move #303 up" }));
  fireEvent.click(screen.getByRole("button", { name: "Save for everyone" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

  await vi.waitFor(() => expect(toast.success).toHaveBeenCalledWith("Suggested run order saved."));
  expect(saved).toEqual([
    {
      organization: "acme",
      project: "Web",
      pbiId: 42,
      cases: [{ id: 301, group: "Auth" }, { id: 303 }, { id: 302 }],
    },
  ]);
  await vi.waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  expect(activeOrder()).toHaveTextContent("Suggested run order");
  expect(rowNames()).toEqual(["Alpha check", "Charlie check", "Bravo check"]);
});

test("a failed Save for everyone keeps the modal open and says why", async () => {
  mockOrder({
    points: ABC,
    onSave: () => Promise.reject({ kind: "Network", detail: "Could not reach Azure DevOps." }),
  });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));

  openOrderModal();
  fireEvent.click(screen.getByRole("button", { name: "Save for everyone" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

  await vi.waitFor(() =>
    expect(toast.error).toHaveBeenCalledWith("Could not save the suggested run order: Could not reach Azure DevOps."),
  );
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  expect(activeOrder()).toHaveTextContent("Spec order");
});

test("grouping follows the suggested file's groups, and groups have no move buttons", async () => {
  mockOrder({
    points: ABC,
    runOrder: RUN_ORDER_FILE([
      { id: 301, group: "Web\\Sign in" },
      { id: 302, group: "Web\\Checkout" },
      { id: 303, group: "Web\\Sign in" },
    ]),
  });
  renderPanel();
  await vi.waitFor(() => expect(activeOrder()).toHaveTextContent("Suggested run order"));
  fireEvent.click(screen.getByText("Group by title"));

  expect(screen.getByText("Web\\Sign in (2)")).toBeInTheDocument();
  expect(screen.getByText("Web\\Checkout (1)")).toBeInTheDocument();
  expect(rowNames()).toEqual(["Alpha check", "Charlie check", "Bravo check"]);
  expect(screen.queryByRole("button", { name: /^Move group / })).not.toBeInTheDocument();
});

test("a case run on two configurations keeps both rows together", async () => {
  mockOrder({
    points: [
      { point_id: 1, test_case_id: 301, name: "Alpha check", config: "Windows" },
      { point_id: 2, test_case_id: 302, name: "Bravo check" },
      { point_id: 3, test_case_id: 301, name: "Alpha check", config: "Mac" },
    ],
    entries: [302, 301],
  });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Bravo check", "Alpha check", "Alpha check"]));
});

test("the runner opens with the selection in the order on screen", async () => {
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 303 }, { id: 301 }, { id: 302 }]) });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Charlie check", "Alpha check", "Bravo check"]));

  fireEvent.click(screen.getByText("Alpha check"));
  fireEvent.click(screen.getByText("Charlie check"));
  fireEvent.click(screen.getByRole("button", { name: /Run 2 in runner/ }));

  const session = JSON.parse(localStorage.getItem("tcm-v2-runner-session") as string);
  expect(session.caseIds).toEqual([303, 301]);
});

test("the chosen order is remembered for the suite across a remount, and My order is kept", async () => {
  localStorage.setItem(MY_KEY, JSON.stringify([303, 302, 301]));
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 302 }, { id: 303 }, { id: 301 }]) });
  const first = renderPanel();
  await vi.waitFor(() => expect(activeOrder()).toHaveTextContent("Suggested run order"));
  await vi.waitFor(() => expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]));

  openOrderModal();
  fireEvent.click(startFrom());
  fireEvent.click(screen.getByRole("option", { name: "Spec order" }));
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));
  first.unmount();

  // No findByText on a title here: the closed modal's fading copy still
  // shows the same titles for a moment. rowNames reads the table only.
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));
  expect(activeOrder()).toHaveTextContent("Spec order");
  expect(JSON.parse(localStorage.getItem(MY_KEY) as string)).toEqual([303, 302, 301]);
});
```

- [ ] **Step 2: Run the Run Tests suite to see it fail**

Run: `npx vitest run src/screens/RunPanel.test.tsx --exclude "**/.claude/**"`
Expected: FAIL. There is no `Set execution order` button and no `status` element; `combobox "Order"` and the Move buttons still exist.

- [ ] **Step 3: Add the button icon**

In `src/lib/actionIcons.ts`, old:

```ts
  ArrowUp as IconMoveUp,
  ArrowDown as IconMoveDown,
```

New:

```ts
  ArrowUp as IconMoveUp,
  ArrowDown as IconMoveDown,
  // Choosing the order a list of cases runs in, in one dialog: a numbered
  // list, because what it sets is which case comes first.
  ListOrdered as IconSetOrder,
```

- [ ] **Step 4: Replace `src/screens/RunPanel/useRunOrder.ts` with the list-only version**

Write the whole file:

```ts
// The order Run Tests lists its cases in (run-order design §5.1, as
// amended by the execution-order-modal design): the PBI's suggested run
// order, the suite's spec order, or this tester's own order on this
// machine. Kept out of the screen so the screen only wires it in.
//
// The list never changes an order by itself. The Execution order modal
// picks one: `changeView` for a stored order as it is, `saveMine` for a
// list of the tester's own (My order, on this machine). Nothing here calls
// reorderSuiteCases or saveRunOrder - Save for everyone is the modal's own
// call.

import { useQuery } from "@tanstack/react-query";
import { useMemo, useReducer } from "react";
import { toast } from "sonner";
import { commands, type TestPoint } from "../../bindings";
import { CACHE, cacheKeys, persistentQuery } from "../../lib/cache";
import { groupIndices } from "../../lib/grouping";
import { unwrap } from "../../lib/ipc";
import {
  loadMyOrder,
  loadOrderView,
  reconcile,
  saveMyOrder,
  saveOrderView,
  type OrderKey,
  type OrderView,
} from "../../lib/runOrder";
import type { SuiteCase } from "../../lib/suiteOrder";
import { loadSuiteCases, suiteCasesKey } from "../ManageCases/suiteCasesQuery";

export type PointSection = { name: string; pts: TestPoint[] };

export const ORDER_LABELS: Record<OrderView, string> = {
  suggested: "Suggested run order",
  spec: "Spec order",
  mine: "My order",
};

/** The points' distinct case ids in spec order: by position in the suite's
 * entries when those are known, otherwise (or for a case the entries do
 * not list) in the points' own order. Only cases that have a row are in
 * it, so the order never names something invisible. */
export function specOrderOf(points: readonly TestPoint[], suiteIds: readonly number[] | undefined): number[] {
  const seen = new Set<number>();
  const ids: number[] = [];
  for (const p of points) {
    if (p.test_case_id != null && !seen.has(p.test_case_id)) {
      seen.add(p.test_case_id);
      ids.push(p.test_case_id);
    }
  }
  if (!suiteIds) return ids;
  const at = new Map<number, number>();
  suiteIds.forEach((id, i) => at.set(id, i));
  // Array#sort is stable: cases the entries do not list keep the points'
  // order among themselves, after every listed one.
  return [...ids].sort((a, b) => (at.get(a) ?? Infinity) - (at.get(b) ?? Infinity));
}

/** Points sorted by their case's place in `order`. Stable, so a case's
 * several configurations stay together in their own order; points with no
 * case id go last. */
export function sortPoints(points: readonly TestPoint[], order: readonly number[]): TestPoint[] {
  const at = new Map<number, number>();
  order.forEach((id, i) => at.set(id, i));
  const rank = (p: TestPoint) =>
    p.test_case_id == null ? Number.MAX_SAFE_INTEGER : (at.get(p.test_case_id) ?? Number.MAX_SAFE_INTEGER - 1);
  return [...points].sort((a, b) => rank(a) - rank(b));
}

/** The ordered points as sections: by the suggested file's groups when it
 * has any, else by title. Each section gathers its members and sections
 * follow their first case, so the list reads top to bottom in the order. */
export function sectionsFor(ordered: readonly TestPoint[], groupOf: Map<number, string> | null): PointSection[] {
  let keys: string[];
  if (groupOf) {
    keys = ordered.map((p) => (p.test_case_id != null ? (groupOf.get(p.test_case_id) ?? "") : ""));
  } else {
    keys = new Array<string>(ordered.length).fill("");
    for (const g of groupIndices(ordered.map((p) => p.test_case_name))) {
      for (const i of g.indices) keys[i] = g.name;
    }
  }
  const byName = new Map<string, TestPoint[]>();
  ordered.forEach((p, i) => {
    const name = keys[i] || "Ungrouped";
    const list = byName.get(name);
    if (list) list.push(p);
    else byName.set(name, [p]);
  });
  return [...byName].map(([name, pts]) => ({ name, pts }));
}

/** A reason sentence ends in a full stop before the pointer to the logs;
 * the reasons from the backend are fragments without one. */
export const sentence = (s: string) => (/[.!?]$/.test(s.trim()) ? s.trim() : `${s.trim()}.`);

/** The one line that says why the suggested run order is not in use: under
 * Run Tests' toolbar and under Start from in the Execution order modal. A
 * reason that already sends the reader to the logs is not told to go there
 * twice. */
export function noteFor(reason: string): string {
  const head = `The suggested run order could not be read: ${sentence(reason)}`;
  return /Settings\s*(→|,)\s*Logs/.test(reason) ? head : `${head} See Settings → Logs.`;
}

/** The run-order query exactly as this hook builds it: queryKey and
 * persistentQuery options together, so the Execution order modal's Save
 * for everyone writes into this very cache entry. */
export function runOrderQueryOptions(org: string, project: string, pbiId: number) {
  return {
    queryKey: ["run-order", org, project, pbiId] as const,
    ...persistentQuery({
      key: cacheKeys.runOrder(org, project, pbiId),
      fetcher: () => unwrap(commands.getRunOrder(org, project, pbiId)),
      ...CACHE.structure,
      staleMs: 5 * 60_000,
    }),
    enabled: pbiId > 0,
    retry: false,
  };
}

export function useRunOrder({
  org,
  project,
  pbiId,
  suite,
  points,
  grouped,
}: {
  org: string;
  project: string;
  pbiId: number;
  suite: { plan_id: number; suite_id: number } | undefined;
  points: readonly TestPoint[];
  grouped: boolean;
}) {
  const planId = suite?.plan_id ?? 0;
  const suiteId = suite?.suite_id ?? 0;
  const key: OrderKey | null = suite ? { org, planId, suiteId } : null;
  // Local reads (My order, the chosen view) are re-done on every bump: a
  // My order or a view chosen in the Execution order modal.
  const [rev, bump] = useReducer((n: number) => n + 1, 0);

  const runOrder = useQuery(runOrderQueryOptions(org, project, pbiId));

  // The same key and loader as Suite Management, so both screens share one
  // cache entry. While it loads (or if it fails) spec order is the points'
  // own order: the list never waits on it.
  const suiteCases = useQuery({
    queryKey: suiteCasesKey(org, project, planId, suiteId),
    ...persistentQuery({
      key: cacheKeys.suiteCases(org, project, planId, suiteId),
      fetcher: () => loadSuiteCases(org, project, planId, suiteId),
      ...CACHE.structure,
      staleMs: 5 * 60_000,
    }),
    enabled: Boolean(suite),
    retry: false,
  });

  const read = runOrder.data;
  const file = read?.state === "found" ? read.file : null;
  const reason = runOrder.isError ? runOrder.error.message : read?.state === "unreadable" ? read.reason : null;
  const note = reason == null ? null : noteFor(reason);

  const keyStr = key ? `${org}/${planId}/${suiteId}` : "";
  const myOrder = useMemo(() => (key ? loadMyOrder(key) : null), [keyStr, rev]);
  const storedView = useMemo(() => (key ? loadOrderView(key) : null), [keyStr, rev]);

  const available = (v: OrderView) => (v === "suggested" ? file != null : v === "mine" ? myOrder != null : true);
  const view: OrderView = storedView && available(storedView) ? storedView : file ? "suggested" : "spec";

  const specIds = useMemo(
    () => specOrderOf(points, suiteCases.data?.map((c) => c.id)),
    [points, suiteCases.data],
  );
  // The same cases with their titles, for the Execution order modal's list:
  // a case is titled by its first point's name.
  const specCases = useMemo<SuiteCase[]>(() => {
    const titles = new Map<number, string>();
    for (const p of points) {
      if (p.test_case_id != null && !titles.has(p.test_case_id)) titles.set(p.test_case_id, p.test_case_name);
    }
    return specIds.map((id) => ({ id, title: titles.get(id) ?? `Test case ${id}` }));
  }, [points, specIds]);
  const order = useMemo(() => {
    const chosen =
      view === "suggested" && file ? file.cases.map((c) => c.id) : view === "mine" && myOrder ? myOrder : specIds;
    return reconcile(chosen, specIds);
  }, [view, file, myOrder, specIds]);
  const ordered = useMemo(() => sortPoints(points, order), [points, order]);

  // Areas from the suggested file, when it names one for a case on screen.
  const groupOf = useMemo(() => {
    if (!file) return null;
    const onScreen = new Set(specIds);
    if (!file.cases.some((c) => c.group && onScreen.has(c.id))) return null;
    return new Map(file.cases.map((c) => [c.id, c.group ?? ""]));
  }, [file, specIds]);

  const sections = useMemo(
    () => (grouped ? sectionsFor(ordered, groupOf) : [{ name: "", pts: ordered }]),
    [grouped, ordered, groupOf],
  );

  /** `ids` as My order on this machine, and My order as the list's order:
   * the Execution order modal's Use this order on a list of the tester's
   * own. False (after saying so) when storage dropped the save - switching
   * to a My order that is not there would mislead. */
  const saveMine = (ids: readonly number[]): boolean => {
    if (!key) return false;
    saveMyOrder(key, ids);
    // Read it back: with storage unavailable the save is silently dropped.
    const saved = loadMyOrder(key);
    if (!saved || saved.length !== ids.length || saved.some((id, i) => id !== ids[i])) {
      toast.error("Your own order could not be saved on this machine.");
      return false;
    }
    saveOrderView(key, "mine");
    bump();
    return true;
  };

  /** A stored order as the list's order, as it is. My order stays stored. */
  const changeView = (v: OrderView) => {
    if (!key) return;
    saveOrderView(key, v);
    bump();
  };

  return {
    view,
    changeView,
    saveMine,
    note,
    file,
    loading: runOrder.isLoading,
    myOrder,
    specCases,
    ordered,
    sections,
  };
}
```

- [ ] **Step 5: Wire the screen (`src/screens/RunPanel/index.tsx`)**

Edit A, line 2. Old:

```tsx
import { ChevronDown, ChevronRight, GripVertical, RefreshCw, X } from "lucide-react";
```

New:

```tsx
import { ChevronDown, ChevronRight, RefreshCw, X } from "lucide-react";
```

Edit B, lines 29-38. Old:

```tsx
import {
  IconCollapseAll,
  IconMoveDown,
  IconMoveUp,
  IconReport,
  IconRun,
  IconUndo,
} from "../../lib/actionIcons";
import type { OrderView } from "../../lib/runOrder";
import { ORDER_LABELS, runOrderQueryOptions, useRunOrder } from "./useRunOrder";
```

New:

```tsx
import { IconCollapseAll, IconReport, IconRun, IconSetOrder } from "../../lib/actionIcons";
import ExecutionOrderModal from "./ExecutionOrderModal";
import { ORDER_LABELS, runOrderQueryOptions, useRunOrder } from "./useRunOrder";
```

Edit C, lines 286-289. Old:

```tsx
  // Drag a row onto another to put its case there (a copy into My order,
  // same as the arrows). Native drag events, as Suite Management's list.
  const [dragCase, setDragCase] = useState<number | null>(null);
  const [overPoint, setOverPoint] = useState<number | null>(null);
```

New:

```tsx
  // The order is chosen only in the Execution order modal: the list shows
  // it and never edits it (execution-order-modal design §2).
  const [orderOpen, setOrderOpen] = useState(false);
```

Edit D, the toolbar at lines 464-490. Old:

```tsx
      {points.data && points.data.length > 0 && (
        <div className="space-y-1">
          <div className="flex items-center gap-2">
            <span className="text-xs text-muted">Order</span>
            <Select
              aria-label="Order"
              className="w-56"
              triggerClassName="px-2 py-1.5"
              value={order.view}
              onChange={(e) => order.changeView(e.target.value as OrderView)}
            >
              {order.options.map((o) => (
                <option key={o.view} value={o.view} disabled={o.disabled}>
                  {ORDER_LABELS[o.view]}
                </option>
              ))}
            </Select>
            {order.view === "mine" && (
              <Button variant="outline" size="sm" onClick={order.reset}>
                <IconUndo aria-hidden />
                {order.resetLabel}
              </Button>
            )}
          </div>
          {order.note && <p className="text-xs text-warning">{order.note}</p>}
        </div>
      )}
```

New:

```tsx
      {points.data && points.data.length > 0 && (
        <div className="space-y-1">
          <div className="flex items-center gap-2">
            <Button variant="outline" size="sm" onClick={() => setOrderOpen(true)}>
              <IconSetOrder aria-hidden />
              Set execution order
            </Button>
            {/* A status, so the new order is announced when the modal closes. */}
            <span role="status" className="text-xs text-muted">
              {ORDER_LABELS[order.view]}
            </span>
          </div>
          {order.note && <p className="text-xs text-warning">{order.note}</p>}
        </div>
      )}
      {orderOpen && (
        <ExecutionOrderModal
          org={org}
          project={project}
          pbiId={pbiId}
          cases={order.specCases}
          view={order.view}
          file={order.file}
          unreadableNote={order.note}
          loading={order.loading}
          myOrder={order.myOrder}
          onUseView={order.changeView}
          onUseMine={order.saveMine}
          onClose={() => setOrderOpen(false)}
        />
      )}
```

Edit E, the group header's move buttons at lines 606-630. Old:

```tsx
                        <span aria-hidden className="h-px flex-1 bg-linear-to-r from-border to-transparent" />
                        {/* The whole group past its neighbour, into My order. */}
                        <span className="flex shrink-0 items-center gap-0.5">
                          <button
                            type="button"
                            aria-label={`Move group ${name} up`}
                            title="Move group up"
                            disabled={!order.canMoveGroup(name, "up")}
                            className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                            onClick={() => order.moveGroup(name, "up")}
                          >
                            <IconMoveUp aria-hidden />
                          </button>
                          <button
                            type="button"
                            aria-label={`Move group ${name} down`}
                            title="Move group down"
                            disabled={!order.canMoveGroup(name, "down")}
                            className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                            onClick={() => order.moveGroup(name, "down")}
                          >
                            <IconMoveDown aria-hidden />
                          </button>
                        </span>
                      </div>
```

New:

```tsx
                        <span aria-hidden className="h-px flex-1 bg-linear-to-r from-border to-transparent" />
                      </div>
```

Edit F, the row's drag styling and handlers at lines 646-668. Old:

```tsx
                  "hover:bg-surface-2",
                  dragCase != null && dragCase === p.test_case_id && "opacity-50",
                  overPoint === p.point_id && dragCase !== p.test_case_id && "border-t-2 border-t-accent",
                )}
                onClick={(e) => handleRowClick(p, e)}
                draggable={p.test_case_id != null}
                onDragStart={() => setDragCase(p.test_case_id)}
                onDragEnd={() => {
                  setDragCase(null);
                  setOverPoint(null);
                }}
                onDragOver={(e) => {
                  e.preventDefault();
                  if (overPoint !== p.point_id) setOverPoint(p.point_id);
                }}
                onDragLeave={() => setOverPoint((o) => (o === p.point_id ? null : o))}
                onDrop={(e) => {
                  e.preventDefault();
                  if (dragCase != null && p.test_case_id != null) order.dropCase(dragCase, p.test_case_id);
                  setDragCase(null);
                  setOverPoint(null);
                }}
              >
```

New:

```tsx
                  "hover:bg-surface-2",
                )}
                onClick={(e) => handleRowClick(p, e)}
              >
```

Edit G, the drag handle at line 671. Old (the whole line, with its newline):

```tsx
                    <GripVertical size={14} className="shrink-0 cursor-grab text-faint" aria-hidden />
```

New: (nothing; delete the line).

Edit H, the row's Move buttons at lines 690-720. Old:

```tsx
                    {p.test_case_id != null && (
                      <span className="flex shrink-0 items-center gap-0.5">
                        <button
                          type="button"
                          aria-label={`Move ${p.test_case_name} up`}
                          title="Move up"
                          disabled={!order.canMove(p.test_case_id, "up")}
                          className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                          onClick={(e) => {
                            e.stopPropagation();
                            order.moveCase(p.test_case_id!, "up");
                          }}
                        >
                          <IconMoveUp aria-hidden />
                        </button>
                        <button
                          type="button"
                          aria-label={`Move ${p.test_case_name} down`}
                          title="Move down"
                          disabled={!order.canMove(p.test_case_id, "down")}
                          className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                          onClick={(e) => {
                            e.stopPropagation();
                            order.moveCase(p.test_case_id!, "down");
                          }}
                        >
                          <IconMoveDown aria-hidden />
                        </button>
                      </span>
                    )}
                  </div>
```

New:

```tsx
                  </div>
```

After these edits, `order.` in `index.tsx` appears only as `order.ordered`, `order.sections`, `order.view`, `order.note` and the modal's props. Confirm with Grep (pattern `order\.(canMove|moveCase|dropCase|canMoveGroup|moveGroup|reset|options)|dragCase|overPoint|GripVertical|IconMove|IconUndo|OrderView` in `src/screens/RunPanel/index.tsx`): no matches.

- [ ] **Step 6: Run the tests and the gates to see them pass**

Run: `npx vitest run src/screens/RunPanel.test.tsx src/screens/RunPanel/useRunOrder.test.tsx src/screens/RunPanel/ExecutionOrderModal.test.tsx src/ui-consistency.test.ts src/a11y.test.tsx --exclude "**/.claude/**"`
Expected: PASS. `ui-consistency` checks the new `IconSetOrder` against the vocabulary and that every icon is `aria-hidden`.

Run: `npx tsc --noEmit`
Expected: no errors. In particular, no unused imports remain in `index.tsx` or `useRunOrder.ts`.

- [ ] **Step 7: Commit**

```bash
git add src/lib/actionIcons.ts src/screens/RunPanel/index.tsx src/screens/RunPanel/useRunOrder.ts src/screens/RunPanel.test.tsx
git commit -F - <<'EOF'
feat(v2): Run Tests sets its order through one Set execution order button

The Order dropdown, row drag, Move up/down on rows and groups, and Reset
are gone. One button opens the Execution order modal; beside it the list
names the order it is in. The list only shows the chosen order.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1 --stat
```

---

### Task 3: Suite Management drops the Suggested run order section

**Files:**
- Delete: `src/screens/ManageCases/SuggestedOrder.tsx`
- Delete: `src/screens/ManageCases/SuggestedOrder.test.tsx`
- Modify: `src/screens/ManageCases/SuiteCases.tsx` (import line 20; `pbiId` prop lines 32 and 42-46; heading line 172; render lines 278-288)
- Modify: `src/screens/ManageCases/PlanTable.tsx` (the `pbiId` prop, lines 202-206)
- Modify: `src/screens/ManageCases/SuiteCases.test.tsx` (Harness lines 28-57; `mount` lines 85-106; tests lines 581-600)
- Modify: `src/lib/testerOrderStart.ts` (doc comment, lines 3-4)
- Modify: `src/lib/runOrder.ts` (header comment, lines 1-2)
- Modify: `docs/superpowers/specs/2026-09-23-run-order-design.md` (§3 rule 4, §5.1, §5.3)

**Interfaces:**
- Consumes: nothing new. `runOrderQueryOptions`, `noteFor` (in `useRunOrder.ts`), `testerOrderSources` and `runOrderPayload` stay; the modal uses them.
- Produces: `SuiteCases` props lose `pbiId?: number`: `{ org; project; planId; suiteId; suiteName; selected; onToggle }`.

- [ ] **Step 1: Replace the Suggested run order tests with the removal test**

In `src/screens/ManageCases/SuiteCases.test.tsx`, edit the Harness signature. Old:

```tsx
function Harness({
  onToggle,
  pbiId,
}: {
  onToggle?: (cases: SuiteCase[], on: boolean) => void;
  /** Passed straight through to SuiteCases - a real caller (PlanTable) only
   * sets this for a requirement suite. */
  pbiId?: number;
}) {
```

New:

```tsx
function Harness({ onToggle }: { onToggle?: (cases: SuiteCase[], on: boolean) => void }) {
```

Old:

```tsx
      suiteName="Regression"
      pbiId={pbiId}
      selected={selected}
```

New:

```tsx
      suiteName="Regression"
      selected={selected}
```

Edit `mount`. Old:

```tsx
  points = DEFAULT_POINTS,
  pbiId?: number,
) {
```

New:

```tsx
  points = DEFAULT_POINTS,
) {
```

Old:

```tsx
    if (answered !== undefined) return answered;
    if (cmd === "get_run_order") return { state: "none" };
  });
```

New:

```tsx
    if (answered !== undefined) return answered;
  });
```

Old:

```tsx
      <Harness onToggle={onToggle} pbiId={pbiId} />
```

New:

```tsx
      <Harness onToggle={onToggle} />
```

Replace the tail of the file (lines 581-600). Old: everything from `// ---- Suggested run order (design doc §5.3) ----` to the end of the file. New:

```tsx
// ---- Run order (execution-order-modal design §6) ----

test("Suite Management shows only the Azure DevOps order editor and never reads the run order", async () => {
  const { calls } = mount();
  await list();
  expect(screen.queryByRole("heading", { name: "Suggested run order" })).not.toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "Order in Azure DevOps" })).not.toBeInTheDocument();
  expect(screen.queryByRole("list", { name: /^Suggested run order/ })).not.toBeInTheDocument();
  expect(calls.some((c) => c.cmd === "get_run_order")).toBe(false);
});
```

- [ ] **Step 2: Run it to see it fail**

Run: `npx vitest run src/screens/ManageCases/SuiteCases.test.tsx --exclude "**/.claude/**"`
Expected: FAIL. The `Order in Azure DevOps` heading is still rendered.

- [ ] **Step 3: Remove the section from `SuiteCases.tsx`**

Old (line 20, with its newline):

```tsx
import SuggestedOrder from "./SuggestedOrder";
```

New: (nothing; delete the line).

Old:

```tsx
  suiteName,
  pbiId,
  selected,
```

New:

```tsx
  suiteName,
  selected,
```

Old:

```tsx
  suiteName: string;
  /** The PBI this suite belongs to, from the suite's `requirement_id` - only
   * a requirement suite has one. Renders the Suggested run order editor
   * below this one when set (design doc §5.3); a static suite has no PBI
   * and so no suggested order. */
  pbiId?: number;
```

New:

```tsx
  suiteName: string;
```

Old:

```tsx
    <div className="my-2 space-y-2">
      <h3 className="text-sm font-semibold text-text">Order in Azure DevOps</h3>
      <div ref={toolbarRef} className="flex flex-wrap items-center gap-2">
```

New:

```tsx
    <div className="my-2 space-y-2">
      <div ref={toolbarRef} className="flex flex-wrap items-center gap-2">
```

Old:

```tsx
        />
      )}
      {pbiId != null && (
        <SuggestedOrder
          org={org}
          project={project}
          planId={planId}
          suiteId={suiteId}
          pbiId={pbiId}
          suiteName={suiteName}
          cases={cases.data}
        />
      )}
    </div>
  );
}
```

New:

```tsx
        />
      )}
    </div>
  );
}
```

- [ ] **Step 4: Stop passing `pbiId` from `PlanTable.tsx`**

Old:

```tsx
                    suiteName={suite.name}
                    pbiId={
                      suite.suite_type === "requirementTestSuite" && suite.requirement_id != null
                        ? suite.requirement_id
                        : undefined
                    }
                    selected={selectedIds}
```

New:

```tsx
                    suiteName={suite.name}
                    selected={selectedIds}
```

- [ ] **Step 5: Delete the component and its tests**

Run:

```bash
git rm src/screens/ManageCases/SuggestedOrder.tsx src/screens/ManageCases/SuggestedOrder.test.tsx
```

Then use Grep (pattern `SuggestedOrder`, path `src`) to confirm there are no matches left.

- [ ] **Step 6: Point the comments at the modal**

In `src/lib/testerOrderStart.ts`, old:

```ts
/** One watched draft file's tester order, offered as a "Start from" choice
 * in Suite Management's suggested run order editor. */
```

New:

```ts
/** One watched draft file's tester order, offered as a "Start from" choice
 * in Run Tests' Execution order modal. */
```

In `src/lib/runOrder.ts`, old:

```ts
// Run order: pure reordering helpers shared by Run Tests, the runner and
// Suite Management, plus a tester's own order for one suite on this
```

New:

```ts
// Run order: pure reordering helpers shared by Run Tests, the runner and
// the Execution order modal, plus a tester's own order for one suite on this
```

- [ ] **Step 7: Note the rule change in the parent design**

In `docs/superpowers/specs/2026-09-23-run-order-design.md`, §3. Old:

```md
4. Shared orders (spec order and suggested run order) are changed only in
   Suite Management. Run Tests and the runner only ever change My order.
```

New:

```md
4. Shared orders (spec order and suggested run order) are changed only in
   Suite Management. Run Tests and the runner only ever change My order.

   > Superseded on 2026-09-23 by `2026-09-23-execution-order-modal-design.md`
   > §3: spec order still changes only in Suite Management and at upload,
   > but the suggested run order is saved only through Run Tests' Execution
   > order modal (Save for everyone) and at upload. My order changes only
   > through the modal's Use this order. The "Written by" column of §4 is
   > read the same way.
```

§5.1. Old:

```md
### 5.1 Run Tests

- An **Order** picker above the list:
```

New:

```md
### 5.1 Run Tests

> Superseded on 2026-09-23 by `2026-09-23-execution-order-modal-design.md`
> §4-§5: the Order picker, row drag, row and group moves and Reset are gone.
> Run Tests has one **Set execution order** button that opens a modal, and
> the list only shows the chosen order.

- An **Order** picker above the list:
```

§5.3. Old:

```md
### 5.3 Suite Management

For a PBI's suite it shows two orders, each with the existing order editor:
```

New:

```md
### 5.3 Suite Management

> Superseded on 2026-09-23 by `2026-09-23-execution-order-modal-design.md`
> §6: the Suggested run order section is removed and Suite Management shows
> only the Azure DevOps order editor. The suggested run order is saved from
> the Run Tests modal.

For a PBI's suite it shows two orders, each with the existing order editor:
```

- [ ] **Step 8: Run the tests and the full gate**

Run: `npx vitest run src/screens/ManageCases src/lib src/screens/RunPanel src/screens/RunPanel.test.tsx src/ui-consistency.test.ts src/a11y.test.tsx --exclude "**/.claude/**"`
Expected: PASS. The Azure DevOps editor tests in `SuiteCases.test.tsx`, `suites.test.tsx` and `index.test.tsx` are unchanged and green.

Run: `npx tsc --noEmit`
Expected: no errors.

Run the whole frontend suite once: `npx vitest run --exclude "**/.claude/**"`
Expected: PASS. `src/App.test.tsx` has a documented load-induced flake. One failure that passes on a re-run is that flake; two different failures are real.

- [ ] **Step 9: Commit**

```bash
git add src/screens/ManageCases/SuiteCases.tsx src/screens/ManageCases/PlanTable.tsx src/screens/ManageCases/SuiteCases.test.tsx src/lib/testerOrderStart.ts src/lib/runOrder.ts docs/superpowers/specs/2026-09-23-run-order-design.md
git commit -F - <<'EOF'
feat(v2): Suite Management drops the Suggested run order section

The suggested run order is now set from Run Tests' Execution order
modal. Suite Management shows only the order in Azure DevOps, as it did
before the run-order work.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1 --stat
```

(The two `git rm` deletions from Step 5 are already staged and go into this commit.)

---

## After execution

These checks need a live org and are for the owner. Tests cannot cover them: jsdom does no layout or hit testing, and the save goes to a real PBI.

1. **Open the modal on a PBI with a saved suggested run order.** Run Tests shows `Suggested run order` beside **Set execution order**. The modal opens on it, with `Saved by <account> on <date>`, and its Start from list drops down over the case list without being clipped. Drag and Move up/down both work in the modal; neither exists on the Run Tests rows or group headers.
2. **Open it on a PBI without one.** Start from offers `Spec order` (plus `My order` and any `Tester order from <file>` that apply), and the note reads `No suggested run order yet.`
3. **Use this order changes only this machine.** Reorder, then Use this order: the list switches to `My order`. On a second machine (or another tester's) signed in to the same PBI, Run Tests is unchanged after Refresh, and so is the suite order in Azure DevOps' own Test Plans page.
4. **Save for everyone reaches another machine after Refresh.** Save for everyone, confirm the sentence, then Save: a success toast, and the list switches to `Suggested run order`. On the second machine, Refresh outcomes: its default order is the new one, and its modal shows your account and today's date. The PBI's attachments show a new `tcm-run-order.json`. The old one stays unreferenced, because nothing is deleted.
5. **Suite Management shows only the Azure DevOps editor.** Expand the PBI's requirement suite: there is no Suggested run order section and no `Order in Azure DevOps` heading. Apply order, Reset, Apply order from files and Group by title still work as before.
6. **The runner.** Select a few cases after changing the order in the modal, then Run N in runner: the runner walks them in the order on screen.
