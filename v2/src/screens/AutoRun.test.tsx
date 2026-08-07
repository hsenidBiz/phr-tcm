import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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

/// The whole feature is local. Saying so on the screen is what stops
/// someone assuming a green run updated Azure DevOps.
test("says plainly that nothing reaches Azure DevOps", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") return null;
  });
  renderAutoRun();
  expect(await screen.findByText(/nothing is sent to azure devops/i)).toBeInTheDocument();
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

/// Saving records the human's verdict and the evidence together, into a
/// LOCAL run - and never calls anything that writes to Azure DevOps.
test("saving stores the verdict locally and touches no ADO command", async () => {
  let saved: Record<string, unknown> | null = null;
  const calls: string[] = [];
  mockIPC((cmd, args) => {
    calls.push(String(cmd));
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      const a = args as { caseId: number };
      return a.caseId === 201 ? scriptFor201 : null;
    }
    if (cmd === "auto_run_new_id") return "run-1786000000000";
    if (cmd === "auto_run_open_browser") return null;
    if (cmd === "auto_run_step") return [{ ok: true, detail: "page contains Dashboard" }];
    if (cmd === "auto_run_save_run") {
      saved = args as Record<string, unknown>;
      return null;
    }
    if (cmd === "auto_run_close_browser") return null;
  });
  renderAutoRun();

  fireEvent.click(await screen.findByRole("button", { name: "Run #201" }));
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run step 1" }));
  await screen.findByText("page contains Dashboard");
  fireEvent.click(screen.getByRole("button", { name: "Failed" }));
  fireEvent.click(screen.getByRole("button", { name: "Save result" }));

  await waitFor(() => expect(saved).not.toBeNull());
  const run = (saved as unknown as { run: { cases: { verdict: string }[] } }).run;
  expect(run.cases[0].verdict).toBe("Failed");

  // The guard that matters: no run-recording command was ever invoked.
  expect(calls).not.toContain("start_test_run");
  expect(calls).not.toContain("record_result");
  expect(calls).not.toContain("finish_test_run");
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
