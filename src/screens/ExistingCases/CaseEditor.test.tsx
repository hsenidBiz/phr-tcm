// The case editor and step attachments: the notes a save returns reach the
// person, and a case whose attachments could not be read says so.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { TestCaseFull } from "../../bindings";
import CaseEditor from "./CaseEditor";

const { toastMock } = vi.hoisted(() => ({
  toastMock: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn(), dismiss: vi.fn() },
}));
vi.mock("../../lib/toast", () => ({ toast: toastMock }));
const { openDialog } = vi.hoisted(() => ({ openDialog: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openDialog }));

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
      return { notes: ["old.png was not found on the work item."], attached: [] };
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

test("a second save after attaching sends the stored id, never the path again", async () => {
  const sent: { steps: { attachments?: { id?: string; path?: string }[] }[] }[] = [];
  openDialog.mockResolvedValue(["C:/shots/n.png"]);
  mockIPC((cmd, args) => {
    if (cmd === "describe_step_files")
      return [{ path: "C:/shots/n.png", name: "n.png", size: 3, refused: null }];
    if (cmd === "update_test_case") {
      sent.push((args as { tc: (typeof sent)[number] }).tc);
      return { notes: [], attached: [{ step: 0, path: "C:/shots/n.png", id: "gnew", name: "n.png" }] };
    }
    return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const ui = (o: TestCaseFull) => (
    <QueryClientProvider client={qc}>
      <CaseEditor original={o} org="acme" project="Web" moduleRef={null} preconditionsRef={null} onSaved={() => {}} />
    </QueryClientProvider>
  );
  const first = full();
  const { rerender } = render(ui(first));
  fireEvent.click(screen.getByRole("button", { name: "Attach a file to step 1" }));
  await screen.findByText("n.png");
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
  await waitFor(() => expect(sent).toHaveLength(1));
  expect(sent[0].steps[0].attachments?.some((a) => a.path)).toBe(true);

  // The refetch comes back unchanged (or failed): the draft must not care.
  rerender(ui({ ...first }));
  fireEvent.change(screen.getByLabelText("Case title"), { target: { value: "Login works again" } });
  fireEvent.click(await screen.findByRole("button", { name: "Save changes" }));
  await waitFor(() => expect(sent).toHaveLength(2));
  expect(sent[1].steps[0].attachments?.some((a) => a.path)).toBe(false);
  expect(sent[1].steps[0].attachments?.map((a) => a.id)).toContain("gnew");
  expect((screen.getByLabelText("Case title") as HTMLInputElement).value).toBe("Login works again");
});
