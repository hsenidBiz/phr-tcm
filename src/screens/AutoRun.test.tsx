import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { Toaster } from "../components/ui/toaster";
import { afterEach, expect, test } from "vitest";
import AutoRun from "./AutoRun";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const PBI = { id: 42, title: "Login flow", work_item_type: "Product Backlog Item" };

function renderAutoRun() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <AutoRun org="acme" project="Web" pbi={PBI} />
    </QueryClientProvider>,
  );
}

const cases = [
  {
    id: 201,
    title: "Valid login",
    tags: "",
    automation_status: "Not Automated",
    steps: [{ action: "Open the login page", expected: "The form is shown" }],
    step_ids: ["2"],
    module_value: "",
    preconditions: "",
    steps_xml: "",
  },
  {
    id: 202,
    title: "Locked account",
    tags: "",
    automation_status: "Not Automated",
    steps: [{ action: "Sign in", expected: "A lockout message appears" }],
    step_ids: ["2"],
    module_value: "",
    preconditions: "",
    steps_xml: "",
  },
];

/// Most cases have no script, and that is the normal state - the screen
/// has to say which are drivable rather than looking broken.
test("lists the PBI's cases and marks which ones have a script", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      const a = args as { caseId: number };
      return a.caseId === 201
        ? { case_id: 201, title: "Valid login", steps: [{ step_number: 1, actions: [] }] }
        : null;
    }
  });
  renderAutoRun();

  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  expect(screen.getByText("Locked account")).toBeInTheDocument();
  expect(await screen.findByText("Script ready")).toBeInTheDocument();
  expect(screen.getByText("No script")).toBeInTheDocument();
});

/// The whole feature is local until the person presses Send. Saying so on
/// the screen is what stops someone assuming a green run updated Azure
/// DevOps on its own.
test("says plainly that nothing reaches Azure DevOps without pressing Send", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
  });
  renderAutoRun();
  expect(
    await screen.findByText(/nothing goes to azure devops unless you press send to azure devops/i),
  ).toBeInTheDocument();
});

test("without a PBI it asks for one instead of loading", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <AutoRun org="acme" project="Web" pbi={null} />
    </QueryClientProvider>,
  );
  expect(await screen.findByText(/pick a pbi/i)).toBeInTheDocument();
});

/// The picked PATH goes to Rust, not the file's contents: reading it here
/// and sending base64 (the old approach) mangled non-ASCII text and a BOM
/// on the way through `atob`. This just pins the IPC contract - that the
/// path itself is what `auto_run_import_scripts` receives - and that a
/// successful import refreshes the badges and reports what changed.
test("importing scripts sends the picked file's path, and the badge updates", async () => {
  let receivedArgs: unknown = null;
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "plugin:dialog|open") return "C:\\scripts.json";
    if (cmd === "auto_run_import_scripts") {
      receivedArgs = args;
      return [201];
    }
  });
  renderAutoRun();
  render(<Toaster />);

  const row = (await screen.findByText("Valid login")).closest("li");
  if (!row) throw new Error("row for case #201 not found");
  expect(within(row).getByText("No script")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Import scripts" }));

  await waitFor(() => expect(receivedArgs).not.toBeNull());
  expect(receivedArgs).toEqual({ path: "C:\\scripts.json" });
  expect(await screen.findByText(/imported 1 script/i)).toBeInTheDocument();
});

/// A 60-case import naming every id would be unreadable - the toast has
/// to fall back to a count past some point.
test("a large import's toast names a few ids and counts the rest", async () => {
  const ids = Array.from({ length: 14 }, (_, i) => 300 + i);
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "plugin:dialog|open") return "C:\\scripts.json";
    if (cmd === "auto_run_import_scripts") return ids;
  });
  renderAutoRun();
  render(<Toaster />);

  fireEvent.click(await screen.findByRole("button", { name: "Import scripts" }));

  expect(await screen.findByText(/imported 14 scripts/i)).toBeInTheDocument();
  expect(screen.getByText(/and 4 more/i)).toBeInTheDocument();
  // Only the first ten are spelled out.
  expect(screen.queryByText(/314/)).not.toBeInTheDocument();
});

/// Same failure class the `typedError` rethrow comment already documents
/// for `auto_run_open_browser` above: an IPC-level rejection (rather than
/// a `{status: "error"}` resolution) must not leave the Import button
/// wedged disabled or escape as an unhandled rejection - the latter is
/// what made this suite report PASS while exiting 1 before it was fixed
/// on this branch.
test("a rejected import call shows an error toast and re-enables the button", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "plugin:dialog|open") return "C:\\scripts.json";
    if (cmd === "auto_run_import_scripts") throw new Error("disk read failed");
  });
  renderAutoRun();
  render(<Toaster />);

  const importButton = await screen.findByRole("button", { name: "Import scripts" });
  fireEvent.click(importButton);

  await waitFor(() => expect(importButton).not.toBeDisabled());
  expect(await screen.findByText(/could not import that file/i)).toBeInTheDocument();
});

/// The script is authored as JSON for now - an assistant will generate
/// these later, and hand-editing is how the format gets proven first.
/// Invalid JSON must be refused at the point of saving, not written and
/// discovered mid-run.
test("the script editor refuses invalid JSON instead of saving it", async () => {
  let saved = 0;
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "auto_run_save_script") {
      saved++;
      return null;
    }
  });
  renderAutoRun();

  fireEvent.click(await screen.findByRole("button", { name: "Edit script for #201" }));
  const box = await screen.findByLabelText("Action script JSON");
  fireEvent.change(box, { target: { value: "{ not json" } });
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));

  expect(await screen.findByText(/not valid json/i)).toBeInTheDocument();
  expect(saved).toBe(0);
});

test("a valid script is saved for that case id", async () => {
  let payload: Record<string, unknown> | null = null;
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "auto_run_save_script") {
      payload = args as Record<string, unknown>;
      return null;
    }
  });
  renderAutoRun();

  fireEvent.click(await screen.findByRole("button", { name: "Edit script for #201" }));
  const box = await screen.findByLabelText("Action script JSON");
  fireEvent.change(box, {
    target: {
      value: JSON.stringify([
        { step_number: 1, actions: [{ kind: "check_text", value: "Dashboard" }] },
      ]),
    },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));

  await waitFor(() => expect(payload).not.toBeNull());
  const script = (payload as unknown as { script: { case_id: number; steps: unknown[] } }).script;
  expect(script.case_id).toBe(201);
  expect(script.steps).toHaveLength(1);
});

/// The list's badge and Run button are driven by the SAME `["autorun-script",
/// caseId]` query the editor reads. A save that never invalidates that key
/// leaves both stuck on "No script" until the person leaves the section and
/// comes back - the case they just scripted can't be run. `scriptFor201`
/// starts null and only becomes non-null once the save handler below fires,
/// so this fails without the invalidation (the query would keep serving its
/// cached `null` forever, `refetchOnWindowFocus` being off).
test("saving a script invalidates its query so the badge and Run button update in place", async () => {
  let scriptFor201: unknown = null;
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      const a = args as { caseId: number };
      return a.caseId === 201 ? scriptFor201 : null;
    }
    if (cmd === "auto_run_save_script") {
      scriptFor201 = {
        case_id: 201,
        title: "Valid login",
        steps: [{ step_number: 1, actions: [{ kind: "check_text", value: "Dashboard" }] }],
      };
      return null;
    }
  });
  renderAutoRun();

  const editButton = await screen.findByRole("button", { name: "Edit script for #201" });
  const row = editButton.closest("li");
  if (!row) throw new Error("row for case #201 not found");
  expect(within(row).getByText("No script")).toBeInTheDocument();
  expect(within(row).queryByRole("button", { name: "Run #201" })).not.toBeInTheDocument();

  fireEvent.click(editButton);
  const box = await screen.findByLabelText("Action script JSON");
  fireEvent.change(box, {
    target: {
      value: JSON.stringify([
        { step_number: 1, actions: [{ kind: "check_text", value: "Dashboard" }] },
      ]),
    },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));

  await waitFor(() => expect(within(row).getByText("Script ready")).toBeInTheDocument());
  expect(within(row).getByRole("button", { name: "Run #201" })).toBeInTheDocument();
});

/// A case that already has a saved script has never had its load-and-prefill
/// path exercised - both prior tests mock the load as returning null.
test("prefills the editor with a case's existing script", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      return {
        case_id: 201,
        title: "Valid login",
        steps: [{ step_number: 1, actions: [{ kind: "check_text", value: "Dashboard" }] }],
      };
    }
  });
  renderAutoRun();

  fireEvent.click(await screen.findByRole("button", { name: "Edit script for #201" }));
  const box = (await screen.findByLabelText("Action script JSON")) as HTMLTextAreaElement;
  await waitFor(() => expect(box.value).toContain("check_text"));
});

/// While the existing script hasn't resolved yet, an empty textarea is
/// indistinguishable from "this case has no script" - saving in that window
/// would silently overwrite whatever script was already there.
test("refuses to save while the existing script is still loading", async () => {
  let saved = 0;
  let resolveLoad: (value: unknown) => void = () => {};
  const pending = new Promise((resolve) => {
    resolveLoad = resolve;
  });
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return pending;
    if (cmd === "auto_run_save_script") {
      saved++;
      return null;
    }
  });
  renderAutoRun();

  fireEvent.click(await screen.findByRole("button", { name: "Edit script for #201" }));
  const saveButton = await screen.findByRole("button", { name: "Save script" });
  expect(saveButton).toBeDisabled();

  fireEvent.click(saveButton);
  expect(saved).toBe(0);

  resolveLoad(null);
  await waitFor(() => expect(saveButton).not.toBeDisabled());
});

/// A load failure must be distinct from "no script yet" - it must not
/// silently default to an empty, savable editor either.
test("refuses to save when the existing script fails to load", async () => {
  let saved = 0;
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") throw "boom";
    if (cmd === "auto_run_save_script") {
      saved++;
      return null;
    }
  });
  renderAutoRun();

  fireEvent.click(await screen.findByRole("button", { name: "Edit script for #201" }));
  const saveButton = await screen.findByRole("button", { name: "Save script" });
  await waitFor(() => expect(saveButton).toBeDisabled());

  fireEvent.click(saveButton);
  expect(saved).toBe(0);
  expect(await screen.findByText(/could not load the existing script/i)).toBeInTheDocument();
});

const scriptFor201 = {
  case_id: 201,
  title: "Valid login",
  steps: [
    { step_number: 1, actions: [{ kind: "check_text", value: "Dashboard" }] },
    { step_number: 2, actions: [{ kind: "click", selector: "text=Sign out" }] },
  ],
};

function mockRunnable(extra?: (cmd: string, args: unknown) => unknown) {
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      const a = args as { caseId: number };
      return a.caseId === 201 ? scriptFor201 : null;
    }
    if (cmd === "auto_run_new_id") return "run-1786000000000";
    if (cmd === "auto_run_open_browser") return null;
    if (cmd === "auto_run_close_browser") return null;
    return extra?.(String(cmd), args);
  });
}

/// The outcomes are EVIDENCE, not a vote: the pane shows what each
/// action reported and leaves the verdict buttons untouched.
test("running a step shows each action's outcome and picks no verdict", async () => {
  mockRunnable((cmd) => {
    if (cmd === "auto_run_step")
      return [{ ok: true, detail: "page contains Dashboard" }];
  });
  renderAutoRun();

  fireEvent.click(await screen.findByRole("button", { name: "Run #201" }));
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run step 1" }));

  expect(await screen.findByText("page contains Dashboard")).toBeInTheDocument();
  // Nothing is pre-selected - the human has not decided yet.
  expect(screen.getByRole("button", { name: "Passed" })).toHaveAttribute(
    "aria-pressed",
    "false",
  );
  expect(screen.getByRole("button", { name: "Failed" })).toHaveAttribute(
    "aria-pressed",
    "false",
  );
});

/// A failed action is reported plainly and STILL does not decide the
/// verdict - the person may know the failure is the harness's fault.
test("a failed action is shown but the human still chooses", async () => {
  mockRunnable((cmd) => {
    if (cmd === "auto_run_step") return [{ ok: false, detail: "not found: #nope" }];
  });
  renderAutoRun();

  fireEvent.click(await screen.findByRole("button", { name: "Run #201" }));
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run step 1" }));

  expect(await screen.findByText("not found: #nope")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Passed" })).toBeEnabled();
});

/// The generated `typedError` wrapper rethrows when the caught value is an
/// `Error` instance, so a transport failure on `auto_run_open_browser`
/// rejects rather than resolving to `{status: "error"}`. Before RunPane
/// wrapped this call in try/catch, that rejection skipped the
/// `setBusy(false)` that follows the await, wedging "Open browser"
/// permanently disabled with no message and leaving the rejection
/// unhandled (the mechanism that made this suite exit 1 despite a PASS
/// line). This asserts the button becomes usable again and a retry can
/// still succeed.
test("a rejected open-browser call resets busy state instead of wedging the button", async () => {
  let attempts = 0;
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      const a = args as { caseId: number };
      return a.caseId === 201 ? scriptFor201 : null;
    }
    if (cmd === "auto_run_open_browser") {
      attempts++;
      if (attempts === 1) throw new Error("edge failed to start");
      return null;
    }
  });
  renderAutoRun();
  // The error surfaces as a toast - mount a Toaster alongside the screen.
  render(<Toaster />);

  fireEvent.click(await screen.findByRole("button", { name: "Run #201" }));
  const openButton = await screen.findByRole("button", { name: "Open browser" });
  fireEvent.click(openButton);

  await waitFor(() => expect(openButton).not.toBeDisabled());
  expect(attempts).toBe(1);
  expect(await screen.findByText(/could not open the browser/i)).toBeInTheDocument();

  // Not wedged: pressing it again reaches the command a second time.
  fireEvent.click(openButton);
  await waitFor(() => expect(attempts).toBe(2));
});

const scriptFor202 = {
  case_id: 202,
  title: "Locked account",
  steps: [{ step_number: 1, actions: [{ kind: "check_text", value: "Locked out" }] }],
};

const unattendedFromReplay = {
  id: "run-unattended-1",
  pbi_id: 42,
  started_at: "1786000500000",
  mode: "unattended",
  cases: [
    {
      case_id: 202,
      title: "Locked account",
      verdict: "",
      note: "",
      proposed: "Passed",
      reason: "every action of 1 step passed",
      steps: [],
    },
  ],
};

/// Saving records the human's verdict and the evidence together, into a
/// LOCAL run. Nothing on this screen ever sends a result to Azure DevOps
/// on its own - not loading the screen, not ticking a selection, not a
/// supervised run, not an unattended one landing straight in its review.
/// The only door to Azure DevOps is the review's own Send button, and
/// this test never presses it.
test("no path through this screen - load, selection, a supervised run or an unattended run - ever sends to Azure DevOps", async () => {
  let saved: Record<string, unknown> | null = null;
  const calls: string[] = [];
  mockIPC((cmd, args) => {
    calls.push(String(cmd));
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      const a = args as { caseId: number };
      if (a.caseId === 201) return scriptFor201;
      if (a.caseId === 202) return scriptFor202;
      return null;
    }
    if (cmd === "auto_run_new_id") return "run-1786000000000";
    if (cmd === "auto_run_open_browser") return null;
    if (cmd === "auto_run_step") return [{ ok: true, detail: "page contains Dashboard" }];
    if (cmd === "auto_run_save_run") {
      saved = args as Record<string, unknown>;
      return null;
    }
    if (cmd === "auto_run_close_browser") return null;
    if (cmd === "auto_run_replay") return unattendedFromReplay;
    if (cmd === "auto_run_load_run") return unattendedFromReplay;
  });
  renderAutoRun();

  // On load: nothing but reads so far (asserted below).

  // A supervised run, start to finish.
  fireEvent.click(await screen.findByRole("button", { name: "Run #201" }));
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run step 1" }));
  await screen.findByText("page contains Dashboard");
  fireEvent.click(screen.getByRole("button", { name: "Failed" }));
  fireEvent.click(screen.getByRole("button", { name: "Save result" }));

  await waitFor(() => expect(saved).not.toBeNull());
  const run = (saved as unknown as { run: { cases: { verdict: string }[] } }).run;
  expect(run.cases[0].verdict).toBe("Failed");

  // The supervised pane closes itself once the last case is saved - back
  // to the case list, selection cleared.
  await screen.findByRole("button", { name: "Run #201" });

  // Selection is local state - ticking a case makes no IPC call at all.
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #202" }));

  // An unattended run, start to finish, landing straight in its review.
  fireEvent.click(await screen.findByRole("button", { name: "Run 1 unattended" }));
  fireEvent.click(await screen.findByRole("button", { name: "Start" }));
  expect(await screen.findByText(/proposed: passed/i)).toBeInTheDocument();

  // The review opened, loaded the run, and nothing was pressed to send it.
  expect(calls).not.toContain("auto_run_publish");

  // The guard that matters: nothing outside this known-safe set was ever
  // invoked. An allowlist (rather than naming the ADO run-recording
  // commands we know about today) means a write command added later - one
  // nobody thought to add to a denylist - fails this test instead of
  // slipping straight through it. If this trips, check whether the new
  // command writes to Azure DevOps: if it does, this feature must not call
  // it; if it is a genuine local/read-only addition, add it to the list
  // below deliberately. `auto_run_publish` (the one command that DOES
  // write to Azure DevOps) is deliberately left off this list - see the
  // assertion just above.
  const allowed = new Set([
    "list_test_case_fields", // read-only: field discovery for module/preconditions refs
    "pbi_test_cases_full", // read-only: the case list itself, from ADO
    "auto_run_load_script",
    "auto_run_new_id",
    "auto_run_open_browser",
    "auto_run_step",
    "auto_run_save_run",
    "auto_run_close_browser",
    "auto_run_list_runs", // read-only: PastRuns' own listing, rendered alongside this screen
    "auto_run_replay", // local: drives the browser itself, writes nothing to ADO
    "auto_run_load_run", // read-only: the review dialog loading its own run
    "plugin:event|listen", // Tauri's own event subscription - ReplayPane's progress feed
    "plugin:event|unlisten", // the same subscription's cleanup on unmount
  ]);
  for (const cmd of calls) {
    expect(allowed.has(cmd), `unexpected command "${cmd}" - does it write to Azure DevOps?`).toBe(
      true,
    );
  }
});

/// The pane opens a real Edge process with its own temp profile
/// directory. If the person navigates away instead of pressing Close or
/// Save, the pane unmounts without either handler running - and the
/// process would otherwise leak for the rest of the app's lifetime.
test("closes the browser on unmount if a session was opened and never closed", async () => {
  let closeCalls = 0;
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      const a = args as { caseId: number };
      return a.caseId === 201 ? scriptFor201 : null;
    }
    if (cmd === "auto_run_open_browser") return null;
    if (cmd === "auto_run_close_browser") {
      closeCalls++;
      return null;
    }
  });
  const view = renderAutoRun();

  fireEvent.click(await screen.findByRole("button", { name: "Run #201" }));
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  await screen.findByRole("button", { name: "Run step 1" });

  view.unmount();

  await waitFor(() => expect(closeCalls).toBe(1));
});

/// A pane that was never opened has no browser to close - the unmount
/// cleanup must not call the close command speculatively.
test("does not close the browser on unmount if it was never opened", async () => {
  let closeCalls = 0;
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      const a = args as { caseId: number };
      return a.caseId === 201 ? scriptFor201 : null;
    }
    if (cmd === "auto_run_close_browser") {
      closeCalls++;
      return null;
    }
  });
  const view = renderAutoRun();

  fireEvent.click(await screen.findByRole("button", { name: "Run #201" }));
  await screen.findByRole("button", { name: "Open browser" });

  view.unmount();

  expect(closeCalls).toBe(0);
});

/// Pressing Close already closes the browser. The unmount that follows
/// (the pane leaving the tree once `onClose` fires) must not send a
/// second close command for the same session.
test("does not close the browser twice when Close is pressed then the pane unmounts", async () => {
  let closeCalls = 0;
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      const a = args as { caseId: number };
      return a.caseId === 201 ? scriptFor201 : null;
    }
    if (cmd === "auto_run_open_browser") return null;
    if (cmd === "auto_run_close_browser") {
      closeCalls++;
      return null;
    }
  });
  renderAutoRun();

  fireEvent.click(await screen.findByRole("button", { name: "Run #201" }));
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  await screen.findByRole("button", { name: "Run step 1" });

  fireEvent.click(screen.getByRole("button", { name: "Close" }));

  await waitFor(() => expect(closeCalls).toBe(1));
});

test("past runs list newest first with their verdicts", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "auto_run_list_runs")
      return [
        {
          id: "run-2",
          pbi_id: 42,
          started_at: "1786000200000",
          cases: [
            { case_id: 202, title: "Locked account", verdict: "Passed", note: "", steps: [] },
          ],
        },
        {
          id: "run-1",
          pbi_id: 42,
          started_at: "1786000100000",
          cases: [
            { case_id: 201, title: "Valid login", verdict: "Failed", note: "wrong name", steps: [] },
          ],
        },
      ];
  });
  renderAutoRun();

  expect(await screen.findByText("Locked account")).toBeInTheDocument();
  const rows = await screen.findAllByRole("listitem", { name: /run of/i });
  expect(rows[0]).toHaveTextContent("Passed");
  expect(rows[1]).toHaveTextContent("Failed");
  expect(screen.getByText("wrong name")).toBeInTheDocument();
});

/// Past runs are grouped one block per run, each labelled with the mode it
/// ran in - the word a person needs before deciding whether "Review" even
/// makes sense for it.
test("runs are grouped by run, each labelled with its mode", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "auto_run_list_runs")
      return [
        {
          id: "run-unattended",
          pbi_id: 42,
          started_at: "1786000300000",
          mode: "unattended",
          cases: [{ case_id: 202, title: "Locked account", verdict: "Passed", note: "", steps: [] }],
        },
        {
          id: "run-supervised",
          pbi_id: 42,
          started_at: "1786000100000",
          cases: [{ case_id: 201, title: "Valid login", verdict: "Failed", note: "wrong name", steps: [] }],
        },
      ];
  });
  renderAutoRun();

  expect(await screen.findByText("unattended")).toBeInTheDocument();
  expect(screen.getByText("supervised")).toBeInTheDocument();
});

/// A supervised run was decided by the person watching it live - there is
/// no proposal to review again, so it must never offer the button, whether
/// or not every case ended up with a verdict.
test("a supervised run shows no Review button", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "auto_run_list_runs")
      return [
        {
          id: "run-supervised",
          pbi_id: 42,
          started_at: "1786000100000",
          cases: [{ case_id: 201, title: "Valid login", verdict: "", note: "", steps: [] }],
        },
      ];
  });
  renderAutoRun();

  await screen.findByText("Valid login");
  expect(screen.queryByRole("button", { name: /review/i })).not.toBeInTheDocument();
});

/// An unattended run with a still-unconfirmed case shows a count next to
/// the button, and pressing it opens that run's review dialog directly -
/// this is the same `reviewing` state a finished unattended run lands on
/// by itself.
test("an unattended run with unconfirmed cases offers Review, and pressing it opens that run's review", async () => {
  const unattendedRun = {
    id: "run-unattended",
    pbi_id: 42,
    started_at: "1786000300000",
    mode: "unattended",
    cases: [
      {
        case_id: 202,
        title: "Locked account",
        verdict: "",
        note: "",
        proposed: "Passed",
        reason: "every action of 1 step passed",
        steps: [],
      },
    ],
  };
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "auto_run_list_runs") return [unattendedRun];
    if (cmd === "auto_run_load_run") {
      const a = args as { runId: string };
      return a.runId === unattendedRun.id ? unattendedRun : null;
    }
  });
  renderAutoRun();

  expect(await screen.findByText("1 to review")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Review" }));

  expect(await screen.findByText(/proposed: passed/i)).toBeInTheDocument();
});

/// Once every case has a verdict, the button still offers a way back in -
/// reviewing again is how a person catches their own mistake before Send.
test("a fully confirmed unattended run offers Open review instead", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "auto_run_list_runs")
      return [
        {
          id: "run-unattended",
          pbi_id: 42,
          started_at: "1786000300000",
          mode: "unattended",
          cases: [{ case_id: 202, title: "Locked account", verdict: "Passed", note: "", steps: [] }],
        },
      ];
  });
  renderAutoRun();

  expect(await screen.findByRole("button", { name: "Open review" })).toBeInTheDocument();
  expect(screen.queryByText(/to review/)).not.toBeInTheDocument();
});

/// Once sent, a run is done - no button back into an edit screen that
/// would no longer be allowed to change anything.
test("a sent run shows Sent instead of a review button", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "auto_run_list_runs")
      return [
        {
          id: "run-sent",
          pbi_id: 42,
          started_at: "1786000300000",
          mode: "unattended",
          cases: [{ case_id: 202, title: "Locked account", verdict: "Passed", note: "", steps: [] }],
          published: {
            run_id: 5,
            web_url: "https://dev.azure.com/acme/_testManagement/runs/5",
            at: "1786000400000",
          },
        },
      ];
  });
  renderAutoRun();

  expect(await screen.findByText("Sent")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /review/i })).not.toBeInTheDocument();
});

/// The regrouping must not have dropped anything the flat list used to
/// show - id, title, verdict tone and note all still render on the row.
test("the old flat row content still renders inside its run's group", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "auto_run_list_runs")
      return [
        {
          id: "run-1",
          pbi_id: 42,
          started_at: "1786000100000",
          cases: [{ case_id: 201, title: "Valid login", verdict: "Failed", note: "wrong name", steps: [] }],
        },
      ];
  });
  renderAutoRun();

  const row = (await screen.findByText("Valid login")).closest("li");
  if (!row) throw new Error("row for the case not found");
  expect(within(row).getByText("#201")).toBeInTheDocument();
  expect(within(row).getByText("Failed")).toBeInTheDocument();
  expect(within(row).getByText("wrong name")).toBeInTheDocument();
});

test("no past runs says so rather than showing an empty box", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "auto_run_list_runs") return [];
  });
  renderAutoRun();
  expect(await screen.findByText(/no runs on this machine yet/i)).toBeInTheDocument();
});

/// The past-runs query has a fixed key that nothing else touches. If
/// saving a run doesn't invalidate it, a freshly saved verdict is
/// invisible until the whole screen is left and reopened - the mock
/// below returns a DIFFERENT list after the save than before it, so
/// this only passes if a refetch is actually forced.
test("a saved run appears in past runs without leaving the screen", async () => {
  let runsNow: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      const a = args as { caseId: number };
      return a.caseId === 201 ? scriptFor201 : null;
    }
    if (cmd === "auto_run_list_runs") return runsNow;
    if (cmd === "auto_run_new_id") return "run-1786000000000";
    if (cmd === "auto_run_open_browser") return null;
    if (cmd === "auto_run_step") return [{ ok: true, detail: "page contains Dashboard" }];
    if (cmd === "auto_run_save_run") {
      runsNow = [
        {
          id: "run-1786000000000",
          pbi_id: 42,
          started_at: "1786000000000",
          cases: [
            { case_id: 201, title: "Valid login", verdict: "Failed", note: "", steps: [] },
          ],
        },
      ];
      return null;
    }
    if (cmd === "auto_run_close_browser") return null;
  });
  renderAutoRun();

  await screen.findByText(/no runs on this machine yet/i);

  fireEvent.click(await screen.findByRole("button", { name: "Run #201" }));
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run step 1" }));
  await screen.findByText("page contains Dashboard");
  fireEvent.click(screen.getByRole("button", { name: "Failed" }));
  fireEvent.click(screen.getByRole("button", { name: "Save result" }));

  expect(await screen.findAllByRole("listitem", { name: /run of valid login/i })).toHaveLength(1);
});
