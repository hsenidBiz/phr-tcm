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

/** Choose Leave and press Start recording, without waiting for
 * `auto_run_record_start` to settle - for the "starting" phase tests
 * below, where that call is held open on purpose. */
async function chooseAndStart() {
  const record = await screen.findByRole("button", { name: "Record a module…" });
  await waitFor(() => expect(record).toBeEnabled());
  fireEvent.click(record);
  fireEvent.click(screen.getByRole("combobox", { name: "Module" }));
  fireEvent.click(await screen.findByRole("option", { name: "Leave" }));
  fireEvent.click(screen.getByRole("button", { name: "Start recording" }));
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

/// Fix round 1: Start must always be cancellable, even while it is still
/// signing in - there was previously no way out of the "starting" phase.
test("Cancel is available while the recording browser is opening, not Stop", async () => {
  mount((cmd) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    // Never resolves on its own - stands in for a slow sign-in, same
    // trick RunPane.test.tsx uses for `auto_run_sign_in`.
    if (cmd === "auto_run_record_start") return new Promise(() => {});
  });
  await chooseAndStart();

  expect(await screen.findByRole("button", { name: "Cancel" })).toBeEnabled();
  expect(screen.queryByRole("button", { name: "Stop" })).not.toBeInTheDocument();
});

test("Cancel while starting cancels the pending sign-in and returns to the list", async () => {
  let rejectStart: ((reason: string) => void) | null = null;
  let cancels = 0;
  mount((cmd) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_record_start") {
      return new Promise((_resolve, reject) => {
        rejectStart = reject;
      });
    }
    if (cmd === "auto_run_record_cancel") {
      cancels += 1;
      return null;
    }
  });
  await chooseAndStart();

  fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(cancels).toBe(1));

  // The backend's pending-cancel flag is what actually ends the still-open
  // `auto_run_record_start` call - simulate it settling that way.
  await act(async () => {
    rejectStart?.("the recording was cancelled - nothing was saved");
  });

  expect(await screen.findByRole("button", { name: "Record a module…" })).toBeInTheDocument();
  await waitFor(() =>
    expect(toast.info).toHaveBeenCalledWith("The recording was cancelled. Nothing was saved."),
  );
});

/// Review M10: whether Start's answer is a cancel is decided by the Cancel
/// this dialog asked for, never by matching the backend's words, so a
/// wording change on either side cannot turn a cancel into a red panel.
test("after Cancel, Start's refusal reads as a cancel whatever its words", async () => {
  let rejectStart: ((reason: string) => void) | null = null;
  mount((cmd) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_record_start") {
      return new Promise((_resolve, reject) => {
        rejectStart = reject;
      });
    }
    if (cmd === "auto_run_record_cancel") return null;
  });
  await chooseAndStart();
  fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
  await act(async () => {
    rejectStart?.("some other words entirely");
  });
  expect(await screen.findByRole("button", { name: "Record a module…" })).toBeInTheDocument();
  expect(screen.queryByText("some other words entirely")).not.toBeInTheDocument();
  expect(toast.info).toHaveBeenCalledWith("The recording was cancelled. Nothing was saved.");
});

/// Review M4: the Cancel crossed Start's success - the recording opened
/// anyway. It is cancelled after all, not shown for a closed browser.
test("a Start that succeeds after Cancel was pressed is cancelled, not shown as recording", async () => {
  let resolveStart: ((v: null) => void) | null = null;
  let cancels = 0;
  mount((cmd) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_record_start") {
      return new Promise((resolve) => {
        resolveStart = resolve;
      });
    }
    if (cmd === "auto_run_record_cancel") {
      cancels += 1;
      return null;
    }
  });
  await chooseAndStart();
  fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(cancels).toBe(1));
  await act(async () => {
    resolveStart?.(null);
  });
  expect(await screen.findByRole("button", { name: "Record a module…" })).toBeInTheDocument();
  await waitFor(() => expect(cancels).toBe(2));
  expect(screen.queryByRole("button", { name: "Stop" })).not.toBeInTheDocument();
  expect(toast.info).toHaveBeenCalledWith("The recording was cancelled. Nothing was saved.");
});

/// Review M3: the check after Stop can take minutes; Cancel ends it.
test("the check after Stop can be cancelled", async () => {
  let resolveStop: ((v: unknown) => void) | null = null;
  let cancels = 0;
  mount((cmd) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_record_start") return null;
    if (cmd === "auto_run_record_stop") {
      return new Promise((resolve) => {
        resolveStop = resolve;
      });
    }
    if (cmd === "auto_run_record_cancel") {
      cancels += 1;
      return null;
    }
  });
  await startRecording();
  await send({ kind: "click", index: 1, readable: 'link "Leave"', detail: "" });
  fireEvent.click(await screen.findByRole("button", { name: "Stop" }));
  expect(await screen.findByText("Checking the path for Leave in a fresh browser…")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(cancels).toBe(1));
  await act(async () => {
    resolveStop?.({ saved: false, module: "Leave", failure: "the recording was cancelled - nothing was saved" });
  });
  expect(await screen.findByRole("button", { name: "Record a module…" })).toBeInTheDocument();
  expect(screen.queryByText("the recording was cancelled - nothing was saved")).not.toBeInTheDocument();
  expect(toast.info).toHaveBeenCalledWith("The recording was cancelled. Nothing was saved.");
});

/// Review M3: a Try runs the same check, and can be cancelled the same way.
test("a Try can be cancelled, and a cancelled Try is not shown as the path failing", async () => {
  let resolveTry: ((v: unknown) => void) | null = null;
  let cancels = 0;
  mount((cmd) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [LEAVE] };
    if (cmd === "auto_run_try_module_path") {
      return new Promise((resolve) => {
        resolveTry = resolve;
      });
    }
    if (cmd === "auto_run_record_cancel") {
      cancels += 1;
      return null;
    }
  });
  const tryIt = await screen.findByRole("button", { name: "Try Leave" });
  await waitFor(() => expect(tryIt).toBeEnabled());
  fireEvent.click(tryIt);
  fireEvent.click(await screen.findByRole("button", { name: "Cancel trying Leave" }));
  await waitFor(() => expect(cancels).toBe(1));
  await act(async () => {
    resolveTry?.({ ok: false, detail: "the recording was cancelled - nothing was saved" });
  });
  await waitFor(() => expect(toast.info).toHaveBeenCalledWith("Stopped trying Leave."));
  expect(screen.queryByText("the recording was cancelled - nothing was saved")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Cancel trying Leave" })).not.toBeInTheDocument();
});

/// Review M5: leaving the Auto Run section mid-recording unmounts the
/// dialog but not the recording. A dialog opened afresh offers to end it.
test("a recording left open from before can be cancelled from a freshly opened dialog", async () => {
  let open = true;
  let cancels = 0;
  mount((cmd) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_recording_is_open") return open;
    if (cmd === "auto_run_record_cancel") {
      cancels += 1;
      open = false;
      return null;
    }
  });
  fireEvent.click(await screen.findByRole("button", { name: "Cancel that recording" }));
  await waitFor(() => expect(cancels).toBe(1));
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Cancel that recording" })).not.toBeInTheDocument(),
  );
});

/// Re-review N1: once this dialog starts something of its own, the offer
/// must go - pressing it then would cancel this dialog's own work.
test("the leftover-recording offer is gone once Record starts", async () => {
  let cancels = 0;
  mount((cmd) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_recording_is_open") return true;
    if (cmd === "auto_run_record_start") return new Promise(() => {});
    if (cmd === "auto_run_record_cancel") {
      cancels += 1;
      return null;
    }
  });
  await screen.findByRole("button", { name: "Cancel that recording" });
  await chooseAndStart();
  expect(await screen.findByText(/Opening the browser and signing in/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Cancel that recording" })).not.toBeInTheDocument();
  expect(cancels).toBe(0);
});

/// Re-review N1: the offer is asked about once, on opening. By the time it
/// is pressed the leftover may have ended by itself (a check finishing),
/// and then there is nothing of anyone's to cancel.
test("a stale leftover-recording offer asks again and cancels nothing once the recorder is free", async () => {
  let open = true;
  let cancels = 0;
  mount((cmd) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_recording_is_open") return open;
    if (cmd === "auto_run_record_cancel") {
      cancels += 1;
      return null;
    }
  });
  const offer = await screen.findByRole("button", { name: "Cancel that recording" });
  open = false;
  fireEvent.click(offer);
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Cancel that recording" })).not.toBeInTheDocument(),
  );
  expect(cancels).toBe(0);
  expect(toast.info).not.toHaveBeenCalled();
});

test("nothing is offered to cancel when nothing was left open", async () => {
  let asked = 0;
  mount((cmd) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_recording_is_open") {
      asked += 1;
      return false;
    }
  });
  await waitFor(() => expect(asked).toBe(1));
  expect(await screen.findByRole("button", { name: "Record a module…" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Cancel that recording" })).not.toBeInTheDocument();
});

test("Escape while starting cancels instead of being ignored", async () => {
  let cancels = 0;
  mount((cmd) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_record_start") return new Promise(() => {});
    if (cmd === "auto_run_record_cancel") {
      cancels += 1;
      return null;
    }
  });
  await chooseAndStart();
  await screen.findByRole("button", { name: "Cancel" });

  fireEvent.keyDown(window, { key: "Escape" });
  await waitFor(() => expect(cancels).toBe(1));
});
