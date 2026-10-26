// The queue editor drops a step left completely blank, as Manual Entry does.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { TestCase } from "../bindings";
import QueueCaseEditor from "./QueueCaseEditor";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const original: TestCase = {
  title: "Login works",
  steps: [{ action: "Open page", expected: "Page shown" }],
  tags: "",
  automation_status: "Not Automated",
  module_value: "",
  preconditions: "",
  update_id: null,
  priority: 2,
  spec_order: null,
  tester_order: null,
};

function mount() {
  mockIPC((cmd) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
  });
  const onSave = vi.fn();
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <QueueCaseEditor original={original} org="acme" project="Web" onSave={onSave} onCancel={() => {}} />
    </QueryClientProvider>,
  );
  return onSave;
}

const save = () => screen.getByRole("button", { name: "Save to queue" });

test("a step added and left blank is not saved, and does not block the save", () => {
  const onSave = mount();
  fireEvent.click(screen.getByRole("button", { name: /Add Step/ }));
  expect(screen.getByLabelText("Step 2 action")).toBeInTheDocument();
  // Nothing real changed, so there is nothing to save yet.
  expect(save()).toBeDisabled();

  fireEvent.change(screen.getByLabelText("Case title"), { target: { value: "Login works again" } });
  expect(save()).toBeEnabled();
  expect(screen.queryByText(/action is empty/)).not.toBeInTheDocument();
  fireEvent.click(save());
  expect(onSave).toHaveBeenCalledTimes(1);
  expect(onSave.mock.calls[0][0]).toMatchObject({
    title: "Login works again",
    steps: [{ action: "Open page", expected: "Page shown" }],
  });
});

test("a step with only an expected result still stops the save and is named", () => {
  mount();
  fireEvent.click(screen.getByRole("button", { name: /Add Step/ }));
  fireEvent.change(screen.getByLabelText("Step 2 expected"), { target: { value: "Something" } });
  expect(screen.getByText("Step 2 action is empty.")).toBeInTheDocument();
  expect(save()).toBeDisabled();
});
