# Suite Ordering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In Suite Management, a ticked selection drags as one block, cases can be arranged by title group, and "Apply order from files" takes several files and lets the user arrange whose block comes first.

**Architecture:** Every ordering rule is a pure function in `src/lib/suiteOrder.ts` (beside the existing `moveItem`, `sameOrder`, `orderFromFile`), unit-tested on its own. `CaseOrderList` grows a grouped mode and block behaviour but holds no ordering logic; a new `FileOrderDialog` arranges files. `SuiteCases` wires the switch, the A–Z button and the dialog. Saving is unchanged.

**Tech Stack:** React 19 + TypeScript, TanStack React Query, native HTML drag events (no library — the list already works this way), `@tauri-apps/plugin-dialog` `open({ multiple: true })`, vitest + Testing Library (jsdom).

**Spec:** `docs/superpowers/specs/2026-09-14-suite-ordering-design.md` — read it first; every task below implements one of its numbered decisions.

## Global Constraints

- **No Rust changes.** `src/bindings.ts` is generated and must not change; if it shows as modified with content identical to HEAD, that is a line-ending artifact: `git checkout -- src/bindings.ts`.
- **No DELETE calls** to Azure DevOps (nothing here touches the client at all).
- **Colours from theme tokens only** (`text-text`, `text-muted`, `text-faint`, `bg-surface`, `bg-surface-2`, `border-border`, `border-accent`, `accent`…). `src/ui-consistency.test.ts` is the gate and must never be weakened. It also forbids text under 10px, requires `aria-label`/`title` on icon-only buttons, `aria-hidden` on button icons, action icons from `src/lib/actionIcons.ts`, and no hand-set `size={n}` on an icon inside a `<Button>`.
- **`tester_order` inside files is never rewritten**, and is ignored when ordering from a file: the file's row order decides.
- **The file only proposes.** Nothing saves until Apply order; the dialog remembers nothing between opens.
- **Don't run `npx prettier`.** Match the surrounding style by hand.
- **The machine is shared with the user.** One test/build command at a time; long runs via `run_in_background` and the notification, never polling. `src/App.test.tsx` has a documented load flake (one failure that passes on a re-run).
- **Commit with a Bash heredoc** (`git commit -F - <<'MSG' … MSG`), ending every message with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`. Confirm with `git log -1`.
- Work on the branch this plan is committed on; do not merge, push or release.

---

## File Structure

- Modify `src/lib/suiteOrder.ts`: the rules — `moveBlock`, `nudgeBlock`, `groupKeys`, `sectionsOf`, `orderByGroups`, `orderGroupsAZ`, `orderFromFiles`. `orderFromFile` is removed (replaced by `orderFromFiles`).
- Modify `src/lib/suiteOrder.test.ts`: tests for each rule.
- Modify `src/screens/ManageCases/CaseOrderList.tsx`: block drag/nudge; grouped mode with section headers.
- Create `src/screens/ManageCases/FileOrderDialog.tsx`: the files dialog.
- Create `src/screens/ManageCases/FileOrderDialog.test.tsx`.
- Modify `src/screens/ManageCases/SuiteCases.tsx`: the switch, A–Z, dialog wiring, renamed button.
- Modify `src/screens/ManageCases/SuiteCases.test.tsx`: block, grouped and files behaviour.
- Modify `README.md` (Suite Management section, if it describes the file button by its old name).

Existing facts the tasks rely on:
- `SuiteCase` is `{ id: number; title: string }`. `moveItem(list, from, to)` moves DOWNWARD to land *after* the target and UPWARD to land *before* it (list `a,b,c,d`: `moveItem(0,2)` → `b,c,a,d`; `moveItem(3,1)` → `a,d,b,c`). Block moves must feel the same.
- `groupIndices(titles)` in `src/lib/grouping.ts` returns `{ name, indices }[]` — groups sorted alphabetically, then one `{ name: "", indices }` for ungrouped; a group needs ≥ 2 members.
- `CaseOrderList` props today: `cases, selected, onChange, onSelect, ariaLabel, disabled`. Rows are `<li>` with `Select #id`, `Move #id up`, `Move #id down` controls and native drag handlers; tests count rows with `within(list).getAllByRole("listitem")`.
- `SuiteCases` holds `order` (screen order), `dirty = !sameOrder(order, cases.data)`, an `apply` mutation and a `fromFile` mutation that opens one file, calls `commands.parseImportFile(path)` (returns `{ cases: [{ update_id, tester_order?, title, … }], warnings }`) and calls `orderFromFile`.
- `SuiteCases.test.tsx` mocks `@tauri-apps/plugin-dialog`'s `open` as `openDialog` (`vi.hoisted`), and `parse_import_file` through `mockIPC`; its fixture suite has cases 201 "Valid login", 202 "Bad password", 203 "Locked out"; `list()` returns the `<ol>` after a settle tick.
- Run Tests persists its group switch as `localStorage["tcm-v2-group-points"] = "on" | "off"`.

---

### Task 1: The ordering rules

**Files:**
- Modify: `src/lib/suiteOrder.ts`
- Test: `src/lib/suiteOrder.test.ts`

**Interfaces:**
- Consumes: `groupIndices` from `src/lib/grouping.ts`.
- Produces (all exported from `src/lib/suiteOrder.ts`):
  - `moveBlock(list: SuiteCase[], ids: ReadonlySet<number>, targetId: number): SuiteCase[]`
  - `nudgeBlock(list: SuiteCase[], ids: ReadonlySet<number>, dir: "up" | "down"): SuiteCase[]`
  - `groupKeys(cases: SuiteCase[]): string[]` — one key per case, `""` for ungrouped
  - `type Section = { name: string; ids: number[] }`; `sectionsOf(cases: SuiteCase[]): Section[]`
  - `orderByGroups(cases: SuiteCase[]): SuiteCase[]`
  - `orderGroupsAZ(cases: SuiteCase[]): SuiteCase[]`
  - `type FileForOrder = { name: string; cases: Array<{ update_id: number | null }> }`
  - `orderFromFiles(current: SuiteCase[], files: FileForOrder[]): { order: SuiteCase[]; placed: number[]; duplicates: number }`
  - Removed: `orderFromFile`.

- [ ] **Step 1: Write the failing tests**

Append to `src/lib/suiteOrder.test.ts` (extend the import line to include every name used):

```ts
import {
  groupKeys, moveBlock, moveItem, nudgeBlock, orderByGroups, orderFromFiles, orderGroupsAZ,
  sameOrder, sectionsOf, type SuiteCase,
} from "./suiteOrder";

const c = (id: number, title = `Case ${id}`): SuiteCase => ({ id, title });
const ids = (l: SuiteCase[]) => l.map((x) => x.id);

describe("moveBlock", () => {
  const list = [c(1), c(2), c(3), c(4), c(5)];
  test("a block dropped below lands after the target, keeping its own order", () => {
    expect(ids(moveBlock(list, new Set([1, 2]), 4))).toEqual([3, 4, 1, 2, 5]);
  });
  test("a block dropped above lands before the target", () => {
    expect(ids(moveBlock(list, new Set([4, 5]), 2))).toEqual([1, 4, 5, 2, 3]);
  });
  test("a scattered selection is gathered into one block at the drop", () => {
    expect(ids(moveBlock(list, new Set([1, 3]), 5))).toEqual([2, 4, 5, 1, 3]);
  });
  test("dropping onto a member of the block, or an unknown target, changes nothing", () => {
    expect(ids(moveBlock(list, new Set([2, 3]), 3))).toEqual([1, 2, 3, 4, 5]);
    expect(ids(moveBlock(list, new Set([2]), 99))).toEqual([1, 2, 3, 4, 5]);
    expect(moveBlock(list, new Set([2]), 99)).not.toBe(list);
  });
  test("one id behaves exactly like moveItem", () => {
    expect(ids(moveBlock(list, new Set([1]), 3))).toEqual(ids(moveItem(list, 0, 2)));
    expect(ids(moveBlock(list, new Set([4]), 2))).toEqual(ids(moveItem(list, 3, 1)));
  });
});

describe("nudgeBlock", () => {
  const list = [c(1), c(2), c(3), c(4), c(5)];
  test("moves the block one step, gathering a scattered selection first", () => {
    expect(ids(nudgeBlock(list, new Set([3, 4]), "up"))).toEqual([1, 3, 4, 2, 5]);
    expect(ids(nudgeBlock(list, new Set([3, 4]), "down"))).toEqual([1, 2, 5, 3, 4]);
    expect(ids(nudgeBlock(list, new Set([2, 4]), "up"))).toEqual([2, 4, 1, 3, 5]);
  });
  test("stops at the ends", () => {
    expect(ids(nudgeBlock(list, new Set([1, 2]), "up"))).toEqual([1, 2, 3, 4, 5]);
    expect(ids(nudgeBlock(list, new Set([4, 5]), "down"))).toEqual([1, 2, 3, 4, 5]);
  });
});

describe("groups", () => {
  // Title groups as the rest of the app sees them: the text before the
  // first separator, and only when at least two titles share it.
  const list = [
    c(1, "Alerts | ring once"),
    c(2, "Filter Card | collapses"),
    c(3, "Alerts | ring twice"),
    c(4, "Just one of these"),
    c(5, "Filter Card | expands"),
  ];
  test("groupKeys names each case's group, blank for the ungrouped", () => {
    expect(groupKeys(list)).toEqual(["Alerts", "Filter Card", "Alerts", "", "Filter Card"]);
  });
  test("sectionsOf reads the CURRENT order as runs, so a split group is two sections", () => {
    expect(sectionsOf(list)).toEqual([
      { name: "Alerts", ids: [1] },
      { name: "Filter Card", ids: [2] },
      { name: "Alerts", ids: [3] },
      { name: "", ids: [4] },
      { name: "Filter Card", ids: [5] },
    ]);
  });
  test("orderByGroups makes each group contiguous in order of first appearance, ungrouped last", () => {
    expect(ids(orderByGroups(list))).toEqual([1, 3, 2, 5, 4]);
    // Already arranged: same order back.
    expect(ids(orderByGroups(orderByGroups(list)))).toEqual([1, 3, 2, 5, 4]);
  });
  test("orderGroupsAZ sorts the groups by name, case-insensitively, cases keeping their order", () => {
    const l = [c(1, "zeta | a"), c(2, "Alpha | a"), c(3, "zeta | b"), c(4, "alpha | b"), c(5, "solo")];
    expect(ids(orderGroupsAZ(l))).toEqual([2, 4, 1, 3, 5]);
  });
});

describe("orderFromFiles", () => {
  const suite = [c(1), c(2), c(3), c(4), c(5), c(6)];
  const file = (name: string, ...update_ids: Array<number | null>) => ({
    name,
    cases: update_ids.map((update_id) => ({ update_id })),
  });
  test("each file is a block in the file's ROW order - tester_order plays no part", () => {
    const { order, placed } = orderFromFiles(suite, [file("a.json", 3, 1)]);
    expect(ids(order)).toEqual([3, 1, 2, 4, 5, 6]);
    expect(placed).toEqual([2]);
  });
  test("blocks follow the files' order, and cases in no file trail in their current order", () => {
    const { order, placed } = orderFromFiles(suite, [file("b.json", 6, 5), file("a.json", 2)]);
    expect(ids(order)).toEqual([6, 5, 2, 1, 3, 4]);
    expect(placed).toEqual([2, 1]);
  });
  test("a case in two files goes with the first, and is counted", () => {
    const { order, placed, duplicates } = orderFromFiles(suite, [file("a.json", 1, 2), file("b.json", 2, 3)]);
    expect(ids(order)).toEqual([1, 2, 3, 4, 5, 6]);
    expect(placed).toEqual([2, 1]);
    expect(duplicates).toBe(1);
  });
  test("ids not in this suite, null ids and repeats inside one file are ignored", () => {
    const { order, placed, duplicates } = orderFromFiles(suite, [file("a.json", 99, null, 4, 4)]);
    expect(ids(order)).toEqual([4, 1, 2, 3, 5, 6]);
    expect(placed).toEqual([1]);
    expect(duplicates).toBe(0);
  });
  test("no files, or files placing nothing, return the current order and zero counts", () => {
    expect(ids(orderFromFiles(suite, []).order)).toEqual([1, 2, 3, 4, 5, 6]);
    expect(orderFromFiles(suite, [file("x.json", 99)]).placed).toEqual([0]);
  });
});
```

Also delete the existing `describe("orderFromFile", …)` block (the function is being replaced; its tester_order-sorting behaviour is exactly what the spec drops).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run src/lib/suiteOrder.test.ts`
Expected: FAIL — the new names are not exported.

- [ ] **Step 3: Implement the rules**

In `src/lib/suiteOrder.ts`, add the import `import { groupIndices } from "./grouping";`, delete `orderFromFile` and its doc comment, and add:

```ts
/** Everything in `ids` gathered into one block, in its current relative
 * order, and dropped at `targetId` the way `moveItem` drops one row: after
 * the target when the block came from above it, before it when the block
 * came from below. A target inside the block, or not in the list, is a
 * no-op (a fresh copy either way). */
export function moveBlock(list: SuiteCase[], ids: ReadonlySet<number>, targetId: number): SuiteCase[] {
  const targetAt = list.findIndex((c) => c.id === targetId);
  if (targetAt < 0 || ids.has(targetId)) return list.slice();
  const block = list.filter((c) => ids.has(c.id));
  if (block.length === 0) return list.slice();
  const rest = list.filter((c) => !ids.has(c.id));
  const firstAt = list.findIndex((c) => ids.has(c.id));
  const restTargetAt = rest.findIndex((c) => c.id === targetId);
  const at = firstAt < targetAt ? restTargetAt + 1 : restTargetAt;
  return [...rest.slice(0, at), ...block, ...rest.slice(at)];
}

/** The block one step up or down - what Move up / Move down do on a
 * ticked row. A scattered selection is gathered at its first member. */
export function nudgeBlock(list: SuiteCase[], ids: ReadonlySet<number>, dir: "up" | "down"): SuiteCase[] {
  const block = list.filter((c) => ids.has(c.id));
  if (block.length === 0) return list.slice();
  const rest = list.filter((c) => !ids.has(c.id));
  const firstAt = list.findIndex((c) => ids.has(c.id));
  // Where the block sits in `rest` terms: how many non-members precede it.
  const before = list.slice(0, firstAt).filter((c) => !ids.has(c.id)).length;
  const at = dir === "up" ? Math.max(0, before - 1) : Math.min(rest.length, before + 1);
  return [...rest.slice(0, at), ...block, ...rest.slice(at)];
}

/** Each case's title group, as Run Tests and View Test Cases see them:
 * "" for a case in no group. */
export function groupKeys(cases: SuiteCase[]): string[] {
  const keys = new Array<string>(cases.length).fill("");
  for (const g of groupIndices(cases.map((c) => c.title))) {
    for (const i of g.indices) keys[i] = g.name;
  }
  return keys;
}

export type Section = { name: string; ids: number[] };

/** The list as runs of one group, in the CURRENT order: a group split by a
 * drag shows as two sections rather than being silently re-joined. */
export function sectionsOf(cases: SuiteCase[]): Section[] {
  const keys = groupKeys(cases);
  const out: Section[] = [];
  cases.forEach((c, i) => {
    const last = out[out.length - 1];
    if (last && last.name === keys[i]) last.ids.push(c.id);
    else out.push({ name: keys[i], ids: [c.id] });
  });
  return out;
}

function arrangeByGroup(cases: SuiteCase[], groupOrder: (names: string[]) => string[]): SuiteCase[] {
  const keys = groupKeys(cases);
  const firstSeen: string[] = [];
  for (const k of keys) if (k && !firstSeen.includes(k)) firstSeen.push(k);
  const out: SuiteCase[] = [];
  for (const name of groupOrder(firstSeen)) {
    cases.forEach((c, i) => {
      if (keys[i] === name) out.push(c);
    });
  }
  cases.forEach((c, i) => {
    if (!keys[i]) out.push(c);
  });
  return out;
}

/** Every group contiguous, groups in the order they first appear, cases
 * keeping their order inside; ungrouped cases last. What the Group by
 * title switch does when turned on. */
export function orderByGroups(cases: SuiteCase[]): SuiteCase[] {
  return arrangeByGroup(cases, (names) => names);
}

/** As orderByGroups, with the groups sorted by name. */
export function orderGroupsAZ(cases: SuiteCase[]): SuiteCase[] {
  return arrangeByGroup(cases, (names) =>
    [...names].sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase())),
  );
}

export type FileForOrder = { name: string; cases: Array<{ update_id: number | null }> };

/** The order a set of draft files asks for. Each file is one block in the
 * file's ROW order (tester_order plays no part - the numbers are what
 * jumbled a suite once); blocks follow the files' order; a case named by
 * two files goes with the first and is counted in `duplicates`; every
 * suite case in no file trails in its current order. `placed[i]` is how
 * many suite cases file i placed, so the dialog can flag a file that has
 * nothing to say about this suite. */
export function orderFromFiles(
  current: SuiteCase[],
  files: FileForOrder[],
): { order: SuiteCase[]; placed: number[]; duplicates: number } {
  const byId = new Map(current.map((c) => [c.id, c]));
  const taken = new Set<number>();
  const blocks: SuiteCase[] = [];
  const placed: number[] = [];
  let duplicates = 0;
  for (const f of files) {
    let n = 0;
    const seenHere = new Set<number>();
    for (const fc of f.cases) {
      const id = fc.update_id;
      if (id == null || !byId.has(id) || seenHere.has(id)) continue;
      seenHere.add(id);
      if (taken.has(id)) {
        duplicates += 1;
        continue;
      }
      taken.add(id);
      blocks.push(byId.get(id)!);
      n += 1;
    }
    placed.push(n);
  }
  const rest = current.filter((c) => !taken.has(c.id));
  return { order: [...blocks, ...rest], placed, duplicates };
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `npx vitest run src/lib/suiteOrder.test.ts`
Expected: PASS.

Run: `npx tsc --noEmit`
Expected: ONE error — `SuiteCases.tsx` still imports `orderFromFile`. That is Task 4's job; do not touch `SuiteCases.tsx` here. (If you would rather keep the tree compiling between tasks, leave a one-line `export const orderFromFile = …` shim marked for removal in Task 4 — say which you did in your report.)

- [ ] **Step 5: Commit**

```bash
git add src/lib/suiteOrder.ts src/lib/suiteOrder.test.ts
git commit -F - <<'MSG'
feat(v2): suite ordering rules - blocks, title groups and multi-file order

Pure functions for everything Suite Management is about to do: move a
ticked selection as one block, nudge it a step, read the list as title
sections, arrange by group (first appearance, or A-Z), and order from
several files as blocks in each file's ROW order. orderFromFile and its
tester_order sort are gone: the numbers are what jumbled a suite once.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
MSG
git log -1 --stat
```

---

### Task 2: A ticked selection drags as one block

**Files:**
- Modify: `src/screens/ManageCases/CaseOrderList.tsx`
- Test: `src/screens/ManageCases/SuiteCases.test.tsx`

**Interfaces:**
- Consumes: `moveBlock`, `nudgeBlock` from Task 1.
- Produces: no prop changes. Behaviour: dragging a ticked row moves every ticked row; Move up / Move down on a ticked row nudges the block. Unticked rows behave as before.

- [ ] **Step 1: Write the failing tests**

In `src/screens/ManageCases/SuiteCases.test.tsx`, after the existing arrow tests:

```tsx
/// Tick several rows and the arrows move them together, keeping their
/// order - the keyboard's version of dragging the block.
test("Move up on a ticked row moves the whole selection as a block", async () => {
  mount();
  const l = await list();
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #202" }));
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #203" }));
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
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #201" }));
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #202" }));
  const [r201, , r203] = within(l).getAllByRole("listitem");
  fireEvent.dragStart(r201);
  fireEvent.dragOver(r203);
  fireEvent.drop(r203);
  expect(within(l).getAllByRole("listitem").map((r) => r.textContent?.match(/#\d+/)?.[0])).toEqual(["#203", "#201", "#202"]);
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run src/screens/ManageCases/SuiteCases.test.tsx`
Expected: the two new tests FAIL (the block does not move together).

- [ ] **Step 3: Implement**

In `src/screens/ManageCases/CaseOrderList.tsx`, import `moveBlock` and `nudgeBlock` beside `moveItem`, then:

Replace `dropOn` with one that carries the selection when the dragged row is ticked:

```tsx
  // A ticked row drags its whole selection; an unticked one drags alone.
  const blockFor = (id: number): ReadonlySet<number> => (selected.has(id) ? selected : new Set([id]));
  const dropOn = (targetId: number) => {
    if (dragId == null || dragId === targetId) return;
    onChange(moveBlock(cases, blockFor(dragId), targetId));
  };
```

Replace the two arrow `onClick`s:

```tsx
                onClick={() => onChange(nudgeBlock(cases, blockFor(c.id), "up"))}
```
```tsx
                onClick={() => onChange(nudgeBlock(cases, blockFor(c.id), "down"))}
```

Their `disabled` conditions stay (`i === 0` / `i === cases.length - 1`); a block whose first member is at the top has nowhere to go anyway, and `nudgeBlock` clamps.

Update the component's doc comment: "Drag a row onto another to put it there; a ticked row carries the whole selection with it, in its order. The arrow buttons do the same one step at a time (and are what a keyboard user gets). The checkbox is the one selection the screen has: the bulk actions act on it, and so does a drag."

While dragging, dim every row in the block, not just the grabbed one: change `dragId === c.id && "opacity-50"` to `dragId != null && blockFor(dragId).has(c.id) && "opacity-50"`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `npx vitest run src/screens/ManageCases/SuiteCases.test.tsx`
Expected: PASS, including the pre-existing single-row arrow tests.

- [ ] **Step 5: Commit**

```bash
git add src/screens/ManageCases/CaseOrderList.tsx src/screens/ManageCases/SuiteCases.test.tsx
git commit -F - <<'MSG'
feat(v2): a ticked selection drags as one block in Suite Management

Tick rows, drag any of them: the whole selection moves together, keeping
its order. Move up / Move down on a ticked row nudge the block, so the
keyboard gets the same. Unticked rows move alone, as before.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
MSG
git log -1 --stat
```

---

### Task 3: Group by title, with sections that tick and drag

**Files:**
- Modify: `src/screens/ManageCases/CaseOrderList.tsx` (grouped mode)
- Modify: `src/screens/ManageCases/SuiteCases.tsx` (the switch, A–Z, the remembered setting)
- Test: `src/screens/ManageCases/SuiteCases.test.tsx`

**Interfaces:**
- Consumes: `sectionsOf`, `orderByGroups`, `orderGroupsAZ`, `moveBlock`, `nudgeBlock` from Task 1; `Switch` from `src/components/ui/switch.tsx`.
- Produces: `CaseOrderList` gains `grouped?: boolean` (default false). In grouped mode it renders a header per section with `Select group {name}` (checkbox), `Move group {name} up` / `Move group {name} down` buttons, and the header is draggable; drops onto a header land before that section's first case. Headers are `<li role="presentation">` so `getAllByRole("listitem")` still counts only case rows.

- [ ] **Step 1: Write the failing tests**

Append to `src/screens/ManageCases/SuiteCases.test.tsx`. The fixture suite's titles ("Valid login", "Bad password", "Locked out") form no groups, so these tests supply their own points through `mount`'s `extra` — read `mount()` and `point()` first: `list_test_points` is answered inside `mount` itself, so add an optional `points` argument to `mount` (default: the existing four) rather than overriding through `extra`.

```tsx
const GROUPED_POINTS = [
  point(201, "Alerts | ring once"),
  point(202, "Filter Card | collapses"),
  point(203, "Alerts | ring twice"),
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
  expect(rows()).toEqual(["#201", "#203", "#202"]);
  expect(within(l).getByText("Alerts")).toBeInTheDocument();
  expect(within(l).getByText("Filter Card")).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: "Apply order" })[0]).toBeEnabled();
  expect(localStorage.getItem("tcm-v2-group-manage")).toBe("on");

  fireEvent.click(screen.getByRole("switch", { name: "Group by title" }));
  expect(within(l).queryByText("Alerts")).not.toBeInTheDocument();
  expect(rows()).toEqual(["#201", "#203", "#202"]);
});

/// A suite opened with the switch already on is NOT rearranged - nothing
/// the user did not ask for may make the list dirty. Its headers show the
/// order as it is, split groups and all.
test("a remembered switch shows sections without reordering", async () => {
  localStorage.setItem("tcm-v2-group-manage", "on");
  mount(undefined, undefined, GROUPED_POINTS);
  const l = await list();
  expect(within(l).getAllByRole("listitem").map((r) => r.textContent?.match(/#\d+/)?.[0])).toEqual(["#201", "#202", "#203"]);
  expect(within(l).getAllByText("Alerts")).toHaveLength(2);
  expect(screen.getAllByRole("button", { name: "Apply order" })[0]).toBeDisabled();
});

test("A-Z groups sorts the groups by name; a header ticks its cases and moves as a block", async () => {
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

  fireEvent.click(within(l).getByRole("checkbox", { name: "Select group Zeta" }));
  expect(within(l).getByRole("checkbox", { name: "Select #201" })).toBeChecked();
  expect(within(l).getByRole("checkbox", { name: "Select #203" })).toBeChecked();

  fireEvent.click(within(l).getByRole("button", { name: "Move group Zeta up" }));
  expect(rows()).toEqual(["#201", "#203", "#202", "#204"]);
});
```

Change `mount`'s signature to `mount(extra = () => undefined, onToggle?, points = DEFAULT_POINTS)` where `DEFAULT_POINTS` is the four `point(...)` calls it answers today, and make `list_test_points` return `points`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run src/screens/ManageCases/SuiteCases.test.tsx`
Expected: the three new tests FAIL — there is no switch.

- [ ] **Step 3: The grouped list**

In `src/screens/ManageCases/CaseOrderList.tsx`, add the prop `grouped?: boolean` (default `false`) and import `sectionsOf`. Render sections when grouped; keep the flat map otherwise. The row renderer becomes a local `row(c, i)` (the existing `<li>` body, unchanged), and the list body becomes:

```tsx
      <ol aria-label={ariaLabel} className="divide-y divide-border">
        {!grouped && cases.map((c, i) => row(c, i))}
        {grouped &&
          sections.map((s, si) => {
            const members = new Set(s.ids);
            const firstId = s.ids[0];
            // The arrows move a section past its NEIGHBOUR section, not one
            // case: up lands before the previous section's first case, down
            // after the next section's last.
            const prev = sections[si - 1];
            const next = sections[si + 1];
            const allTicked = s.ids.every((id) => selected.has(id));
            const someTicked = s.ids.some((id) => selected.has(id));
            const name = s.name || "Ungrouped";
            return [
              // role=presentation: the header is a control strip for its
              // section, not one of the suite's cases, so anything that
              // counts listitems (tests, assistive tech) still sees the
              // cases alone.
              <li
                key={`section-${si}-${firstId}`}
                role="presentation"
                draggable={!disabled}
                onDragStart={() => setDragId(-firstId)}
                onDragEnd={() => {
                  setDragId(null);
                  setOverId(null);
                }}
                onDragOver={(e) => {
                  e.preventDefault();
                  if (overId !== firstId) setOverId(firstId);
                }}
                onDrop={(e) => {
                  e.preventDefault();
                  dropOn(firstId);
                  setDragId(null);
                  setOverId(null);
                }}
                className={cn(
                  "flex items-center gap-3 bg-surface-2/60 px-3 py-1.5 text-xs font-medium text-muted",
                  !disabled && "cursor-grab",
                  overId === firstId && "border-t-2 border-accent",
                )}
              >
                <GripVertical size={14} className="shrink-0 text-faint" aria-hidden />
                <Checkbox
                  checked={allTicked}
                  indeterminate={!allTicked && someTicked}
                  onCheckedChange={(on) => {
                    const next = new Set(selected);
                    for (const id of s.ids) on ? next.add(id) : next.delete(id);
                    onSelect(next);
                  }}
                  ariaLabel={`Select group ${name}`}
                />
                <span className="min-w-0 flex-1 truncate">{name}</span>
                <span className="text-faint">{s.ids.length}</span>
                <span className="flex shrink-0 items-center gap-1">
                  <button type="button" aria-label={`Move group ${name} up`} title="Move group up" disabled={disabled || !prev}
                    className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                    onClick={() => prev && onChange(moveBlock(cases, members, prev.ids[0]))}>
                    <IconMoveUp aria-hidden />
                  </button>
                  <button type="button" aria-label={`Move group ${name} down`} title="Move group down" disabled={disabled || !next}
                    className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                    onClick={() => next && onChange(moveBlock(cases, members, next.ids[next.ids.length - 1]))}>
                    <IconMoveDown aria-hidden />
                  </button>
                </span>
              </li>,
              ...s.ids.map((id) => row(cases.find((c) => c.id === id)!, indexOf(id))),
            ];
          })}
      </ol>
```

Dragging a header: `dragId` is set to the NEGATIVE first id as a marker. Extend `blockFor` and `dropOn` from Task 2 so a negative `dragId` means "the section whose first case is `-dragId`":

```tsx
  const blockFor = (dragged: number): ReadonlySet<number> => {
    if (dragged < 0) {
      const s = sectionsOf(cases).find((x) => x.ids[0] === -dragged);
      return new Set(s?.ids ?? []);
    }
    return selected.has(dragged) ? selected : new Set([dragged]);
  };
  const dropOn = (targetId: number) => {
    if (dragId == null) return;
    const block = blockFor(dragId);
    if (block.has(targetId)) return;
    onChange(moveBlock(cases, block, targetId));
  };
```

Compute `const sections = grouped ? sectionsOf(cases) : [];` once, above the `return`, so the header map and `blockFor` share it (`blockFor` looks a section up by its first id from `sections`, not by calling `sectionsOf` again).

Also dim a dragged section's rows: the Task 2 opacity rule already keys off `blockFor(dragId)`, so it covers this.

- [ ] **Step 4: The switch and A–Z**

In `src/screens/ManageCases/SuiteCases.tsx`, import `Switch`, `orderByGroups`, `orderGroupsAZ`, and add state:

```tsx
  // Remembered app-wide like the other screens' group switches. Turning
  // it ON arranges the list (a real reorder - Apply order lights up);
  // OFF only hides the headers. A suite opened with it already on is NOT
  // rearranged: nothing the user did not ask for may make the list dirty.
  const [grouped, setGrouped] = useState(() => {
    try {
      return localStorage.getItem("tcm-v2-group-manage") === "on";
    } catch {
      return false;
    }
  });
  const setGroupedAndRemember = (on: boolean) => {
    setGrouped(on);
    try {
      localStorage.setItem("tcm-v2-group-manage", on ? "on" : "off");
    } catch {
      // session-only
    }
    if (on) setOrder((o) => orderByGroups(o));
  };
```

In the inline button row, after "Apply order from files" (Task 4 renames it; here it is still the old button), add:

```tsx
        <label className="ml-2 flex items-center gap-2 text-xs text-muted">
          <Switch checked={grouped} onCheckedChange={setGroupedAndRemember} ariaLabel="Group by title" />
          Group by title
        </label>
        {grouped && (
          <Button size="sm" variant="ghost" disabled={busy} title="Every group together, groups A to Z" onClick={() => setOrder((o) => orderGroupsAZ(o))}>
            A-Z groups
          </Button>
        )}
```

and pass `grouped={grouped}` to `<CaseOrderList>`. Check `Switch` renders `role="switch"` with the `aria-label` (it does, per its source) so `getByRole("switch", { name: "Group by title" })` resolves.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `npx vitest run src/screens/ManageCases/SuiteCases.test.tsx`
Expected: PASS — all three new tests and every earlier one (grouping is off by default, so the flat tests see no headers).

- [ ] **Step 6: Commit**

```bash
git add src/screens/ManageCases/CaseOrderList.tsx src/screens/ManageCases/SuiteCases.tsx src/screens/ManageCases/SuiteCases.test.tsx
git commit -F - <<'MSG'
feat(v2): Suite Management groups a suite by title

A Group by title switch arranges the list so each title group sits
together and shows a header per group, with a tick that selects the
group and drag or arrows that move it as a block. A-Z groups sorts the
groups by name. Turning the switch on is a real reorder - Apply order
lights up - while a suite opened with it remembered on is left as it is.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
MSG
git log -1 --stat
```

---

### Task 4: Apply order from files

**Files:**
- Create: `src/screens/ManageCases/FileOrderDialog.tsx`
- Create: `src/screens/ManageCases/FileOrderDialog.test.tsx`
- Modify: `src/screens/ManageCases/SuiteCases.tsx` (replace `fromFile`)
- Modify: `src/screens/ManageCases/SuiteCases.test.tsx` (the two file tests)
- Modify: `README.md` if it names "Apply tester order from file"

**Interfaces:**
- Consumes: `orderFromFiles`, `FileForOrder`, `moveItem` from `src/lib/suiteOrder.ts`; `Modal`, `Button`; `open` from `@tauri-apps/plugin-dialog`; `commands.parseImportFile`.
- Produces: `FileOrderDialog({ suiteCases, files, onAddFiles, onClose, onApply })` where
  - `files: Array<FileForOrder & { path: string }>` — already parsed, in the order the user has arranged;
  - `onAddFiles: () => void` — the parent opens the picker again and appends;
  - `onApply: (order: SuiteCase[], placedTotal: number) => void`.
  The dialog owns only the arrangement (it reorders a local copy of `files`); parsing stays in `SuiteCases`, where the IPC and its error toast already live.

- [ ] **Step 1: Write the failing dialog tests**

Create `src/screens/ManageCases/FileOrderDialog.test.tsx`:

```tsx
import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import type { SuiteCase } from "../../lib/suiteOrder";
import FileOrderDialog from "./FileOrderDialog";

const suite: SuiteCase[] = [1, 2, 3, 4, 5].map((id) => ({ id, title: `Case ${id}` }));
const file = (name: string, ...ids: number[]) => ({
  path: `C:\\drafts\\${name}`,
  name,
  cases: ids.map((update_id) => ({ update_id })),
});

test("lists the files with what each places, flags an empty one, and counts cases claimed twice", () => {
  render(
    <FileOrderDialog
      suiteCases={suite}
      files={[file("a.json", 1, 2), file("b.json", 2, 3), file("c.json", 99)]}
      onAddFiles={() => {}}
      onClose={() => {}}
      onApply={() => {}}
    />,
  );
  const rows = screen.getAllByRole("listitem");
  expect(rows[0]).toHaveTextContent("a.json");
  expect(rows[0]).toHaveTextContent("places 2 of 5");
  expect(rows[1]).toHaveTextContent("places 1 of 5");
  expect(rows[2]).toHaveTextContent("places nothing from this suite");
  expect(screen.getByText(/1 test case is named in more than one file/)).toBeInTheDocument();
});

test("the files can be rearranged, and Apply hands back the resulting order", () => {
  const onApply = vi.fn();
  render(
    <FileOrderDialog
      suiteCases={suite}
      files={[file("a.json", 1, 2), file("b.json", 5, 4)]}
      onAddFiles={() => {}}
      onClose={() => {}}
      onApply={onApply}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Move b.json up" }));
  expect(screen.getAllByRole("listitem")[0]).toHaveTextContent("b.json");
  fireEvent.click(screen.getByRole("button", { name: "Apply" }));
  const [order, placed] = onApply.mock.calls[0];
  expect((order as SuiteCase[]).map((c) => c.id)).toEqual([5, 4, 1, 2, 3]);
  expect(placed).toBe(4);
});

test("Add more files asks the parent; Apply is disabled while nothing is placed", () => {
  const onAddFiles = vi.fn();
  render(
    <FileOrderDialog suiteCases={suite} files={[file("x.json", 99)]} onAddFiles={onAddFiles} onClose={() => {}} onApply={() => {}} />,
  );
  expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Add more files" }));
  expect(onAddFiles).toHaveBeenCalledTimes(1);
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run src/screens/ManageCases/FileOrderDialog.test.tsx`
Expected: FAIL — module not found.

- [ ] **Step 3: The dialog**

Create `src/screens/ManageCases/FileOrderDialog.tsx`:

```tsx
import { useEffect, useMemo, useState } from "react";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { IconCancel, IconConfirm, IconImport, IconMoveDown, IconMoveUp } from "../../lib/actionIcons";
import { moveItem, orderFromFiles, type FileForOrder, type SuiteCase } from "../../lib/suiteOrder";

export type OrderFile = FileForOrder & { path: string };

/** Arrange several draft files into the order their blocks should take in
 * the suite. Each file is one block in the file's ROW order; blocks follow
 * this list; whatever is in no file trails behind. The dialog only
 * proposes: Apply arranges the list on screen, and Apply order saves. */
export default function FileOrderDialog({
  suiteCases,
  files,
  onAddFiles,
  onClose,
  onApply,
}: {
  suiteCases: SuiteCase[];
  files: OrderFile[];
  onAddFiles: () => void;
  onClose: () => void;
  onApply: (order: SuiteCase[], placedTotal: number) => void;
}) {
  // The parent appends when the picker adds files; this keeps the user's
  // arrangement of the ones already here and tacks the new ones on the end.
  const [arranged, setArranged] = useState<OrderFile[]>(files);
  useEffect(() => {
    setArranged((cur) => {
      const known = new Set(cur.map((f) => f.path));
      return [...cur.filter((f) => files.some((n) => n.path === f.path)), ...files.filter((f) => !known.has(f.path))];
    });
  }, [files]);

  const result = useMemo(() => orderFromFiles(suiteCases, arranged), [suiteCases, arranged]);
  const placedTotal = result.placed.reduce((a, b) => a + b, 0);

  return (
    <Modal onClose={onClose} className="w-[560px] max-w-full p-4">
      <h2 className="text-sm font-semibold text-text">Apply order from files</h2>
      <p className="mt-1 text-xs text-muted">
        Each file becomes a block of cases in the order they appear in that file. Put the files in the order
        the blocks should take; anything in no file stays after them in its current order.
      </p>
      <ol className="mt-3 divide-y divide-border rounded-md border border-border">
        {arranged.map((f, i) => {
          const n = result.placed[i];
          return (
            <li key={f.path} className="flex items-center gap-3 px-3 py-2 text-sm">
              <span className="id-mono w-6 shrink-0 text-right text-faint">{i + 1}</span>
              <span className="min-w-0 flex-1 truncate text-text" title={f.path}>
                {f.name}
              </span>
              <span className={n === 0 ? "text-xs text-warning" : "text-xs text-muted"}>
                {n === 0 ? "places nothing from this suite" : `places ${n} of ${suiteCases.length}`}
              </span>
              <span className="flex shrink-0 items-center gap-1">
                <button type="button" aria-label={`Move ${f.name} up`} title="Move up" disabled={i === 0}
                  className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                  onClick={() => setArranged((a) => moveItem(a, i, i - 1))}>
                  <IconMoveUp aria-hidden />
                </button>
                <button type="button" aria-label={`Move ${f.name} down`} title="Move down" disabled={i === arranged.length - 1}
                  className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                  onClick={() => setArranged((a) => moveItem(a, i, i + 1))}>
                  <IconMoveDown aria-hidden />
                </button>
              </span>
            </li>
          );
        })}
      </ol>
      {result.duplicates > 0 && (
        <p className="mt-2 text-xs text-warning">
          {result.duplicates === 1
            ? "1 test case is named in more than one file; the first file keeps it."
            : `${result.duplicates} test cases are named in more than one file; the first file keeps them.`}
        </p>
      )}
      <div className="mt-4 flex items-center gap-2">
        <Button size="sm" variant="ghost" onClick={onAddFiles}>
          <IconImport aria-hidden />
          Add more files
        </Button>
        <span className="flex-1" />
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button size="sm" disabled={placedTotal === 0} onClick={() => onApply(result.order, placedTotal)}>
          <IconConfirm aria-hidden />
          Apply
        </Button>
      </div>
    </Modal>
  );
}
```

Check `IconCancel`, `IconConfirm`, `IconImport` exist in `src/lib/actionIcons.ts` (NewSuiteDialog and SuiteCases already use them); if `text-warning` is not a token in this app's Tailwind config, use the warning token the theme defines (grep `warning` in `src/index.css` / the Tailwind config) — never a raw colour.

`moveItem` is generic over `T`, so it works on `OrderFile[]` as written.

- [ ] **Step 4: Run the dialog tests**

Run: `npx vitest run src/screens/ManageCases/FileOrderDialog.test.tsx`
Expected: PASS.

- [ ] **Step 5: Rewrite the two file tests in SuiteCases.test.tsx**

Replace `"Apply tester order from file re-orders from the file's tester_order"` and `"a file naming none of the suite's cases changes nothing; cancelling the picker does nothing"` with:

```tsx
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
```

- [ ] **Step 6: Wire the dialog into SuiteCases**

In `src/screens/ManageCases/SuiteCases.tsx`: replace the `orderFromFile` import with `import { orderFromFiles, sameOrder, type SuiteCase } from "../../lib/suiteOrder";` (keep whatever else that line imported), import `FileOrderDialog, { type OrderFile }` from `./FileOrderDialog`, and replace the `fromFile` mutation with:

```tsx
  // The files the dialog is arranging; null = no dialog. Parsing happens
  // here, where the IPC and its error toast live; the dialog only arranges.
  const [orderFiles, setOrderFiles] = useState<OrderFile[] | null>(null);
  const pickFiles = useMutation({
    mutationFn: async () => {
      const picked = await open({
        multiple: true,
        directory: false,
        filters: [{ name: "Test case files", extensions: ["json"] }],
      });
      const paths = Array.isArray(picked) ? picked : typeof picked === "string" ? [picked] : [];
      const out: OrderFile[] = [];
      for (const path of paths) {
        const parsed = await unwrapStr(commands.parseImportFile(path));
        out.push({ path, name: path.split(/[\\/]/).pop() ?? path, cases: parsed.cases });
      }
      return out;
    },
    onSuccess: (files) => {
      if (files.length === 0) return;
      setOrderFiles((cur) => {
        const known = new Set((cur ?? []).map((f) => f.path));
        return [...(cur ?? []), ...files.filter((f) => !known.has(f.path))];
      });
    },
    onError: (e) => toast.error(`Could not read the file: ${e.message ?? e}`),
  });
```

Replace the old button with:

```tsx
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => pickFiles.mutate()}>
          <IconImport aria-hidden />
          {pickFiles.isPending ? "Reading files" : "Apply order from files"}
        </Button>
```

`busy` becomes `apply.isPending || pickFiles.isPending`. Render the dialog at the end of the component's returned fragment/div:

```tsx
      {orderFiles && orderFiles.length > 0 && (
        <FileOrderDialog
          suiteCases={order}
          files={orderFiles}
          onAddFiles={() => pickFiles.mutate()}
          onClose={() => setOrderFiles(null)}
          onApply={(next, placedTotal) => {
            setOrder(next);
            setOrderFiles(null);
            toast.info(
              `Placed ${placedTotal} of ${order.length} test cases from ${orderFiles.length === 1 ? "1 file" : `${orderFiles.length} files`}. Apply order to save.`,
            );
          }}
        />
      )}
```

Remove any shim left from Task 1. Then grep `README.md` for "tester order from file" and rename the button in the sentence if it is described there (do not touch `src/lib/changelog.ts`).

- [ ] **Step 7: Run the tests and gates**

Run: `npx vitest run src/screens/ManageCases`
Expected: PASS.

Run: `npx tsc --noEmit`
Expected: no errors (the `orderFromFile` import is gone).

Run: `npx vitest run`
Expected: every file passes, `src/ui-consistency.test.ts` included.

- [ ] **Step 8: Commit**

```bash
git add src/screens/ManageCases src/lib README.md
git commit -F - <<'MSG'
feat(v2): Apply order from files takes several files, arranged in a dialog

Pick any number of draft files; a dialog lists what each one places in
this suite and lets you put the files in the order their blocks should
take. Each block follows the file's ROW order - the tester_order numbers
no longer decide anything, since sorting by them is what jumbled a suite.
A case named in two files goes with the first, and the dialog says so.
The file still only proposes: Apply order is what saves.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
MSG
git log -1 --stat
```

---

### Task 5: Gates

**Files:** none new.

- [ ] **Step 1: The release gates, one at a time**

Run: `cd src-tauri && cargo test --tests`
Expected: unchanged (nothing in Rust changed); every suite passes.

Run: `npx tsc --noEmit`
Expected: no errors.

Run: `npx vitest run`
Expected: every file passes.

Run: `npm run build`
Expected: succeeds.

Then `git status`: the tree must be clean apart from untracked scratch. `src/bindings.ts` must be unchanged (line-ending artifact → `git checkout -- src/bindings.ts`).

- [ ] **Step 2: Report**

No commit. Report the gate results and stop; merging, pushing and a release are the user's call. The changelog entry for a release should name: block drag of a selection, Group by title with A–Z, and Apply order from files taking several files in the file's own order.

---

## Notes for the executor

- Tasks 2, 3 and 4 all edit `SuiteCases.tsx` / `SuiteCases.test.tsx` / `CaseOrderList.tsx`; do them in order and re-read each file before editing.
- The tree may not typecheck between Task 1 and Task 4 unless the shim is left; either is acceptable, but say which.
- If an anchor in a step does not match the source, apply the change to the obvious equivalent place and say so in the report. If the intent is unclear, ask.
