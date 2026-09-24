// The Module paths dialog: the recorded paths in words, Remove that asks
// first, the address switch, and a recording driven by mocked events.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "../../lib/toast";
import ModulePathsDialog from "./ModulePathsDialog";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

const ACCOUNTS = [{ key: "hr.admin", label: "HR Admin", username: "kim", password: "p" }];
const LEAVE = {
  module: "Leave",
  clicks: ['link "Leave"', 'link "Apply Leave"'],
  arrived: "/hr/leave/apply",
  recorded: "2026-09-24T10:00:00Z",
};

function mount(handler: (cmd: string, args: Record<string, unknown>) => unknown) {
  mockIPC(
    (cmd, args) => {
      if (cmd === "auto_run_list_accounts") return ACCOUNTS;
      const out = handler(String(cmd), (args ?? {}) as Record<string, unknown>);
      return out === undefined ? null : out;
    },
    { shouldMockEvents: true },
  );
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ModulePathsDialog org="acme" project="Web" caseModules={["Leave", "Payroll"]} onClose={vi.fn()} />
    </QueryClientProvider>,
  );
}

async function startRecording() {
  const record = await screen.findByRole("button", { name: "Record a module…" });
  await waitFor(() => expect(record).toBeEnabled());
  fireEvent.click(record);
  fireEvent.click(screen.getByRole("combobox", { name: "Module" }));
  fireEvent.click(await screen.findByRole("option", { name: "Leave" }));
  fireEvent.click(screen.getByRole("button", { name: "Start recording" }));
  await screen.findByRole("button", { name: "Stop" });
}

async function send(payload: { kind: string; index: number; readable: string; detail: string }) {
  const { emit } = await import("@tauri-apps/api/event");
  await act(async () => {
    await emit("recording-event", payload);
  });
}

test("the list shows each module's clicks in words and where it ends", async () => {
  mount((cmd) => (cmd === "auto_run_load_nav" ? { direct_urls: true, modules: [LEAVE] } : undefined));
  expect(await screen.findByText('link "Leave" › link "Apply Leave"')).toBeInTheDocument();
  expect(screen.getByText("ends on /hr/leave/apply")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Re-record Leave" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Try Leave" })).toBeInTheDocument();
});

test("Remove asks first, and only the confirm removes", async () => {
  const removed: unknown[] = [];
  mount((cmd, args) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [LEAVE] };
    if (cmd === "auto_run_remove_module_path") {
      removed.push(args);
      return { direct_urls: true, modules: [] };
    }
  });
  fireEvent.click(await screen.findByRole("button", { name: "Remove Leave" }));
  expect(screen.getByText("Remove the path for Leave?")).toBeInTheDocument();
  expect(removed).toEqual([]);
  fireEvent.click(screen.getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(removed).toEqual([{ organization: "acme", project: "Web", module: "Leave" }]));
  expect(await screen.findByText(/No module paths yet/)).toBeInTheDocument();
});

test("the address switch saves the moment it is flipped", async () => {
  const sets: unknown[] = [];
  mount((cmd, args) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_set_direct_urls") {
      sets.push(args);
      return { direct_urls: false, modules: [] };
    }
  });
  const sw = await screen.findByRole("switch", { name: "Scripts may open pages by address" });
  await waitFor(() => expect(sw).toBeEnabled());
  expect(sw).toHaveAttribute("aria-checked", "true");
  fireEvent.click(sw);
  await waitFor(() => expect(sets).toEqual([{ organization: "acme", project: "Web", allowed: false }]));
  await waitFor(() => expect(sw).toHaveAttribute("aria-checked", "false"));
});

test("a recording lists each click as it arrives and saves on Stop", async () => {
  const started: unknown[] = [];
  mount((cmd, args) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_record_start") {
      started.push(args);
      return null;
    }
    if (cmd === "auto_run_record_stop") return { saved: true, module: "Leave", failure: "" };
  });
  await startRecording();
  expect(started).toEqual([
    { organization: "acme", project: "Web", module: "Leave", account: "hr.admin", browserName: "edge" },
  ]);
  expect(screen.getByRole("button", { name: "Stop" })).toBeDisabled();
  await send({ kind: "click", index: 1, readable: 'link "Leave"', detail: "" });
  expect(await screen.findByText('1. link "Leave"')).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  await waitFor(() => expect(toast.success).toHaveBeenCalledWith("Path saved for Leave."));
  expect(await screen.findByRole("button", { name: "Record a module…" })).toBeInTheDocument();
});

test("a path that does not replay says which click failed and offers to record again", async () => {
  let starts = 0;
  mount((cmd) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_record_start") {
      starts += 1;
      return null;
    }
    if (cmd === "auto_run_record_stop") {
      return { saved: false, module: "Leave", failure: 'click 2, link "Apply Leave": no visible match' };
    }
  });
  await startRecording();
  await send({ kind: "click", index: 1, readable: 'link "Leave"', detail: "" });
  fireEvent.click(await screen.findByRole("button", { name: "Stop" }));
  expect(await screen.findByText('click 2, link "Apply Leave": no visible match')).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Record again" }));
  await waitFor(() => expect(starts).toBe(2));
});

/// Review focus 1.
test("closing the recording browser ends the recording and frees it", async () => {
  let cancels = 0;
  mount((cmd) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_record_start") return null;
    if (cmd === "auto_run_record_cancel") {
      cancels += 1;
      return null;
    }
  });
  await startRecording();
  await send({ kind: "closed", index: 0, readable: "", detail: "the recording browser was closed - nothing was saved" });
  expect(await screen.findByText("The recording browser was closed. Nothing was saved.")).toBeInTheDocument();
  await waitFor(() => expect(cancels).toBe(1));
});
