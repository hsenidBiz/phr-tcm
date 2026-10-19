// The case editors keep test case text as the Markdown typed - line breaks
// included - and Preview shows it formatted without changing it.

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

const PREREQ = "Signed in as **HR admin**\n- Leave module on\n- Payroll off";
const ACTION = "Run:\n\n```sql\nSELECT COUNT(*) FROM leave\n```";

const queued: TestCase = {
  title: "Leave count",
  steps: [{ action: "Open", expected: "Shown" }],
  tags: "",
  automation_status: "Not Automated",
  module_value: "",
  preconditions: "",
  update_id: null,
};

/** Type the Markdown, check Preview shows it formatted, and come back. */
function typeAndPreview() {
  const pre = screen.getByLabelText("Preconditions");
  expect(pre.tagName).toBe("TEXTAREA");
  fireEvent.change(pre, { target: { value: PREREQ } });
  fireEvent.change(screen.getByLabelText("Step 1 action"), { target: { value: ACTION } });

  fireEvent.click(screen.getByRole("button", { name: "Preview preconditions" }));
  expect(screen.getByText("HR admin").tagName).toBe("STRONG");
  expect(screen.getByText("Leave module on").tagName).toBe("LI");
  fireEvent.click(screen.getByRole("button", { name: "Preview steps" }));
  expect(document.querySelector(".md-preview pre code")?.textContent).toContain("SELECT COUNT(*)");

  fireEvent.click(screen.getByRole("button", { name: "Preview preconditions" }));
  fireEvent.click(screen.getByRole("button", { name: "Preview steps" }));
  expect((screen.getByLabelText("Preconditions") as HTMLTextAreaElement).value).toBe(PREREQ);
  expect((screen.getByLabelText("Step 1 action") as HTMLTextAreaElement).value).toBe(ACTION);
}

test("queue editor: Markdown typed is saved exactly, and Preview shows it formatted", () => {
  mocks();
  const onSave = vi.fn();
  wrap(<QueueCaseEditor original={queued} org="acme" project="Web" onSave={onSave} onCancel={vi.fn()} />);
  typeAndPreview();
  fireEvent.click(screen.getByRole("button", { name: /Save to queue/ }));
  expect(onSave).toHaveBeenCalledWith(
    expect.objectContaining({
      preconditions: PREREQ,
      steps: [{ action: ACTION, expected: "Shown" }],
    }),
  );
});

test("Update Test Cases editor: Markdown typed is sent exactly", async () => {
  const sent: TestCase[] = [];
  mocks((cmd, args) => {
    if (cmd === "update_test_case") {
      sent.push((args as { tc: TestCase }).tc);
      return null;
    }
  });
  const existing: TestCaseFull = {
    id: 201,
    title: "Leave count",
    tags: "",
    automation_status: "Not Automated",
    steps: [{ action: "Open", expected: "Shown" }],
    step_ids: ["2"],
    steps_xml: "",
    module_value: "",
    preconditions: "",
    priority: 2,
  };
  wrap(
    <CaseEditor
      original={existing}
      org="acme"
      project="Web"
      moduleRef={null}
      preconditionsRef="Custom.Prerequisites"
      onSaved={vi.fn()}
    />,
  );
  typeAndPreview();
  fireEvent.click(screen.getByRole("button", { name: /Save changes/ }));
  await waitFor(() => expect(sent).toHaveLength(1));
  expect(sent[0]).toMatchObject({ preconditions: PREREQ, steps: [{ action: ACTION, expected: "Shown" }] });
});

test("Manual Entry: Markdown typed reaches the queue exactly", async () => {
  mocks();
  wrap(<ManualEntry org="acme" project="Web" pbi={{ id: 42, title: "PBI", work_item_type: "Product Backlog Item" }} />);
  fireEvent.change(screen.getByPlaceholderText("Test case title"), { target: { value: "Leave count" } });
  typeAndPreview();
  fireEvent.click(screen.getByRole("button", { name: "Add to queue" }));
  await waitFor(() => expect(loadDraftQueue("acme", 42)).toHaveLength(1));
  expect(loadDraftQueue("acme", 42)[0]).toMatchObject({
    preconditions: PREREQ,
    steps: [{ action: ACTION, expected: "" }],
  });
});
