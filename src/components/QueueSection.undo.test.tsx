import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Dispatch, SetStateAction } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { TestCase } from "../bindings";
import { useQueue } from "../hooks/useQueue";
import type { WatchedFile } from "../lib/fileSync";
import { submitFinished, submitPhaseSnapshot, submitStarted } from "../lib/submitRun";
import { toast } from "../lib/toast";
import QueueSection from "./QueueSection";

vi.mock("../hooks/useOnScreen", () => ({ useOnScreen: () => [() => {}, true] }));
vi.mock("../lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn(), dismiss: vi.fn() },
}));

const DRAFT = "tcm-v2-draft:acme/42";

beforeEach(() => {
  localStorage.clear();
});
afterEach(() => {
  const phase = submitPhaseSnapshot();
  if (phase) submitFinished(phase.run);
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

function makeCase(title: string, overrides: Partial<TestCase> = {}): TestCase {
  return {
    title,
    steps: [{ action: `Do ${title}`, expected: `Saw ${title}` }],
    tags: "smoke",
    automation_status: "Not Automated",
    module_value: "",
    preconditions: "",
    update_id: null,
    spec_order: null,
    tester_order: null,
    ...overrides,
  };
}

let setQueueOut: Dispatch<SetStateAction<TestCase[]>> = () => {};

/** The queue is owned by the real `useQueue`, as Manual Entry owns it, so
 * what reaches storage is what the app would really save. */
type Owner = {
  watches?: WatchedFile[];
  onQueueCleared?: () => void;
  onQueueRestored?: (dropped: WatchedFile[]) => void;
};
function Harness({ owner }: { owner: Owner }) {
  const { queue, setQueue } = useQueue("acme", 42);
  setQueueOut = setQueue;
  return (
    <QueueSection
      org="acme"
      project="Web"
      pbiId={42}
      queue={queue}
      setQueue={setQueue}
      watches={owner.watches}
      onQueueCleared={owner.onQueueCleared}
      onQueueRestored={owner.onQueueRestored}
      onWatchPatched={owner.watches ? () => {} : undefined}
    />
  );
}

function renderQueue(initial: TestCase[], owner: Owner = {}) {
  localStorage.setItem(DRAFT, JSON.stringify(initial));
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return ["smoke"];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "save_draft_cases") {
      const p = args as { edits: Array<{ after: TestCase | null }> };
      return { stamp: "stamp-2", cases: p.edits.flatMap((e) => (e.after ? [e.after] : [])) };
    }
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Harness owner={owner} />
    </QueryClientProvider>,
  );
}

type Shown = { message: string; undo?: () => void };
/** The newest toast the queue showed, with its Undo if it has one. */
function lastToast(): Shown {
  const all = [toast.info, toast.success].flatMap((fn) => {
    const m = vi.mocked(fn).mock;
    return m.calls.map((call, i) => ({ order: m.invocationCallOrder[i], call }));
  });
  expect(all.length).toBeGreaterThan(0);
  all.sort((x, y) => x.order - y.order);
  const [message, opts] = all[all.length - 1].call as unknown as [string, { action?: { onClick: () => void } }?];
  return { message, undo: opts?.action?.onClick };
}

/** The queued cases on screen, in order, read off their Remove buttons. */
const rowTitles = () =>
  screen
    .queryAllByRole("button", { name: /^Remove .* from the queue$/ })
    .map((b) => /^Remove (.*) from the queue$/.exec(b.getAttribute("aria-label") ?? "")?.[1] ?? "");
const stored = (): TestCase[] => JSON.parse(localStorage.getItem(DRAFT) ?? "[]");

function pressUndo(shown: Shown) {
  expect(shown.undo).toBeTypeOf("function");
  act(() => shown.undo!());
}

test("removing one row says so, and Undo puts it back in place with its content", async () => {
  const a = makeCase("Alpha");
  const b = makeCase("Bravo", { steps: [{ action: "Bravo act", expected: "Bravo exp" }], tags: "smoke;edge" });
  const c = makeCase("Charlie");
  renderQueue([a, b, c]);
  await screen.findByText("Charlie");

  fireEvent.click(screen.getByRole("button", { name: "Remove Bravo from the queue" }));
  const shown = lastToast();
  expect(shown.message).toBe("Removed 1 case");
  expect(screen.queryByText("Bravo")).not.toBeInTheDocument();
  expect(stored().map((t) => t.title)).toEqual(["Alpha", "Charlie"]);

  pressUndo(shown);
  expect(await screen.findByText("Bravo")).toBeInTheDocument();
  expect(rowTitles()).toEqual(["Alpha", "Bravo", "Charlie"]);
  // Same row, same content - and it reached storage like any queue change.
  await waitFor(() => expect(stored()).toEqual([a, b, c]));
  // A restored row can be removed again.
  fireEvent.click(screen.getByRole("button", { name: "Remove Bravo from the queue" }));
  expect(screen.queryByText("Bravo")).not.toBeInTheDocument();
});

test("a row added after the removal stays when Undo puts the removed one back", async () => {
  renderQueue([makeCase("Alpha"), makeCase("Bravo"), makeCase("Charlie")]);
  await screen.findByText("Charlie");

  fireEvent.click(screen.getByRole("button", { name: "Remove Alpha from the queue" }));
  const shown = lastToast();
  act(() => setQueueOut((q) => [...q, makeCase("Delta")]));
  await screen.findByText("Delta");

  pressUndo(shown);
  await screen.findByText("Alpha");
  expect(rowTitles()).toEqual(["Alpha", "Bravo", "Charlie", "Delta"]);
});

test("Undo clamps to the list as it is now", async () => {
  renderQueue([makeCase("Alpha"), makeCase("Bravo"), makeCase("Charlie")]);
  await screen.findByText("Charlie");

  fireEvent.click(screen.getByRole("button", { name: "Remove Charlie from the queue" }));
  const shown = lastToast();
  // The list shrank further before Undo: Charlie's old slot is past the end.
  act(() => setQueueOut((q) => q.slice(0, 1)));
  await waitFor(() => expect(rowTitles()).toEqual(["Alpha"]));

  pressUndo(shown);
  await screen.findByText("Charlie");
  expect(rowTitles()).toEqual(["Alpha", "Charlie"]);
});

test("Undo works once; a second click does nothing", async () => {
  renderQueue([makeCase("Alpha"), makeCase("Bravo")]);
  await screen.findByText("Bravo");

  fireEvent.click(screen.getByRole("button", { name: "Remove Alpha from the queue" }));
  const shown = lastToast();
  pressUndo(shown);
  await screen.findByText("Alpha");
  // Remove it again by hand, then click the OLD Undo: nothing comes back.
  fireEvent.click(screen.getByRole("button", { name: "Remove Alpha from the queue" }));
  const again = lastToast();
  act(() => shown.undo!());
  expect(screen.queryByText("Alpha")).not.toBeInTheDocument();
  // The newer toast's own Undo still works.
  pressUndo(again);
  await screen.findByText("Alpha");
  expect(rowTitles()).toEqual(["Alpha", "Bravo"]);
  act(() => again.undo!());
  expect(rowTitles()).toEqual(["Alpha", "Bravo"]);
});

test("a newer removal takes the older Undo down: only the latest can be undone", async () => {
  renderQueue([makeCase("Alpha"), makeCase("Bravo"), makeCase("Charlie")]);
  await screen.findByText("Charlie");

  fireEvent.click(screen.getByRole("button", { name: "Remove Alpha from the queue" }));
  const first = lastToast();
  fireEvent.click(screen.getByRole("button", { name: "Remove Bravo from the queue" }));
  const second = lastToast();

  act(() => first.undo!());
  expect(rowTitles()).toEqual(["Charlie"]);
  pressUndo(second);
  await screen.findByText("Bravo");
  expect(rowTitles()).toEqual(["Bravo", "Charlie"]);
});

test("removing the selected rows: the toast counts them and Undo restores their positions", async () => {
  renderQueue([makeCase("Alpha"), makeCase("Bravo"), makeCase("Charlie"), makeCase("Delta")]);
  await screen.findByText("Delta");

  fireEvent.click(screen.getByRole("row", { name: /^Alpha / }));
  fireEvent.click(screen.getByRole("row", { name: /^Charlie / }), { ctrlKey: true });
  fireEvent.click(screen.getByRole("button", { name: /^Remove 2/ }));
  const shown = lastToast();
  expect(shown.message).toBe("Removed 2 cases");
  await waitFor(() => expect(rowTitles()).toEqual(["Bravo", "Delta"]));

  pressUndo(shown);
  await screen.findByText("Charlie");
  expect(rowTitles()).toEqual(["Alpha", "Bravo", "Charlie", "Delta"]);
  // Selection is cleared on the restored rows.
  expect(screen.queryByText(/of 4 selected/)).not.toBeInTheDocument();
  await waitFor(() => expect(stored().map((t) => t.title)).toEqual(["Alpha", "Bravo", "Charlie", "Delta"]));
});

test("Clear queue says how many went, and Undo puts them all back ahead of newer rows", async () => {
  const rows = [makeCase("Alpha"), makeCase("Bravo"), makeCase("Charlie")];
  renderQueue(rows);
  await screen.findByText("Charlie");

  fireEvent.click(screen.getByRole("button", { name: /Clear queue/ }));
  const shown = lastToast();
  expect(shown.message).toBe("Removed all 3 cases");
  await waitFor(() => expect(localStorage.getItem(DRAFT)).toBeNull());

  act(() => setQueueOut([makeCase("Zulu")]));
  await screen.findByText("Zulu");
  pressUndo(shown);
  await screen.findByText("Alpha");
  expect(rowTitles()).toEqual(["Alpha", "Bravo", "Charlie", "Zulu"]);
  await waitFor(() => expect(stored().slice(0, 3)).toEqual(rows));
});

async function bulkSetStatus(count: number, status: string) {
  fireEvent.click(await screen.findByRole("button", { name: /Bulk edit/ }));
  await screen.findByText(new RegExp(`Bulk edit ${count} queued draft`));
  fireEvent.click(screen.getByLabelText(/Automation status/));
  fireEvent.click(screen.getByRole("option", { name: status }));
  fireEvent.click(screen.getByRole("button", { name: new RegExp(`Apply to ${count}`) }));
}

test("a bulk edit says how many it edited, and Undo reverts the fields it set", async () => {
  renderQueue([makeCase("Alpha"), makeCase("Bravo"), makeCase("Charlie")]);
  await screen.findByText("Charlie");

  fireEvent.click(screen.getByRole("row", { name: /^Alpha / }));
  fireEvent.click(screen.getByRole("row", { name: /^Bravo / }), { ctrlKey: true });
  await bulkSetStatus(2, "Planned");
  await waitFor(() => expect(stored().map((t) => t.automation_status)).toEqual(["Planned", "Planned", "Not Automated"]));
  const shown = lastToast();
  expect(shown.message).toBe("Edited 2 cases");

  pressUndo(shown);
  await waitFor(() =>
    expect(stored().map((t) => t.automation_status)).toEqual(["Not Automated", "Not Automated", "Not Automated"]),
  );
  expect(stored().map((t) => t.title)).toEqual(["Alpha", "Bravo", "Charlie"]);
  expect(stored()[0].steps).toEqual([{ action: "Do Alpha", expected: "Saw Alpha" }]);
});

test("a field changed by hand after a bulk edit is not reverted", async () => {
  renderQueue([makeCase("Alpha"), makeCase("Bravo")]);
  await screen.findByText("Bravo");

  fireEvent.click(screen.getByRole("row", { name: /^Alpha / }));
  fireEvent.click(screen.getByRole("row", { name: /^Bravo / }), { ctrlKey: true });
  await bulkSetStatus(2, "Planned");
  await waitFor(() => expect(stored().map((t) => t.automation_status)).toEqual(["Planned", "Planned"]));
  const shown = lastToast();

  // Bravo's status is changed by hand afterwards, and Alpha gets a new tag.
  act(() =>
    setQueueOut((q) =>
      q.map((t) =>
        t.title === "Bravo" ? { ...t, automation_status: "Automated" } : { ...t, tags: "smoke;later" },
      ),
    ),
  );
  pressUndo(shown);
  await waitFor(() => expect(stored().map((t) => t.automation_status)).toEqual(["Not Automated", "Automated"]));
  // A field the bulk edit never set is not touched.
  expect(stored()[0].tags).toBe("smoke;later");
});

test("a bulk edit's Undo skips rows that are gone and finds the rest after a row was removed", async () => {
  renderQueue([makeCase("Alpha"), makeCase("Bravo"), makeCase("Charlie")]);
  await screen.findByText("Charlie");

  fireEvent.click(screen.getByRole("row", { name: /^Bravo / }));
  fireEvent.click(screen.getByRole("row", { name: /^Charlie / }), { ctrlKey: true });
  await bulkSetStatus(2, "Planned");
  await waitFor(() => expect(stored().map((t) => t.automation_status)).toEqual(["Not Automated", "Planned", "Planned"]));
  const shown = lastToast();

  // Alpha goes: Bravo and Charlie shift up. Bravo is then removed too.
  act(() => setQueueOut((q) => q.filter((t) => t.title === "Charlie")));
  pressUndo(shown);
  await waitFor(() => expect(stored().map((t) => [t.title, t.automation_status])).toEqual([["Charlie", "Not Automated"]]));
});

test("Undo does nothing to Azure DevOps", async () => {
  const sent: string[] = [];
  renderQueue([makeCase("Alpha"), makeCase("Bravo")]);
  mockIPC((cmd) => {
    sent.push(cmd);
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    return undefined;
  });
  await screen.findByText("Bravo");
  sent.length = 0;

  fireEvent.click(screen.getByRole("button", { name: "Remove Alpha from the queue" }));
  pressUndo(lastToast());
  await screen.findByText("Alpha");
  const writes = sent.filter((c) => !c.startsWith("plugin:") && /submit|create|update|patch|upload/i.test(c));
  expect(writes).toEqual([]);
});


const fileOf = (rows: TestCase[]): WatchedFile => ({ path: "C:/drafts/a.json", stamp: "stamp-1", snapshot: rows });

test("removing a row a watched file owns gets no Undo; a hand-typed row still does", async () => {
  const owned = makeCase("Owned");
  const owned2 = makeCase("Owned too");
  const hand = makeCase("Typed by hand");
  renderQueue([owned, owned2, hand], { watches: [fileOf([owned, owned2])], onQueueCleared: () => {} });
  await screen.findByText("Typed by hand");

  fireEvent.click(screen.getByRole("button", { name: "Remove Owned from the queue" }));
  expect(lastToast()).toEqual({ message: "Removed 1 case", undo: undefined });

  // Selected rows: one owned among them is enough to withhold Undo.
  fireEvent.click(screen.getByRole("row", { name: /^Owned too / }));
  fireEvent.click(screen.getByRole("row", { name: /^Typed by hand / }), { ctrlKey: true });
  fireEvent.click(screen.getByRole("button", { name: /^Remove 2/ }));
  expect(lastToast()).toEqual({ message: "Removed 2 cases", undo: undefined });
});

test("removing only hand-typed rows keeps Undo even when a file feeds the queue", async () => {
  const owned = makeCase("Owned");
  renderQueue([owned, makeCase("Typed by hand")], { watches: [fileOf([owned])] });
  await screen.findByText("Typed by hand");

  fireEvent.click(screen.getByRole("button", { name: "Remove Typed by hand from the queue" }));
  pressUndo(lastToast());
  await screen.findByText("Typed by hand");
  expect(rowTitles()).toEqual(["Owned", "Typed by hand"]);
});

test("Undo of Remove all arms the dropped file watches again", async () => {
  const a = makeCase("Alpha");
  const watch = fileOf([a]);
  const cleared = vi.fn();
  const restored = vi.fn();
  renderQueue([a, makeCase("Bravo")], { watches: [watch], onQueueCleared: cleared, onQueueRestored: restored });
  await screen.findByText("Bravo");

  fireEvent.click(screen.getByRole("button", { name: /Clear queue/ }));
  expect(cleared).toHaveBeenCalledTimes(1);
  const shown = lastToast();
  expect(restored).not.toHaveBeenCalled();
  pressUndo(shown);
  await screen.findByText("Alpha");
  expect(restored).toHaveBeenCalledTimes(1);
  expect(restored.mock.calls[0][0]).toEqual([watch]);
});

test("a bulk remove that empties the queue is not reported cleared after Undo came first", async () => {
  const cleared = vi.fn();
  renderQueue([makeCase("Alpha"), makeCase("Bravo")], { onQueueCleared: cleared });
  await screen.findByText("Bravo");

  fireEvent.click(screen.getByRole("checkbox", { name: "Select all queued cases" }));
  fireEvent.click(screen.getByRole("button", { name: /^Remove 2/ }));
  // Undo before the write-back has had a turn to return.
  pressUndo(lastToast());
  await new Promise((r) => setTimeout(r, 20));
  expect(cleared).not.toHaveBeenCalled();
  expect(rowTitles()).toEqual(["Alpha", "Bravo"]);
});

test("a bulk edit Undo does not guess between rows that share a title", async () => {
  renderQueue([makeCase("Alpha"), makeCase("Bravo")]);
  await screen.findByText("Bravo");

  fireEvent.click(screen.getByRole("row", { name: /^Alpha / }));
  await bulkSetStatus(1, "Planned");
  await waitFor(() => expect(stored().map((t) => t.automation_status)).toEqual(["Planned", "Not Automated"]));
  const shown = lastToast();

  // The rows are replaced by new ones, two of which now share the title.
  act(() =>
    setQueueOut([makeCase("Alpha", { automation_status: "Planned" }), makeCase("Alpha", { automation_status: "Planned" })]),
  );
  pressUndo(shown);
  await new Promise((r) => setTimeout(r, 20));
  expect(stored().map((t) => t.automation_status)).toEqual(["Planned", "Planned"]);
});

test("while an upload runs, a removal gets no Undo and Undo is refused", async () => {
  renderQueue([makeCase("Alpha"), makeCase("Bravo"), makeCase("Charlie")]);
  await screen.findByText("Charlie");

  fireEvent.click(screen.getByRole("button", { name: "Remove Alpha from the queue" }));
  const before = lastToast();
  expect(before.undo).toBeTypeOf("function");

  // An upload starts after the removal: Undo is refused and says why.
  act(() => void submitStarted("acme", 42, 2));
  act(() => before.undo!());
  expect(rowTitles()).toEqual(["Bravo", "Charlie"]);
  expect(vi.mocked(toast.info)).toHaveBeenLastCalledWith("Undo is not available while an upload is running.");

  // A removal made during the upload offers none.
  fireEvent.click(screen.getByRole("button", { name: "Remove Bravo from the queue" }));
  expect(lastToast()).toEqual({ message: "Removed 1 case", undo: undefined });
});
