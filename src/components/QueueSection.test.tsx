import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import type { TestCase } from "../bindings";
import type { WatchedFile } from "../lib/fileSync";
import QueueSection from "./QueueSection";

/** The floating copy only exists while the real row is off screen, so the
 * tests drive the hook rather than jsdom's (non-existent) layout. The
 * hook itself - including the part that has to notice the action row
 * arriving in a queue that started empty - is exercised UNMOCKED in
 * QueueSection.floating.test.tsx. */
vi.mock("../hooks/useOnScreen", () => ({ useOnScreen: () => [() => {}, onScreen] }));
let onScreen = true;

afterEach(() => {
  clearMocks();
  localStorage.clear();
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
}: {
  initial: TestCase[];
  watches?: WatchedFile[];
  onWatchPatched?: (path: string, fields: Partial<WatchedFile>) => void;
}) {
  const [queue, setQueue] = useState<TestCase[]>(initial);
  return (
    <QueueSection
      org="acme"
      project="Web"
      pbiId={42}
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
  },
) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Harness initial={initial} watches={extra?.watches} onWatchPatched={extra?.onWatchPatched} />
    </QueryClientProvider>,
  );
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
  submitStarted("acme", 42, 10);
  submitProgressed(3, 10, "Login works");
  try {
    renderQueue([makeCase()]);
    expect(await screen.findByText(/Processing 3\/10/)).toBeInTheDocument();
  } finally {
    submitFinished();
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
test("an upload in flight turns the main button into Stop, and there is no second one", async () => {
  const { submitStarted, submitProgressed, submitFinished } = await import("../lib/submitRun");
  let cancelled = 0;
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "cancel_submit") {
      cancelled += 1;
      return null;
    }
    return undefined;
  });
  submitStarted("acme", 42, 10);
  submitProgressed(3, 10, "Login works");
  try {
    renderQueue([makeCase()]);
    expect(await screen.findByText(/Processing 3\/10/)).toBeInTheDocument();

    const stop = screen.getByRole("button", { name: "Stop" });
    expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument();
    // And it has taken the action row's place - Review is not on offer
    // while the queue it would review is being written.
    expect(screen.queryByRole("button", { name: /Review 1 test case/ })).not.toBeInTheDocument();

    // Held at first. The click that starts an upload lands on Confirm; the
    // second half of a habitual double-click lands here, and must not stop
    // the upload it just started.
    expect(stop).toBeDisabled();
    fireEvent.click(stop);
    expect(cancelled).toBe(0);

    await waitFor(() => expect(stop).toBeEnabled(), { timeout: 3000 });
    fireEvent.click(stop);
    expect(cancelled).toBe(1);
  } finally {
    submitFinished();
  }
});

/// And a submit for a DIFFERENT scope stays invisible - PBI 7's progress
/// must never render over PBI 42's queue.
test("another PBI's submit does not show here", async () => {
  const { submitStarted, submitFinished } = await import("../lib/submitRun");
  baseMocks();
  submitStarted("acme", 7, 5);
  try {
    renderQueue([makeCase()]);
    await screen.findByText("Login works");
    expect(screen.queryByText(/Processing/)).not.toBeInTheDocument();
  } finally {
    submitFinished();
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
test("bulk remove updates the queue AND the owning .json file", async () => {
  const a = makeCase({ title: "From file A" });
  const b = makeCase({ title: "Also from file A" });
  const hand = makeCase({ title: "Typed by hand" });
  const saved: Array<{ path: string; titles: string[] }> = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "save_draft_cases") {
      const p = args as { path: string; cases: TestCase[] };
      saved.push({ path: p.path, titles: p.cases.map((c) => c.title) });
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
      const p = args as { cases: TestCase[] };
      for (const c of p.cases) saved.push({ title: c.title, status: c.automation_status });
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
      const p = args as { cases: TestCase[] };
      saved.push({ titles: p.cases.map((c) => c.title) });
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
  fireEvent.click(screen.getByRole("button", { name: /Power Rename 1/ }));
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
      const p = args as { path: string; cases: TestCase[] };
      saved.push({ path: p.path, titles: p.cases.map((c) => c.title) });
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

  // It does exactly what the real button does.
  fireEvent.click(floating.querySelector("button")!);
  expect(await screen.findByRole("button", { name: /Confirm & create 2/ })).toBeInTheDocument();
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

/// Upload order is suite order. A queue whose cases all carry a
/// tester_order goes out in that order whatever the screen shows, so the
/// suite - and the runner walking it - read like the run sheet.
test("an optimized queue is uploaded in tester order, not screen order", async () => {
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
  renderQueue([
    makeCase({ title: "Third on the sheet", tester_order: 3 }),
    makeCase({ title: "First on the sheet", tester_order: 1 }),
    makeCase({ title: "Second on the sheet", tester_order: 2 }),
  ]);

  fireEvent.click(screen.getByRole("button", { name: /Review 3 test cases/ }));
  fireEvent.click(await screen.findByRole("button", { name: /Yes — create 3/ }));
  await waitFor(() => expect(sentTitles).toHaveLength(3));
  expect(sentTitles).toEqual(["First on the sheet", "Second on the sheet", "Third on the sheet"]);
});
