import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test } from "vitest";
import type { TestCase } from "../bindings";
import type { WatchedFile } from "../lib/fileSync";
import QueueSection from "./QueueSection";

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

test("Edit opens the inline editor and Save writes back into the queue", async () => {
  baseMocks();
  renderQueue([makeCase()]);

  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  const title = await screen.findByLabelText("Case title");
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
test("duplicate-title creates stop the submit until explicitly allowed", async () => {
  let submits = 0;
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases")
      return [{ id: 201, title: "Login works", tags: "", automation_status: "Planned" }];
    if (cmd === "submit_queue") {
      submits += 1;
      return [{ index: 0, title: "Login works", action: "created", id: 900, error: null }];
    }
    return undefined;
  });
  renderQueue([makeCase()]); // a CREATE row titled "Login works"

  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  fireEvent.click(await screen.findByRole("button", { name: /Confirm & create 1/ }));
  fireEvent.click(screen.getByRole("button", { name: /Yes — create 1/ }));

  // The gate trips instead of submitting.
  expect(await screen.findByText(/Stopped: 1 case/)).toBeInTheDocument();
  expect(submits).toBe(0);

  // Only the explicit choice goes through.
  fireEvent.click(screen.getByRole("button", { name: "Create duplicates anyway" }));
  await waitFor(() => expect(submits).toBe(1));
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

