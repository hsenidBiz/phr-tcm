import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test } from "vitest";
import ImportFile from "./ImportFile";
import { Toaster } from "sonner";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const pbi = { id: 42, title: "Login flow", work_item_type: "Product Backlog Item" };

function renderScreen() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ImportFile org="acme" project="Web" pbi={pbi} />
    </QueryClientProvider>,
  );
}

/** Mirrors App: the PBI lives above ImportFile, so onPickPbi genuinely
 * re-keys useQueue - the condition the switch path has to survive. */
function StatefulHost({ initial }: { initial: typeof pbi }) {
  const [current, setCurrent] = useState(initial);
  return (
    <>
      <span data-testid="current-pbi">{current.id}</span>
      <ImportFile org="acme" project="Web" pbi={current} onPickPbi={setCurrent} />
    </>
  );
}

function renderHosted(initial = pbi) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <StatefulHost initial={initial} />
    </QueryClientProvider>,
  );
}

const sharedFor = (pbiId: number) => ({
  pbi_id: pbiId,
  pbi_title: "Timeline - split weight",
  pbi_work_item_type: "Product Backlog Item",
  organization: "acme",
  project: "Web",
  cases: [
    {
      update_id: null, title: "Shared case", tags: "", automation_status: "Not Automated",
      module_value: "", preconditions: "", comment: "",
      steps: [{ action: "a", expected: "b" }],
    },
  ],
  warnings: [],
});

/** An import is mostly cases that have not moved. The review gate already
 *  printed "no-op - nothing will change" on those rows and then submitted
 *  them anyway: 81 cases where ten had changed meant 71 pointless PATCHes,
 *  each behind the half-second pacing gap. They are not sent now.
 *
 *  The dangerous half is the indices - results are numbered against the
 *  list that was SENT, so filtering it and then pruning against the
 *  original queue is exactly the mistake that stranded created cases four
 *  times before. This checks both: what went, and what is left. */
test("an update with nothing to change is not submitted at all", async () => {
  const unchanged = {
    title: "Unchanged", steps: [{ action: "A", expected: "ok" }], tags: "",
    automation_status: "Not Automated", module_value: "", preconditions: "", update_id: 201,
  };
  const edited = {
    title: "Edited now", steps: [{ action: "B", expected: "ok" }], tags: "",
    automation_status: "Not Automated", module_value: "", preconditions: "", update_id: 202,
  };
  let sent: Array<{ title: string }> = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "list_repos") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "classification_paths") return [];
    if (cmd === "list_iterations") return [];
    if (cmd === "plugin:dialog|open") return "C:\\cases.json";
    if (cmd === "parse_import_file") return { cases: [unchanged, edited], warnings: [] };
    // What Azure DevOps currently holds: 201 identical, 202 different.
    if (cmd === "test_cases_by_ids")
      return [
        { id: 201, title: "Unchanged", tags: "", automation_status: "Not Automated",
          steps: [{ action: "A", expected: "ok" }], step_ids: ["2"], module_value: "",
          preconditions: "" },
        { id: 202, title: "Edited BEFORE", tags: "", automation_status: "Not Automated",
          steps: [{ action: "B", expected: "ok" }], step_ids: ["2"], module_value: "",
          preconditions: "" },
      ];
    if (cmd === "submit_queue") {
      const a = args as { queue: Array<{ title: string }> };
      sent = a.queue;
      return a.queue.map((tc, index) => ({
        index, title: tc.title, action: "updated", id: 900 + index, error: null,
      }));
    }
  });
  renderScreen();
  fireEvent.click(screen.getByRole("button", { name: "Import JSON" }));
  await screen.findByText("Unchanged");

  fireEvent.click(screen.getByRole("button", { name: /Review 2 test cases/ }));
  // Both rows are updates, so the gate says "update", not "create" - and
  // a pure-update queue skips the check-the-PBI stage entirely (updates
  // never read the selected PBI), so the first Confirm submits.
  fireEvent.click(await screen.findByRole("button", { name: /Confirm & update 2/ }));

  await waitFor(() => expect(sent.length).toBeGreaterThan(0));
  expect(sent.map((c) => c.title)).toEqual(["Edited now"]);

  // And the skipped row does not linger: nothing was written for it
  // because nothing needed to be, so the import is finished for it too.
  await waitFor(() => expect(screen.queryByText("Unchanged")).not.toBeInTheDocument());
});

test("import feeds the shared queue; failed items stay queued", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "plugin:dialog|open") return "C:\\cases.json";
    if (cmd === "list_repos") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "parse_import_file")
      return {
        cases: [
          { title: "Good", steps: [{ action: "A", expected: "" }], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "", update_id: null },
          { title: "Bad", steps: [{ action: "B", expected: "" }], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "", update_id: null },
        ],
        warnings: ["Row 9: something odd"],
      };
    if (cmd === "submit_queue") {
      const a = args as { queue: Array<{ title: string }> };
      return a.queue.map((tc, index) => ({
        index,
        title: tc.title,
        action: tc.title === "Bad" ? "failed" : "created",
        id: tc.title === "Bad" ? null : 901,
        error: tc.title === "Bad" ? "boom" : null,
      }));
    }
  });
  renderScreen();
  fireEvent.click(screen.getByRole("button", { name: "Import JSON" }));
  await screen.findByText("Good");
  expect(screen.getByText("Row 9: something odd")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: /Review 2 test cases/ }));
  fireEvent.click(await screen.findByRole("button", { name: /Confirm & create 2/ }));
  fireEvent.click(screen.getByRole("button", { name: /Yes — create 2/ }));
  expect(await screen.findByText(/Failed: Bad - boom/)).toBeInTheDocument();
  expect(screen.getByText(/1 queued/)).toBeInTheDocument();
});

test("a matching PBI imports straight into the queue", async () => {
  const calls = { watched: 0 };
  mockIPC((cmd) => {
    if (cmd === "fetch_shared_queue") return sharedFor(42); // same as selected
    if (cmd === "materialize_shared_draft")
      return { path: "C:/drafts/shared.json", stamp: "shared-stamp-1" };
    if (cmd === "watch_file") {
      calls.watched += 1;
      return null;
    }
  });
  renderHosted();
  fireEvent.change(screen.getByLabelText("Share link"), {
    target: { value: "tcm-share:acme/Web/42/aaaa-1111" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Import shared" }));

  expect(await screen.findByText("Shared case")).toBeInTheDocument();
  // No question asked when there is nothing to choose between.
  expect(screen.queryByText("This draft is for a different PBI")).not.toBeInTheDocument();
  // The share was materialized to a local file AND followed - the whole
  // point of the fix: a share-link queue now has a file for the id
  // write-back to land in. Awaiting it also keeps the async chain inside
  // the test instead of rejecting after teardown.
  await waitFor(() => expect(calls.watched).toBe(1));
});

test("a different PBI asks first, and Switch loads into THAT PBI's queue", async () => {
  const calls = { watched: 0 };
  mockIPC((cmd) => {
    if (cmd === "fetch_shared_queue") return sharedFor(9999); // not the selected 42
    if (cmd === "materialize_shared_draft")
      return { path: "C:/drafts/shared.json", stamp: "shared-stamp-1" };
    if (cmd === "watch_file") {
      calls.watched += 1;
      return null;
    }
  });
  renderHosted();
  fireEvent.change(screen.getByLabelText("Share link"), {
    target: { value: "tcm-share:acme/Web/9999/aaaa-1111" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Import shared" }));

  // Nothing is loaded until the user chooses.
  expect(await screen.findByText("This draft is for a different PBI")).toBeInTheDocument();
  expect(screen.queryByText("Shared case")).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Switch to #9999" }));

  // The PBI actually changed, and the cases landed AFTER the switch - so
  // they live in 9999's queue and survive being there (the reported bug
  // was them vanishing on switch because they went to the old queue).
  expect(await screen.findByText("Shared case")).toBeInTheDocument();
  expect(screen.getByTestId("current-pbi")).toHaveTextContent("9999");
  expect(localStorage.getItem("tcm-v2-draft:acme/9999")).toContain("Shared case");
  expect(localStorage.getItem("tcm-v2-draft:acme/42")).toBeNull();
  await waitFor(() => expect(calls.watched).toBe(1));
});

test("Stay keeps the current PBI and warns about the mismatch", async () => {
  const calls = { watched: 0 };
  mockIPC((cmd) => {
    if (cmd === "fetch_shared_queue") return sharedFor(9999);
    if (cmd === "materialize_shared_draft")
      return { path: "C:/drafts/shared.json", stamp: "shared-stamp-1" };
    if (cmd === "watch_file") {
      calls.watched += 1;
      return null;
    }
  });
  renderHosted();
  fireEvent.change(screen.getByLabelText("Share link"), {
    target: { value: "tcm-share:acme/Web/9999/aaaa-1111" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Import shared" }));
  fireEvent.click(await screen.findByRole("button", { name: "Stay on #42" }));

  expect(await screen.findByText("Shared case")).toBeInTheDocument();
  expect(screen.getByTestId("current-pbi")).toHaveTextContent("42");
  expect(screen.getByText(/shared for PBI #9999/)).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-draft:acme/42")).toContain("Shared case");
  await waitFor(() => expect(calls.watched).toBe(1));
});

test("Cancel loads nothing anywhere", async () => {
  mockIPC((cmd) => {
    if (cmd === "fetch_shared_queue") return sharedFor(9999);
    if (cmd === "materialize_shared_draft")
      return { path: "C:/drafts/shared.json", stamp: "shared-stamp-1" };
  });
  renderHosted();
  fireEvent.change(screen.getByLabelText("Share link"), {
    target: { value: "tcm-share:acme/Web/9999/aaaa-1111" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Import shared" }));
  fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));

  expect(screen.queryByText("Shared case")).not.toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-draft:acme/42")).toBeNull();
  expect(localStorage.getItem("tcm-v2-draft:acme/9999")).toBeNull();
});

test("a spent share link shows the one-time-use explanation", async () => {
  mockIPC((cmd) => {
    if (cmd === "fetch_shared_queue")
      throw "This share link has already been used, or was revoked by the sender.";
  });
  // The error surfaces as a toast - mount a Toaster alongside the screen.
  renderScreen();
  render(<Toaster />);
  fireEvent.change(screen.getByLabelText("Share link"), {
    target: { value: "tcm-share:acme/Web/1/aaaa" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Import shared" }));
  // Surfaced via toast; the queue stays empty.
  expect(await screen.findByText(/already been used/)).toBeInTheDocument();
});

// ---------------------------------------------------------------------
// Watched import file: an assistant edits the JSON on disk and the queue
// follows. fileSync.test.ts owns the reconciliation rules and the Rust
// filewatch suite owns the OS watcher; these cover the seam between them.

// A Windows path, since that is what the app actually hands around - the
// display has to trim it to the file name on either separator.
const CASE_PATH = String.raw`C:\work\cases.json`;

const jsonCase = (title: string, over: Record<string, unknown> = {}) => ({
  title,
  steps: [{ action: "do", expected: "ok" }],
  tags: "",
  automation_status: "Not Automated",
  module_value: "",
  preconditions: "",
  update_id: null,
  ...over,
});

/** A queue of one case, imported from CASE_PATH and being watched. */
function seedWatchedImport(stamp = "stamp-1") {
  localStorage.setItem("tcm-v2-draft:acme/42", JSON.stringify([jsonCase("Login works")]));
  localStorage.setItem(
    "tcm-v2-watch:acme/42",
    JSON.stringify({ path: CASE_PATH, stamp, snapshot: [jsonCase("Login works")] }),
  );
}

/** IPC for a watched screen. `contents` is what the file parses to next,
 * `stampNow` what a fresh fingerprint check returns. */
function mockWatched(contents: unknown[], stampNow: string) {
  mockIPC((cmd) => {
    if (cmd === "file_stamp") return stampNow;
    if (cmd === "watch_file") return null;
    if (cmd === "unwatch_file") return null;
    if (cmd === "unwatch_all_files") return null;
    if (cmd === "parse_import_file") return { cases: contents, warnings: [] };
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "test_case_field_values") return [];
  }, { shouldMockEvents: true });
}

const fileChanged = (stamp: string) =>
  emit("watched-file-changed", { path: CASE_PATH, stamp });

test("a stored watch is re-armed on mount and named on screen", async () => {
  seedWatchedImport();
  mockWatched([jsonCase("Login works")], "stamp-1"); // unchanged
  renderScreen();

  expect(await screen.findByText("cases.json")).toBeInTheDocument();
  expect(screen.getByText(/Watching 1 file/)).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "Stop watching cases.json" }),
  ).toBeInTheDocument();
});

test("an edit on disk lands in the queue and is reported", async () => {
  seedWatchedImport();
  mockWatched(
    [
      // Retitled? No - same case, an extra step. Plus a brand new one.
      jsonCase("Login works", {
        steps: [{ action: "do", expected: "ok" }, { action: "then", expected: "done" }],
      }),
      jsonCase("Login rejects a bad password"),
    ],
    "stamp-1",
  );
  renderScreen();
  await screen.findByText("cases.json");

  await act(async () => {
    await fileChanged("stamp-2");
  });

  expect(await screen.findByText("Login rejects a bad password")).toBeInTheDocument();
  expect(screen.getByText(/Updated from cases.json/)).toBeInTheDocument();
  expect(screen.getByText("+1 added")).toBeInTheDocument();
  expect(screen.getByText("~1 changed")).toBeInTheDocument();

  // The detail shows the step that arrived, not the words "Steps (1 → 2)".
  // Knowing the count changed still means opening the file to see what the
  // new step says, which is the whole reason this renders a diff now.
  fireEvent.click(screen.getByRole("button", { name: "Show details" }));
  expect(await screen.findByText(/then/)).toBeInTheDocument();
});

/** The watcher only reports changes from the moment it starts, so an edit
 * made while the app was closed has to be caught when the watch re-arms. */
test("an edit made while the app was closed is caught on arming", async () => {
  seedWatchedImport("stamp-1");
  mockWatched([jsonCase("Login works"), jsonCase("Added while you were away")], "stamp-2");
  renderScreen();

  expect(await screen.findByText("Added while you were away")).toBeInTheDocument();
  expect(screen.getByText("+1 added")).toBeInTheDocument();
});

test("a case the file dropped leaves the queue; a hand-typed one stays", async () => {
  seedWatchedImport();
  // The queue also holds a case that never came from the file.
  localStorage.setItem(
    "tcm-v2-draft:acme/42",
    JSON.stringify([jsonCase("Login works"), jsonCase("Typed by hand")]),
  );
  mockWatched([], "stamp-1"); // the file was emptied
  renderScreen();
  await screen.findByText("cases.json");

  await act(async () => {
    await fileChanged("stamp-2");
  });

  expect(await screen.findByText("−1 removed")).toBeInTheDocument();
  expect(screen.queryByText("Login works")).not.toBeInTheDocument();
  expect(screen.getByText("Typed by hand")).toBeInTheDocument();
});

test("stopping a watch asks about its cases, and Keep leaves them queued", async () => {
  seedWatchedImport();
  mockWatched([jsonCase("Login works")], "stamp-1");
  renderScreen();

  fireEvent.click(await screen.findByRole("button", { name: "Stop watching cases.json" }));
  expect(await screen.findByText(/Stop watching cases.json\?/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Keep the cases" }));

  expect(screen.queryByText(/Watching 1 file/)).not.toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-watch:acme/42")).toBeNull();
  expect(screen.getByText("Login works")).toBeInTheDocument();
});

test("Remove them too takes that file's cases out of the queue", async () => {
  seedWatchedImport();
  // Plus a case that never came from the file.
  localStorage.setItem(
    "tcm-v2-draft:acme/42",
    JSON.stringify([jsonCase("Login works"), jsonCase("Typed by hand")]),
  );
  mockWatched([jsonCase("Login works")], "stamp-1");
  renderScreen();

  fireEvent.click(await screen.findByRole("button", { name: "Stop watching cases.json" }));
  fireEvent.click(await screen.findByRole("button", { name: "Remove them too" }));

  expect(screen.queryByText("Login works")).not.toBeInTheDocument();
  // Hand-typed work is never collateral damage.
  expect(screen.getByText("Typed by hand")).toBeInTheDocument();
});

test("Cancel leaves the watch running", async () => {
  seedWatchedImport();
  mockWatched([jsonCase("Login works")], "stamp-1");
  renderScreen();

  fireEvent.click(await screen.findByRole("button", { name: "Stop watching cases.json" }));
  fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));

  expect(screen.getByText(/Watching 1 file/)).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-watch:acme/42")).toContain("cases.json");
});

test("several files are listed with their own counts and Stop buttons", async () => {
  localStorage.setItem(
    "tcm-v2-draft:acme/42",
    JSON.stringify([jsonCase("Login works"), jsonCase("Logout works")]),
  );
  localStorage.setItem(
    "tcm-v2-watch:acme/42",
    JSON.stringify([
      { path: CASE_PATH, stamp: "s1", snapshot: [jsonCase("Login works")] },
      {
        path: String.raw`C:\work\logout.json`,
        stamp: "s2",
        snapshot: [jsonCase("Logout works")],
      },
    ]),
  );
  mockWatched([], "s1");
  renderScreen();

  expect(await screen.findByText(/Watching 2 files/)).toBeInTheDocument();
  expect(screen.getByText("cases.json")).toBeInTheDocument();
  expect(screen.getByText("logout.json")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Stop watching logout.json" })).toBeInTheDocument();

  // Dropping one leaves the other watching, and its cases alone.
  fireEvent.click(screen.getByRole("button", { name: "Stop watching logout.json" }));
  fireEvent.click(await screen.findByRole("button", { name: "Remove them too" }));

  expect(await screen.findByText(/Watching 1 file/)).toBeInTheDocument();
  expect(screen.getByText("Login works")).toBeInTheDocument();
  expect(screen.queryByText("Logout works")).not.toBeInTheDocument();
});

/** The old single-object shape must keep working after an update. */
test("a watch stored before multi-file support still loads", async () => {
  localStorage.setItem("tcm-v2-draft:acme/42", JSON.stringify([jsonCase("Login works")]));
  localStorage.setItem(
    "tcm-v2-watch:acme/42",
    JSON.stringify({ path: CASE_PATH, stamp: "stamp-1", snapshot: [jsonCase("Login works")] }),
  );
  mockWatched([jsonCase("Login works")], "stamp-1");
  renderScreen();

  expect(await screen.findByText("cases.json")).toBeInTheDocument();
});
