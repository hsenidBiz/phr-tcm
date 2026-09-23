import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState, type Dispatch, type SetStateAction } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "../lib/toast";
import type { TestCase } from "../bindings";
import type { WatchedFile } from "../lib/fileSync";
import { cacheKeys, cacheWrite } from "../lib/cache";
import { submitFinished, submitPhaseSnapshot } from "../lib/submitRun";
import QueueSection from "./QueueSection";

/** The floating copy only exists while the real row is off screen, so the
 * tests drive the hook rather than jsdom's (non-existent) layout. The
 * hook itself - including the part that has to notice the action row
 * arriving in a queue that started empty - is exercised UNMOCKED in
 * QueueSection.floating.test.tsx. */
vi.mock("../hooks/useOnScreen", () => ({ useOnScreen: () => [() => {}, onScreen] }));
let onScreen = true;

// Fix round 1 (C2 hold): toasts are asserted by content below - no
// <Toaster/> is mounted in these tests, so the real module has nothing to
// render them into.
vi.mock("../lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
  // A test that fails mid-submit (or forgets to finish one) must not leave
  // the module-scope phase claimed - the next test's "no submit running"
  // assumption would otherwise fail for a reason that has nothing to do
  // with it.
  const p = submitPhaseSnapshot();
  if (p) submitFinished(p.run);
});

function makeCase(overrides: Partial<TestCase> = {}): TestCase {
  return {
    title: "Login works",
    steps: [{ action: "Open page", expected: "Page shown" }],
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

/** Owns the queue state the way ManualEntry / ImportFile do. */
function Harness({
  initial,
  watches,
  onWatchPatched,
  pbiId = 42,
  exposeSetQueue,
}: {
  initial: TestCase[];
  watches?: WatchedFile[];
  onWatchPatched?: (path: string, fields: Partial<WatchedFile>) => void;
  pbiId?: number;
  /** Hands the test the setter React itself would only give a real parent
   * screen - so a test can simulate the queue changing out from under this
   * mount (a file sync, a kept-uploaded row) without going through UI. */
  exposeSetQueue?: (setter: Dispatch<SetStateAction<TestCase[]>>) => void;
}) {
  const [queue, setQueue] = useState<TestCase[]>(initial);
  exposeSetQueue?.(setQueue);
  return (
    <QueueSection
      org="acme"
      project="Web"
      pbiId={pbiId}
      queue={queue}
      setQueue={setQueue}
      watches={watches}
      onWatchPatched={onWatchPatched}
    />
  );
}

function renderQueue(
  initial: TestCase[],
  extra?: {
    watches?: WatchedFile[];
    onWatchPatched?: (path: string, fields: Partial<WatchedFile>) => void;
    pbiId?: number;
    exposeSetQueue?: (setter: Dispatch<SetStateAction<TestCase[]>>) => void;
  },
) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Harness
        initial={initial}
        watches={extra?.watches}
        onWatchPatched={extra?.onWatchPatched}
        pbiId={extra?.pbiId}
        exposeSetQueue={extra?.exposeSetQueue}
      />
    </QueryClientProvider>,
  );
}

/** Same harness as `renderQueue`, but hands back the QueryClient so a test
 * can pre-seed a query and check it comes out invalidated. */
function renderQueueWithClient(
  initial: TestCase[],
  extra?: {
    watches?: WatchedFile[];
    onWatchPatched?: (path: string, fields: Partial<WatchedFile>) => void;
  },
) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const result = render(
    <QueryClientProvider client={qc}>
      <Harness initial={initial} watches={extra?.watches} onWatchPatched={extra?.onWatchPatched} />
    </QueryClientProvider>,
  );
  return { ...result, qc };
}

function baseMocks() {
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return ["smoke", "regression"];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    return undefined;
  });
}

/// Both steps that change what the action row CONTAINS also make it
/// taller, and on a long queue that pushed the button you need next below
/// the fold. The floating copy covers the first step but deliberately
/// stands down for the armed warning, which is the one that must be read -
/// so the row itself comes to the reader instead.
test("opening review scrolls the action row into view", async () => {
  baseMocks();
  const spy = vi.spyOn(Element.prototype, "scrollIntoView");
  // Restored even when an assertion below throws: a spy left on
  // Element.prototype leaks into the next test, which then counts this
  // test's calls as well as its own.
  try {
    renderQueue([makeCase()]);

    // Not on arrival: the row is wherever the user already is, and moving
    // the page under someone who has not asked for anything is worse than
    // the scroll this fixes.
    expect(spy).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(1));
    // Smoothly: a jump saves the scrolling and spends it again on working
    // out where you were thrown to.
    expect(spy).toHaveBeenLastCalledWith({ block: "end", behavior: "smooth" });

    // Once, not twice. The review and the PBI warning now arrive together,
    // so there is no second growth of the row to chase.
    await screen.findByRole("button", { name: /Yes — create 1/ });
    expect(spy).toHaveBeenCalledTimes(1);
  } finally {
    spy.mockRestore();
  }
});

/// The tour rings the review and upload controls, and that stop has to
/// make sense whether or not review is already open - so the anchor goes
/// on the action row, which holds Review before and the confirm/upload
/// button during. An anchor on the Review button alone would vanish the
/// moment it was clicked.
test("the tour's anchor holds the review controls before and during review", async () => {
  baseMocks();
  const { container } = renderQueue([makeCase()]);
  const box = () => container.querySelector('[data-tour="queue-review"]') as HTMLElement;

  expect(box(), 'no [data-tour="queue-review"] in the queue').not.toBeNull();
  expect(box().textContent).toMatch(/Review 1 test case/);

  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  await screen.findByRole("button", { name: /Yes — create 1/ });
  expect(box(), "the anchor disappeared when review opened").not.toBeNull();
  expect(box().textContent).toMatch(/Yes — create 1/);
});

/// The app turns its animations off under prefers-reduced-motion in seven
/// other places. A scroll that animated anyway would be the one that got
/// away, and for someone who set that preference motion is not a nicety.
test("the review scroll does not animate under prefers-reduced-motion", async () => {
  const realMatchMedia = window.matchMedia;
  window.matchMedia = ((q: string) => ({
    matches: q.includes("prefers-reduced-motion"),
    media: q,
    addEventListener() {},
    removeEventListener() {},
  })) as unknown as typeof window.matchMedia;
  const spy = vi.spyOn(Element.prototype, "scrollIntoView");
  try {
    baseMocks();
    renderQueue([makeCase()]);
    fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(1));
    expect(spy).toHaveBeenLastCalledWith({ block: "end", behavior: "auto" });
  } finally {
    spy.mockRestore();
    window.matchMedia = realMatchMedia;
  }
});

test("Edit opens the inline editor and Save writes back into the queue", async () => {
  baseMocks();
  renderQueue([makeCase()]);

  expect(screen.getByText("Login works").closest("li")!.className).toContain("cv-row");

  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  const title = await screen.findByLabelText("Case title");
  // The editor's Combobox dropdown paints past the row, so an editing row
  // must drop content-visibility's paint containment.
  expect(title.closest("li")!.className).not.toContain("cv-row");
  fireEvent.change(title, { target: { value: "Login works — edited" } });
  fireEvent.change(screen.getByLabelText("Step 1 expected"), {
    target: { value: "Dashboard shown" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save to queue" }));

  // The row shows the new title and the editor is gone.
  expect(await screen.findByText("Login works — edited")).toBeInTheDocument();
  expect(screen.queryByLabelText("Case title")).not.toBeInTheDocument();
});

test("editing preserves the UPDATE badge (update_id survives a save)", async () => {
  baseMocks();
  renderQueue([makeCase({ update_id: 777 })]);
  expect(screen.getByText("UPDATE #777")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(await screen.findByLabelText("Case title"), {
    target: { value: "Renamed update" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save to queue" }));

  expect(await screen.findByText("Renamed update")).toBeInTheDocument();
  expect(screen.getByText("UPDATE #777")).toBeInTheDocument();
});

test("Save is blocked while the edited case is invalid", async () => {
  baseMocks();
  renderQueue([makeCase()]);

  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(await screen.findByLabelText("Case title"), { target: { value: "  " } });

  expect(screen.getByRole("button", { name: "Save to queue" })).toBeDisabled();
  expect(screen.getByText("Title is required.")).toBeInTheDocument();
});

test("Cancel discards the edits", async () => {
  baseMocks();
  renderQueue([makeCase()]);

  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(await screen.findByLabelText("Case title"), {
    target: { value: "Should not stick" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

  expect(screen.getByText("Login works")).toBeInTheDocument();
  expect(screen.queryByText("Should not stick")).not.toBeInTheDocument();
});

test("removing a row closes any open editor (indices shift)", async () => {
  baseMocks();
  renderQueue([makeCase(), makeCase({ title: "Second case" })]);

  fireEvent.click(screen.getAllByRole("button", { name: "Edit" })[1]);
  await screen.findByLabelText("Case title");
  fireEvent.click(screen.getAllByRole("button", { name: "Remove" })[0]);

  expect(screen.queryByLabelText("Case title")).not.toBeInTheDocument();
  expect(screen.getByText("Second case")).toBeInTheDocument();
});

/// The upload takes minutes and the person watching it is exactly the
/// person who wanders to another tab. The bar reads MODULE-scope phase, so
/// a fresh mount shows a submit some other mount started - which is the
/// whole reported bug: navigating away reset the bar to nothing.
test("a fresh mount shows a submit already in flight", async () => {
  const { submitStarted, submitProgressed, submitFinished } = await import("../lib/submitRun");
  baseMocks();
  const run = submitStarted("acme", 42, 10)!;
  submitProgressed(3, 10, "Login works");
  try {
    renderQueue([makeCase()]);
    const bar = await screen.findByRole("progressbar", { name: "Uploading" });
    expect(bar).toHaveAttribute("aria-valuenow", "3");
    expect(screen.getByText("3 / 10")).toBeInTheDocument();
  } finally {
    submitFinished(run);
  }
});

/// One control while an upload runs, not two. The count beside the bar
/// already says "Processing 3/10", so a second button repeating the word
/// while doing nothing was the duplicate - the button's job is the only
/// thing still available to do.
///
/// Gated on the PROGRESS store rather than this mount's own mutation, for
/// the same reason the bar is: the upload outlives the mount that started
/// it, and a stop control that a fresh mount cannot show would be a stop
/// control missing exactly when someone came back to use it.
test("an upload in flight shows the sweeping bar first, then the count, and no Stop", async () => {
  const { submitStarted, submitProgressed, submitFinished } = await import("../lib/submitRun");
  baseMocks();
  const run = submitStarted("acme", 42, 10)!;
  try {
    renderQueue([makeCase()]);
    // Before the first batch answers: the suite is being resolved and the
    // batch is in flight, so the bar sweeps with no count.
    const bar = await screen.findByRole("progressbar", { name: "Processing the upload" });
    expect(bar).not.toHaveAttribute("aria-valuenow");
    // No way to stop, and Review is not on offer while the queue it would
    // review is being written: the action spot says what is happening.
    expect(screen.queryByRole("button", { name: /Stop|Cancel/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Review 1 test case/ })).not.toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Processing" })[0]).toBeDisabled();

    submitProgressed(3, 10, "Login works");
    const filled = await screen.findByRole("progressbar", { name: "Uploading" });
    expect(filled).toHaveAttribute("aria-valuenow", "3");
    expect(filled).toHaveAttribute("aria-valuemax", "10");
  } finally {
    submitFinished(run);
  }
});

/// And a submit for a DIFFERENT scope stays invisible - PBI 7's progress
/// must never render over PBI 42's queue.
test("another PBI's submit does not show here", async () => {
  const { submitStarted, submitFinished } = await import("../lib/submitRun");
  baseMocks();
  const run = submitStarted("acme", 7, 5)!;
  try {
    renderQueue([makeCase()]);
    await screen.findByText("Login works");
    expect(screen.queryByText(/Processing/)).not.toBeInTheDocument();
  } finally {
    submitFinished(run);
  }
});

/// Offline, the deliberate network writes PAUSE with a reason instead of
/// failing with a toast - and everything local (edit, remove, export)
/// stays usable.
test("offline disables the network writes and says why", async () => {
  baseMocks();
  Object.defineProperty(navigator, "onLine", { value: false, configurable: true });
  window.dispatchEvent(new Event("offline"));
  try {
    renderQueue([makeCase()]);
    const share = screen.getByRole("button", { name: /Share for review/ });
    expect(share).toBeDisabled();
    expect(share).toHaveAttribute("title", expect.stringContaining("No internet"));
    // Local work is untouched by the gate.
    expect(screen.getByRole("button", { name: "Edit" })).toBeEnabled();
    expect(screen.getByRole("button", { name: /Export JSON/ })).toBeEnabled();

    Object.defineProperty(navigator, "onLine", { value: true, configurable: true });
    window.dispatchEvent(new Event("online"));
    await waitFor(() => expect(share).toBeEnabled());
  } finally {
    Object.defineProperty(navigator, "onLine", { value: true, configurable: true });
    window.dispatchEvent(new Event("online"));
  }
});

/// Selecting rows arms the bulk bar; Remove writes the survivors back into
/// the file the removed cases came from - the file must say what the queue
/// says, or the next external save quietly reverts the removal.
/** What `save_draft_cases` receives: one edit per owned row, in queue order. */
type SentEdits = { path: string; edits: Array<{ before: TestCase; after: TestCase | null }> };
/** The titles the file will hold for the queue's rows (removed rows drop out). */
const keptTitles = (p: SentEdits) =>
  p.edits.flatMap((e) => (e.after ? [e.after.title] : []));

test("bulk remove updates the queue AND the owning .json file", async () => {
  const a = makeCase({ title: "From file A" });
  const b = makeCase({ title: "Also from file A" });
  const hand = makeCase({ title: "Typed by hand" });
  const saved: Array<{ path: string; titles: string[]; edits: Array<[string, string | null]> }> = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "save_draft_cases") {
      const p = args as SentEdits;
      saved.push({
        path: p.path,
        titles: keptTitles(p),
        edits: p.edits.map((e) => [e.before.title, e.after?.title ?? null]),
      });
      return "stamp-2";
    }
    return undefined;
  });
  const patched: Array<{ path: string; stamp?: string }> = [];
  renderQueue([a, b, hand], {
    watches: [{ path: "C:/drafts/a.json", stamp: "stamp-1", snapshot: [a, b] }],
    onWatchPatched: (path, fields) => patched.push({ path, stamp: fields.stamp }),
  });

  fireEvent.click(screen.getByRole("checkbox", { name: "Select From file A" }));
  fireEvent.click(screen.getByRole("button", { name: /Remove 1/ }));

  // The queue lost the case; the file was rewritten WITHOUT it but keeps
  // its other case; the watch fingerprint moved forward.
  expect(screen.queryByText("From file A")).not.toBeInTheDocument();
  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0].path).toBe("C:/drafts/a.json");
  expect(saved[0].titles).toEqual(["Also from file A"]);
  // In QUEUE order, the removal interleaved where it happened: the file
  // claims its entries in this order, so the Nth same-titled row is the
  // Nth same-titled entry.
  expect(saved[0].edits).toEqual([
    ["From file A", null],
    ["Also from file A", "Also from file A"],
  ]);
  await waitFor(() => expect(patched).toEqual([{ path: "C:/drafts/a.json", stamp: "stamp-2" }]));
});

/// The bulk edit dialog hands back one pure edit. The observable contract
/// is the write-back: the owning file receives the selected case CHANGED
/// and the unselected one exactly as it was.
test("bulk edit applies to the selection and leaves unselected rows alone", async () => {
  const picked = makeCase({ title: "Picked", automation_status: "Not Automated" });
  const alone = makeCase({ title: "Left alone", automation_status: "Not Automated" });
  const saved: Array<Record<string, string>> = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return ["smoke"];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "save_draft_cases") {
      const p = args as SentEdits;
      for (const e of p.edits) {
        if (e.after) saved.push({ title: e.after.title, status: e.after.automation_status });
      }
      return "stamp-2";
    }
    return undefined;
  });
  renderQueue([picked, alone], {
    watches: [{ path: "C:/drafts/a.json", stamp: "stamp-1", snapshot: [picked, alone] }],
    onWatchPatched: () => {},
  });

  fireEvent.click(screen.getByRole("checkbox", { name: "Select Picked" }));
  fireEvent.click(screen.getByRole("button", { name: /Bulk edit/ }));

  expect(await screen.findByText(/Bulk edit 1 queued draft/)).toBeInTheDocument();
  fireEvent.click(screen.getByLabelText(/Automation status/));
  fireEvent.click(screen.getByRole("option", { name: "Planned" }));
  fireEvent.click(screen.getByRole("button", { name: /Apply to 1/ }));

  await waitFor(() => expect(saved).toHaveLength(2));
  expect(saved).toEqual([
    { title: "Picked", status: "Planned" },
    { title: "Left alone", status: "Not Automated" },
  ]);
});

/// With a selection, Power Rename covers exactly the selected rows - and
/// the rename reaches the owning file, because a renamed case whose file
/// still holds the old title is reverted by the file's next save.
test("power rename scoped to the selection writes the file back", async () => {
  const a = makeCase({ title: "Old name" });
  const b = makeCase({ title: "Untouched" });
  const saved: Array<{ titles: string[] }> = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "save_draft_cases") {
      saved.push({ titles: keptTitles(args as SentEdits) });
      return "stamp-2";
    }
    return undefined;
  });
  renderQueue([a, b], {
    watches: [{ path: "C:/drafts/a.json", stamp: "stamp-1", snapshot: [a, b] }],
    onWatchPatched: () => {},
  });

  fireEvent.click(screen.getByRole("checkbox", { name: "Select Old name" }));
  // The bulk bar's rename button carries the count - proof of the scoping.
  fireEvent.click(screen.getByRole("button", { name: /^Rename 1/ }));
  const dialog = await screen.findByText(/1 selected draft/);
  expect(dialog).toBeInTheDocument();
});

/// The white window, caught by the app log on 2026-09-11: "Cannot read
/// properties of undefined (reading 'update_id')" right after Remove all.
/// The selection is a set of positions; Remove all emptied the queue under
/// it and the next render indexed the empty queue with a stale position.
test("Remove all with every row selected does not crash the screen", async () => {
  baseMocks();
  renderQueue([makeCase({ title: "One" }), makeCase({ title: "Two" })]);
  const quiet = vi.spyOn(console, "error").mockImplementation(() => {});
  try {
    fireEvent.click(screen.getByRole("checkbox", { name: "Select all queued cases" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove all" }));
  } finally {
    quiet.mockRestore();
  }
  expect(screen.queryByText("One")).not.toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

/// An optimized file carries both readings; the queue can be laid out in
/// either. The sort is real - the queue's order is the creation order.
test("the queue flips between tester order and spec order", async () => {
  baseMocks();
  renderQueue([
    makeCase({ title: "Walks the spec first", spec_order: 1, tester_order: 3 }),
    makeCase({ title: "Runs first for the tester", spec_order: 3, tester_order: 1 }),
    makeCase({ title: "Middle either way", spec_order: 2, tester_order: 2 }),
  ]);

  const TITLE = /Walks the spec first|Runs first for the tester|Middle either way/;
  // The matcher can land on a row container whose text also carries the
  // NEW badge and the steps count - extract just the title for comparing.
  const titles = () => screen.getAllByText(TITLE).map((el) => el.textContent?.match(TITLE)?.[0]);

  fireEvent.click(screen.getByRole("button", { name: "For testing" }));
  expect(titles()).toEqual([
    "Runs first for the tester",
    "Middle either way",
    "Walks the spec first",
  ]);

  fireEvent.click(screen.getByRole("button", { name: "Down the spec" }));
  expect(titles()).toEqual([
    "Walks the spec first",
    "Middle either way",
    "Runs first for the tester",
  ]);
});

/// A partial sort would interleave ordered cases with ones that have no
/// opinion - which is neither reading. Cases typed by hand have no orders,
/// so a mixed queue offers the buttons disabled, and an unordered queue
/// not at all.
test("the order buttons need every case to carry the field", async () => {
  baseMocks();
  const { unmount } = renderQueue([
    makeCase({ title: "Stamped", spec_order: 1, tester_order: 1 }),
    makeCase({ title: "Hand-typed" }),
  ]);
  expect(screen.getByRole("button", { name: "For testing" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Down the spec" })).toBeDisabled();
  unmount();

  renderQueue([makeCase(), makeCase({ title: "Also plain" })]);
  expect(screen.queryByRole("button", { name: "For testing" })).not.toBeInTheDocument();
});

test("a case's in-app comment shows on the row and is editable in the editor", async () => {
  baseMocks();
  renderQueue([makeCase({ comment: "Imported from sprint 12 sheet" })]);

  // The note from the JSON file renders on the queue row.
  expect(screen.getByText("Imported from sprint 12 sheet")).toBeInTheDocument();

  // The inline editor exposes it as an in-app-only field.
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  const comment = await screen.findByLabelText("Comment (in-app only)");
  fireEvent.change(comment, { target: { value: "Re-check with QA" } });
  fireEvent.click(screen.getByRole("button", { name: "Save to queue" }));

  expect(await screen.findByText("Re-check with QA")).toBeInTheDocument();
  expect(screen.queryByText("Imported from sprint 12 sheet")).not.toBeInTheDocument();
});

/// Fix for a real duplicate incident: a single-row Edit that renamed a case
/// left the FILE with the old title, so the post-submit id stamping could
/// not find an owner and the created id was recorded nowhere. The editor
/// now writes through to the owning file exactly like bulk edits do.
test("single Edit save writes the change through to the owning file", async () => {
  const a = makeCase({ title: "Original title" });
  const saved: Array<{ path: string; titles: string[] }> = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "save_draft_cases") {
      const p = args as SentEdits;
      saved.push({ path: p.path, titles: keptTitles(p) });
      return "stamp-2";
    }
    return undefined;
  });
  const patched: Array<{ path: string; stamp?: string }> = [];
  renderQueue([a], {
    watches: [{ path: "C:/drafts/a.json", stamp: "stamp-1", snapshot: [a] }],
    onWatchPatched: (path, fields) => patched.push({ path, stamp: fields.stamp }),
  });

  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(await screen.findByLabelText("Case title"), {
    target: { value: "Renamed title" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save to queue" }));

  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0].path).toBe("C:/drafts/a.json");
  expect(saved[0].titles).toEqual(["Renamed title"]);
  await waitFor(() => expect(patched).toEqual([{ path: "C:/drafts/a.json", stamp: "stamp-2" }]));
});

/// A rename changes the very title the file would find its copy by, so the
/// write-back sends the row as it was before the edit alongside it.
test("a rename write-back tells the file which case it was", async () => {
  const a = makeCase({ title: "Original title" });
  const saved: Array<Array<{ before: string; after: string | null }>> = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "save_draft_cases") {
      const p = args as SentEdits;
      saved.push(p.edits.map((e) => ({ before: e.before.title, after: e.after?.title ?? null })));
      return "stamp-2";
    }
    return undefined;
  });
  renderQueue([a], {
    watches: [{ path: "C:/drafts/a.json", stamp: "stamp-1", snapshot: [a] }],
    onWatchPatched: () => {},
  });

  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(await screen.findByLabelText("Case title"), {
    target: { value: "Renamed title" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save to queue" }));

  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0]).toEqual([{ before: "Original title", after: "Renamed title" }]);
});

/// The last safeguard: the final Yes re-checks ADO and STOPS when a case
/// about to be created already exists by title - 43 duplicates once went
/// through because the per-row hint was scrollable-past.
test("the duplicate check runs when review opens, not after the final yes", async () => {
  let submits = 0;
  let fetches = 0;
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") {
      fetches += 1;
      return [{ id: 201, title: "Login works", tags: "", automation_status: "Planned" }];
    }
    if (cmd === "submit_queue") {
      submits += 1;
      return [{ index: 0, title: "Login works", action: "created", id: 900, error: null }];
    }
    return undefined;
  });
  renderQueue([makeCase()]); // a CREATE row whose title already exists

  // ONE click into the review, and the check has already run and stopped
  // it. It used to take three, and the third was a button that said it
  // would create.
  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  expect(await screen.findByText(/Stopped: 1 case/)).toBeInTheDocument();
  expect(fetches).toBeGreaterThan(0);
  expect(submits).toBe(0);

  // The way on is disabled while the warning stands, so it cannot be
  // scrolled past on a long queue.
  expect(await screen.findByRole("button", { name: /Yes — create 1/ })).toBeDisabled();

  // Accepting clears the warning and frees the button - and still has not
  // written anything.
  fireEvent.click(screen.getByRole("button", { name: "Create duplicates anyway" }));
  await waitFor(() =>
    expect(screen.getByRole("button", { name: /Yes — create 1/ })).toBeEnabled(),
  );
  expect(submits).toBe(0);

  // And now the final click writes, without stopping to ask again.
  fireEvent.click(screen.getByRole("button", { name: /Yes — create 1/ }));
  await waitFor(() => expect(submits).toBe(1));
});

/// The check that used to guard the last click still does. Moving it
/// earlier opened a window - the queue can be edited while someone reads a
/// hundred cases - so the write re-verifies, and stops only for a
/// duplicate nobody has been shown yet.
test("a duplicate that appears after review still stops the write", async () => {
  let submits = 0;
  let existing = [{ id: 201, title: "Something else", tags: "", automation_status: "Planned" }];
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return existing;
    if (cmd === "submit_queue") {
      submits += 1;
      return [{ index: 0, title: "Login works", action: "created", id: 900, error: null }];
    }
    return undefined;
  });
  renderQueue([makeCase()]);

  // Review opens clean: nothing on the PBI clashes yet.
  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  const go = await screen.findByRole("button", { name: /Yes — create 1/ });
  await waitFor(() => expect(go).toBeEnabled());

  // Someone else creates that case in the meantime.
  existing = [{ id: 202, title: "Login works", tags: "", automation_status: "Planned" }];

  fireEvent.click(go);
  expect(await screen.findByText(/Stopped: 1 case/)).toBeInTheDocument();
  expect(submits).toBe(0);
});

/// Unchanged rows are not sent, but they still hold their place in the
/// file: the order set after the upload needs them, or one new case in a
/// re-uploaded file lands at the top of the suite. Each goes as a hint
/// with its place on screen before the filter.
test("unchanged rows go to the upload as order hints, with their place in the queue", async () => {
  let call: { queue: TestCase[]; orderHint: unknown } | null = null;
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return ["smoke"];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "test_cases_by_ids") {
      // What Azure DevOps holds for both updates: identical to the queue.
      const base = {
        title: "Login works",
        tags: "smoke",
        automation_status: "Not Automated",
        steps: [{ action: "Open page", expected: "Page shown" }],
        step_ids: ["2"],
        module_value: "",
        preconditions: "",
      };
      return [
        { id: 201, ...base },
        { id: 202, ...base },
      ];
    }
    if (cmd === "submit_queue") {
      call = args as { queue: TestCase[]; orderHint: unknown };
      return [{ index: 0, title: "Brand new", action: "created", id: 900, error: null }];
    }
    return undefined;
  });
  renderQueue([
    makeCase({ update_id: 201, spec_order: 1, tester_order: 2, area: "Login" }),
    makeCase({ title: "Brand new", spec_order: 2, tester_order: 1 }),
    makeCase({ update_id: 202, spec_order: 3, tester_order: 3 }),
  ]);

  fireEvent.click(screen.getByRole("button", { name: /Review 3 test cases/ }));
  const go = await screen.findByRole("button", { name: /Yes — create 1/ });
  await waitFor(() => expect(go).toBeEnabled());
  fireEvent.click(go);

  await waitFor(() => expect(call).not.toBeNull());
  const sent = call as unknown as { queue: TestCase[]; orderHint: unknown };
  expect(sent.queue.map((c) => c.title)).toEqual(["Brand new"]);
  expect(sent.orderHint).toEqual([
    { index: 0, id: 201, spec_order: 1, tester_order: 2, area: "Login" },
    { index: 2, id: 202, spec_order: 3, tester_order: 3, area: "" },
  ]);
});

/// A created case can change the suite order and/or the suggested run
/// order (design doc §4.1, §4.2). Run Tests and Suite Management must not
/// go on serving what they had cached before the upload.
test("an upload that creates a case invalidates the run order and suite cases caches", async () => {
  cacheWrite(cacheKeys.runOrder("acme", "Web", 42), { state: "found", file: { cases: [] } });
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "submit_queue") {
      return [{ index: 0, title: "Login works", action: "created", id: 900, error: null }];
    }
    return undefined;
  });
  const { qc } = renderQueueWithClient([makeCase()]);
  qc.setQueryData(["run-order", "acme", "Web", 42], { state: "found", file: { cases: [] } });
  qc.setQueryData(["suite-cases", "acme", "Web", 1, 2], []);

  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  const go = await screen.findByRole("button", { name: /Yes — create 1/ });
  await waitFor(() => expect(go).toBeEnabled());
  fireEvent.click(go);

  await waitFor(() =>
    expect(qc.getQueryState(["run-order", "acme", "Web", 42])?.isInvalidated).toBe(true),
  );
  expect(qc.getQueryState(["suite-cases", "acme", "Web", 1, 2])?.isInvalidated).toBe(true);
  expect(cacheReadRunOrder()).toBeNull();

  function cacheReadRunOrder() {
    return JSON.parse(localStorage.getItem(`tcm-v2-cache:${cacheKeys.runOrder("acme", "Web", 42)}`) ?? "null");
  }
});

/// The upload succeeded but an order did not save: the backend says which
/// in one sentence, and the screen shows it as it came.
test("an order that could not be saved at upload is a warning toast", async () => {
  const reason = "The spec order could not be set in Azure DevOps: TF400000: You cannot reorder this suite.";
  const warn = vi.spyOn(toast, "warning");
  try {
    // `shouldMockEvents` connects the mock's `emit` to the screen's
    // `listen`, so the event travels the real path; the listen/unlisten
    // commands are left to the mock for the same reason.
    mockIPC(
      async (cmd) => {
        if (cmd === "list_test_case_fields") return [];
        if (cmd === "list_project_tags") return [];
        if (cmd === "test_case_field_values") return [];
        if (cmd === "pbi_test_cases") return [];
        if (cmd === "submit_queue") {
          await emit("run-order-not-saved", { reason });
          return [{ index: 0, title: "Login works", action: "created", id: 900, error: null }];
        }
        return undefined;
      },
      { shouldMockEvents: true },
    );
    renderQueue([makeCase()]);

    fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
    const go = await screen.findByRole("button", { name: /Yes — create 1/ });
    await waitFor(() => expect(go).toBeEnabled());
    fireEvent.click(go);

    await waitFor(() => expect(warn).toHaveBeenCalledWith(reason));
  } finally {
    warn.mockRestore();
  }
});

/// Unfolding steps (or diffs, or the editor) earns a sticky Collapse all
/// in the bottom-LEFT - the same corner as every other screen's; clicking
/// it folds everything shut. "Collapse", not "Close" - nothing is removed.
test("a sticky Collapse all folds every unfolded row", async () => {
  renderQueue([makeCase(), makeCase({ title: "Second case" })]);

  expect(screen.queryByRole("button", { name: /Collapse all/ })).not.toBeInTheDocument();

  fireEvent.click(screen.getByLabelText("Expand steps of Login works"));
  fireEvent.click(screen.getByLabelText("Expand steps of Second case"));
  const collapse = screen.getByRole("button", { name: /Collapse all \(2\)/ });

  // Bottom LEFT, like View/Update Test Cases and Run Tests: positioned by
  // a sidebar-clearing left offset, never parked at right-6.
  const wrapper = collapse.parentElement!;
  expect(wrapper.className).not.toContain("right-6");
  expect(wrapper.style.left).not.toBe("");

  fireEvent.click(collapse);
  expect(screen.queryByRole("button", { name: /Collapse all/ })).not.toBeInTheDocument();
  expect(screen.getByLabelText("Expand steps of Login works")).toBeInTheDocument();
});

// ---- Recent JSON Imports in the empty state ----------------------------

function renderEmptyWithRecents(opts: {
  recents: { path: string; when: number }[];
  onOpen?: (p: string) => void;
  onForget?: (p: string) => void;
}) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <QueueSection
        org="acme"
        project="Web"
        pbiId={42}
        queue={[]}
        setQueue={() => {}}
        recentImports={opts.recents}
        onOpenRecent={opts.onOpen ?? (() => {})}
        onForgetRecent={opts.onForget}
      />
    </QueryClientProvider>,
  );
}

test("an empty queue offers the recent imports instead of the dead-end empty state", async () => {
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "file_stamp") return "stamp-1";
    return null;
  });
  const opened: string[] = [];
  renderEmptyWithRecents({
    recents: [{ path: "C:/work/login-cases.json", when: 1754800000000 }],
    onOpen: (p) => opened.push(p),
  });

  expect(screen.getByText("Recent JSON Imports")).toBeInTheDocument();
  expect(screen.queryByText("Nothing queued yet")).not.toBeInTheDocument();
  // The queue itself is gone until something is imported: no header, no
  // disabled action row, no Review button.
  expect(screen.queryByText(/Queue for PBI/)).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Review 0 test cases/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Export JSON" })).not.toBeInTheDocument();

  // The Open button holds disabled until the existence probe answers -
  // wait for it to arm before clicking.
  const openBtn = await screen.findByRole("button", { name: "Reopen login-cases.json" });
  await waitFor(() => expect(openBtn).toBeEnabled());
  fireEvent.click(openBtn);
  expect(opened).toEqual(["C:/work/login-cases.json"]);
});

test("a recent whose file is gone says so and cannot be opened", async () => {
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "file_stamp") return null; // fileStamp: null = missing
    return null;
  });
  const forgotten: string[] = [];
  renderEmptyWithRecents({
    recents: [{ path: "C:/work/deleted.json", when: 1754800000000 }],
    onForget: (p) => forgotten.push(p),
  });

  expect(await screen.findByText("File no longer exists")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Reopen deleted.json" })).not.toBeInTheDocument();

  // The X still works - a dead entry can be cleaned up by hand.
  fireEvent.click(screen.getByRole("button", { name: "Remove deleted.json from recent imports" }));
  expect(forgotten).toEqual(["C:/work/deleted.json"]);
});

test("with no recents recorded yet, the area explains itself instead of showing a dead queue", () => {
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "list_test_case_fields") return [];
    return null;
  });
  renderEmptyWithRecents({ recents: [] });
  expect(screen.getByText("Recent JSON Imports")).toBeInTheDocument();
  expect(screen.getByText(/Import a JSON file above/)).toBeInTheDocument();
  expect(screen.queryByText(/Queue for PBI/)).not.toBeInTheDocument();
});

test("Manual Entry (no recents wiring) renders nothing at all when the queue is empty", () => {
  baseMocks();
  const { container } = renderQueue([]);
  // No header, no disabled action row, no explanatory island - the queue
  // section simply does not exist until a case is added.
  expect(screen.queryByText(/Queue for PBI/)).not.toBeInTheDocument();
  expect(screen.queryByText("Nothing queued yet")).not.toBeInTheDocument();
  expect(screen.queryByText("Recent JSON Imports")).not.toBeInTheDocument();
  expect(container).toBeEmptyDOMElement();
});


/// An update PATCHes its own work item where it already lives - the
/// check-the-PBI arming stage exists to stop CREATES landing under the
/// wrong PBI, so a queue of nothing but updates goes straight from the
/// first Confirm to the submit.
test("a pure-update queue submits on the first Confirm, no PBI stage", async () => {
  let submits = 0;
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "submit_queue") {
      submits += 1;
      return [{ index: 0, title: "Login works", action: "updated", id: 777, error: null }];
    }
    return undefined;
  });
  renderQueue([makeCase({ update_id: 777 })]);

  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  fireEvent.click(await screen.findByRole("button", { name: /Confirm & update 1/ }));

  // No "Check the highlighted PBI" stage, no second Yes - it submits.
  expect(screen.queryByText(/Check the highlighted PBI/)).not.toBeInTheDocument();
  await waitFor(() => expect(submits).toBe(1));
});

/// One create in the queue is enough to bring the PBI stage back - the
/// skip is strictly for ALL-update queues.
test("a mixed queue still gets the check-the-PBI stage", async () => {
  let submits = 0;
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "submit_queue") {
      submits += 1;
      return [];
    }
    return undefined;
  });
  renderQueue([makeCase({ update_id: 777 }), makeCase({ title: "Brand new" })]);

  // One click. The stage now arrives WITH the review rather than after a
  // button that claimed it would create.
  fireEvent.click(screen.getByRole("button", { name: /Review 2 test cases/ }));

  // Armed, not submitted: the warning is up and nothing was written.
  expect(await screen.findByText(/Check the highlighted PBI/)).toBeInTheDocument();
  expect(submits).toBe(0);
  expect(await screen.findByRole("button", { name: /Yes —/ })).toBeInTheDocument();
});

/// The chip glows to say "about to create on this PBI" - once the create
/// has actually happened there is nothing left to warn about. A mixed
/// upload (some rows new, some updates) used to leave the chip glowing
/// forever after a successful submit, because onSuccess cleared reviewing
/// but never disarmed.
test("the PBI stops glowing once a mixed upload has succeeded", async () => {
  const glows: boolean[] = [];
  const onGlow = (e: Event) => glows.push((e as CustomEvent<boolean>).detail);
  window.addEventListener("tcm-pbi-glow", onGlow);
  try {
    mockIPC((cmd) => {
      if (cmd === "plugin:event|listen") return 1;
      if (cmd === "plugin:event|unlisten") return null;
      if (cmd === "list_test_case_fields") return [];
      if (cmd === "list_project_tags") return [];
      if (cmd === "test_case_field_values") return [];
      if (cmd === "pbi_test_cases") return [];
      if (cmd === "submit_queue") {
        return [
          { index: 0, title: "Login works", action: "updated", id: 777, error: null },
          { index: 1, title: "Brand new", action: "created", id: 900, error: null },
        ];
      }
      return undefined;
    });
    renderQueue([makeCase({ update_id: 777 }), makeCase({ title: "Brand new" })]);

    fireEvent.click(screen.getByRole("button", { name: /Review 2 test cases/ }));
    // Armed while reviewing a queue that creates anything.
    await waitFor(() => expect(glows[glows.length - 1]).toBe(true));

    const go = await screen.findByRole("button", { name: /Yes —/ });
    await waitFor(() => expect(go).toBeEnabled());
    fireEvent.click(go);

    // The upload went through - the chip stops glowing, it does not stay
    // lit until the component happens to unmount.
    await screen.findByRole("button", { name: "Clear results" });
    await waitFor(() => expect(glows[glows.length - 1]).toBe(false));
  } finally {
    window.removeEventListener("tcm-pbi-glow", onGlow);
  }
});

/// Backing out at the duplicate gate ("Stop - take me back") is also a way
/// of abandoning the armed confirmation, not just Back - it must disarm
/// the same way.
test("stopping at the duplicate gate also stops the glow", async () => {
  const glows: boolean[] = [];
  const onGlow = (e: Event) => glows.push((e as CustomEvent<boolean>).detail);
  window.addEventListener("tcm-pbi-glow", onGlow);
  try {
    mockIPC((cmd) => {
      if (cmd === "plugin:event|listen") return 1;
      if (cmd === "plugin:event|unlisten") return null;
      if (cmd === "list_test_case_fields") return [];
      if (cmd === "list_project_tags") return [];
      if (cmd === "test_case_field_values") return [];
      if (cmd === "pbi_test_cases") {
        return [{ id: 201, title: "Login works", tags: "", automation_status: "Planned" }];
      }
      return undefined;
    });
    renderQueue([makeCase()]); // a CREATE row whose title already exists

    fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
    await waitFor(() => expect(glows[glows.length - 1]).toBe(true));
    expect(await screen.findByText(/Stopped: 1 case/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Stop — take me back" }));
    await waitFor(() => expect(glows[glows.length - 1]).toBe(false));
  } finally {
    window.removeEventListener("tcm-pbi-glow", onGlow);
  }
});

/// "Remove all" stays enabled during review (it is only gated on an empty
/// queue or a submit in flight), so emptying the queue while armed is
/// reachable - and the effect that leaves review mode on an empty queue
/// must disarm the same way every other exit does.
test("emptying the queue during review also stops the glow", async () => {
  const glows: boolean[] = [];
  const onGlow = (e: Event) => glows.push((e as CustomEvent<boolean>).detail);
  window.addEventListener("tcm-pbi-glow", onGlow);
  try {
    baseMocks();
    renderQueue([makeCase({ update_id: 777 }), makeCase({ title: "Brand new" })]);

    fireEvent.click(screen.getByRole("button", { name: /Review 2 test cases/ }));
    // Armed while reviewing a queue that creates anything.
    await waitFor(() => expect(glows[glows.length - 1]).toBe(true));

    fireEvent.click(screen.getByRole("button", { name: "Remove all" }));
    await waitFor(() => expect(glows[glows.length - 1]).toBe(false));
  } finally {
    window.removeEventListener("tcm-pbi-glow", onGlow);
  }
});

test("a floating copy of the main button appears once the real one scrolls away", async () => {
  onScreen = false;
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "test_case_field_values") return [];
  });
  renderQueue([makeCase(), makeCase({ title: "Logout works" })]);

  const floating = await waitFor(() => {
    const el = document.querySelector("[data-sticky-action]");
    expect(el).not.toBeNull();
    return el as HTMLElement;
  });
  // Same words as the real control, and out of the reading order.
  expect(floating).toHaveTextContent("Review 2 test cases");
  expect(floating).toHaveAttribute("aria-hidden");

  // It does exactly what the real button does: the same review, with the
  // PBI stage armed and the duplicate check already run.
  fireEvent.click(floating.querySelector("button")!);
  expect(await screen.findByText(/Check the highlighted PBI/)).toBeInTheDocument();
  expect(await screen.findByRole("button", { name: /Yes — create 2/ })).toBeInTheDocument();
});

test("the floating copy stands down when the real button is in view", async () => {
  onScreen = true;
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "test_case_field_values") return [];
  });
  renderQueue([makeCase()]);
  await screen.findByRole("button", { name: /Review 1 test case/ });

  // Still mounted (it animates out), but hidden and unclickable.
  const floating = document.querySelector("[data-sticky-action]") as HTMLElement;
  expect(floating.className).toContain("opacity-0");
  expect(floating.className).toContain("pointer-events-none");
});

/// The "Click to view ... changing" affordance used to appear only once
/// Review opened - the current server values were fetched behind the
/// review gate. It now shows as soon as an update sits in the queue, so
/// the person sees what an edit will do while they are still editing; a
/// row that would change nothing says so just as early.
test("queued updates show their diff before Review is opened", async () => {
  const fetched: number[][] = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return ["smoke"];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "test_cases_by_ids") {
      fetched.push((args as { ids: number[] }).ids);
      const base = {
        automation_status: "Not Automated",
        steps: [{ action: "Open page", expected: "Page shown" }],
        step_ids: ["2"],
        module_value: "",
        preconditions: "",
      };
      return [
        { id: 201, title: "Login works", tags: "", ...base }, // queued copy adds a tag
        { id: 202, title: "Login works", tags: "smoke", ...base }, // identical
      ];
    }
    return undefined;
  });
  renderQueue([makeCase({ update_id: 201 }), makeCase({ update_id: 202 })]);

  // No Review click anywhere in this test.
  expect(await screen.findByRole("button", { name: /Click to view 1 field changing/ })).toBeInTheDocument();
  expect(screen.getByText(/nothing will change/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Review/ })).toBeInTheDocument();
  expect(fetched).toEqual([[201, 202]]);
});

/// Comments sync both ways on the work item id. Notes -> file: a case
/// imported for update with no comment takes the note kept in View Test
/// Cases - onto its card, and into the file it came from, with the watch
/// told the new stamp so the write is not reported back as an edit.
test("a queued update with no comment takes the note kept for its id, and the file learns it", async () => {
  localStorage.setItem("tcm-v2-case-notes:acme", JSON.stringify({ "201": "Re-check with QA" }));
  const saved: Array<Record<string, unknown>> = [];
  const patched: Array<{ path: string; stamp?: string; snapshot?: TestCase[] }> = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return ["smoke"];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "test_cases_by_ids") return [];
    if (cmd === "save_draft_comment") {
      saved.push(args as Record<string, unknown>);
      return "stamp-2";
    }
    return undefined;
  });
  const a = makeCase({ update_id: 201, title: "Login works" });
  renderQueue([a], {
    watches: [{ path: "C:/drafts/a.json", stamp: "stamp-1", snapshot: [a] }],
    onWatchPatched: (path, fields) =>
      patched.push({ path, stamp: fields.stamp, snapshot: fields.snapshot }),
  });

  expect(await screen.findByText("Re-check with QA")).toBeInTheDocument();
  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0]).toMatchObject({
    path: "C:/drafts/a.json",
    id: 201,
    title: "Login works",
    text: "Re-check with QA",
  });
  await waitFor(() => expect(patched).toHaveLength(1));
  expect(patched[0].stamp).toBe("stamp-2");
  expect(patched[0].snapshot?.[0].comment).toBe("Re-check with QA");
});

/// The file keeps its own comment where it has one: the note fills only
/// an empty slot and never overwrites.
test("a queued update that already has a comment keeps it over the stored note", async () => {
  localStorage.setItem("tcm-v2-case-notes:acme", JSON.stringify({ "201": "Re-check with QA" }));
  baseMocks();
  renderQueue([makeCase({ update_id: 201, comment: "From the file" })]);
  expect(await screen.findByText("From the file")).toBeInTheDocument();
  expect(screen.queryByText("Re-check with QA")).not.toBeInTheDocument();
});

/// Upload order is suite order, and the order on screen is the one the
/// user chose: the Order bar sorts the queue itself, so the send list is
/// simply the queue. It used to re-sort by tester_order at submit, which
/// silently overrode "Down the spec" - a file laid out one way landed in
/// the suite another, with nothing on screen to explain it.
test("the queue is uploaded in the order on screen, and the Order bar decides that order", async () => {
  let sentTitles: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "submit_queue") {
      const q = (args as { queue: TestCase[] }).queue;
      sentTitles = q.map((c) => c.title);
      return q.map((c, i) => ({ index: i, title: c.title, action: "created", id: 900 + i, error: null }));
    }
    return undefined;
  });
  // Every case carries a tester_order that disagrees with the file's own
  // order, and nobody has touched the Order bar: the file's order stands.
  const { unmount } = renderQueue([
    makeCase({ title: "Third on the sheet", spec_order: 1, tester_order: 3 }),
    makeCase({ title: "First on the sheet", spec_order: 2, tester_order: 1 }),
    makeCase({ title: "Second on the sheet", spec_order: 3, tester_order: 2 }),
  ]);

  fireEvent.click(screen.getByRole("button", { name: /Review 3 test cases/ }));
  fireEvent.click(await screen.findByRole("button", { name: /Yes — create 3/ }));
  await waitFor(() => expect(sentTitles).toHaveLength(3));
  expect(sentTitles).toEqual(["Third on the sheet", "First on the sheet", "Second on the sheet"]);
  unmount();

  // And with "For testing" chosen, THAT is what goes out.
  sentTitles = [];
  renderQueue([
    makeCase({ title: "Third on the sheet", spec_order: 1, tester_order: 3 }),
    makeCase({ title: "First on the sheet", spec_order: 2, tester_order: 1 }),
    makeCase({ title: "Second on the sheet", spec_order: 3, tester_order: 2 }),
  ]);
  fireEvent.click(screen.getByRole("button", { name: "For testing" }));
  fireEvent.click(screen.getByRole("button", { name: /Review 3 test cases/ }));
  fireEvent.click(await screen.findByRole("button", { name: /Yes — create 3/ }));
  await waitFor(() => expect(sentTitles).toHaveLength(3));
  expect(sentTitles).toEqual(["First on the sheet", "Second on the sheet", "Third on the sheet"]);
});

/// A watch's `specs` travel to the browser review page through the same
/// `files` array as its label and comment - the page (Task 4) reads them
/// from there. Missing them here would mean specs never reach the page.
test("View in browser passes each watch's specs through to the draft page", async () => {
  let files: Array<{ path: string; label: string; comment: string; specs: string[] }> = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "view_draft_html") {
      files = (args as { files: typeof files }).files;
      return null;
    }
    return undefined;
  });
  const watch: WatchedFile = {
    path: "C:/w/cases.json",
    stamp: "stamp-1",
    snapshot: [],
    specs: ["Step13.md"],
  };
  renderQueue([makeCase()], { watches: [watch] });

  fireEvent.click(screen.getByRole("button", { name: "View in browser" }));
  await waitFor(() => expect(files).toHaveLength(1));
  expect(files[0].specs).toEqual(["Step13.md"]);
});

/// C2: a failed batch whose creates Azure DevOps could not be asked about
/// holds those rows until someone checks.
///
/// Deviation from the brief (controller ruling): `reconcile_upload` now
/// answers `{ found, ambiguous }` rather than a bare array - a title with
/// more unclaimed matches in Azure DevOps than rows being checked cannot be
/// told apart from a colleague's case, so it comes back separately and the
/// row stays held rather than being cleared on a guess. `onReconcile` may
/// return either the old bare array (wrapped here as `{ found, ambiguous:
/// [] }`) or a full answer object, so the existing scenarios below need no
/// other change.
function holdMocks(onReconcile: (args: Record<string, unknown>) => unknown) {
  let submits = 0;
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "submit_queue") {
      submits += 1;
      return [
        {
          index: 0,
          title: "Brand new",
          action: "unknown",
          id: null,
          error: "Outcome unknown (http 500). Azure DevOps may have created this case - check before uploading it again.",
        },
      ];
    }
    if (cmd === "reconcile_upload") {
      const r = onReconcile(args as Record<string, unknown>);
      return Array.isArray(r) ? { found: r, ambiguous: [] } : r;
    }
    return undefined;
  });
  return { submits: () => submits };
}

async function uploadOnce() {
  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  fireEvent.click(await screen.findByRole("button", { name: /Yes —/ }));
  await screen.findByText("Outcome unknown - check before uploading again");
}

test("an unknown outcome marks the row and refuses the next upload", async () => {
  const ipc = holdMocks(() => []);
  renderQueue([makeCase({ title: "Brand new" })]);
  await uploadOnce();
  expect(screen.getByText("UNKNOWN")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Check with Azure DevOps/ })).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  expect(await screen.findByText(/Check the cases marked "Outcome unknown" first/)).toBeInTheDocument();
  const yes = screen.getByRole("button", { name: /Yes —/ });
  expect(yes).toBeDisabled();
  fireEvent.click(yes);
  expect(ipc.submits()).toBe(1);
});

test("Check that finds the case stamps its id and lifts the hold", async () => {
  let asked: Record<string, unknown> | null = null;
  holdMocks((args) => {
    asked = args;
    return [{ title: "Brand new", id: 901 }];
  });
  renderQueue([makeCase({ title: "Brand new" })]);
  await uploadOnce();

  fireEvent.click(screen.getByRole("button", { name: /Check with Azure DevOps/ }));
  await waitFor(() =>
    expect(screen.queryByText("Outcome unknown - check before uploading again")).not.toBeInTheDocument(),
  );
  expect(screen.getByText("UPDATE #901")).toBeInTheDocument();
  expect(asked).toMatchObject({ organization: "acme", project: "Web", pbiId: 42, titles: ["Brand new"] });
  expect(typeof asked!.since).toBe("string");
  expect(localStorage.getItem("tcm-v2-upload-hold:acme/42")).toBeNull();
});

/// Final review: a Check could stamp a held row with a case this upload had
/// already reported for another row (a title repeated across chunks), or
/// one that was on the PBI before the upload began. The hold records both
/// at upload time, and the Check sends them - with every update id in the
/// queue - for Rust to exclude before its exactly-one rule.
test("Check sends the PBI's earlier cases, the upload's own ids and the queue's update ids to exclude", async () => {
  let asked: Record<string, unknown> | null = null;
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "test_cases_by_ids") return [];
    if (cmd === "pbi_test_cases") {
      return [{ id: 50, title: "Already there", tags: "", automation_status: "Not Automated" }];
    }
    if (cmd === "submit_queue") {
      return [
        { index: 0, title: "Login", action: "created", id: 900, error: null },
        {
          index: 1,
          title: "Brand new",
          action: "unknown",
          id: null,
          error: "Outcome unknown (http 500). Azure DevOps may have created this case - check before uploading it again.",
        },
        { index: 2, title: "Existing", action: "updated", id: 77, error: null },
      ];
    }
    if (cmd === "reconcile_upload") {
      asked = args as Record<string, unknown>;
      return { found: [], ambiguous: [] };
    }
    return undefined;
  });
  renderQueue([
    makeCase({ title: "Login" }),
    makeCase({ title: "Brand new" }),
    makeCase({ title: "Existing", update_id: 77 }),
  ]);
  fireEvent.click(screen.getByRole("button", { name: /Review 3 test cases/ }));
  fireEvent.click(await screen.findByRole("button", { name: /Yes —/ }));
  await screen.findByText("Outcome unknown - check before uploading again");
  expect(JSON.parse(localStorage.getItem("tcm-v2-upload-hold:acme/42")!).ids).toEqual([50, 900, 77]);

  fireEvent.click(screen.getByRole("button", { name: /Check with Azure DevOps/ }));
  await waitFor(() => expect(asked).not.toBeNull());
  expect(asked!.titles).toEqual(["Brand new"]);
  expect([...(asked!.excludeIds as number[])].sort((a, b) => a - b)).toEqual([50, 77, 900]);
});

test("Check that finds nothing returns the row to normal", async () => {
  holdMocks(() => []);
  renderQueue([makeCase({ title: "Brand new" })]);
  await uploadOnce();

  fireEvent.click(screen.getByRole("button", { name: /Check with Azure DevOps/ }));
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: /Check with Azure DevOps/ })).not.toBeInTheDocument(),
  );
  expect(screen.queryByText("Outcome unknown - check before uploading again")).not.toBeInTheDocument();
  expect(screen.getByText("NEW")).toBeInTheDocument();
});

test("a Check that cannot reach Azure DevOps keeps the hold", async () => {
  holdMocks(() => {
    throw "Could not reach Azure DevOps.";
  });
  renderQueue([makeCase({ title: "Brand new" })]);
  await uploadOnce();

  fireEvent.click(screen.getByRole("button", { name: /Check with Azure DevOps/ }));
  await waitFor(() => expect(screen.getByRole("button", { name: /Check with Azure DevOps/ })).toBeEnabled());
  expect(screen.getByText("Outcome unknown - check before uploading again")).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-upload-hold:acme/42")).not.toBeNull();
});

test("a hold from an earlier session is still enforced", () => {
  localStorage.setItem(
    "tcm-v2-upload-hold:acme/42",
    JSON.stringify({ since: "2026-09-18T10:00:00.000Z", titles: ["Brand new"] }),
  );
  baseMocks();
  renderQueue([makeCase({ title: "Brand new" })]);
  expect(screen.getByText("Outcome unknown - check before uploading again")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Check with Azure DevOps/ })).toBeInTheDocument();
});

/// Deviation from the brief (controller ruling): a title Azure DevOps
/// cannot disambiguate stays held, with its own reason on the row, and the
/// hold shrinks to just that title rather than lifting.
test("Check that finds an ambiguous title keeps that row held, with its own reason", async () => {
  holdMocks(() => ({ found: [], ambiguous: ["Brand new"] }));
  renderQueue([makeCase({ title: "Brand new" })]);
  await uploadOnce();

  fireEvent.click(screen.getByRole("button", { name: /Check with Azure DevOps/ }));
  await screen.findByText(
    "More than one test case with this title exists in Azure DevOps - check there before uploading again.",
  );
  expect(screen.queryByText("Outcome unknown - check before uploading again")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Check with Azure DevOps/ })).toBeInTheDocument();
  const stored = JSON.parse(localStorage.getItem("tcm-v2-upload-hold:acme/42")!);
  expect(stored.titles).toEqual(["Brand new"]);
  expect(stored.ambiguous).toEqual(["Brand new"]);
});

// ---- fix round 1 --------------------------------------------------------
// Review found: `ReconcileAnswer.ambiguous` names a TITLE once (it comes
// from a Rust HashSet), not once per held ROW. Rebuilding the hold straight
// from that list dropped the second row of a repeated title, and undercounted
// `missing` into inviting a re-upload of a case that might still be a
// duplicate. Fixed by keeping every entry of the OLD hold whose title is in
// the answer's ambiguous set, so multiplicity survives.
function holdMocksMulti(onReconcile: (args: Record<string, unknown>) => unknown, titles: string[]) {
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "submit_queue") {
      return titles.map((title, index) => ({
        index,
        title,
        action: "unknown",
        id: null,
        error: "Outcome unknown (http 500). Azure DevOps may have created this case - check before uploading it again.",
      }));
    }
    if (cmd === "reconcile_upload") {
      const r = onReconcile(args as Record<string, unknown>);
      return Array.isArray(r) ? { found: r, ambiguous: [] } : r;
    }
    return undefined;
  });
}

test("an ambiguous answer for a repeated title keeps every held row, and reports none re-uploadable", async () => {
  holdMocksMulti(() => ({ found: [], ambiguous: ["Brand new"] }), ["Brand new", "Brand new"]);
  renderQueue([makeCase({ title: "Brand new" }), makeCase({ title: "Brand new" })]);
  fireEvent.click(screen.getByRole("button", { name: /Review 2 test cases/ }));
  fireEvent.click(await screen.findByRole("button", { name: /Yes —/ }));
  await waitFor(() =>
    expect(screen.getAllByText("Outcome unknown - check before uploading again")).toHaveLength(2),
  );

  fireEvent.click(screen.getByRole("button", { name: /Check with Azure DevOps/ }));
  await waitFor(() =>
    expect(
      screen.getAllByText(
        "More than one test case with this title exists in Azure DevOps - check there before uploading again.",
      ),
    ).toHaveLength(2),
  );
  expect(screen.queryByText("Outcome unknown - check before uploading again")).not.toBeInTheDocument();

  const stored = JSON.parse(localStorage.getItem("tcm-v2-upload-hold:acme/42")!);
  expect(stored.titles).toEqual(["Brand new", "Brand new"]);
  expect(stored.ambiguous).toEqual(["Brand new", "Brand new"]);

  // Nothing was resolved for either row, so nothing is offered back for
  // re-upload - the old bug undercounted this to 1.
  expect(toast.info).toHaveBeenCalledWith("0 of 2 case(s) had been created.");
});

// ---- fix round 1: controller ruling - an ambiguous hold must not freeze
// ---- the PBI forever. -----------------------------------------------------

/** Seeds an ambiguous hold directly, bypassing the Check round trip, for
 * tests only concerned with what the banner and its buttons do. */
function seedAmbiguousHold(titles: string[]) {
  localStorage.setItem(
    "tcm-v2-upload-hold:acme/42",
    JSON.stringify({ since: "2026-09-18T10:00:00.000Z", titles, ambiguous: titles }),
  );
}

// Fix round 2 (CRITICAL): a hold is never cleared by an effect watching
// (hold, queue). `useQueue` (src/hooks/useQueue.ts:60-93) delivers a scope
// change one render before the reload - the new PBI's hold is already
// visible but its queue is still the OLD PBI's (or the tour fixture's) -
// and any effect that deleted a hold with no matching row in THAT queue
// would delete the incoming PBI's hold before its own rows ever arrived.
// So removing every held row leaves the hold exactly as it was in storage:
// only Check and a confirmed Release ever clear one. `holdActive` (held
// rows actually present) is what the refusal reads, so the hold is simply
// inert until a same-titled row reappears.
test("removing every held row leaves the hold stored but inert - uploads are allowed again", async () => {
  localStorage.setItem(
    "tcm-v2-upload-hold:acme/42",
    JSON.stringify({ since: "2026-09-18T10:00:00.000Z", titles: ["Brand new"] }),
  );
  baseMocks();
  renderQueue([makeCase({ title: "Brand new" }), makeCase({ title: "Other" })]);
  expect(screen.getByText("Outcome unknown - check before uploading again")).toBeInTheDocument();

  const held = screen.getByText("Brand new").closest("li")!;
  fireEvent.click(within(held).getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(screen.queryByText("Brand new")).not.toBeInTheDocument());

  // Never auto-cleared - still exactly what Check or Release last left.
  expect(localStorage.getItem("tcm-v2-upload-hold:acme/42")).not.toBeNull();

  // But inert: no row of the queue matches it, so it does not block.
  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  const yes = await screen.findByRole("button", { name: /Yes —/ });
  expect(yes).not.toBeDisabled();
});

test("a PBI switch does not clear the incoming PBI's hold, even for the one render still showing the outgoing queue", () => {
  localStorage.setItem(
    "tcm-v2-upload-hold:acme/99",
    JSON.stringify({ since: "2026-09-18T10:00:00.000Z", titles: ["Brand new"] }),
  );
  baseMocks();
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  function Scoped({ pbiId, queue }: { pbiId: number; queue: TestCase[] }) {
    return (
      <QueryClientProvider client={qc}>
        <QueueSection org="acme" project="Web" pbiId={pbiId} queue={queue} setQueue={() => {}} />
      </QueryClientProvider>
    );
  }

  const { rerender } = render(<Scoped pbiId={42} queue={[makeCase({ title: "Other" })]} />);
  expect(screen.queryByRole("button", { name: /Check with Azure DevOps/ })).not.toBeInTheDocument();

  // useQueue's transition, reproduced exactly: pbiId flips to the PBI whose
  // hold is being tested (99) while `queue` is still what PBI 42 had -
  // there is no row anywhere named by 99's hold yet.
  rerender(<Scoped pbiId={99} queue={[makeCase({ title: "Other" })]} />);
  expect(localStorage.getItem("tcm-v2-upload-hold:acme/99")).not.toBeNull();

  // The reload arrives: PBI 99's own queue, which does have the held row.
  rerender(<Scoped pbiId={99} queue={[makeCase({ title: "Brand new" })]} />);
  expect(localStorage.getItem("tcm-v2-upload-hold:acme/99")).not.toBeNull();
  expect(screen.getByText("Outcome unknown - check before uploading again")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Check with Azure DevOps/ })).toBeInTheDocument();
});

test("a hold still naming a row present in the queue keeps refusing, even if renamed rows elsewhere were dropped", () => {
  seedAmbiguousHold(["Brand new"]);
  baseMocks();
  renderQueue([makeCase({ title: "Brand new" })]);
  expect(screen.getByRole("button", { name: /Check with Azure DevOps/ })).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-upload-hold:acme/42")).not.toBeNull();
});

test("Release asks for confirmation, and confirming clears the hold", async () => {
  seedAmbiguousHold(["Brand new"]);
  baseMocks();
  renderQueue([makeCase({ title: "Brand new" })]);

  fireEvent.click(screen.getByRole("button", { name: "Release" }));
  const dialog = within(screen.getByRole("dialog"));
  expect(
    dialog.getByText("I have checked in Azure DevOps — release these cases so they can be uploaded again?"),
  ).toBeInTheDocument();
  fireEvent.click(dialog.getByRole("button", { name: "Release" }));

  await waitFor(() => expect(localStorage.getItem("tcm-v2-upload-hold:acme/42")).toBeNull());
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Release" })).not.toBeInTheDocument();
});

test("cancelling the release confirmation leaves the hold in place", () => {
  seedAmbiguousHold(["Brand new"]);
  baseMocks();
  renderQueue([makeCase({ title: "Brand new" })]);

  fireEvent.click(screen.getByRole("button", { name: "Release" }));
  const dialog = within(screen.getByRole("dialog"));
  fireEvent.click(dialog.getByRole("button", { name: "Cancel" }));

  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-upload-hold:acme/42")).not.toBeNull();
  expect(screen.getByRole("button", { name: "Release" })).toBeInTheDocument();
});

/// Review finding: the X on one row only filtered the queue, while bulk
/// remove wrote the file back. The next outside edit of that file then
/// saw the case in both snapshots, not in the queue, and put it back.
test("removing one row writes the owning file back, so the case cannot return", async () => {
  const a = makeCase({ title: "From file A" });
  const b = makeCase({ title: "Also from file A" });
  const saved: Array<{ path: string; titles: string[] }> = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "save_draft_cases") {
      const p = args as SentEdits;
      saved.push({ path: p.path, titles: keptTitles(p) });
      return "stamp-2";
    }
    return undefined;
  });
  const patched: Array<{ path: string; stamp?: string }> = [];
  renderQueue([a, b], {
    watches: [{ path: "C:/drafts/a.json", stamp: "stamp-1", snapshot: [a, b] }],
    onWatchPatched: (path, fields) => patched.push({ path, stamp: fields.stamp }),
  });

  fireEvent.click(screen.getAllByRole("button", { name: "Remove" })[0]);

  expect(screen.queryByText("From file A")).not.toBeInTheDocument();
  await waitFor(() => expect(saved).toEqual([{ path: "C:/drafts/a.json", titles: ["Also from file A"] }]));
  await waitFor(() => expect(patched).toEqual([{ path: "C:/drafts/a.json", stamp: "stamp-2" }]));
});

/// Review finding: the note filled an update case's empty comment on EVERY
/// queue change, so clearing the comment on the card undid itself at once.
test("a comment cleared on an update case stays cleared, and View's note follows it", async () => {
  localStorage.setItem("tcm-v2-case-notes:acme", JSON.stringify({ "201": "Re-check with QA" }));
  baseMocks();
  renderQueue([makeCase({ update_id: 201 })]);
  expect(await screen.findByText("Re-check with QA")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(await screen.findByLabelText("Comment (in-app only)"), { target: { value: "" } });
  fireEvent.click(screen.getByRole("button", { name: "Save to queue" }));

  await waitFor(() => expect(screen.queryByText("Re-check with QA")).not.toBeInTheDocument());
  await waitFor(
    () => expect(JSON.parse(localStorage.getItem("tcm-v2-case-notes:acme") ?? "{}")).toEqual({}),
    { timeout: 3000 },
  );
  // Still cleared once the note write has landed.
  expect(screen.queryByText("Re-check with QA")).not.toBeInTheDocument();
});

/// ...and an edit to that comment reaches the note, so View Test Cases
/// stops showing the old text.
test("editing an update case's comment reaches the View Test Cases note", async () => {
  baseMocks();
  renderQueue([makeCase({ update_id: 201, comment: "From the file" })]);
  await screen.findByText("From the file");

  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(await screen.findByLabelText("Comment (in-app only)"), {
    target: { value: "Re-check with QA" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save to queue" }));

  await waitFor(
    () =>
      expect(JSON.parse(localStorage.getItem("tcm-v2-case-notes:acme") ?? "{}")).toEqual({
        "201": "Re-check with QA",
      }),
    { timeout: 3000 },
  );
});

/// Review finding: a submit that finished after the user left and came back
/// called the ORIGINAL mount's onWatchPatched - a setter on an unmounted
/// screen, which never runs its persist step - and skipped storage because
/// a (new) mount was registered. The new stamp was recorded nowhere.
test("a submit that finishes after a remount moves the watch forward where it is shown now", async () => {
  const a = makeCase({ title: "Brand new" });
  const watch: WatchedFile = { path: "C:/drafts/a.json", stamp: "stamp-1", snapshot: [a] };
  localStorage.setItem("tcm-v2-watch:acme/42", JSON.stringify([watch]));
  let submitting = false;
  let finish: (v: unknown) => void = () => {};
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "submit_queue") {
      submitting = true;
      return new Promise((r) => {
        finish = r;
      });
    }
    if (cmd === "save_draft_cases") return "stamp-2";
    return undefined;
  });
  const first: string[] = [];
  const second: string[] = [];
  const mountA = renderQueue([a], {
    watches: [watch],
    onWatchPatched: (_p, f) => first.push(f.stamp ?? ""),
  });
  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  const go = await screen.findByRole("button", { name: /Yes — create 1/ });
  await waitFor(() => expect(go).toBeEnabled());
  fireEvent.click(go);
  await waitFor(() => expect(submitting).toBe(true));

  mountA.unmount();
  renderQueue([a], { watches: [watch], onWatchPatched: (_p, f) => second.push(f.stamp ?? "") });
  await act(async () => {
    finish([{ index: 0, title: "Brand new", action: "created", id: 900, error: null }]);
  });

  await waitFor(() => expect(second).toEqual(["stamp-2"]));
  expect(first).toEqual([]);
  const stored = JSON.parse(localStorage.getItem("tcm-v2-watch:acme/42") as string) as WatchedFile[];
  expect(stored[0].stamp).toBe("stamp-2");
});

/// Review finding: a comment typed in the browser went to EVERY row
/// with that title, and to whichever PBI was on screen.
test("a comment from the browser page lands on its own row, and only for this PBI", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    return undefined;
  }, { shouldMockEvents: true });
  renderQueue([makeCase({ tags: "first" }), makeCase({ tags: "second" })]);
  await screen.findAllByText("Login works");

  const saved = (text: string, key: string, pbi_id: number) =>
    emit("draft-comment-saved", { path: "", stamp: "", id: null, title: "Login works", key, pbi_id, text });

  await act(async () => {
    await saved("Only the second", "t:login works#2", 42);
  });
  expect(await screen.findByText("Only the second")).toBeInTheDocument();
  expect(screen.getAllByText("Only the second")).toHaveLength(1);
  // On the SECOND row: `li.rounded-md` is a queue row (ImportFile.test.tsx counts rows the same way).
  const rows = document.querySelectorAll("li.rounded-md");
  expect(rows[0].textContent).not.toContain("Only the second");
  expect(rows[1].textContent).toContain("Only the second");

  // A page left open for another PBI: nothing here moves.
  await act(async () => {
    await saved("From PBI 7's page", "t:login works", 7);
  });
  expect(screen.queryByText("From PBI 7's page")).not.toBeInTheDocument();
});

/// Fix round 1 (F5): "seen" was only ever reset on a scope change, never
/// pruned as rows came and went - a row that left and came back inside the
/// SAME mount (removed, or swapped out by a kept-uploaded/file-sync
/// update) was still "seen" with its old comment, so its empty comment on
/// return read as an EDIT to blank, not a first appearance to fill.
test("a row that leaves and returns is treated as new again, not edited to blank", async () => {
  localStorage.setItem("tcm-v2-case-notes:acme", JSON.stringify({ "201": "Re-check with QA" }));
  baseMocks();
  let externalSetQueue: Dispatch<SetStateAction<TestCase[]>> = () => {};
  renderQueue([makeCase({ update_id: 201, comment: "Re-check with QA" })], {
    exposeSetQueue: (setter) => {
      externalSetQueue = setter;
    },
  });
  // First appearance: the comment already matches the note, so it is just
  // remembered - nothing to fill, nothing to write.
  expect(await screen.findByText("Re-check with QA")).toBeInTheDocument();

  // The row leaves the queue (removed, or replaced by a fresh copy without
  // its comment - what a re-import or the created-case flow would do)...
  await act(async () => {
    externalSetQueue([]);
  });
  // ...and returns with no comment.
  await act(async () => {
    externalSetQueue([makeCase({ update_id: 201, comment: "" })]);
  });

  // Filled from the stored note again - not written over with blank.
  expect(await screen.findByText("Re-check with QA")).toBeInTheDocument();
  await waitFor(() =>
    expect(JSON.parse(localStorage.getItem("tcm-v2-case-notes:acme") ?? "{}")).toEqual({
      "201": "Re-check with QA",
    }),
  );
});

/// Controller ruling (F6, upgraded to Important): the run has to be
/// claimed BEFORE the pre-flight fetch, not after it. Otherwise a second
/// mount whose OWN pre-flight outlasts the first mount's whole upload
/// finds the slot free once the first finishes, and resends the same
/// id-less creates a second time.
test("a second submit is refused the instant it starts, before its own pre-flight ever runs", async () => {
  let releaseFirstPreflight: (v: unknown[]) => void = () => {};
  // Mount B's queue holds an update, so mounting it ALSO fires the
  // diff-preview query (EDT-B) for the same ids the submit's own
  // pre-flight would use - that automatic fetch is not what this test is
  // about, so it is counted and excluded rather than mistaken for one.
  let secondIdsCalls = 0;
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "test_cases_by_ids") {
      const ids = (args as { ids: number[] }).ids;
      if (ids.includes(777)) {
        return new Promise((r) => {
          releaseFirstPreflight = r;
        });
      }
      secondIdsCalls += 1;
      return [];
    }
    return undefined;
  });

  // Mount A: a pure-update queue, so Confirm submits on the first click -
  // and its own pre-flight never answers.
  renderQueue([makeCase({ update_id: 777 })]);
  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  fireEvent.click(await screen.findByRole("button", { name: /Confirm & update 1/ }));
  await waitFor(() => expect(submitPhaseSnapshot()).not.toBeNull());

  // Mount B: a separate screen, a DIFFERENT PBI - nothing about its own
  // button is blocked by A's progress, so this is the shared run guard
  // being tested, not the UI disabling a button. Let its own mount-time
  // diff-preview fetch settle before touching Confirm, so only a NEW call
  // after that point can be the submit's own pre-flight.
  renderQueue([makeCase({ update_id: 888 })], { pbiId: 43 });
  await waitFor(() => expect(secondIdsCalls).toBeGreaterThan(0));
  const callsBeforeSubmit = secondIdsCalls;

  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  fireEvent.click(await screen.findByRole("button", { name: /Confirm & update 1/ }));

  await waitFor(() =>
    expect(toast.error).toHaveBeenCalledWith(expect.stringContaining("another upload is still running")),
  );
  // Refused before it ever reached its OWN pre-flight - not after: no NEW
  // call to test_cases_by_ids came from the submit attempt.
  expect(secondIdsCalls).toBe(callsBeforeSubmit);

  releaseFirstPreflight([]);
});
