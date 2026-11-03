// The case editor and step attachments: the notes a save returns reach the
// person, and a case whose attachments could not be read says so.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { TestCaseFull } from "../../bindings";
import CaseEditor from "./CaseEditor";

const { toastMock } = vi.hoisted(() => ({
  toastMock: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn(), dismiss: vi.fn() },
}));
vi.mock("../../lib/toast", () => ({ toast: toastMock }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

const full = (extra: Partial<TestCaseFull> = {}): TestCaseFull => ({
  id: 77,
  title: "Login works",
  tags: "",
  automation_status: "Not Automated",
  steps: [{ action: "Open", expected: "Shown", attachments: [{ id: "g1", name: "old.png", size: 10 }] }],
  steps_legacy: null,
  step_ids: ["2"],
  steps_xml: "<steps/>",
  module_value: "",
  preconditions: "",
  preconditions_legacy: null,
  preconditions_stored: null,
  priority: 2,
  ...extra,
});

function mount(original: TestCaseFull, sent: unknown[] = []) {
  mockIPC((cmd, args) => {
    if (cmd === "update_test_case") {
      sent.push((args as { tc: unknown }).tc);
      return ["old.png was not found on the work item."];
    }
    return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <CaseEditor
        original={original}
        org="acme"
        project="Web"
        moduleRef={null}
        preconditionsRef={null}
        onSaved={() => {}}
      />
    </QueryClientProvider>,
  );
}

test("a removed existing attachment is saved as removed, and the save's notes are shown", async () => {
  const sent: unknown[] = [];
  mount(full(), sent);
  fireEvent.click(screen.getByRole("button", { name: "Remove old.png from step 1" }));
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
  await vi.waitFor(() => expect(toastMock.warning).toHaveBeenCalledWith("old.png was not found on the work item."));
  expect(sent[0]).toMatchObject({ removed_attachments: ["g1"] });
});

test("a case whose attachments could not be read says so", () => {
  mount(full({ attachments_read: false }));
  expect(screen.getByText(/Attachments could not be read/)).toBeInTheDocument();
});

test("a case read normally shows no such line", () => {
  mount(full());
  expect(screen.queryByText(/Attachments could not be read/)).toBeNull();
});
