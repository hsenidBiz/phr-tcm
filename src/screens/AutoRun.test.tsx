import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { Toaster } from "../components/ui/toaster";
import { afterEach, expect, test } from "vitest";
import { useBackgroundRunHost } from "../hooks/useBackgroundRunHost";
import { resetBackgroundRun } from "../lib/backgroundRun";
import AutoRun from "./AutoRun";
import ReplayPane from "./AutoRun/ReplayPane";

afterEach(() => {
  resetBackgroundRun();
  clearMocks();
  localStorage.clear();
});

const PBI = { id: 42, title: "Login flow", work_item_type: "Product Backlog Item", project: "Web" };

type TabName = "Test cases" | "Past runs";

/** The screen's tab of that name, whatever count or warning it carries. */
const tab = (name: TabName) => screen.getByRole("tab", { name: new RegExp(`^${name}`) });

/** What App mounts around the screen for an unattended run: its window, and
 * the host that catches its end. The screen is already on the run's PBI, so
 * there is nowhere to take the person. */
function RunHost() {
  useBackgroundRunHost(() => {});
  return <ReplayPane />;
}

/** Renders the screen and opens one tab the way a person would. The screen
 * always opens on Test cases; `null` leaves it there without a click. */
function renderAutoRun(open: TabName | null = "Test cases") {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={qc}>
      <AutoRun org="acme" project="Web" pbi={PBI} />
      <RunHost />
    </QueryClientProvider>,
  );
  if (open) fireEvent.click(tab(open));
  return Object.assign(view, { qc });
}

/** Opens every case card once the cases are listed: a card's Script and
 * Run buttons, and its suspected-defect mark, are on the open card. */
async function expandCards() {
  fireEvent.click(await screen.findByRole("button", { name: "Expand all" }));
}

/** Picks an action from the Test cases tab's More menu, as a person would. */
function chooseFromMore(name: string) {
  fireEvent.click(screen.getByRole("button", { name: "More" }));
  fireEvent.click(screen.getByRole("menuitem", { name }));
}

/** A saved one-step script for case 201, for the tests that open the
 * script window: it is only reachable from a case that already has one. */
const EXISTING_201 = {
  case_id: 201,
  title: "Valid login",
  steps: [{ step_number: 1, actions: [{ kind: "check_text", value: "Dashboard" }] }],
};

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
/// has to say which are drivable rather than looking broken. It says it with
/// the row's buttons, not a badge: a scripted case has View script and a Run
/// button outlined in the success colour; an unscripted one has neither and
/// says an assistant writes its script.
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
  await expandCards();

  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  expect(screen.getByText("Locked account")).toBeInTheDocument();
  const run = await screen.findByRole("button", { name: "Run #201" });
  expect(run).toHaveClass("border-success");
  expect(screen.getByRole("button", { name: "View script for #201" })).toHaveTextContent("View script");
  expect(screen.getByText("No script yet. An assistant writes it.")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /script for #202/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Run #202" })).not.toBeInTheDocument();
  // The badges are gone.
  expect(screen.queryByText("Script ready")).not.toBeInTheDocument();
  expect(screen.queryByText("No script")).not.toBeInTheDocument();
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
  // The note sits with the runs it describes.
  renderAutoRun("Past runs");
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
  await expandCards();
  render(<Toaster />);

  const row = (await screen.findByText("Valid login")).closest("li");
  if (!row) throw new Error("row for case #201 not found");
  expect(within(row).getByText("No script yet. An assistant writes it.")).toBeInTheDocument();
  expect(within(row).queryByRole("button", { name: /script for #201/ })).not.toBeInTheDocument();

  chooseFromMore("Import scripts");

  await waitFor(() => expect(receivedArgs).not.toBeNull());
  expect(receivedArgs).toEqual({ organization: "acme", project: "Web", path: "C:\\scripts.json" });
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

  await screen.findByText("Valid login");
  chooseFromMore("Import scripts");

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

  await screen.findByText("Valid login");
  chooseFromMore("Import scripts");

  expect(await screen.findByText(/could not import that file/i)).toBeInTheDocument();
  // Reopened, the item is pressable again.
  fireEvent.click(screen.getByRole("button", { name: "More" }));
  expect(screen.getByRole("menuitem", { name: "Import scripts" })).toBeEnabled();
});

/// An import is checked against the live app like an assistant's save, and
/// a refusal lists every locator that was never seen, then what to do. The
/// person reads the whole list in the error, not just the first line.
test("an import refused for unseen locators shows every one and what to do", async () => {
  const refusal =
    "Case 8, step 1: button \"Publish\" was never seen on the live app.\n" +
    "Case 9, step 3: button \"Archive\" was never seen on the live app.\n" +
    "Explore the area with discovery so its map holds what these scripts use, then import again.";
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "plugin:dialog|open") return "C:\\scripts.json";
    if (cmd === "auto_run_import_scripts") throw refusal;
  });
  renderAutoRun();
  render(<Toaster />);

  await screen.findByText("Valid login");
  chooseFromMore("Import scripts");

  const shown = await screen.findByText(/could not import that file/i);
  expect(shown.textContent).toContain('Case 8, step 1: button "Publish" was never seen on the live app.');
  expect(shown.textContent).toContain('Case 9, step 3: button "Archive" was never seen on the live app.');
  expect(shown.textContent).toContain("then import again.");
});

/// The script's full detail is JSON, behind Edit script - an assistant
/// writes most of them, and hand-editing is how the format got proven.
/// Invalid JSON must be refused at the point of saving, not written and
/// discovered mid-run.
test("the script editor refuses invalid JSON instead of saving it", async () => {
  let saved = 0;
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return EXISTING_201;
    if (cmd === "auto_run_save_script") {
      saved++;
      return null;
    }
  });
  renderAutoRun();
  await expandCards();

  fireEvent.click(await screen.findByRole("button", { name: "View script for #201" }));
  // The window opens on the readable script; the JSON is behind Edit script.
  fireEvent.click(await screen.findByRole("button", { name: "Edit script" }));
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
    if (cmd === "auto_run_load_script") return EXISTING_201;
    if (cmd === "auto_run_save_script") {
      payload = args as Record<string, unknown>;
      return null;
    }
  });
  renderAutoRun();
  await expandCards();

  fireEvent.click(await screen.findByRole("button", { name: "View script for #201" }));
  // The window opens on the readable script; the JSON is behind Edit script.
  fireEvent.click(await screen.findByRole("button", { name: "Edit script" }));
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

/// The card's script summary and its buttons are driven by the SAME
/// `["autorun-script", caseId]` query the editor reads. A save that never
/// invalidates that key leaves the card showing the old script until the
/// person leaves the section and comes back. `scriptFor201` only changes
/// once the save handler below fires, so this fails without the
/// invalidation (the query would keep serving its cached script forever,
/// `refetchOnWindowFocus` being off).
test("saving a script invalidates its query so the card updates in place", async () => {
  let scriptFor201: unknown = EXISTING_201;
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
        steps: [
          { step_number: 1, actions: [{ kind: "check_text", value: "Dashboard" }] },
          { step_number: 2, actions: [{ kind: "check_text", value: "Welcome" }] },
        ],
      };
      return null;
    }
  });
  renderAutoRun();
  await expandCards();

  const viewButton = await screen.findByRole("button", { name: "View script for #201" });
  const row = viewButton.closest("li");
  if (!row) throw new Error("row for case #201 not found");
  expect(within(row).getByRole("button", { name: "Run #201" })).toBeInTheDocument();
  expect(within(row).getByRole("list", { name: "Script steps of #201" }).children).toHaveLength(1);

  fireEvent.click(viewButton);
  // The window opens on the readable script; the JSON is behind Edit script.
  fireEvent.click(await screen.findByRole("button", { name: "Edit script" }));
  const box = await screen.findByLabelText("Action script JSON");
  fireEvent.change(box, {
    target: {
      value: JSON.stringify([
        { step_number: 1, actions: [{ kind: "check_text", value: "Dashboard" }] },
        { step_number: 2, actions: [{ kind: "check_text", value: "Welcome" }] },
      ]),
    },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save script" }));

  await waitFor(() =>
    expect(within(row).getByRole("list", { name: "Script steps of #201" }).children).toHaveLength(2),
  );
  expect(within(row).getByRole("button", { name: "Run #201" })).toHaveClass("border-success");
});

/// The window opens from View script on a case that already has a script,
/// and the editor reads that script back into its box.
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
  await expandCards();

  fireEvent.click(await screen.findByRole("button", { name: "View script for #201" }));
  // The window opens on the readable script; the JSON is behind Edit script.
  fireEvent.click(await screen.findByRole("button", { name: "Edit script" }));
  const box = (await screen.findByLabelText("Action script JSON")) as HTMLTextAreaElement;
  await waitFor(() => expect(box.value).toContain("check_text"));
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
      return { outcomes: [{ ok: true, detail: "page contains Dashboard" }], tab: null };
  });
  renderAutoRun();
  await expandCards();

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
    if (cmd === "auto_run_step") return { outcomes: [{ ok: false, detail: "not found: #nope" }], tab: null };
  });
  renderAutoRun();
  await expandCards();

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
  await expandCards();
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
    if (cmd === "auto_run_step") return { outcomes: [{ ok: true, detail: "page contains Dashboard" }], tab: null };
    if (cmd === "auto_run_save_run") {
      saved = args as Record<string, unknown>;
      return null;
    }
    if (cmd === "auto_run_close_browser") return null;
    if (cmd === "auto_run_replay") return unattendedFromReplay;
    if (cmd === "auto_run_load_run") return unattendedFromReplay;
  });
  renderAutoRun();
  await expandCards();

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
    "auto_run_load_scripts",
    "auto_run_load_divergences", // read-only, local: the case card's reported divergences
    "auto_run_needs_ruling", // read-only, local: whether a person must rule before the case runs
    "auto_run_new_id",
    "auto_run_open_browser",
    "auto_run_step",
    "auto_run_save_run",
    "auto_run_count_evidence", // local: counts the saved run into the project's quirks file, never ADO
    "auto_run_close_browser",
    "auto_run_list_runs", // read-only: PastRuns' own listing, rendered alongside this screen
    "auto_run_plan", // read-only: the order and reset points, worked out from local scripts
    "auto_run_waiting_reset", // read-only, local: whether an unattended run is paused at a reset point, to show its panel again
    "auto_run_waiting_network", // read-only, local: whether an unattended run is waiting for the company network, to show its panel again
    "auto_run_replay", // local: drives the browser itself, writes nothing to ADO
    "auto_run_list_accounts", // read-only: the Sign in as choices in the unattended run dialog
    "auto_run_expiring_accounts", // read-only: the expiry query for the run window's password warning, never ADO
    "auto_run_load_run", // read-only: the review dialog loading its own run
    "auto_run_load_recipe", // read-only, local: the Setup card and header's site address
    "auto_run_load_nav", // read-only, local: the Setup card's module path count
    "auto_run_load_map", // read-only, local: the Setup card's Discovery line
    "auto_run_load_components", // read-only, local: the Setup card's Components line
    "auto_run_discovery_active", // read-only, local: whether discovery holds the Auto Run browser
    "auto_run_end_discovery", // local: ends the assistant's discovery and closes its browser (End discovery); never ADO
    "auto_run_agents", // read-only, local: the Assistants card's list of connected assistants
    "test_files_list", // read-only, local: the Setup card's Test files count
    "env_list", // read-only, local: the header's environment name and its address
    "db_databases", // read-only, local: the environments list checks its databases against these
    "get_app_settings", // read-only, local: the run dialog's Pause before each action box
    "pw_rules_status", // read-only, local: whether a Playwright repo is set, for the Playwright-is-needed dialog (also refreshes the assistant's local agent files); never ADO
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
  await expandCards();

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
  await expandCards();

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
  await expandCards();

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
  renderAutoRun("Past runs");

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
  renderAutoRun("Past runs");

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
  renderAutoRun("Past runs");

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
  renderAutoRun("Past runs");

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
  renderAutoRun("Past runs");

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
  renderAutoRun("Past runs");

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
  renderAutoRun("Past runs");

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
  renderAutoRun("Past runs");
  expect(await screen.findByText(/no runs on this machine yet/i)).toBeInTheDocument();
});

/// The past-runs query has a fixed key that nothing else touches. If
/// saving a run doesn't invalidate it, a freshly saved verdict is
/// invisible until the whole screen is left and reopened - the mock
/// below returns a DIFFERENT list after the save than before it, so
/// this only passes if a refetch is actually forced. The runs live on
/// their own tab now, and opening it reads them afresh anyway - so the
/// refetch is held by the tab's run count, which stays mounted the whole
/// time the run happens on Test cases.
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
    if (cmd === "auto_run_step") return { outcomes: [{ ok: true, detail: "page contains Dashboard" }], tab: null };
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
  renderAutoRun("Past runs");

  await screen.findByText(/no runs on this machine yet/i);
  await waitFor(() => expect(tab("Past runs")).toHaveAccessibleName("Past runs 0"));

  fireEvent.click(tab("Test cases"));
  await expandCards();
  fireEvent.click(await screen.findByRole("button", { name: "Run #201" }));
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run step 1" }));
  await screen.findByText("page contains Dashboard");
  fireEvent.click(screen.getByRole("button", { name: "Failed" }));
  fireEvent.click(screen.getByRole("button", { name: "Save result" }));

  await waitFor(() => expect(tab("Past runs")).toHaveAccessibleName("Past runs 1"));
  fireEvent.click(tab("Past runs"));
  expect(await screen.findAllByRole("listitem", { name: /run of valid login/i })).toHaveLength(1);
});

/// A script an assistant marked as a suspected application defect shows it
/// on the case row. The mark is a finding, not a change to the script, so
/// the row only reports it and lets a person clear it.
const defectScript = (mark: unknown) => ({
  case_id: 201,
  title: "Valid login",
  steps: [{ step_number: 1, actions: [] }],
  suspected_defect: mark,
});
const MARK = { step_number: 3, note: "The form never shows the lockout message", marked_at: "1786000000000" };

test("a case with a suspected-defect mark shows a badge that names the step and the note", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      return (args as { caseId: number }).caseId === 201 ? defectScript(MARK) : null;
    }
  });
  renderAutoRun();
  await expandCards();

  const badge = (await screen.findByText("Suspected defect")).parentElement!;
  expect(badge).toHaveClass("text-warning");
  expect(badge).toHaveAccessibleDescription(/step 3/i);
  expect(badge).toHaveAccessibleDescription(/never shows the lockout message/);
  expect(badge).toHaveAttribute("title", expect.stringContaining("step 3"));
  expect(badge).toHaveAttribute("title", expect.stringContaining("lockout message"));
  // Only the marked case carries one.
  expect(screen.getAllByText("Suspected defect")).toHaveLength(1);
});

test("a case without a mark shows no badge and no Clear button", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      return (args as { caseId: number }).caseId === 201 ? defectScript(null) : null;
    }
  });
  renderAutoRun();
  await expandCards();

  await screen.findByRole("button", { name: "View script for #201" });
  expect(screen.queryByText("Suspected defect")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /clear suspected defect/i })).not.toBeInTheDocument();
});

test("Clear asks first; confirming clears the mark and the badge goes away", async () => {
  let mark: unknown = MARK;
  const cleared: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      return (args as { caseId: number }).caseId === 201 ? defectScript(mark) : null;
    }
    if (cmd === "auto_run_clear_suspected_defect") {
      cleared.push((args as { caseId: number }).caseId);
      mark = null;
      return null;
    }
  });
  renderAutoRun();
  await expandCards();

  await screen.findByText("Suspected defect");
  fireEvent.click(screen.getByRole("button", { name: "Clear suspected defect for #201" }));
  // Nothing is cleared until it is confirmed.
  expect(cleared).toEqual([]);
  expect(screen.getByRole("button", { name: "Keep" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Clear" }));

  await waitFor(() => expect(screen.queryByText("Suspected defect")).not.toBeInTheDocument());
  expect(cleared).toEqual([201]);
  expect(screen.queryByRole("button", { name: "Keep" })).not.toBeInTheDocument();
});

test("Keep leaves the mark and calls nothing", async () => {
  const calls: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      return (args as { caseId: number }).caseId === 201 ? defectScript(MARK) : null;
    }
    if (cmd === "auto_run_clear_suspected_defect") calls.push(cmd);
  });
  renderAutoRun();
  await expandCards();

  await screen.findByText("Suspected defect");
  fireEvent.click(screen.getByRole("button", { name: "Clear suspected defect for #201" }));
  fireEvent.click(screen.getByRole("button", { name: "Keep" }));

  expect(screen.queryByRole("button", { name: "Keep" })).not.toBeInTheDocument();
  expect(screen.getByText("Suspected defect")).toBeInTheDocument();
  expect(calls).toEqual([]);
});

test("a failed Clear says why inline and keeps the badge", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      return (args as { caseId: number }).caseId === 201 ? defectScript(MARK) : null;
    }
    if (cmd === "auto_run_clear_suspected_defect") throw "Could not write the script file.";
  });
  renderAutoRun();
  await expandCards();

  await screen.findByText("Suspected defect");
  fireEvent.click(screen.getByRole("button", { name: "Clear suspected defect for #201" }));
  fireEvent.click(screen.getByRole("button", { name: "Clear" }));

  const problem = await screen.findByRole("status");
  expect(problem).toHaveTextContent("Could not write the script file.");
  expect(problem).toHaveClass("text-danger");
  expect(screen.getByText("Suspected defect")).toBeInTheDocument();
});

/// A run that passes the marked step clears the mark on disk, and the run's
/// own reason says so. The rows read the script through their own queries,
/// so a finished run has to make them read it again - otherwise the row
/// keeps saying "Suspected defect" beside a run that says it was cleared.
test("a finished unattended run refreshes the rows, so a mark it cleared stops showing", async () => {
  let ran = false;
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      const a = args as { caseId: number };
      if (a.caseId === 201) return scriptFor201;
      if (a.caseId === 202) return { ...scriptFor202, suspected_defect: ran ? null : MARK };
      return null;
    }
    if (cmd === "auto_run_replay") {
      ran = true;
      return unattendedFromReplay;
    }
    // The review it lands in never loads, so nothing but the finished
    // run itself can make the rows read their scripts again.
    if (cmd === "auto_run_load_run") return new Promise(() => {});
  });
  renderAutoRun();
  await expandCards();

  await screen.findByText("Suspected defect");
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #202" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run 1 unattended" }));
  fireEvent.click(await screen.findByRole("button", { name: "Start" }));
  await waitFor(() => expect(ran).toBe(true));

  await waitFor(() => expect(screen.queryByText("Suspected defect")).not.toBeInTheDocument());
});

test("a saved supervised run refreshes the rows, so a mark it cleared stops showing", async () => {
  let counted = false;
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      const a = args as { caseId: number };
      if (a.caseId === 201) return { ...scriptFor201, suspected_defect: counted ? null : MARK };
      return null;
    }
    if (cmd === "auto_run_new_id") return "run-1786000000000";
    if (cmd === "auto_run_open_browser") return null;
    if (cmd === "auto_run_step") return { outcomes: [{ ok: true, detail: "page contains Dashboard" }], tab: null };
    if (cmd === "auto_run_save_run") return null;
    if (cmd === "auto_run_count_evidence") {
      counted = true;
      return null;
    }
    if (cmd === "auto_run_close_browser") return null;
  });
  renderAutoRun();
  await expandCards();

  await screen.findByText("Suspected defect");
  fireEvent.click(await screen.findByRole("button", { name: "Run #201" }));
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run step 1" }));
  await screen.findByText("page contains Dashboard");
  fireEvent.click(screen.getByRole("button", { name: "Passed" }));
  fireEvent.click(screen.getByRole("button", { name: "Save result" }));

  await waitFor(() => expect(screen.queryByText("Suspected defect")).not.toBeInTheDocument());
});

/// The unattended run dialog shows each picked case's written steps - the
/// same steps the script editor lists, handed over by this screen.
test("the unattended run dialog can show a picked case's written steps", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return scriptFor202;
    if (cmd === "auto_run_replay") return new Promise(() => {});
    return null;
  });
  renderAutoRun();

  fireEvent.click(await screen.findByRole("checkbox", { name: "Select #202" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run 1 unattended" }));
  fireEvent.click(await screen.findByRole("button", { name: "Start" }));

  fireEvent.click(await screen.findByRole("button", { name: "Show steps for #202" }));
  const steps = await screen.findByRole("list", { name: "Steps of #202" });
  expect(within(steps).getByText(/Sign in/)).toBeInTheDocument();
  expect(within(steps).getByText(/A lockout message appears/)).toBeInTheDocument();
});

// ---- Tabs: Test cases and Past runs, and the Setup panel ----

/** An environment with a site address of its own, using database "hr". */
const envWith = (start_url: string) => ({
  active: "qa",
  environments: [
    {
      id: "qa",
      name: "QA",
      start_url,
      allowed_origins: [],
      db_id: "hr",
      test_environment: false,
      has_default_password: false,
    },
  ],
});
const ONE_ACCOUNT = [{ key: "a", label: "A", username: "u", password: "p" }];
const HR_DB = {
  id: "hr",
  label: "QA HR",
  shipped: true,
  server: "sql01",
  port: null,
  database: "hrdb",
  user: "reader",
  trust_cert: false,
  has_password: true,
  overridden: false,
};

/** A project that is ready to run unless `extra` answers otherwise: a site
 * address on the active environment, the built-in sign-in (no saved
 * recipe) and one account. `extra` answering `undefined` falls through. */
function mockReady(extra?: (cmd: string, args: unknown) => unknown) {
  mockIPC((cmd, args) => {
    const answer = extra?.(String(cmd), args);
    if (answer !== undefined) return answer;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "auto_run_list_runs") return [];
    if (cmd === "env_list") return envWith("https://qa.example.com/");
    if (cmd === "db_databases") return [HR_DB];
    if (cmd === "auto_run_list_accounts") return ONE_ACCOUNT;
    if (cmd === "auto_run_load_recipe") return null;
    return null;
  });
}

/** The Setup panel beside the case list. */
const setupPanel = () => screen.getByRole("region", { name: "Setup" });

/** One line of the shut Setup panel's summary, by its label. */
const summaryLine = (label: string) =>
  within(within(setupPanel()).getByRole("list", { name: "Setup summary" }))
    .getByText(label)
    .closest("li")!;

test("the two sections are one tab list, the screen opens on Test cases, and the Setup panel waits for the setup to load", async () => {
  let answerAccounts: (v: unknown) => void = () => {};
  mockReady((cmd) =>
    cmd === "auto_run_list_accounts" ? new Promise((r) => (answerAccounts = r)) : undefined,
  );
  renderAutoRun(null);

  const list = screen.getByRole("tablist", { name: "Auto Run sections" });
  expect(within(list).getAllByRole("tab").map((t) => t.textContent?.replace(/\s*\d+$/, ""))).toEqual([
    "Test cases",
    "Past runs",
  ]);
  expect(tab("Test cases")).toHaveAttribute("aria-selected", "true");
  // Still loading is not "missing": the panel shows its summary, unflagged.
  await screen.findByRole("tab", { name: /^Past runs 0/ });
  expect(within(setupPanel()).getByRole("button", { name: "Show setup details" })).toHaveAttribute(
    "aria-expanded",
    "false",
  );
  expect(within(setupPanel()).queryByText("Needs attention")).not.toBeInTheDocument();

  answerAccounts(ONE_ACCOUNT);
  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  // The case count rides on its tab.
  await waitFor(() => expect(tab("Test cases")).toHaveAccessibleName("Test cases 2"));
  // Everything a run needs is there, so the panel stays a summary.
  await waitFor(() => expect(within(setupPanel()).getByText("1 account on this machine")).toBeInTheDocument());
  expect(within(setupPanel()).getByRole("button", { name: "Show setup details" })).toBeInTheDocument();
});

test("with no site address the Setup panel starts open and says it needs attention", async () => {
  mockReady((cmd) => (cmd === "env_list" ? envWith("") : undefined));
  renderAutoRun(null);

  await waitFor(() =>
    expect(within(setupPanel()).getByRole("button", { name: "Hide setup details" })).toHaveAttribute(
      "aria-expanded",
      "true",
    ),
  );
  expect(within(setupPanel()).getByText("Needs attention")).toBeInTheDocument();
  // It says what for, right beside it.
  expect(within(setupPanel()).getByText("Needs attention").parentElement).toHaveTextContent(
    /^Needs attention: no site address$/,
  );
  expect(within(setupPanel()).getByRole("button", { name: "Edit site address" })).toBeInTheDocument();
  // The screen stays on the cases: the panel is beside them.
  expect(tab("Test cases")).toHaveAttribute("aria-selected", "true");
});

test("with no accounts the Setup panel starts open", async () => {
  mockReady((cmd) => (cmd === "auto_run_list_accounts" ? [] : undefined));
  renderAutoRun(null);
  await waitFor(() =>
    expect(within(setupPanel()).getByRole("button", { name: "Hide setup details" })).toBeInTheDocument(),
  );
  expect(within(setupPanel()).getByText("Needs attention").parentElement).toHaveTextContent(
    /^Needs attention: no accounts$/,
  );
  expect(within(setupPanel()).getByRole("button", { name: "Edit accounts" })).toBeInTheDocument();
});

test("with a site address, a sign-in and an account the Setup panel starts as a summary", async () => {
  mockReady();
  renderAutoRun(null);
  await expandCards();

  expect(tab("Test cases")).toHaveAttribute("aria-selected", "true");
  expect((await screen.findAllByText("No script yet. An assistant writes it.")).length).toBeGreaterThan(0);
  const summary = within(setupPanel()).getByRole("list", { name: "Setup summary" });
  expect(await within(summary).findByText("https://qa.example.com/")).toBeInTheDocument();
  // The address is shown whole - never cut short with an ellipsis, and not
  // left to a hover tooltip. It breaks inside the URL if it must wrap.
  const address = within(summary).getByText("https://qa.example.com/");
  expect(address).not.toHaveClass("truncate");
  expect(address).not.toHaveAttribute("title");
  // It wraps at its own punctuation (a <wbr> after each / . ? & =), never
  // in the middle of a word.
  expect(address).toHaveClass("break-words");
  expect(address).not.toHaveClass("break-all");
  expect(address.querySelectorAll("wbr").length).toBeGreaterThan(0);
  expect(within(summary).getByText("Built-in")).toBeInTheDocument();
  expect(within(summary).getByText("1 account on this machine")).toBeInTheDocument();
  expect(await within(summary).findByText("QA HR")).toBeInTheDocument();
  // Each line says its state in words too: the dot is only a look.
  expect(within(summary).getByText("Accounts").closest("li")).toHaveTextContent(/, ready$/);
  expect(within(setupPanel()).queryByText("Needs attention")).not.toBeInTheDocument();
  // The full rows are one press away, and the past runs are on their tab.
  expect(screen.queryByRole("button", { name: "Edit site address" })).not.toBeInTheDocument();
  expect(screen.queryByRole("group", { name: "Filter by result" })).not.toBeInTheDocument();

  // The toggle's visible words are its whole accessible name.
  expect(within(setupPanel()).getByRole("button", { name: "Show setup details" })).toHaveTextContent(
    /^Show setup details$/,
  );
  fireEvent.click(within(setupPanel()).getByRole("button", { name: "Show setup details" }));
  expect(within(setupPanel()).getByRole("button", { name: "Edit site address" })).toBeInTheDocument();
  expect(within(setupPanel()).getByRole("button", { name: "Hide setup details" })).toHaveTextContent(
    /^Hide setup details$/,
  );
  expect(within(setupPanel()).queryByRole("list", { name: "Setup summary" })).not.toBeInTheDocument();
  fireEvent.click(within(setupPanel()).getByRole("button", { name: "Hide setup details" }));
  expect(within(setupPanel()).getByRole("list", { name: "Setup summary" })).toBeInTheDocument();
});

test("the full Setup rows keep their status dot and spoken status", async () => {
  mockReady((cmd) => (cmd === "auto_run_list_accounts" ? [] : undefined));
  renderAutoRun(null);
  // No account: the panel opens by itself, so these are the full rows.
  await waitFor(() =>
    expect(within(setupPanel()).getByRole("button", { name: "Hide setup details" })).toBeInTheDocument(),
  );
  const rows = [
    "Environment",
    "Site address",
    "Sign-in",
    "Accounts",
    "Areas",
    "Discovery",
    "Components",
    "Test files",
  ];
  for (const name of rows) {
    const row = within(setupPanel()).getByRole("group", { name });
    const dot = row.querySelector("[data-status-dot]");
    expect(dot, `${name} has a status dot`).not.toBeNull();
    // The dot is a look only: hidden from a screen reader.
    expect(dot!.closest("[aria-hidden]")).not.toBeNull();
  }
  // The same dot and words as the summary line: no account is missing.
  const accounts = within(setupPanel()).getByRole("group", { name: "Accounts" });
  expect(accounts.querySelector("[data-status-dot]")).toHaveClass("bg-danger");
  expect(accounts).toHaveTextContent("Accounts, missing");
  // A site address that is set is ready.
  const site = within(setupPanel()).getByRole("group", { name: "Site address" });
  expect(site.querySelector("[data-status-dot]")).toHaveClass("bg-success");
  expect(site).toHaveTextContent("Site address, ready");
});

test("the Setup card is held to the window in the two-column layout so its toggle stays reachable", async () => {
  mockReady((cmd) => (cmd === "auto_run_list_accounts" ? [] : undefined));
  renderAutoRun(null);
  const toggle = await within(setupPanel()).findByRole("button", { name: "Hide setup details" });
  const card = setupPanel();
  // At most the visible area, only beside the list: stacked it has no limit.
  expect(card.className).toContain("@min-[69rem]:max-h-[var(--follow-room,calc(100vh_-_6rem))]");
  expect(card.className).not.toMatch(/(^|\s)max-h-/);
  expect(card).toHaveClass("flex", "flex-col");
  // The header row (title, Needs attention, the toggle) wraps and never shrinks away.
  const header = toggle.parentElement!;
  expect(header).toHaveClass("flex-wrap", "shrink-0");
  expect(header).toContainElement(screen.getByRole("heading", { name: "Setup" }));
  // The body is what scrolls, and the toggle is not inside it.
  const body = document.getElementById(toggle.getAttribute("aria-controls")!)!;
  expect(body).toHaveClass("overflow-auto", "min-h-0");
  expect(body).not.toContainElement(toggle);
  expect(card).toContainElement(body);
});

test("a summary line with something missing says so in words", async () => {
  mockReady((cmd) => (cmd === "auto_run_list_accounts" ? [] : undefined));
  renderAutoRun(null);
  // Opened by itself: shut it to read the summary.
  fireEvent.click(await within(setupPanel()).findByRole("button", { name: "Hide setup details" }));
  const summary = within(setupPanel()).getByRole("list", { name: "Setup summary" });
  expect(within(summary).getByText("Accounts").closest("li")).toHaveTextContent("AccountsNone yet, missing");
});

/// A setup read that fails is shown on its row; it is not "missing", so it
/// does not open the panel by itself.
test("a setup read that fails still lets the screen open, with the panel a summary", async () => {
  mockReady((cmd) => {
    if (cmd === "auto_run_list_accounts") throw new Error("disk read failed");
    return undefined;
  });
  renderAutoRun(null);
  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  expect(tab("Test cases")).toHaveAttribute("aria-selected", "true");
  await within(setupPanel()).findByText("Needs attention");
  expect(within(setupPanel()).getByRole("button", { name: "Show setup details" })).toBeInTheDocument();
});

/// ...but it must not vanish from what the screen shows. The panel's
/// summary says what could not be read, and the panel carries the flag.
test("a failed accounts read shows in the Setup summary and flags the panel", async () => {
  mockReady((cmd) => {
    if (cmd === "auto_run_list_accounts") throw new Error("disk read failed");
    return undefined;
  });
  renderAutoRun(null);

  await within(setupPanel()).findByText("Needs attention");
  expect(summaryLine("Accounts")).toHaveTextContent("AccountsCould not be read, needs a look");
  // What is known is still said.
  expect(summaryLine("Environment")).toHaveTextContent("EnvironmentQA, ready");
  expect(summaryLine("Site address")).toHaveTextContent("https://qa.example.com/");
  // Flagging is not opening: the panel is still a summary.
  expect(within(setupPanel()).getByRole("button", { name: "Show setup details" })).toBeInTheDocument();
  expect(tab("Test cases")).toHaveAttribute("aria-selected", "true");
});

test("a failed recipe read shows in the Setup summary and flags the panel", async () => {
  mockReady((cmd) => {
    if (cmd === "env_list") return envWith("");
    if (cmd === "auto_run_load_recipe") throw new Error("bad recipe");
    return undefined;
  });
  renderAutoRun(null);

  await within(setupPanel()).findByText("Needs attention");
  expect(summaryLine("Sign-in")).toHaveTextContent("Could not be read");
  // An address that may live in the unreadable recipe is not "missing".
  expect(summaryLine("Site address")).toHaveTextContent("Could not be read");
  expect(summaryLine("Site address")).not.toHaveTextContent("Not set up yet");
});

test("a failed environments read shows in the Setup summary and flags the panel", async () => {
  mockReady((cmd) => {
    if (cmd === "env_list") throw new Error("no environments");
    return undefined;
  });
  renderAutoRun(null);

  await within(setupPanel()).findByText("Needs attention");
  expect(summaryLine("Environment")).toHaveTextContent("EnvironmentCould not be read, needs a look");
  expect(summaryLine("Site address")).not.toHaveTextContent("Not set up yet");
});

test("a failed test files read shows in the Setup summary", async () => {
  mockReady((cmd) => {
    if (cmd === "test_files_list") throw new Error("folder gone");
    return undefined;
  });
  renderAutoRun(null);
  await waitFor(() => expect(summaryLine("Test files")).toHaveTextContent("Could not be read, needs a look"));
});

test("while the setup is only loading the Setup summary says so and flags nothing", async () => {
  mockReady((cmd) => (cmd === "auto_run_list_accounts" ? new Promise(() => {}) : undefined));
  renderAutoRun(null);
  await screen.findByRole("tab", { name: /^Past runs 0/ });
  expect(summaryLine("Accounts")).toHaveTextContent("AccountsLoading…");
  expect(within(setupPanel()).queryByText("Needs attention")).not.toBeInTheDocument();
});

test("a panel the person opens or shuts while the setup loads is never overridden by the opening rule", async () => {
  let answerAccounts: (v: unknown) => void = () => {};
  mockReady((cmd) =>
    cmd === "auto_run_list_accounts" ? new Promise((r) => (answerAccounts = r)) : undefined,
  );
  renderAutoRun(null);

  // Opened, then shut again, before the setup has answered.
  fireEvent.click(within(setupPanel()).getByRole("button", { name: "Show setup details" }));
  fireEvent.click(within(setupPanel()).getByRole("button", { name: "Hide setup details" }));
  // No accounts would open the panel - but the person has already chosen.
  answerAccounts([]);
  await waitFor(() => expect(within(setupPanel()).getByText("Needs attention")).toBeInTheDocument());
  expect(within(setupPanel()).getByRole("button", { name: "Show setup details" })).toBeInTheDocument();
});

test("the panel's starting state is decided once: setup that changes later does not open it", async () => {
  let accounts: unknown = ONE_ACCOUNT;
  mockReady((cmd) => (cmd === "auto_run_list_accounts" ? accounts : undefined));
  const { qc } = renderAutoRun(null);
  await waitFor(() => expect(summaryLine("Accounts")).toHaveTextContent(/, ready$/));
  expect(within(setupPanel()).getByRole("button", { name: "Show setup details" })).toBeInTheDocument();

  // The last account removed (from the Accounts dialog, say).
  accounts = [];
  await qc.invalidateQueries({ queryKey: ["autorun-accounts"] });
  await waitFor(() => expect(within(setupPanel()).getByText("Needs attention")).toBeInTheDocument());
  expect(within(setupPanel()).getByRole("button", { name: "Show setup details" })).toBeInTheDocument();
});

test("the arrow keys, Home and End move between the tabs", async () => {
  mockReady();
  renderAutoRun("Test cases");

  fireEvent.keyDown(tab("Test cases"), { key: "ArrowRight" });
  expect(tab("Past runs")).toHaveAttribute("aria-selected", "true");
  expect(tab("Past runs")).toHaveFocus();
  // Only the chosen tab is in the Tab order.
  expect(tab("Past runs")).toHaveAttribute("tabindex", "0");
  expect(tab("Test cases")).toHaveAttribute("tabindex", "-1");

  // Two tabs: the arrows wrap from either end to the other.
  fireEvent.keyDown(tab("Past runs"), { key: "ArrowRight" });
  expect(tab("Test cases")).toHaveAttribute("aria-selected", "true");
  fireEvent.keyDown(tab("Test cases"), { key: "ArrowLeft" });
  expect(tab("Past runs")).toHaveAttribute("aria-selected", "true");
  fireEvent.keyDown(tab("Past runs"), { key: "Home" });
  expect(tab("Test cases")).toHaveAttribute("aria-selected", "true");
  fireEvent.keyDown(tab("Test cases"), { key: "End" });
  expect(tab("Past runs")).toHaveAttribute("aria-selected", "true");
  fireEvent.keyDown(tab("Past runs"), { key: "Home" });
  expect(tab("Test cases")).toHaveFocus();
});

test("Past runs shows the past runs at full width, with their result filters", async () => {
  // The filters show once there is a run to filter.
  mockReady((cmd) => (cmd === "auto_run_list_runs" ? [unattendedFromReplay] : undefined));
  renderAutoRun("Past runs");

  const panel = screen.getByRole("tabpanel", { name: /^Past runs/ });
  expect(await within(panel).findByRole("group", { name: "Filter by result" })).toBeInTheDocument();
  expect(within(panel).getByRole("button", { name: "Clear results" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Add script for #201" })).not.toBeInTheDocument();
  // The xl two-column layout is gone.
  expect(document.querySelector('[class*="xl:grid-cols-"]')).toBeNull();
});

test("closing a review lands on Past runs, even one an unattended run opened from Test cases", async () => {
  mockReady((cmd, args) => {
    if (cmd === "auto_run_load_script") {
      return (args as { caseId: number }).caseId === 202 ? scriptFor202 : null;
    }
    if (cmd === "auto_run_replay") return unattendedFromReplay;
    if (cmd === "auto_run_list_runs") return [unattendedFromReplay];
    // A run that is not on this machine any more: the review's short form,
    // with its own Close.
    if (cmd === "auto_run_load_run") return null;
    return undefined;
  });
  renderAutoRun(null);
  await waitFor(() => expect(tab("Test cases")).toHaveAttribute("aria-selected", "true"));

  fireEvent.click(await screen.findByRole("checkbox", { name: "Select #202" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run 1 unattended" }));
  fireEvent.click(await screen.findByRole("button", { name: "Start" }));
  expect(await screen.findByText("This run is no longer on this machine.")).toBeInTheDocument();
  expect(tab("Test cases")).toHaveAttribute("aria-selected", "true");

  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  await waitFor(() => expect(tab("Past runs")).toHaveAttribute("aria-selected", "true"));
  expect(await screen.findByRole("group", { name: "Filter by result" })).toBeInTheDocument();
});

test("the Setup panel shows the environment and its database, all six rows, and the assistant's setup command", async () => {
  mockReady();
  renderAutoRun("Test cases");
  fireEvent.click(await within(setupPanel()).findByRole("button", { name: "Show setup details" }));

  const panel = setupPanel();
  // Read-only here: the environment and its database change on AI Bridge.
  expect(await within(panel).findByText("QA HR: hrdb on sql01")).toBeInTheDocument();
  expect(within(panel).getByText(/AI Bridge tab/)).toBeInTheDocument();
  for (const name of [
    "Edit site address",
    "Record sign-in",
    "Edit sign-in recipe",
    "Edit accounts",
    "Edit areas",
    "Manage test files",
  ]) {
    expect(within(panel).getByRole("button", { name })).toBeInTheDocument();
  }
  for (const name of ["Site address", "Sign-in", "Accounts", "Areas", "Test files"]) {
    expect(within(panel).getByRole("group", { name })).toBeInTheDocument();
  }
  expect(within(panel).getByText(/\/tcm:setup/)).toBeInTheDocument();
  expect(await within(panel).findByText("https://qa.example.com/")).toBeInTheDocument();
  expect(within(panel).getByText("Built-in")).toBeInTheDocument();
});

// ---- The Test cases tab's header, the toolbar's search, and the More menu ----

const uploadScript = (caseId: number, file: string) => ({
  case_id: caseId,
  title: "s",
  steps: [{ step_number: 1, actions: [{ kind: "upload", selector: { css: "#f" }, file }] }],
});

test("Test cases opens on its filters, with no second title and no environment line; the Setup panel names the environment", async () => {
  mockReady((cmd, args) => {
    if (cmd === "auto_run_load_script") {
      return (args as { caseId: number }).caseId === 201 ? uploadScript(201, "cv.txt") : null;
    }
    if (cmd === "test_files_list") return [{ name: "appraisal.pdf", size: 1, modified: "1" }];
    if (cmd === "auto_run_load_nav") {
      return { direct_urls: true, modules: [{ module: "Leave", clicks: [], arrived: "", recorded: "" }] };
    }
    return undefined;
  });
  renderAutoRun(null);

  await screen.findByText("Valid login");
  // The tab's own label carries the count; no "Test cases (2)" under it.
  expect(tab("Test cases")).toHaveAccessibleName("Test cases 2");
  expect(screen.queryByRole("heading", { name: /^Test cases/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("group", { name: "Readiness" })).not.toBeInTheDocument();
  // The environment, and the file a script uploads that the folder lacks,
  // are the Setup panel's to say.
  await waitFor(() => expect(summaryLine("Environment")).toHaveTextContent("EnvironmentQA, ready"));
  await waitFor(() => expect(summaryLine("Test files")).toHaveTextContent("1 file missing: cv.txt, needs a look"));
  // A run would fail at that upload: the panel says so in its header too,
  // with the panel still shut.
  expect(within(setupPanel()).getByText("Needs attention")).toBeInTheDocument();
  expect(within(setupPanel()).getByRole("button", { name: "Show setup details" })).toBeInTheDocument();
  // The old line's "Project <name>" is gone.
  expect(screen.queryByText("Web")).not.toBeInTheDocument();
  expect(screen.queryByText(/^Project\b/)).not.toBeInTheDocument();
});

test("missing test files are named in the summary (the first few) and in the open Test files row (all of them)", async () => {
  const files = ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"];
  mockReady((cmd, args) => {
    if (cmd === "auto_run_load_script") {
      return (args as { caseId: number }).caseId === 201
        ? {
            case_id: 201,
            title: "s",
            steps: files.map((file, i) => ({
              step_number: i + 1,
              actions: [{ kind: "upload", selector: { css: "#f" }, file }],
            })),
          }
        : null;
    }
    if (cmd === "test_files_list") return [{ name: "appraisal.pdf", size: 1, modified: "1" }];
    return undefined;
  });
  renderAutoRun(null);

  await waitFor(() =>
    expect(summaryLine("Test files")).toHaveTextContent("5 files missing: a.txt, b.txt, c.txt, and 2 more"),
  );
  expect(within(setupPanel()).getByText("Needs attention")).toBeInTheDocument();

  fireEvent.click(within(setupPanel()).getByRole("button", { name: "Show setup details" }));
  const row = within(setupPanel()).getByRole("group", { name: "Test files" });
  expect(row).toHaveTextContent("1 file");
  expect(row).toHaveTextContent("5 files missing: a.txt, b.txt, c.txt, d.txt, e.txt");
  expect(within(setupPanel()).getByText("Needs attention")).toBeInTheDocument();
});

test("a file the scripts upload that is in the Test files folder is not missing", async () => {
  mockReady((cmd, args) => {
    if (cmd === "auto_run_load_script") {
      return (args as { caseId: number }).caseId === 201 ? uploadScript(201, "CV.txt") : null;
    }
    if (cmd === "test_files_list") return [{ name: "cv.txt", size: 1, modified: "1" }];
    return undefined;
  });
  renderAutoRun(null);
  await waitFor(() => expect(summaryLine("Test files")).toHaveTextContent("1 file"));
  expect(summaryLine("Test files")).not.toHaveTextContent(/missing/);
  expect(within(setupPanel()).queryByText("Needs attention")).not.toBeInTheDocument();
});

test("the expanded Setup panel names the environment in a row of its own", async () => {
  mockReady();
  renderAutoRun(null);
  fireEvent.click(await within(setupPanel()).findByRole("button", { name: "Show setup details" }));
  const env = within(setupPanel()).getByRole("group", { name: "Environment" });
  expect(await within(env).findByText("QA HR: hrdb on sql01")).toBeInTheDocument();
  expect(within(env).getByText("QA")).toBeInTheDocument();
  // Chosen on AI Bridge: nothing to press here.
  expect(within(env).queryByRole("button")).not.toBeInTheDocument();
});

test("the search is in the Test cases tab's toolbar and not in the Setup panel; going to Past runs empties it", async () => {
  mockReady();
  renderAutoRun("Test cases");
  await screen.findByText("Valid login");
  // The Setup panel has no search; the box sits in the tab's own toolbar,
  // level with Group by title and Select all shown.
  expect(within(setupPanel()).queryByRole("textbox", { name: "Search test cases" })).not.toBeInTheDocument();
  const box = screen.getByRole("textbox", { name: "Search test cases" });
  const toolbar = box.closest("[data-list-toolbar]") as HTMLElement;
  expect(toolbar).not.toBeNull();
  expect(setupPanel()).not.toContainElement(box);
  expect(within(toolbar).getByRole("switch", { name: "Group by title" })).toBeInTheDocument();
  expect(within(toolbar).getByRole("checkbox", { name: "Select all shown" })).toBeInTheDocument();
  expect(within(toolbar).getByRole("button", { name: "More" })).toBeInTheDocument();
  expect(box).toHaveAttribute("placeholder", "Search by id, title, tag, module…");

  fireEvent.change(box, { target: { value: "lock" } });
  expect(await screen.findByText("Locked account")).toBeInTheDocument();
  expect(screen.queryByText("Valid login")).not.toBeInTheDocument();
  // The tab's count says how many of the cases show.
  expect(tab("Test cases")).toHaveAccessibleName("Test cases 1 of 2");

  // Going to Past runs by its tab empties the search: it only ever narrows
  // the case list, and left there it would read as filtering runs. The
  // Past runs tab has no search box of its own.
  fireEvent.click(tab("Past runs"));
  expect(tab("Past runs")).toHaveAttribute("aria-selected", "true");
  expect(screen.queryByRole("textbox", { name: "Search test cases" })).not.toBeInTheDocument();
  expect(within(setupPanel()).queryByRole("textbox", { name: "Search test cases" })).not.toBeInTheDocument();
  // The same by the keyboard: back to Test cases, search, then the arrow.
  fireEvent.click(tab("Test cases"));
  expect(screen.getByRole("textbox", { name: "Search test cases" })).toHaveValue("");
  fireEvent.change(screen.getByRole("textbox", { name: "Search test cases" }), { target: { value: "lock" } });
  fireEvent.keyDown(tab("Test cases"), { key: "ArrowRight" });
  expect(tab("Past runs")).toHaveAttribute("aria-selected", "true");
  // Back on Test cases every case shows again.
  fireEvent.click(tab("Test cases"));
  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Search test cases" })).toHaveValue("");
  expect(tab("Test cases")).toHaveAccessibleName("Test cases 2");
});

test("More holds Import scripts, with its description, and Clear scripts; Group by title stays outside", async () => {
  mockReady();
  renderAutoRun("Test cases");
  await screen.findByText("Valid login");

  // Nothing rare sits on the toolbar.
  expect(screen.queryByRole("button", { name: "Import scripts" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Clear scripts" })).not.toBeInTheDocument();
  expect(screen.getByRole("switch", { name: "Group by title" })).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "More" }));
  const menu = screen.getByRole("menu", { name: "More" });
  const imp = within(menu).getByRole("menuitem", { name: "Import scripts" });
  expect(imp).toHaveAccessibleDescription("One JSON file can carry every case in this PBI.");
  expect(within(menu).getByRole("menuitem", { name: "Clear scripts" })).toBeInTheDocument();
  // Group by title is not in the menu.
  expect(within(menu).queryByText("Group by title")).not.toBeInTheDocument();
});

test("Clear scripts in More is disabled with no script, and still asks first when there is one", async () => {
  mockReady((cmd, args) => {
    if (cmd === "auto_run_load_script") {
      return (args as { caseId: number }).caseId === 202 ? scriptFor202 : null;
    }
    return undefined;
  });
  renderAutoRun("Test cases");
  await expandCards();
  await screen.findByRole("button", { name: "Run #202" });
  fireEvent.click(screen.getByRole("button", { name: "More" }));
  const clear = screen.getByRole("menuitem", { name: "Clear scripts" });
  expect(clear).toBeEnabled();
  fireEvent.click(clear);
  // The menu closes, and the existing confirm opens.
  expect(await screen.findByRole("heading", { name: "Clear scripts?" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Clear 2 scripts" })).toBeInTheDocument();
});

test("Clear scripts in More is disabled when no case has a script", async () => {
  mockReady();
  renderAutoRun("Test cases");
  await expandCards();
  // Wait for the cards to show (no script: the line saying so), or the
  // item would be disabled merely because they are still pending.
  expect(await screen.findAllByText("No script yet. An assistant writes it.")).toHaveLength(2);
  fireEvent.click(screen.getByRole("button", { name: "More" }));
  expect(screen.getByRole("menuitem", { name: "Clear scripts" })).toBeDisabled();
});

// ---- Follow-ups from the tabs review ----

test("closing a review puts focus on the Past runs tab, not on the body", async () => {
  mockReady((cmd, args) => {
    if (cmd === "auto_run_load_script") {
      return (args as { caseId: number }).caseId === 202 ? scriptFor202 : null;
    }
    if (cmd === "auto_run_replay") return unattendedFromReplay;
    if (cmd === "auto_run_list_runs") return [unattendedFromReplay];
    if (cmd === "auto_run_load_run") return null;
    return undefined;
  });
  renderAutoRun(null);
  await waitFor(() => expect(tab("Test cases")).toHaveAttribute("aria-selected", "true"));

  fireEvent.click(await screen.findByRole("checkbox", { name: "Select #202" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run 1 unattended" }));
  fireEvent.click(await screen.findByRole("button", { name: "Start" }));
  await screen.findByText("This run is no longer on this machine.");

  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  await waitFor(() => expect(tab("Past runs")).toHaveAttribute("aria-selected", "true"));
  await waitFor(() => expect(tab("Past runs")).toHaveFocus());
});

test("arrow keys pressed with Alt, Ctrl or Meta are left alone, so Alt+Left still means back", async () => {
  mockReady();
  renderAutoRun("Test cases");
  for (const mod of [{ altKey: true }, { ctrlKey: true }, { metaKey: true }]) {
    for (const key of ["ArrowLeft", "ArrowRight", "Home", "End"]) {
      const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...mod });
      tab("Test cases").dispatchEvent(event);
      // Not handled: the browser's own shortcut still runs, and no tab moves.
      expect(event.defaultPrevented).toBe(false);
    }
  }
  expect(tab("Test cases")).toHaveAttribute("aria-selected", "true");
  // A plain arrow still moves.
  fireEvent.keyDown(tab("Test cases"), { key: "ArrowRight" });
  expect(tab("Past runs")).toHaveAttribute("aria-selected", "true");
});

test("the Past runs result filter survives switching tabs and back", async () => {
  const failedRun = {
    id: "run-f",
    pbi_id: 42,
    started_at: "1786000100000",
    mode: "supervised",
    cases: [{ case_id: 201, title: "Valid login", verdict: "Failed", note: "", proposed: null }],
  };
  const passedRun = {
    id: "run-p",
    pbi_id: 42,
    started_at: "1786000200000",
    mode: "supervised",
    cases: [{ case_id: 202, title: "Locked account", verdict: "Passed", note: "", proposed: null }],
  };
  mockReady((cmd) => (cmd === "auto_run_list_runs" ? [failedRun, passedRun] : undefined));
  renderAutoRun("Past runs");

  fireEvent.click(await screen.findByRole("button", { name: "Failed (1)" }));
  expect(screen.getByRole("button", { name: "Failed (1)" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.queryByText("Locked account")).not.toBeInTheDocument();

  fireEvent.click(tab("Test cases"));
  await screen.findByText("Valid login");
  fireEvent.click(tab("Past runs"));

  expect(await screen.findByRole("button", { name: "Failed (1)" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("button", { name: "All (2)" })).toHaveAttribute("aria-pressed", "false");
  expect(screen.queryByText("Locked account")).not.toBeInTheDocument();
});

test("a Clear scripts dialog that is cancelled returns focus to the More button", async () => {
  mockReady((cmd, args) => {
    if (cmd === "auto_run_load_script") {
      return (args as { caseId: number }).caseId === 202 ? scriptFor202 : null;
    }
    return undefined;
  });
  renderAutoRun("Test cases");
  await expandCards();
  await screen.findByRole("button", { name: "Run #202" });
  chooseFromMore("Clear scripts");
  await screen.findByRole("heading", { name: "Clear scripts?" });
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("heading", { name: "Clear scripts?" })).not.toBeInTheDocument());
  await waitFor(() => expect(screen.getByRole("button", { name: "More" })).toHaveFocus());
});

const reviewableRun = {
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

test("closing a review opened from Past runs leaves focus on the Review button that opened it", async () => {
  mockReady((cmd, args) => {
    if (cmd === "auto_run_list_runs") return [reviewableRun];
    if (cmd === "auto_run_load_run") {
      return (args as { runId: string }).runId === reviewableRun.id ? reviewableRun : null;
    }
    return undefined;
  });
  renderAutoRun("Past runs");

  const open = await screen.findByRole("button", { name: "Review" });
  open.focus();
  fireEvent.click(open);
  await screen.findByText(/proposed: passed/i);

  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  await waitFor(() => expect(screen.queryByText(/proposed: passed/i)).not.toBeInTheDocument());
  await waitFor(() => expect(screen.getByRole("button", { name: "Review" })).toHaveFocus());
  expect(tab("Past runs")).not.toHaveFocus();
  expect(tab("Past runs")).toHaveAttribute("aria-selected", "true");
});

/// Opening the screen reads every listed case's script in one call, not one
/// call per case: a PBI of hundreds of cases was hundreds of round trips,
/// and the screen froze while they ran.
test("opening the screen reads all its cases' scripts in one call", async () => {
  const batches: number[][] = [];
  mockReady((cmd, args) => {
    if (cmd === "auto_run_load_scripts") {
      const ids = (args as { caseIds: number[] }).caseIds;
      batches.push(ids);
      return ids.map((id) => ({ case_id: id, script: null, error: null }));
    }
    if (cmd === "auto_run_load_script") throw new Error("one case at a time");
    return undefined;
  });
  renderAutoRun("Test cases");
  await expandCards();
  await waitFor(() => expect(batches).toEqual([[201, 202]]));
});
