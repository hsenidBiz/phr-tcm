import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test } from "vitest";
import ImportFile, { specEntryFor } from "./ImportFile";
import { Toaster } from "../components/ui/toaster";

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
 *  list that was SENT, so filtering it and then matching against the
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

  // Nothing leaves the queue: both rows stay until the user removes them.
  // The written one says so; the skipped one wrote nothing, so it does not.
  await screen.findByText("UPLOADED");
  const rows = [...document.querySelectorAll("li.rounded-md")];
  expect(rows.map((li) => li.textContent)).toEqual([
    expect.stringContaining("Unchanged"),
    expect.stringContaining("Edited now"),
  ]);
  expect(rows[0].textContent).not.toContain("UPLOADED");
  expect(rows[1].textContent).toContain("UPLOADED");
});

/// Field request: after an upload, a tester needs to know which cases
/// changed - by id, so they can tell whether one they already ran needs
/// running again - and what changed in each.
test("Copy changes puts each updated case's id and changes on the clipboard", async () => {
  const unchanged = {
    title: "Unchanged", steps: [{ action: "A", expected: "ok" }], tags: "",
    automation_status: "Not Automated", module_value: "", preconditions: "", update_id: 201,
  };
  const edited = {
    title: "Edited now", steps: [{ action: "B", expected: "shown" }], tags: "",
    automation_status: "Not Automated", module_value: "", preconditions: "", update_id: 202,
  };
  let copied: string | null = null;
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "plugin:clipboard-manager|write_text") {
      copied = (args as { text: string }).text;
      return null;
    }
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "list_repos") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "classification_paths") return [];
    if (cmd === "list_iterations") return [];
    if (cmd === "plugin:dialog|open") return "C:\\cases.json";
    if (cmd === "parse_import_file") return { cases: [unchanged, edited], warnings: [] };
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
      const a = args as { queue: Array<{ title: string; update_id: number }> };
      return a.queue.map((tc, index) => ({
        index, title: tc.title, action: "updated", id: tc.update_id, error: null,
      }));
    }
  });
  renderScreen();
  fireEvent.click(screen.getByRole("button", { name: "Import JSON" }));
  await screen.findByText("Unchanged");
  fireEvent.click(screen.getByRole("button", { name: /Review 2 test cases/ }));
  fireEvent.click(await screen.findByRole("button", { name: /Confirm & update 2/ }));

  fireEvent.click(await screen.findByRole("button", { name: "Copy changes" }));
  await waitFor(() => expect(copied).not.toBeNull());
  expect(copied).toBe(
    [
      "Test case changes for PBI #42",
      "",
      "Updated (1)",
      "#202  Edited now",
      '  - Title: "Edited BEFORE" -> "Edited now"',
      "  - Step 1 changed",
      '      Expected: "ok" -> "shown"',
    ].join("\n"),
  );
  // The skipped, unchanged case is not something to re-test.
  expect(copied).not.toContain("#201");

  // Clearing the results takes the button with them.
  fireEvent.click(screen.getByRole("button", { name: "Clear results" }));
  expect(screen.queryByRole("button", { name: "Copy changes" })).not.toBeInTheDocument();
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
  fireEvent.click(await screen.findByRole("button", { name: /Yes — create 2/ }));
  // The results panel reports a failure as a badge, the title and the
  // reason rather than one run-together sentence, and the headline counts
  // only what actually reached Azure DevOps.
  expect(await screen.findByText("1 test case uploaded - 1 created, 1 failed")).toBeInTheDocument();
  expect(screen.getByText("FAILED")).toBeInTheDocument();
  expect(screen.getByText("boom")).toBeInTheDocument();
  expect(screen.getByText(/2 queued/)).toBeInTheDocument();

  // Both rows stay. "Good" was created, so it now carries its new id - it
  // is an update of the case it made, and cannot be created a second time -
  // and says it was uploaded. "Bad" is ringed as still needing a decision.
  //
  // Each title is on screen twice now - once as the queue row, once in the
  // results panel - so pick the queue row by the shape only it has.
  const queueRow = (title: string) =>
    screen
      .getAllByText(title)
      .map((el) => el.closest("li"))
      .find((li) => li?.className.includes("rounded-md"));
  const good = queueRow("Good");
  expect(good, "the created case should still be queued").toBeTruthy();
  expect(good?.textContent).toContain("UPDATE #901");
  expect(good?.textContent).toContain("UPLOADED");
  const bad = queueRow("Bad");
  expect(bad, "the failed case should still be queued").toBeTruthy();
  expect(bad?.className).toMatch(/border-danger/);
  expect(bad?.textContent).not.toContain("UPLOADED");

  // Clearing the results puts the rows back to normal: no Uploaded badge,
  // no failure ring. The rows themselves stay, and so does the new id.
  fireEvent.click(screen.getByRole("button", { name: "Clear results" }));
  await waitFor(() => expect(screen.queryByText("UPLOADED")).not.toBeInTheDocument());
  const rows = [...document.querySelectorAll("li.rounded-md")];
  expect(rows).toHaveLength(2);
  expect(rows[0].textContent).toContain("UPDATE #901");
  expect(rows.map((li) => li.className).join(" ")).not.toMatch(/border-danger|border-success/);
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

/// Field report 2026-09-02: Import File was already open during intake,
/// the "Watching …" toast appeared, the assistant wrote the file - and
/// nothing imported. App had saved the watch to storage, but the mounted
/// tab only re-reads storage on mount or a scope switch, so no OS watcher
/// was ever armed for the new path. The tab must hear the intake itself.
test("an intake path announced while the tab is open is watched at once", async () => {
  const watched: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "file_stamp") return null; // not written yet
    if (cmd === "watch_file") {
      watched.push((args as { path: string }).path);
      return null;
    }
    if (cmd === "unwatch_file") return null;
    if (cmd === "unwatch_all_files") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "test_case_field_values") return [];
  }, { shouldMockEvents: true });
  renderScreen();
  expect(await screen.findByRole("button", { name: "Import JSON" })).toBeInTheDocument();
  expect(watched).toEqual([]);

  const path = "D:\\repo\\.test-cases\\login.json";
  await act(async () => {
    await emit("intake-output-path", { path });
  });

  await waitFor(() => expect(watched).toContain(path));
  expect(screen.getByText(/Watching 1 file/)).toBeInTheDocument();
  expect(JSON.parse(localStorage.getItem("tcm-v2-watch:acme/42") as string)).toEqual([
    { path, stamp: "", snapshot: [] },
  ]);
});

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

// ---------------------------------------------------------------------
// Per-repo workspace: with a working repository set, a picked file is
// copied into its .test-cases folder before it is parsed/watched.

const oneCase = {
  title: "Copied case", steps: [{ action: "A", expected: "" }], tags: "",
  automation_status: "Not Automated", module_value: "", preconditions: "", update_id: null,
};

/// With a working repository set, a picked file is copied into its
/// .test-cases folder and THAT copy is what gets parsed and watched - the
/// repo, not wherever the file happened to be, is the source of truth.
test("a picked file is copied into the repo's .test-cases and imported from there", async () => {
  localStorage.setItem("tcm-v2-working-dir", "D:\\repo");
  let copyArgs: unknown;
  const parsed: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "plugin:dialog|open") return "C:\\Downloads\\cases.json";
    if (cmd === "copy_into_cases") {
      copyArgs = args;
      return { path: "D:\\repo\\.test-cases\\cases.json", displaced: null };
    }
    if (cmd === "parse_import_file") {
      parsed.push((args as { path: string }).path);
      return { cases: [oneCase], warnings: [] };
    }
    if (cmd === "file_stamp") return "abc";
    if (cmd === "read_general_comment") return "";
    if (cmd === "watch_file") return null;
    if (cmd === "unwatch_all_files") return null;
    return [];
  });
  renderScreen();

  fireEvent.click(await screen.findByRole("button", { name: "Import JSON" }));
  await screen.findByText("Copied case");
  expect(copyArgs).toMatchObject({ root: "D:\\repo", source: "C:\\Downloads\\cases.json" });
  expect(parsed).toEqual(["D:\\repo\\.test-cases\\cases.json"]);
  const watches = JSON.parse(localStorage.getItem("tcm-v2-watch:acme/42") as string);
  expect(watches[0].path).toBe("D:\\repo\\.test-cases\\cases.json");
});

/// Re-importing a file that is already being watched from OUTSIDE the repo:
/// the copy takes over, so the watch on the original has to go with it -
/// two files claiming the same cases, and the one the assistant edits is
/// not the one being followed.
test("copying on import stops watching the original file", async () => {
  localStorage.setItem("tcm-v2-working-dir", "D:\\repo");
  // The stored stamp must match what `file_stamp` reports below. A stale
  // one is a file "edited while the app was closed", and the mount-time
  // check would (correctly) sync its case into the queue - racing the
  // import's own append, so the row showed up once or twice depending on
  // which microtask chain finished first. That sync is its own feature;
  // this test is about the watch entry, so the file is unchanged here.
  localStorage.setItem(
    "tcm-v2-watch:acme/42",
    JSON.stringify([
      { path: "C:\\Downloads\\cases.json", stamp: "abc", snapshot: [oneCase] },
    ]),
  );
  const unwatched: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "plugin:dialog|open") return "C:\\Downloads\\cases.json";
    if (cmd === "copy_into_cases") return { path: "D:\\repo\\.test-cases\\cases.json", displaced: null };
    if (cmd === "parse_import_file") return { cases: [oneCase], warnings: [] };
    if (cmd === "file_stamp") return "abc";
    if (cmd === "read_general_comment") return "";
    if (cmd === "watch_file") return null;
    if (cmd === "unwatch_file") {
      unwatched.push((args as { path: string }).path);
      return null;
    }
    if (cmd === "unwatch_all_files") return null;
    return [];
  });
  renderScreen();

  fireEvent.click(await screen.findByRole("button", { name: "Import JSON" }));
  await screen.findByText("Copied case");

  await waitFor(() => {
    const stored = JSON.parse(localStorage.getItem("tcm-v2-watch:acme/42") as string);
    expect(stored.map((w: { path: string }) => w.path)).toEqual([
      "D:\\repo\\.test-cases\\cases.json",
    ]);
  });
  expect(unwatched).toContain("C:\\Downloads\\cases.json");
});

/// Round 8 §11: a re-pick with different content replaces the copy the app
/// follows, and the toast says where the old one went.
test("a re-picked file with new content replaces the copy and says so", async () => {
  localStorage.setItem("tcm-v2-working-dir", "D:\\repo");
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "plugin:dialog|open") return "C:\\Downloads\\cases.json";
    if (cmd === "copy_into_cases")
      return { path: "D:\\repo\\.test-cases\\cases.json", displaced: "D:\\repo\\.test-cases\\.history\\cases.20260908-101500.json" };
    if (cmd === "parse_import_file") return { cases: [oneCase], warnings: [] };
    if (cmd === "file_stamp") return "abc";
    if (cmd === "read_general_comment") return "";
    if (cmd === "watch_file") return null;
    if (cmd === "unwatch_all_files") return null;
    return [];
  });
  renderScreen();
  render(<Toaster />);
  fireEvent.click(await screen.findByRole("button", { name: "Import JSON" }));
  await screen.findByText("Copied case");
  expect(await screen.findByText(/previous one is in \.history/)).toBeInTheDocument();
});

test("without a working repository the picked file is imported where it is", async () => {
  let copied = false;
  const parsed: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "plugin:dialog|open") return "C:\\Downloads\\cases.json";
    if (cmd === "copy_into_cases") {
      copied = true;
      return "never";
    }
    if (cmd === "parse_import_file") {
      parsed.push((args as { path: string }).path);
      return { cases: [oneCase], warnings: [] };
    }
    if (cmd === "file_stamp") return "abc";
    if (cmd === "read_general_comment") return "";
    if (cmd === "watch_file") return null;
    if (cmd === "unwatch_all_files") return null;
    return [];
  });
  renderScreen();

  fireEvent.click(await screen.findByRole("button", { name: "Import JSON" }));
  await screen.findByText("Copied case");
  expect(copied).toBe(false);
  expect(parsed).toEqual(["C:\\Downloads\\cases.json"]);
});

/// Field report: an assistant saved twice, and the first save's changes
/// vanished from the panel - they were only visible again at review time.
/// Changes now pile up until the user clears them by hand.
test("a second save adds to the report instead of replacing it", async () => {
  seedWatchedImport();
  let contents: unknown[] = [jsonCase("Login works")];
  let stamp = "stamp-1";
  mockIPC((cmd) => {
    if (cmd === "file_stamp") return stamp;
    if (cmd === "watch_file") return null;
    if (cmd === "unwatch_file") return null;
    if (cmd === "unwatch_all_files") return null;
    if (cmd === "parse_import_file") return { cases: contents, warnings: [] };
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "test_case_field_values") return [];
  }, { shouldMockEvents: true });
  renderScreen();
  await screen.findByText("cases.json");

  // First save: the existing case grows a step.
  contents = [
    jsonCase("Login works", {
      steps: [{ action: "do", expected: "ok" }, { action: "then", expected: "done" }],
    }),
  ];
  stamp = "stamp-2";
  await act(async () => {
    await fileChanged("stamp-2");
  });
  expect(await screen.findByText(/~1 changed/)).toBeInTheDocument();

  // Second save: a brand new case. The first save's change is STILL there.
  contents = [
    jsonCase("Login works", {
      steps: [{ action: "do", expected: "ok" }, { action: "then", expected: "done" }],
    }),
    jsonCase("Login rejects a bad password"),
  ];
  stamp = "stamp-3";
  await act(async () => {
    await fileChanged("stamp-3");
  });
  expect(await screen.findByText(/\+1 added/)).toBeInTheDocument();
  expect(screen.getByText(/~1 changed/)).toBeInTheDocument();

  // The X is the only thing that clears them - and it says WHICH file's
  // report it clears, because several can be open at once.
  fireEvent.click(
    screen.getByRole("button", { name: "Dismiss the change report for cases.json" }),
  );
  expect(screen.queryByText(/~1 changed/)).not.toBeInTheDocument();
  expect(screen.queryByText(/\+1 added/)).not.toBeInTheDocument();

  // And the next save starts a fresh report, not the old pile.
  contents = [
    jsonCase("Login works", {
      steps: [{ action: "do", expected: "ok" }, { action: "then", expected: "done" }],
    }),
    jsonCase("Login rejects a bad password"),
    jsonCase("Login locks after five tries"),
  ];
  stamp = "stamp-4";
  await act(async () => {
    await fileChanged("stamp-4");
  });
  expect(await screen.findByText(/\+1 added/)).toBeInTheDocument();
  expect(screen.queryByText(/~1 changed/)).not.toBeInTheDocument();
});

// Field report 2026-09-05: after uploading a file that both updated some
// cases and added new ones, EVERY row in the queue came back ringed green
// and the queue refilled itself with the cases that had just been uploaded.
//
// The cause is a stamp comparison that cannot tell direction. `detected`
// holds the fingerprint last OBSERVED for a file; the watch holds the one
// last folded in. The change test is a plain inequality, so when the app
// writes the file itself - stamping the new work item ids in after a
// submit - and moves the watch's stamp forward, `detected` is left behind
// and the two differ. That reads exactly like an outside edit.
//
// It only showed after an upload because that is the one moment the queue
// is empty: the prune has just taken the uploaded cases out, so the
// phantom "edit" loads the whole file back in and every case in it is new.
// That also put cases one Create away from a duplicate back in the queue.
test("the app's own id write-back after a submit does not re-import the file", async () => {
  const before = [jsonCase("Already there", { update_id: 151340 }), jsonCase("Brand new")];
  const after = [
    jsonCase("Already there", { update_id: 151340 }),
    jsonCase("Brand new", { update_id: 153450 }),
  ];
  localStorage.setItem("tcm-v2-draft:acme/42", JSON.stringify(before));
  localStorage.setItem(
    "tcm-v2-watch:acme/42",
    JSON.stringify([{ path: CASE_PATH, stamp: "stamp-1", snapshot: before }]),
  );

  let onDisk = before;
  mockIPC((cmd, args) => {
    if (cmd === "file_stamp") return onDisk === before ? "stamp-1" : "stamp-2";
    if (cmd === "watch_file" || cmd === "unwatch_file" || cmd === "unwatch_all_files") return null;
    if (cmd === "parse_import_file") return { cases: onDisk, warnings: [] };
    if (cmd === "read_general_comment") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "test_case_field_values") return [];
    // The write-back: the created case's id goes into the file, and the
    // file's fingerprint moves on.
    if (cmd === "save_draft_cases") {
      onDisk = after;
      return { stamp: "stamp-2", cases: after };
    }
    if (cmd === "submit_queue") {
      const a = args as { queue: Array<{ title: string; update_id: number | null }> };
      return a.queue.map((tc, index) => ({
        index,
        title: tc.title,
        action: tc.update_id == null ? "created" : "updated",
        id: tc.update_id ?? 153450,
        error: null,
      }));
    }
  }, { shouldMockEvents: true });

  renderScreen();
  await screen.findByText("Already there");
  fireEvent.click(screen.getByRole("button", { name: /Review 2 test case/ }));
  // The duplicate check runs with the review now, so anything it finds is
  // already on screen and has to be accepted before the write.
  const accept = await screen.findByRole("button", { name: "Create duplicates anyway" }).catch(() => null);
  if (accept) fireEvent.click(accept);
  const go = await screen.findByRole("button", { name: /Yes — create|Confirm & / });
  await waitFor(() => expect(go).toBeEnabled());
  fireEvent.click(go);
  await screen.findByText(/uploaded/);

  // Both cases were written and both stay queued, now carrying their ids -
  // and the file must not pour a second copy of them in.
  await screen.findByText("UPDATE #153450");
  await waitFor(() =>
    expect(screen.getAllByText("UPLOADED")).toHaveLength(2),
  );
  expect(document.querySelectorAll("li.rounded-md")).toHaveLength(2);
  expect(screen.queryByText(/Loaded 2 cases/)).not.toBeInTheDocument();
});

// The field report behind keeping uploaded rows: the queue used to empty
// after an upload while its file stayed watched, which left a watch with
// nothing to upload. Now the rows stay, the watch stays with them, and
// an edit to the file after the upload lands on the rows it belongs to -
// not as a second copy of each.
test("after an upload the rows stay, and a later file edit updates them in place", async () => {
  const before = [jsonCase("Brand new")];
  const stamped = [jsonCase("Brand new", { update_id: 153450 })];
  const edited = [jsonCase("Brand new, renamed", { update_id: 153450 })];
  localStorage.setItem("tcm-v2-draft:acme/42", JSON.stringify(before));
  localStorage.setItem(
    "tcm-v2-watch:acme/42",
    JSON.stringify([{ path: CASE_PATH, stamp: "stamp-1", snapshot: before }]),
  );

  let onDisk = before;
  const stampOf = () => (onDisk === before ? "stamp-1" : onDisk === stamped ? "stamp-2" : "stamp-3");
  mockIPC((cmd, args) => {
    if (cmd === "file_stamp") return stampOf();
    if (cmd === "watch_file" || cmd === "unwatch_file" || cmd === "unwatch_all_files") return null;
    if (cmd === "parse_import_file") return { cases: onDisk, warnings: [] };
    if (cmd === "read_general_comment") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "save_draft_cases") {
      onDisk = stamped;
      return { stamp: "stamp-2", cases: stamped };
    }
    if (cmd === "submit_queue") {
      const a = args as { queue: Array<{ title: string }> };
      return a.queue.map((tc, index) => ({ index, title: tc.title, action: "created", id: 153450, error: null }));
    }
  }, { shouldMockEvents: true });

  renderScreen();
  await screen.findByText("Brand new");
  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  const accept = await screen.findByRole("button", { name: "Create duplicates anyway" }).catch(() => null);
  if (accept) fireEvent.click(accept);
  const go = await screen.findByRole("button", { name: /Yes — create|Confirm & / });
  await waitFor(() => expect(go).toBeEnabled());
  fireEvent.click(go);
  await screen.findByText("UPDATE #153450");

  // Still watched: the file can still be dropped.
  expect(screen.getByRole("button", { name: "Stop watching cases.json" })).toBeInTheDocument();

  // The assistant edits the file after the upload.
  onDisk = edited;
  await act(async () => {
    await fileChanged("stamp-3");
  });

  await screen.findByText("Brand new, renamed");
  const rows = document.querySelectorAll("li.rounded-md");
  expect(rows).toHaveLength(1);
  expect(rows[0].textContent).toContain("UPDATE #153450");
});

// ---------------------------------------------------------------------
// Specs per watched file: Attach spec / Add wiki link write the list back
// to the file's own `specs`, and a remove button takes an entry back out.
// Task 4 (specs.rs / import_parser) owns the file format; this covers the
// app's side of the seam - the control and the write-back.

test("a watched file lists its specs and Attach spec writes the list back", async () => {
  let saved: { path: string; specs: string[] } | null = null;
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "plugin:dialog|open") {
      const a = args as { options?: { multiple?: boolean } };
      // The initial import picks the JSON file (single); Attach spec picks
      // one or more markdown files under the same directory.
      return a.options?.multiple ? ["C:/w/specs/Rules.md"] : "C:/w/cases.json";
    }
    if (cmd === "parse_import_file")
      return { cases: [jsonCase("A")], warnings: [], specs: ["Step13.md"] };
    if (cmd === "read_general_comment") return "";
    if (cmd === "save_specs") {
      saved = args as { path: string; specs: string[] };
      return "stamp-2";
    }
    if (cmd === "file_stamp") return "stamp-1";
    if (cmd === "watch_file") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "test_case_field_values") return [];
  });
  renderScreen();
  fireEvent.click(screen.getByRole("button", { name: "Import JSON" }));
  await screen.findByText("Step13.md");
  fireEvent.click(screen.getByRole("button", { name: /Attach spec/i }));
  await waitFor(() => expect(saved).not.toBeNull());
  expect(saved!.path).toBe("C:/w/cases.json");
  expect(saved!.specs).toEqual(["Step13.md", "specs/Rules.md"]);
  expect(await screen.findByText("specs/Rules.md")).toBeInTheDocument();
});

test("a wiki link is added by pasting it, and an entry can be removed", async () => {
  const saved: string[][] = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "plugin:dialog|open") return "C:/w/cases.json";
    if (cmd === "parse_import_file")
      return { cases: [jsonCase("A")], warnings: [], specs: ["Step13.md"] };
    if (cmd === "read_general_comment") return "";
    if (cmd === "save_specs") {
      saved.push((args as { specs: string[] }).specs);
      return "stamp-" + saved.length;
    }
    if (cmd === "file_stamp") return "stamp-1";
    if (cmd === "watch_file") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "test_case_field_values") return [];
  });
  renderScreen();
  fireEvent.click(screen.getByRole("button", { name: "Import JSON" }));
  await screen.findByText("Step13.md");
  fireEvent.click(screen.getByRole("button", { name: /Add wiki link/i }));
  const box = screen.getByLabelText("Wiki page link");
  fireEvent.change(box, {
    target: { value: " https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine " },
  });
  fireEvent.keyDown(box, { key: "Enter" });
  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0]).toEqual(["Step13.md", "https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine"]);
  fireEvent.click(screen.getByRole("button", { name: "Remove spec Step13.md" }));
  await waitFor(() => expect(saved).toHaveLength(2));
  expect(saved[1]).toEqual(["https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine"]);
});

// ---------------------------------------------------------------------
// specEntryFor: a picked spec path is stored relative to the JSON file's
// directory when it lives there (so the pair travels together), else as
// the absolute path the picker returned.

test("specEntryFor stores a spec relative to the JSON file's directory when it lives there", () => {
  expect(specEntryFor("C:/w/cases.json", "C:/w/specs/Rules.md")).toBe("specs/Rules.md");
});

test("specEntryFor handles a backslash JSON path against a forward-slash picked file", () => {
  expect(specEntryFor("C:\\w\\cases.json", "C:/w/specs/Rules.md")).toBe("specs/Rules.md");
});

test("specEntryFor matches the directory case-insensitively", () => {
  expect(specEntryFor("C:/w/cases.json", "c:/W/Specs/Rules.md")).toBe("Specs/Rules.md");
});

test("specEntryFor keeps a file elsewhere as the absolute path", () => {
  expect(specEntryFor("C:/w/cases.json", "D:/elsewhere/Rules.md")).toBe("D:/elsewhere/Rules.md");
});

// ---------------------------------------------------------------------
// Two watched files reconciling at once: the guard that serializes their
// sync must never drop one on the floor.

/// Review finding: the arming loop sets `detected` once per watched file.
/// With two stale files, the second file's arrival re-ran the reconcile
/// while the first file's sync was mid-flight: the cleanup cancelled it,
/// the new run bailed on the busy guard, and nothing ever retried. Neither
/// file's edits reached the queue.
test("two watched files that both changed while the app was closed both sync on arming", async () => {
  const LOGOUT_PATH = String.raw`C:\work\logout.json`;
  localStorage.setItem(
    "tcm-v2-draft:acme/42",
    JSON.stringify([jsonCase("Login works"), jsonCase("Logout works")]),
  );
  localStorage.setItem(
    "tcm-v2-watch:acme/42",
    JSON.stringify([
      { path: CASE_PATH, stamp: "s1", snapshot: [jsonCase("Login works")] },
      { path: LOGOUT_PATH, stamp: "s1", snapshot: [jsonCase("Logout works")] },
    ]),
  );
  // Pin the interleaving: the second file is armed only once the first
  // file's sync is parsing, and that parse is held until the second file's
  // fingerprint is in - so the first sync is always cancelled mid-flight.
  let firstParseAsked: () => void = () => {};
  const firstParsing = new Promise<void>((r) => {
    firstParseAsked = r;
  });
  let releaseFirst: () => void = () => {};
  const firstHeld = new Promise<void>((r) => {
    releaseFirst = r;
  });
  mockIPC(async (cmd, args) => {
    const path = (args as { path?: string } | undefined)?.path;
    if (cmd === "watch_file") {
      if (path === LOGOUT_PATH) await firstParsing;
      return null;
    }
    if (cmd === "file_stamp") {
      if (path === LOGOUT_PATH) setTimeout(releaseFirst, 50);
      return "s2";
    }
    if (cmd === "unwatch_file" || cmd === "unwatch_all_files") return null;
    if (cmd === "parse_import_file") {
      if (path === CASE_PATH) {
        firstParseAsked();
        await firstHeld;
        return { cases: [jsonCase("Login works"), jsonCase("Login added while away")], warnings: [] };
      }
      return { cases: [jsonCase("Logout works"), jsonCase("Logout added while away")], warnings: [] };
    }
    if (cmd === "read_general_comment") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "test_case_field_values") return [];
  }, { shouldMockEvents: true });
  renderScreen();

  expect(await screen.findByText("Login added while away", {}, { timeout: 3000 })).toBeInTheDocument();
  expect(await screen.findByText("Logout added while away", {}, { timeout: 3000 })).toBeInTheDocument();
});

/// Review finding: the shared-import write finished with the setter of the
/// PBI it started on, but that setter's `prev` was whatever PBI was on
/// screen by then. It saved the NEW PBI's list (plus the share) under the
/// OLD PBI's key - wiping the old PBI's own watches - and showed the share
/// file as watched on the new PBI.
test("a shared import that finishes after a PBI switch records its watch under its own PBI", async () => {
  localStorage.setItem(
    "tcm-v2-watch:acme/42",
    JSON.stringify([{ path: CASE_PATH, stamp: "s", snapshot: [] }]),
  );
  let asked = false;
  let finish: (v: unknown) => void = () => {};
  mockIPC((cmd) => {
    if (cmd === "fetch_shared_queue") return sharedFor(42);
    if (cmd === "materialize_shared_draft") {
      asked = true;
      return new Promise((r) => {
        finish = r;
      });
    }
    if (cmd === "watch_file" || cmd === "unwatch_file" || cmd === "unwatch_all_files") return null;
    if (cmd === "file_stamp") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "test_case_field_values") return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const at = (p: typeof pbi) => (
    <QueryClientProvider client={qc}>
      <ImportFile org="acme" project="Web" pbi={p} />
    </QueryClientProvider>
  );
  const { rerender } = render(at(pbi));
  fireEvent.change(screen.getByLabelText("Share link"), {
    target: { value: "tcm-share:acme/Web/42/aaaa-1111" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Import shared" }));
  expect(await screen.findByText("Shared case")).toBeInTheDocument();
  await waitFor(() => expect(asked).toBe(true));

  // The user moves to another PBI before the local copy is written.
  rerender(at({ ...pbi, id: 77, title: "Another PBI" }));
  await act(async () => {
    finish({ path: "C:/drafts/shared-pbi-42.json", stamp: "shared-stamp-1" });
  });

  // PBI 77 shows nothing it does not watch, and stores nothing.
  expect(screen.queryByText(/Watching 1 file/)).not.toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-watch:acme/77")).toBeNull();
  // PBI 42 keeps its own watch AND gains the share.
  const stored = JSON.parse(localStorage.getItem("tcm-v2-watch:acme/42") as string) as { path: string }[];
  expect(stored.map((w) => w.path)).toEqual([CASE_PATH, "C:/drafts/shared-pbi-42.json"]);
});
