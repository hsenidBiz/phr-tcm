// The Areas dialog: the recorded areas grouped by module, in words, Remove
// that asks first, the address switch, and a recording driven by mocked
// events.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "../../lib/toast";
import AreasDialog from "./AreasDialog";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

const ACCOUNTS = [{ key: "hr.admin", label: "HR Admin", username: "kim", password: "p" }];
const LEAVE = {
  area: "Leave",
  module: "Leave",
  clicks: ['link "Leave"', 'link "Apply Leave"'],
  arrived: "/hr/leave/apply",
  recorded: "2026-09-24T10:00:00Z",
};

const CYCLE_SETUP = {
  area: "Cycle Setup",
  module: "PMS",
  clicks: ['link "PMS"', 'link "Cycle Setup"'],
  arrived: "/pms/cycle/setup",
  recorded: "2026-09-24T10:00:00Z",
};
const MANAGE_CYCLE = {
  area: "Manage Cycle",
  module: "PMS",
  clicks: ['link "PMS"', 'link "Manage Cycle"'],
  arrived: "/pms/cycle/manage",
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
      <AreasDialog org="acme" project="Web" caseModules={["Leave", "Payroll", "PMS"]} onClose={vi.fn()} />
    </QueryClientProvider>,
  );
}

async function startRecording() {
  const record = await screen.findByRole("button", { name: "Record an area…" });
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
  const record = await screen.findByRole("button", { name: "Record an area…" });
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
  expect(screen.getByText("Remove the area Leave?")).toBeInTheDocument();
  expect(removed).toEqual([]);
  fireEvent.click(screen.getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(removed).toEqual([{ organization: "acme", project: "Web", area: "Leave" }]));
  expect(await screen.findByText(/No areas yet/)).toBeInTheDocument();
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
    if (cmd === "auto_run_record_stop") return { saved: true, module: "Leave", area: "Leave", failure: "" };
  });
  await startRecording();
  expect(started).toEqual([
    { organization: "acme", project: "Web", module: "Leave", area: "Leave", account: "hr.admin", browserName: "edge" },
  ]);
  expect(screen.getByRole("button", { name: "Stop" })).toBeDisabled();
  await send({ kind: "click", index: 1, readable: 'link "Leave"', detail: "" });
  expect(await screen.findByText('1. link "Leave"')).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  await waitFor(() => expect(toast.success).toHaveBeenCalledWith("Area Leave saved."));
  expect(await screen.findByRole("button", { name: "Record an area…" })).toBeInTheDocument();
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
      return { saved: false, module: "Leave", area: "Leave", failure: 'click 2, link "Apply Leave": no visible match' };
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

  expect(await screen.findByRole("button", { name: "Record an area…" })).toBeInTheDocument();
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
  expect(await screen.findByRole("button", { name: "Record an area…" })).toBeInTheDocument();
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
  expect(await screen.findByRole("button", { name: "Record an area…" })).toBeInTheDocument();
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
  expect(await screen.findByText("Checking the area Leave in a fresh browser…")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(cancels).toBe(1));
  await act(async () => {
    resolveStop?.({ saved: false, module: "Leave", area: "Leave", failure: "the recording was cancelled - nothing was saved" });
  });
  expect(await screen.findByRole("button", { name: "Record an area…" })).toBeInTheDocument();
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
    resolveTry?.({ ok: false, cancelled: true, detail: "the check was cancelled - the saved path was not changed" });
  });
  await waitFor(() => expect(toast.info).toHaveBeenCalledWith("Stopped trying Leave."));
  expect(screen.queryByText("the check was cancelled - the saved path was not changed")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Cancel trying Leave" })).not.toBeInTheDocument();
});

/// A Try cancelled from somewhere else - another dialog's "Cancel that
/// recording" - comes back cancelled though THIS dialog never asked. It is
/// still not the path failing.
test("a Try cancelled from elsewhere reads as stopped, not as the path failing", async () => {
  mount((cmd) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [LEAVE] };
    if (cmd === "auto_run_try_module_path") {
      return { ok: false, cancelled: true, detail: "the check was cancelled - the saved path was not changed" };
    }
  });
  const tryIt = await screen.findByRole("button", { name: "Try Leave" });
  await waitFor(() => expect(tryIt).toBeEnabled());
  fireEvent.click(tryIt);
  await waitFor(() => expect(toast.info).toHaveBeenCalledWith("Stopped trying Leave."));
  expect(screen.queryByText("the check was cancelled - the saved path was not changed")).not.toBeInTheDocument();
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
  expect(await screen.findByRole("button", { name: "Record an area…" })).toBeInTheDocument();
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

// ---- Areas: several named areas per module ----

test("the list groups areas under their module, each with its own buttons", async () => {
  mount((cmd) =>
    cmd === "auto_run_load_nav" ? { direct_urls: true, modules: [CYCLE_SETUP, LEAVE, MANAGE_CYCLE] } : undefined,
  );
  const pms = await screen.findByRole("group", { name: "PMS" });
  expect(within(pms).getByText("Cycle Setup")).toBeInTheDocument();
  expect(within(pms).getByText("Manage Cycle")).toBeInTheDocument();
  expect(within(pms).queryByText("Leave")).not.toBeInTheDocument();
  expect(within(screen.getByRole("group", { name: "Leave" })).getByRole("button", { name: "Try Leave" })).toBeInTheDocument();
  expect(within(pms).queryByRole("button", { name: "Try Leave" })).not.toBeInTheDocument();
  for (const area of ["Cycle Setup", "Manage Cycle", "Leave"]) {
    expect(screen.getByRole("button", { name: `Re-record ${area}` })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: `Try ${area}` })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: `Remove ${area}` })).toBeInTheDocument();
  }
});

async function openRecordForm() {
  const record = await screen.findByRole("button", { name: "Record an area…" });
  await waitFor(() => expect(record).toBeEnabled());
  fireEvent.click(record);
}

async function pickModule(name: string) {
  fireEvent.click(screen.getByRole("combobox", { name: "Module" }));
  fireEvent.click(await screen.findByRole("option", { name }));
}

test("Record an area asks for the module and the area, and offers the module's name for its first area", async () => {
  const started: unknown[] = [];
  mount((cmd, args) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [] };
    if (cmd === "auto_run_record_start") {
      started.push(args);
      return null;
    }
  });
  await openRecordForm();
  expect(screen.getByRole("textbox", { name: "Area name" })).toHaveValue("");
  await pickModule("PMS");
  expect(screen.getByRole("textbox", { name: "Area name" })).toHaveValue("PMS");
  // A name the person typed is theirs: picking another module leaves it.
  fireEvent.change(screen.getByRole("textbox", { name: "Area name" }), { target: { value: "Manage Cycle" } });
  await pickModule("Leave");
  expect(screen.getByRole("textbox", { name: "Area name" })).toHaveValue("Manage Cycle");
  await pickModule("PMS");
  fireEvent.click(screen.getByRole("button", { name: "Start recording" }));
  await screen.findByRole("button", { name: "Stop" });
  expect(started).toEqual([
    { organization: "acme", project: "Web", module: "PMS", area: "Manage Cycle", account: "hr.admin", browserName: "edge" },
  ]);
});

test("a module that already has an area is not offered its own name again", async () => {
  mount((cmd) => (cmd === "auto_run_load_nav" ? { direct_urls: true, modules: [MANAGE_CYCLE] } : undefined));
  await screen.findByText("Manage Cycle");
  await openRecordForm();
  await pickModule("PMS");
  expect(screen.getByRole("textbox", { name: "Area name" })).toHaveValue("");
  expect(screen.getByRole("button", { name: "Start recording" })).toBeDisabled();
});

test("an area name that is already recorded asks Replace before recording over it", async () => {
  const started: unknown[] = [];
  mount((cmd, args) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [CYCLE_SETUP, MANAGE_CYCLE] };
    if (cmd === "auto_run_record_start") {
      started.push(args);
      return null;
    }
  });
  await screen.findByText("Manage Cycle");
  await openRecordForm();
  await pickModule("PMS");
  fireEvent.change(screen.getByRole("textbox", { name: "Area name" }), { target: { value: " manage cycle " } });
  fireEvent.click(screen.getByRole("button", { name: "Start recording" }));
  expect(await screen.findByText("Replace Manage Cycle?")).toBeInTheDocument();
  expect(started).toEqual([]);
  // Keeping it goes back to the form with nothing started.
  fireEvent.click(screen.getByRole("button", { name: "Keep it" }));
  expect(started).toEqual([]);
  expect(screen.getByRole("textbox", { name: "Area name" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Start recording" }));
  fireEvent.click(await screen.findByRole("button", { name: "Replace" }));
  await screen.findByRole("button", { name: "Stop" });
  expect(started).toEqual([
    { organization: "acme", project: "Web", module: "PMS", area: "manage cycle", account: "hr.admin", browserName: "edge" },
  ]);
});

test("a name another module holds is refused in the form, and nothing starts", async () => {
  const started: unknown[] = [];
  mount((cmd, args) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [MANAGE_CYCLE] };
    if (cmd === "auto_run_record_start") {
      started.push(args);
      return null;
    }
  });
  await screen.findByText("Manage Cycle");
  await openRecordForm();
  await pickModule("Leave");
  fireEvent.change(screen.getByRole("textbox", { name: "Area name" }), { target: { value: "Manage Cycle" } });
  fireEvent.click(screen.getByRole("button", { name: "Start recording" }));
  expect(
    await screen.findByText('An area named "Manage Cycle" is already recorded under PMS - choose another name.'),
  ).toBeInTheDocument();
  expect(started).toEqual([]);
});

test("Re-record, Try and Remove act on that area, not on its module", async () => {
  const calls: Record<string, unknown>[] = [];
  mount((cmd, args) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [CYCLE_SETUP, MANAGE_CYCLE] };
    if (cmd === "auto_run_record_start") {
      calls.push({ cmd, ...args });
      return null;
    }
    if (cmd === "auto_run_try_module_path") {
      calls.push({ cmd, ...args });
      return { ok: true, cancelled: false, detail: "reached /pms/cycle/manage" };
    }
    if (cmd === "auto_run_remove_module_path") {
      calls.push({ cmd, ...args });
      return { direct_urls: true, modules: [CYCLE_SETUP] };
    }
  });
  const tryIt = await screen.findByRole("button", { name: "Try Manage Cycle" });
  await waitFor(() => expect(tryIt).toBeEnabled());
  fireEvent.click(tryIt);
  expect(await screen.findByText("reached /pms/cycle/manage")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Remove Manage Cycle" }));
  expect(screen.getByText("Remove the area Manage Cycle?")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(screen.queryByText("Manage Cycle")).not.toBeInTheDocument());
  // The other area under PMS stays, and Re-record starts without asking.
  fireEvent.click(screen.getByRole("button", { name: "Re-record Cycle Setup" }));
  await screen.findByRole("button", { name: "Stop" });
  expect(calls).toEqual([
    { cmd: "auto_run_try_module_path", organization: "acme", project: "Web", area: "Manage Cycle", account: "hr.admin", browserName: "edge" },
    { cmd: "auto_run_remove_module_path", organization: "acme", project: "Web", area: "Manage Cycle" },
    { cmd: "auto_run_record_start", organization: "acme", project: "Web", module: "PMS", area: "Cycle Setup", account: "hr.admin", browserName: "edge" },
  ]);
});
