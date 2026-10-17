// The project's protected baseline: rows of a kind and a name, the usual
// PMS records offered on request, and the whole list saved through one
// command.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { NavView, ProtectedRecord } from "../../bindings";
import { toast } from "../../lib/toast";
import ProtectedRecordsDialog from "./ProtectedRecordsDialog";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

function view(records: ProtectedRecord[]): NavView {
  return { direct_urls: true, modules: [], save_words: [], built_in_save_words: [], protected: records };
}

/** Mounts the dialog; every save it sends lands in `sent`, or is refused. */
function mount(start: ProtectedRecord[], sent: ProtectedRecord[][], refuse?: string) {
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_set_protected") {
      const records = (args as { records: ProtectedRecord[] }).records;
      sent.push(records);
      if (refuse) throw refuse;
      return view(records);
    }
    return null;
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ProtectedRecordsDialog org="acme" project="PMS" view={view(start)} onClose={onClose} />
    </QueryClientProvider>,
  );
  return onClose;
}

const names = () =>
  screen.queryAllByRole("textbox").map((t) => (t as HTMLInputElement).value);

test("the dialog is named by its heading and starts empty for a project with none", () => {
  mount([], []);
  expect(screen.getByRole("dialog", { name: "Protected records" })).toBeInTheDocument();
  expect(screen.getByText("None yet.")).toBeInTheDocument();
});

test("a row is added with a kind and a name, and the whole list is saved", async () => {
  const sent: ProtectedRecord[][] = [];
  const onClose = mount([{ kind: "cycle", name: "FY2026 (published)" }], sent);
  fireEvent.click(screen.getByRole("button", { name: "Add a record" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Name of record 2" }), { target: { value: "Customer Focus" } });
  fireEvent.click(screen.getByRole("combobox", { name: "Kind of record 2" }));
  fireEvent.click(screen.getByRole("option", { name: "Rating method" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() =>
    expect(sent).toEqual([
      [
        { kind: "cycle", name: "FY2026 (published)" },
        { kind: "rating_method", name: "Customer Focus" },
      ],
    ]),
  );
  await waitFor(() => expect(onClose).toHaveBeenCalled());
  expect(toast.success).toHaveBeenCalledWith("Protected records saved.");
});

test("a row is removed and the list saved without it", async () => {
  const sent: ProtectedRecord[][] = [];
  mount(
    [
      { kind: "cycle", name: "FY2026 (published)" },
      { kind: "other", name: "Leave policy" },
    ],
    sent,
  );
  fireEvent.click(screen.getByRole("button", { name: "Remove record 1" }));
  expect(names()).toEqual(["Leave policy"]);
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(sent).toEqual([[{ kind: "other", name: "Leave policy" }]]));
});

test("the usual PMS records are filled in without repeating one already there", async () => {
  const sent: ProtectedRecord[][] = [];
  mount([{ kind: "goal_group", name: "customer focus" }], sent);
  // Offered, not written: nothing is sent until Save.
  fireEvent.click(screen.getByRole("button", { name: "Add the usual PMS records" }));
  expect(names()).toEqual([
    "customer focus",
    "Operational Excellence",
    "5-Point Scale",
    "Four Level Scale",
    "6-level proficiency scale",
    "Core Competencies",
    "Technical Competencies",
    "FY2026 (published)",
    "FY2025 (unpublished)",
  ]);
  expect(sent).toEqual([]);
  // Pressing it again adds nothing.
  fireEvent.click(screen.getByRole("button", { name: "Add the usual PMS records" }));
  expect(names()).toHaveLength(9);
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(sent).toHaveLength(1));
  expect(sent[0][2]).toEqual({ kind: "rating_method", name: "5-Point Scale" });
  expect(sent[0][5]).toEqual({ kind: "competency_group", name: "Core Competencies" });
});

test("a refusal from the app is shown and the dialog stays open", async () => {
  const sent: ProtectedRecord[][] = [];
  const onClose = mount([{ kind: "other", name: "x" }], sent, "the settings file cannot be written");
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByText("the settings file cannot be written")).toBeInTheDocument();
  expect(onClose).not.toHaveBeenCalled();
});
