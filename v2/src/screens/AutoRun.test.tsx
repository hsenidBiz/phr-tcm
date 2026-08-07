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
