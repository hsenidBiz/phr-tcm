// Bulk edit of queued drafts: the same no-checkbox form as BulkEditDialog,
// handing back one pure edit the queue applies in memory.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { TestCase } from "../bindings";
import QueueBulkEditDialog from "./QueueBulkEditDialog";

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.restoreAllMocks();
});

const draft = (over: Partial<TestCase> = {}): TestCase => ({
  title: "Login works",
  steps: [{ action: "Open page", expected: "Page shown" }],
  tags: "smoke",
  automation_status: "Not Automated",
  module_value: "Payroll",
  preconditions: "Old precondition",
  update_id: null,
  priority: 3,
  spec_order: null,
  tester_order: null,
  ...over,
});

function mount() {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return ["Billing", "Payroll"];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onApply = vi.fn();
  render(
    <QueryClientProvider client={qc}>
      <QueueBulkEditDialog org="acme" project="Web" count={2} onClose={() => {}} onApply={onApply} />
    </QueryClientProvider>,
  );
  return onApply;
}

const apply = () => screen.getByRole("button", { name: /^Apply to 2$/ });

async function pickModule(value: string) {
  fireEvent.click(screen.getByRole("combobox", { name: "Module" }));
  fireEvent.change(screen.getByPlaceholderText("Search…"), { target: { value } });
  fireEvent.click(await screen.findByRole("button", { name: `Use "${value}"` }));
}

function pickOption(selectName: string, option: string) {
  fireEvent.click(screen.getByRole("combobox", { name: selectName }));
  fireEvent.click(screen.getByRole("option", { name: option }));
}

/** The one edit the dialog handed back, run over a draft. */
function appliedEdit(onApply: ReturnType<typeof vi.fn>, tc: TestCase): TestCase {
  fireEvent.click(apply());
  expect(onApply).toHaveBeenCalledTimes(1);
  return (onApply.mock.calls[0][0] as (t: TestCase) => TestCase)(tc);
}

test("there are no checkboxes: every field shows at once and starts on Leave unchanged", () => {
  mount();
  expect(screen.queryAllByRole("checkbox")).toHaveLength(0);
  for (const name of ["Automation status", "Priority", "Tags", "Preconditions"]) {
    expect(screen.getByRole("combobox", { name })).toHaveTextContent("Leave unchanged");
  }
  expect(screen.getByRole("combobox", { name: "Module" })).toHaveTextContent("Leave unchanged");
  expect(screen.queryByRole("textbox", { name: "Preconditions text" })).not.toBeInTheDocument();
  expect(screen.getByText("Nothing to change yet")).toBeInTheDocument();
  expect(apply()).toBeDisabled();
});

test("picking a module enables Apply, and clearing it returns to unchanged", async () => {
  mount();
  await pickModule("Billing");
  expect(screen.getByText("Will change: Module")).toBeInTheDocument();
  expect(apply()).toBeEnabled();
  fireEvent.click(within(screen.getByRole("combobox", { name: "Module" })).getByRole("button", { name: "Clear" }));
  expect(screen.getByText("Nothing to change yet")).toBeInTheDocument();
  expect(apply()).toBeDisabled();
});

test("the summary names every field that is set", async () => {
  mount();
  await pickModule("Billing");
  pickOption("Priority", "Priority 1");
  expect(screen.getByText("Will change: Priority, Module")).toBeInTheDocument();
});

test("the edit sets the module and keeps everything else on each draft", async () => {
  const onApply = mount();
  await pickModule("Billing");
  const out = appliedEdit(onApply, draft());
  expect(out).toMatchObject({
    module_value: "Billing",
    preconditions: "Old precondition",
    automation_status: "Not Automated",
    tags: "smoke",
    priority: 3,
    title: "Login works",
  });
});

test("Preconditions: Replace with saves the text; Clear saves an empty text", async () => {
  const onApply = mount();
  pickOption("Preconditions", "Replace with");
  expect(apply()).toBeDisabled();
  fireEvent.change(screen.getByRole("textbox", { name: "Preconditions text" }), {
    target: { value: "Signed in as admin" },
  });
  expect(screen.getByText("Will change: Preconditions")).toBeInTheDocument();
  expect(appliedEdit(onApply, draft()).preconditions).toBe("Signed in as admin");
});

test("Preconditions: Clear empties them, and Leave unchanged goes back", () => {
  const onApply = mount();
  pickOption("Preconditions", "Clear");
  expect(screen.getByText("Will change: Preconditions")).toBeInTheDocument();
  pickOption("Preconditions", "Leave unchanged");
  expect(apply()).toBeDisabled();
  pickOption("Preconditions", "Clear");
  expect(appliedEdit(onApply, draft()).preconditions).toBe("");
});

test("Priority sets every draft's priority", () => {
  const onApply = mount();
  pickOption("Priority", "Priority 1");
  expect(appliedEdit(onApply, draft()).priority).toBe(1);
});
