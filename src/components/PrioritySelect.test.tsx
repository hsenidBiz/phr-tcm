import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, expect, test, vi } from "vitest";
import type { TestCase, TestCaseFull } from "../bindings";
import { loadDraftQueue } from "../hooks/useQueue";
import CaseEditor from "../screens/ExistingCases/CaseEditor";
import ManualEntry from "../screens/ManualEntry";
import QueueCaseEditor from "./QueueCaseEditor";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function wrap(ui: ReactNode) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={qc}>{ui}</QueryClientProvider>);
}

function mocks(handler: (cmd: string, args: unknown) => unknown = () => undefined) {
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    return handler(cmd, args);
  });
}

const priority = () => screen.getByRole("combobox", { name: "Priority" });

function pick(label: string) {
  fireEvent.click(priority());
  fireEvent.click(screen.getByRole("option", { name: label }));
}

const queued: TestCase = {
  title: "Login works",
  steps: [{ action: "Open", expected: "Shown" }],
  tags: "",
  automation_status: "Not Automated",
  module_value: "",
  preconditions: "",
  update_id: null,
};

const existing: TestCaseFull = {
  id: 201,
  title: "Login works",
  tags: "",
  automation_status: "Not Automated",
  steps: [{ action: "Open", expected: "Shown" }],
  step_ids: ["2"],
  steps_xml: "",
  module_value: "",
  preconditions: "",
  priority: 3,
};

test("Manual Entry: a new case defaults to priority 2 and the chosen value is saved", async () => {
  mocks();
  wrap(<ManualEntry org="acme" project="Web" pbi={{ id: 42, title: "PBI", work_item_type: "Product Backlog Item", project: "Web" }} />);
  expect(priority()).toHaveTextContent("Priority 2");

  const add = (title: string) => {
    fireEvent.change(screen.getByPlaceholderText("Test case title"), { target: { value: title } });
    fireEvent.change(screen.getByLabelText("Step 1 action"), { target: { value: "Open page" } });
    fireEvent.click(screen.getByRole("button", { name: "Add to queue" }));
  };
  add("Default one");
  pick("Priority 4");
  add("Picked one");

  await waitFor(() => expect(loadDraftQueue("acme", 42)).toHaveLength(2));
  const saved = loadDraftQueue("acme", 42);
  expect(saved.map((c) => [c.title, c.priority])).toEqual([
    ["Default one", 2],
    ["Picked one", 4],
  ]);
});

test("queue editor: a new case without a priority shows 2", () => {
  mocks();
  wrap(<QueueCaseEditor original={queued} org="acme" project="Web" onSave={vi.fn()} onCancel={vi.fn()} />);
  expect(priority()).toHaveTextContent("Priority 2");
});

test("queue editor: an edited case shows its value and the saved case carries the new one", () => {
  mocks();
  const onSave = vi.fn();
  wrap(
    <QueueCaseEditor
      original={{ ...queued, update_id: 201, priority: 3 }}
      org="acme"
      project="Web"
      onSave={onSave}
      onCancel={vi.fn()}
    />,
  );
  expect(priority()).toHaveTextContent("Priority 3");
  pick("Priority 1");
  fireEvent.click(screen.getByRole("button", { name: /Save to queue/ }));
  expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ update_id: 201, priority: 1 }));
});

test("queue editor: an update with no priority offers Keep current rather than implying 2", () => {
  mocks();
  wrap(
    <QueueCaseEditor
      original={{ ...queued, update_id: 201 }}
      org="acme"
      project="Web"
      onSave={vi.fn()}
      onCancel={vi.fn()}
    />,
  );
  expect(priority()).toHaveTextContent("Keep current");
});

test("Update Test Cases editor: shows the loaded priority and sends the chosen one", async () => {
  const sent: TestCase[] = [];
  mocks((cmd, args) => {
    if (cmd === "update_test_case") {
      sent.push((args as { tc: TestCase }).tc);
      return null;
    }
  });
  wrap(
    <CaseEditor
      original={existing}
      org="acme"
      project="Web"
      moduleRef={null}
      preconditionsRef={null}
      onSaved={vi.fn()}
    />,
  );
  expect(priority()).toHaveTextContent("Priority 3");
  pick("Priority 4");
  fireEvent.click(screen.getByRole("button", { name: /Save changes/ }));
  await waitFor(() => expect(sent).toHaveLength(1));
  expect(sent[0]).toMatchObject({ update_id: 201, priority: 4 });
});
