// Starting and watching an unattended run.
//
// The invariant under test: `auto_run_replay` is one call for the whole
// selection, progress arrives as events keyed to the run it belongs to, and
// the dialog cannot be walked away from while a run is actually going -
// only Stop ends it.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ReplayPane, { statusOf } from "./ReplayPane";

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.restoreAllMocks();
});

function renderPane(
  cases: { id: number; title: string; steps?: { action: string; expected: string }[] }[],
  overrides: { onClose?: () => void; onFinished?: (runId: string) => void } = {},
) {
  const onClose = overrides.onClose ?? vi.fn();
  const onFinished = overrides.onFinished ?? vi.fn();
  render(
    <ReplayPane
      org="acme"
      project="Web"
      pbiId={42}
      cases={cases}
      onClose={onClose}
      onFinished={onFinished}
    />,
  );
  return { onClose, onFinished };
}

/** A payload for one `replay-progress` event, with sane defaults for the
 * fields a given phase doesn't care about. */
function progress(overrides: Partial<Record<string, unknown>>) {
  return {
    run_id: "run-9",
    index: 0,
    total: 1,
    case_id: 1,
    title: "Case A",
    phase: "opening",
    step_number: 0,
    steps: 1,
    proposed: "",
    ...overrides,
  };
}

test("statusOf reads a progress event's phase", () => {
  expect(statusOf({ phase: "opening", step_number: 0, steps: 3, proposed: "" })).toBe(
    "Opening the browser",
  );
  expect(statusOf({ phase: "signing_in", step_number: 0, steps: 3, proposed: "" })).toBe(
    "Signing in",
  );
  expect(statusOf({ phase: "step", step_number: 2, steps: 3, proposed: "" })).toBe("Step 2 of 3");
  expect(statusOf({ phase: "done", step_number: 3, steps: 3, proposed: "Failed" })).toBe(
    "Proposed: Failed",
  );
  expect(statusOf({ phase: "done", step_number: 3, steps: 3, proposed: "" })).toBe(
    "Nothing proposed",
  );
  expect(statusOf({ phase: "module", step_number: -1, steps: 3, proposed: "" })).toBe(
    "Going to the module",
  );
});

test("start sends the selection, the browser and the watch choice", async () => {
  const calls: unknown[] = [];
  mockIPC(
    (cmd, args) => {
      if (cmd === "auto_run_replay") {
        calls.push(args);
        return new Promise(() => {});
      }
      return null;
    },
    { shouldMockEvents: true },
  );

  renderPane([
    { id: 1, title: "A" },
    { id: 2, title: "B" },
  ]);

  fireEvent.click(await screen.findByRole("button", { name: "Start" }));

  await waitFor(() => expect(calls).toHaveLength(1));
  expect(calls[0]).toEqual({
    organization: "acme",
    project: "Web",
    pbiId: 42,
    cases: [
      { case_id: 1, title: "A", module: null },
      { case_id: 2, title: "B", module: null },
    ],
    account: null,
    browserName: "edge",
    watch: false,
    retryTransient: true,
    dbReadAccess: true,
  });
});

test("the run is told Database Read Access is off, so it checks no precondition", async () => {
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["db_lookup", "db_query"]));
  const calls: { dbReadAccess: boolean }[] = [];
  mockIPC(
    (cmd, args) => {
      if (cmd === "auto_run_replay") {
        calls.push(args as { dbReadAccess: boolean });
        return new Promise(() => {});
      }
      return null;
    },
    { shouldMockEvents: true },
  );
  render(
    <ReplayPane
      org="acme"
      project="Web"
      pbiId={42}
      cases={[{ id: 1, title: "A" }]}
      onClose={vi.fn()}
      onFinished={vi.fn()}
    />,
  );
  fireEvent.click(await screen.findByRole("button", { name: "Start" }));
  await waitFor(() => expect(calls).toEqual([expect.objectContaining({ dbReadAccess: false })]));
});

test("retrying transient failures is on by default and a remembered choice", async () => {
  const calls: { retryTransient: boolean }[] = [];
  mockIPC(
    (cmd, args) => {
      if (cmd === "auto_run_replay") {
        calls.push(args as { retryTransient: boolean });
        return new Promise(() => {});
      }
      return null;
    },
    { shouldMockEvents: true },
  );

  const { unmount } = render(
    <ReplayPane
      org="acme"
      project="Web"
      pbiId={42}
      cases={[{ id: 1, title: "A" }]}
      onClose={vi.fn()}
      onFinished={vi.fn()}
    />,
  );

  const option = await screen.findByRole("checkbox", { name: "Retry transient failures once" });
  expect(option).toHaveAttribute("aria-checked", "true");
  fireEvent.click(option);
  fireEvent.click(screen.getByRole("button", { name: "Start" }));

  await waitFor(() => expect(calls).toEqual([expect.objectContaining({ retryTransient: false })]));
  expect(localStorage.getItem("tcm-v2-autorun-retry-transient")).toBe("0");
  unmount();

  render(
    <ReplayPane
      org="acme"
      project="Web"
      pbiId={42}
      cases={[{ id: 1, title: "A" }]}
      onClose={vi.fn()}
      onFinished={vi.fn()}
    />,
  );
  expect(
    await screen.findByRole("checkbox", { name: "Retry transient failures once" }),
  ).toHaveAttribute("aria-checked", "false");
});

test("watching is a remembered choice", async () => {
  const calls: { watch: boolean }[] = [];
  mockIPC(
    (cmd, args) => {
      if (cmd === "auto_run_replay") {
        calls.push(args as { watch: boolean });
        return new Promise(() => {});
      }
      return null;
    },
    { shouldMockEvents: true },
  );

  const { unmount } = render(
    <ReplayPane
      org="acme"
      project="Web"
      pbiId={42}
      cases={[{ id: 1, title: "A" }]}
      onClose={vi.fn()}
      onFinished={vi.fn()}
    />,
  );

  fireEvent.click(await screen.findByRole("checkbox", { name: "Watch the browser" }));
  fireEvent.click(screen.getByRole("button", { name: "Start" }));

  await waitFor(() => expect(calls).toEqual([expect.objectContaining({ watch: true })]));
  expect(localStorage.getItem("tcm-v2-autorun-watch")).toBe("1");
  unmount();

  render(
    <ReplayPane
      org="acme"
      project="Web"
      pbiId={42}
      cases={[{ id: 1, title: "A" }]}
      onClose={vi.fn()}
      onFinished={vi.fn()}
    />,
  );
  expect(await screen.findByRole("checkbox", { name: "Watch the browser" })).toHaveAttribute(
    "aria-checked",
    "true",
  );
});

test("progress moves the rows and the finished run is handed on", async () => {
  let resolveReplay: (run: { id: string }) => void = () => {};
  mockIPC(
    (cmd) => {
      if (cmd === "auto_run_replay") {
        return new Promise((resolve) => {
          resolveReplay = resolve as (run: { id: string }) => void;
        });
      }
      return null;
    },
    { shouldMockEvents: true },
  );

  const { onFinished } = renderPane([
    { id: 1, title: "Case A" },
    { id: 2, title: "Case B" },
  ]);

  fireEvent.click(await screen.findByRole("button", { name: "Start" }));

  const { emit } = await import("@tauri-apps/api/event");
  const send = (overrides: Partial<Record<string, unknown>>) =>
    act(async () => {
      await emit("replay-progress", progress(overrides));
    });

  await send({ phase: "opening" });
  expect(await screen.findByText("case 1 of 1")).toBeInTheDocument();
  expect(
    within(screen.getByText("Case A").closest("li")!).getByText("Opening the browser"),
  ).toBeInTheDocument();

  await send({ phase: "signing_in" });
  expect(
    within(screen.getByText("Case A").closest("li")!).getByText("Signing in"),
  ).toBeInTheDocument();

  await send({ phase: "step", step_number: 2, steps: 3 });
  expect(
    within(screen.getByText("Case A").closest("li")!).getByText("Step 2 of 3"),
  ).toBeInTheDocument();

  await send({ phase: "done", proposed: "Failed" });
  expect(
    within(screen.getByText("Case A").closest("li")!).getByText("Proposed: Failed"),
  ).toBeInTheDocument();

  await send({ index: 1, total: 2, case_id: 2, title: "Case B", phase: "opening" });
  expect(await screen.findByText("case 2 of 2")).toBeInTheDocument();

  await send({ index: 1, total: 2, case_id: 2, title: "Case B", phase: "done", proposed: "" });
  expect(
    within(screen.getByText("Case B").closest("li")!).getByText("Nothing proposed"),
  ).toBeInTheDocument();

  await act(async () => {
    resolveReplay({ id: "run-9" });
    await Promise.resolve();
  });
  expect(onFinished).toHaveBeenCalledWith("run-9");
});

test("progress for another run is ignored", async () => {
  mockIPC(
    (cmd) => {
      if (cmd === "auto_run_replay") return new Promise(() => {});
      return null;
    },
    { shouldMockEvents: true },
  );

  renderPane([{ id: 1, title: "Case A" }]);
  fireEvent.click(await screen.findByRole("button", { name: "Start" }));

  const { emit } = await import("@tauri-apps/api/event");
  // The first event this pane sees fixes which run_id it now belongs to.
  await act(async () => {
    await emit("replay-progress", progress({ run_id: "run-1", phase: "opening" }));
  });
  expect(
    within(screen.getByText("Case A").closest("li")!).getByText("Opening the browser"),
  ).toBeInTheDocument();

  // An event for a DIFFERENT run must not touch what is on screen.
  await act(async () => {
    await emit("replay-progress", progress({ run_id: "run-2", phase: "done", proposed: "Passed" }));
  });
  expect(
    within(screen.getByText("Case A").closest("li")!).getByText("Opening the browser"),
  ).toBeInTheDocument();
  expect(screen.queryByText("Proposed: Passed")).not.toBeInTheDocument();
});

test("stop asks the run to stop and says so, and the dialog cannot be dismissed meanwhile", async () => {
  let cancelCalls = 0;
  mockIPC(
    (cmd) => {
      if (cmd === "auto_run_replay") return new Promise(() => {});
      if (cmd === "auto_run_replay_cancel") {
        cancelCalls += 1;
        return null;
      }
      return null;
    },
    { shouldMockEvents: true },
  );

  const { onClose } = renderPane([{ id: 1, title: "Case A" }]);
  fireEvent.click(await screen.findByRole("button", { name: "Start" }));

  fireEvent.click(await screen.findByRole("button", { name: "Stop" }));
  const stopBtn = await screen.findByRole("button", { name: "Stopping after this step" });
  expect(stopBtn).toBeDisabled();
  await waitFor(() => expect(cancelCalls).toBe(1));

  fireEvent.keyDown(window, { key: "Escape" });
  expect(onClose).not.toHaveBeenCalled();
});

test("a run that cannot start says why and lets the person try again", async () => {
  mockIPC(
    (cmd) => {
      if (cmd === "auto_run_replay") {
        // A plain (non-Error) throw is what `typedError` turns into
        // `{status: "error", error}` - the shape the pane's own error
        // discipline is built around.
        // eslint-disable-next-line no-throw-literal
        throw "an unattended run is already going - wait for it, or stop it first";
      }
      return null;
    },
    { shouldMockEvents: true },
  );

  const { onFinished } = renderPane([{ id: 1, title: "Case A" }]);
  fireEvent.click(await screen.findByRole("button", { name: "Start" }));

  expect(
    await screen.findByText("an unattended run is already going - wait for it, or stop it first"),
  ).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Start" })).toBeEnabled();
  expect(onFinished).not.toHaveBeenCalled();
});

const ACCOUNTS = [{ key: "hr.admin", label: "HR Admin", username: "kim", password: "p" }];

function mountForAccount(calls: unknown[], asked: string[] = []) {
  mockIPC(
    (cmd, args) => {
      asked.push(String(cmd));
      if (cmd === "auto_run_list_accounts") return ACCOUNTS;
      if (cmd === "auto_run_replay") {
        calls.push(args);
        return new Promise(() => {});
      }
      return null;
    },
    { shouldMockEvents: true },
  );
  return render(
    <ReplayPane
      org="acme"
      project="Web"
      pbiId={42}
      cases={[{ id: 1, title: "A", module: " Leave " }]}
      onClose={vi.fn()}
      onFinished={vi.fn()}
    />,
  );
}

test("the account for the run is remembered per project and sent with each case's module", async () => {
  const calls: unknown[] = [];
  const { unmount } = mountForAccount(calls);
  const pick = await screen.findByRole("combobox", { name: "Sign in as" });
  expect(pick).toHaveTextContent("Each script's own account");
  fireEvent.click(pick);
  fireEvent.click(await screen.findByRole("option", { name: "HR Admin (hr.admin)" }));
  expect(localStorage.getItem("tcm-v2-autorun-run-account:acme/Web")).toBe("hr.admin");
  fireEvent.click(screen.getByRole("button", { name: "Start" }));
  await waitFor(() => expect(calls).toHaveLength(1));
  expect(calls[0]).toEqual(
    expect.objectContaining({ account: "hr.admin", cases: [{ case_id: 1, title: "A", module: "Leave" }] }),
  );
  unmount();

  mountForAccount([]);
  await waitFor(() =>
    expect(screen.getByRole("combobox", { name: "Sign in as" })).toHaveTextContent("HR Admin (hr.admin)"),
  );
});

/// Review focus 3.
test("an account removed since it was picked falls back to each script's own, silently", async () => {
  localStorage.setItem("tcm-v2-autorun-run-account:acme/Web", "gone.user");
  const calls: unknown[] = [];
  const asked: string[] = [];
  mountForAccount(calls, asked);
  const pick = await screen.findByRole("combobox", { name: "Sign in as" });
  // Let the accounts list arrive, so the fallback is judged against it.
  await waitFor(() => expect(asked).toContain("auto_run_list_accounts"));
  await act(async () => {
    await Promise.resolve();
  });
  expect(pick).toHaveTextContent("Each script's own account");
  fireEvent.click(screen.getByRole("button", { name: "Start" }));
  await waitFor(() => expect(calls).toHaveLength(1));
  expect(calls[0]).toEqual(expect.objectContaining({ account: null }));
  expect(screen.queryByText(/not on this machine/)).not.toBeInTheDocument();
});

// ---------------------------------------------------------------------------
// The dialog shows each case's written steps, live.
// ---------------------------------------------------------------------------

const STEPS = [
  { action: "Open the leave form", expected: "The form shows" },
  { action: "Pick a date", expected: "The date is accepted" },
  { action: "Submit it", expected: "A receipt shows" },
];

type StepCase = { id: number; title: string; steps?: { action: string; expected: string }[] };

const CASES: StepCase[] = [
  { id: 1, title: "Case A", steps: STEPS },
  { id: 2, title: "Case B", steps: STEPS },
  { id: 3, title: "Case C", steps: STEPS },
];

/** A fake `matchMedia` for the window-height query: `set` flips the answer
 * and fires `change` on every listener, as a resize across 900px would. */
function mockTallWindow(initial: boolean) {
  const real = window.matchMedia;
  let matches = initial;
  const listeners = new Set<() => void>();
  window.matchMedia = ((query: string) => ({
    get matches() {
      return matches;
    },
    media: query,
    onchange: null,
    addListener: (l: () => void) => listeners.add(l),
    removeListener: (l: () => void) => listeners.delete(l),
    addEventListener: (_t: string, l: () => void) => listeners.add(l),
    removeEventListener: (_t: string, l: () => void) => listeners.delete(l),
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
  return {
    set(next: boolean) {
      matches = next;
      act(() => listeners.forEach((l) => l()));
    },
    restore() {
      window.matchMedia = real;
    },
  };
}

type RunFile = { cases: unknown[] } | null;

/** Open the pane on `cases` and press Start; `loadRun` answers
 * `auto_run_load_run` (a throw is a failed load). */
async function startRun(cases: StepCase[], loadRun: () => RunFile = () => null) {
  mockIPC(
    (cmd) => {
      if (cmd === "auto_run_replay") return new Promise(() => {});
      if (cmd === "auto_run_load_run") return loadRun();
      return null;
    },
    { shouldMockEvents: true },
  );
  renderPane(cases);
  fireEvent.click(await screen.findByRole("button", { name: "Start" }));
  const { emit } = await import("@tauri-apps/api/event");
  return (overrides: Partial<Record<string, unknown>>) =>
    act(async () => {
      await emit("replay-progress", progress({ total: cases.length, steps: 3, ...overrides }));
    });
}

const toggle = (id: number, shown: boolean) =>
  screen.getByRole("button", { name: `${shown ? "Hide" : "Show"} steps for #${id}` });
const stepsOf = (id: number) => screen.queryByRole("list", { name: `Steps of #${id}` });

test("the running case opens by itself, locked, with the current step marked", async () => {
  const mq = mockTallWindow(false);
  try {
    const send = await startRun(CASES);

    await send({ phase: "opening" });
    const t = toggle(1, true);
    expect(t).toHaveAttribute("aria-expanded", "true");
    expect(t).toBeDisabled();
    expect(t).toHaveAttribute("title", "Open while it runs");
    expect(document.getElementById(t.getAttribute("aria-controls")!)).toContainElement(stepsOf(1));
    // The phase line sits above the steps, before step 1 has started.
    const open = screen.getByText("Case A").closest("li")!;
    expect(within(open).getAllByText("Opening the browser")).toHaveLength(2);
    expect(within(stepsOf(1)!).queryByText("Checking now")).not.toBeInTheDocument();

    await send({ phase: "step", step_number: 2 });
    const items = within(stepsOf(1)!).getAllByRole("listitem");
    expect(items).toHaveLength(3);
    expect(items[0]).not.toHaveAttribute("aria-current");
    expect(within(items[0]).getByText("Done")).toBeInTheDocument();
    expect(items[1]).toHaveAttribute("aria-current", "step");
    expect(within(items[1]).getByText("Checking now")).toBeInTheDocument();
    expect(within(items[1]).getByText(/Pick a date/)).toBeInTheDocument();
    expect(items[2]).not.toHaveAttribute("aria-current");
    expect(within(items[2]).queryByText("Done")).not.toBeInTheDocument();
    expect(within(items[2]).queryByText("Checking now")).not.toBeInTheDocument();

    // Other rows stay shut: only the running case opens in a short window.
    expect(toggle(2, false)).toHaveAttribute("aria-expanded", "false");
    expect(stepsOf(2)).not.toBeInTheDocument();
  } finally {
    mq.restore();
  }
});

test("the next case's first event closes the finished one and opens the new one", async () => {
  const mq = mockTallWindow(false);
  try {
    const send = await startRun(CASES);
    await send({ phase: "step", step_number: 1 });
    expect(stepsOf(1)).toBeInTheDocument();

    await send({ phase: "done", step_number: 3, proposed: "Passed" });
    expect(toggle(1, false)).toHaveAttribute("aria-expanded", "false");
    expect(stepsOf(1)).not.toBeInTheDocument();

    await send({ index: 1, case_id: 2, title: "Case B", phase: "opening" });
    expect(toggle(2, true)).toBeDisabled();
    expect(stepsOf(2)).toBeInTheDocument();
    expect(stepsOf(1)).not.toBeInTheDocument();
  } finally {
    mq.restore();
  }
});

test("any row that is not running opens and closes by hand", async () => {
  const mq = mockTallWindow(false);
  try {
    const send = await startRun(CASES);
    await send({ phase: "step", step_number: 1 });

    // A future case.
    fireEvent.click(toggle(3, false));
    expect(toggle(3, true)).toHaveAttribute("aria-expanded", "true");
    expect(toggle(3, true)).toBeEnabled();
    expect(within(stepsOf(3)!).getAllByRole("listitem")).toHaveLength(3);
    expect(within(stepsOf(3)!).queryByText("Checking now")).not.toBeInTheDocument();
    fireEvent.click(toggle(3, true));
    expect(toggle(3, false)).toHaveAttribute("aria-expanded", "false");
    expect(stepsOf(3)).not.toBeInTheDocument();

    // A finished case reopens and closes the same way.
    await send({ phase: "done", proposed: "Passed" });
    fireEvent.click(toggle(1, false));
    expect(stepsOf(1)).toBeInTheDocument();
    fireEvent.click(toggle(1, true));
    expect(stepsOf(1)).not.toBeInTheDocument();
  } finally {
    mq.restore();
  }
});

test("a finished case shows Passed, Failed with its detail, and Not reached", async () => {
  const mq = mockTallWindow(false);
  try {
    const send = await startRun(CASES, () => ({
      id: "run-9",
      cases: [
        {
          case_id: 1,
          title: "Case A",
          steps: [
            { step_number: 1, outcomes: [{ ok: true, detail: "ok" }, { ok: true, detail: "ok" }] },
            {
              step_number: 2,
              outcomes: [
                { ok: true, detail: "ok" },
                { ok: false, detail: "No element matched #date" },
                { ok: false, detail: "second failure" },
              ],
            },
          ],
        },
      ],
    }));
    await send({ phase: "step", step_number: 1 });
    await send({ phase: "done", proposed: "Failed" });

    fireEvent.click(toggle(1, false));
    const items = within(stepsOf(1)!).getAllByRole("listitem");
    expect(within(items[0]).getByText("Passed")).toBeInTheDocument();
    expect(within(items[1]).getByText("Failed")).toBeInTheDocument();
    expect(within(items[1]).getByText("No element matched #date")).toBeInTheDocument();
    expect(within(items[1]).queryByText("second failure")).not.toBeInTheDocument();
    expect(within(items[2]).getByText("Not reached")).toBeInTheDocument();
  } finally {
    mq.restore();
  }
});

test("a run file that cannot be read leaves a finished case on its status line", async () => {
  const mq = mockTallWindow(false);
  try {
    const send = await startRun(CASES, () => {
      // eslint-disable-next-line no-throw-literal
      throw "no such run";
    });
    await send({ phase: "step", step_number: 1 });
    await send({ phase: "done", proposed: "Passed" });

    fireEvent.click(toggle(1, false));
    const items = within(stepsOf(1)!).getAllByRole("listitem");
    expect(items).toHaveLength(3);
    expect(screen.queryByText("Passed")).not.toBeInTheDocument();
    expect(screen.queryByText("Not reached")).not.toBeInTheDocument();
    expect(
      within(screen.getByText("Case A").closest("li")!).getByText("Proposed: Passed"),
    ).toBeInTheDocument();
  } finally {
    mq.restore();
  }
});

test("a case with no written steps keeps to its status line", async () => {
  const mq = mockTallWindow(false);
  try {
    const send = await startRun([{ id: 1, title: "Case A" }]);
    await send({ phase: "step", step_number: 2 });
    const li = screen.getByText("Case A").closest("li")!;
    expect(within(li).getAllByText("Step 2 of 3")).toHaveLength(1);
    expect(within(li).queryByRole("list")).not.toBeInTheDocument();
  } finally {
    mq.restore();
  }
});

test("a tall window also opens the next case, and only the running one is locked", async () => {
  const mq = mockTallWindow(true);
  try {
    const send = await startRun(CASES);
    await send({ phase: "step", step_number: 1 });

    expect(stepsOf(1)).toBeInTheDocument();
    expect(stepsOf(2)).toBeInTheDocument();
    expect(stepsOf(3)).not.toBeInTheDocument();
    expect(toggle(1, true)).toBeDisabled();
    expect(toggle(2, true)).toBeEnabled();
    // The next case's steps are plain: nothing is checked there yet.
    expect(within(stepsOf(2)!).queryByText("Checking now")).not.toBeInTheDocument();

    // The person can close it ...
    fireEvent.click(toggle(2, true));
    expect(stepsOf(2)).not.toBeInTheDocument();
    // ... and it stays closed when the window changes size.
    mq.set(false);
    mq.set(true);
    expect(stepsOf(2)).not.toBeInTheDocument();

    // Until it is the case running: then it opens, locked, and the case
    // after it opens too, while the finished one folds.
    await send({ phase: "done", proposed: "Passed" });
    await send({ index: 1, case_id: 2, title: "Case B", phase: "opening" });
    expect(stepsOf(1)).not.toBeInTheDocument();
    expect(stepsOf(2)).toBeInTheDocument();
    expect(toggle(2, true)).toBeDisabled();
    expect(stepsOf(3)).toBeInTheDocument();
    expect(toggle(3, true)).toBeEnabled();
  } finally {
    mq.restore();
  }
});

test("a short window opens only the running case, and a resize switches between the two", async () => {
  const mq = mockTallWindow(false);
  try {
    const send = await startRun(CASES);
    await send({ phase: "step", step_number: 1 });
    expect(stepsOf(1)).toBeInTheDocument();
    expect(stepsOf(2)).not.toBeInTheDocument();

    mq.set(true);
    expect(stepsOf(1)).toBeInTheDocument();
    expect(stepsOf(2)).toBeInTheDocument();

    mq.set(false);
    expect(stepsOf(1)).toBeInTheDocument();
    expect(stepsOf(2)).not.toBeInTheDocument();
  } finally {
    mq.restore();
  }
});

test("the list follows the running case until the person scrolls it, then offers Follow the run", async () => {
  const mq = mockTallWindow(false);
  const scroll = vi.spyOn(Element.prototype, "scrollIntoView").mockImplementation(() => {});
  try {
    const send = await startRun(CASES);
    expect(scroll).not.toHaveBeenCalled();

    await send({ phase: "opening" });
    expect(scroll).toHaveBeenCalledTimes(1);
    expect(scroll).toHaveBeenLastCalledWith({ block: "nearest" });
    expect(scroll.mock.contexts[0]).toBe(screen.getByText("Case A").closest("li"));
    expect(screen.queryByRole("button", { name: "Follow the run" })).not.toBeInTheDocument();

    // More events for the SAME case do not scroll again.
    await send({ phase: "step", step_number: 1 });
    expect(scroll).toHaveBeenCalledTimes(1);

    // The person takes the wheel: the next case does not pull the list.
    fireEvent.wheel(screen.getByRole("list", { name: "Cases in this run" }));
    expect(await screen.findByRole("button", { name: "Follow the run" })).toBeInTheDocument();
    await send({ phase: "done", proposed: "Passed" });
    await send({ index: 1, case_id: 2, title: "Case B", phase: "opening" });
    expect(scroll).toHaveBeenCalledTimes(1);

    // Follow the run scrolls to the case running now and resumes.
    fireEvent.click(screen.getByRole("button", { name: "Follow the run" }));
    expect(scroll).toHaveBeenCalledTimes(2);
    expect(scroll.mock.contexts[1]).toBe(screen.getByText("Case B").closest("li"));
    expect(screen.queryByRole("button", { name: "Follow the run" })).not.toBeInTheDocument();

    await send({ index: 2, case_id: 3, title: "Case C", phase: "opening" });
    expect(scroll).toHaveBeenCalledTimes(3);
    expect(scroll.mock.contexts[2]).toBe(screen.getByText("Case C").closest("li"));
  } finally {
    mq.restore();
  }
});

test("scrolling the list by keyboard or touch also pauses following, but a plain scroll does not", async () => {
  const mq = mockTallWindow(false);
  vi.spyOn(Element.prototype, "scrollIntoView").mockImplementation(() => {});
  try {
    const send = await startRun(CASES);
    await send({ phase: "opening" });
    const list = screen.getByRole("list", { name: "Cases in this run" });

    // A scroll event alone is what scrollIntoView itself causes.
    fireEvent.scroll(list);
    expect(screen.queryByRole("button", { name: "Follow the run" })).not.toBeInTheDocument();

    fireEvent.keyDown(list, { key: "PageDown" });
    expect(screen.getByRole("button", { name: "Follow the run" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Follow the run" }));
    expect(screen.queryByRole("button", { name: "Follow the run" })).not.toBeInTheDocument();

    fireEvent.touchMove(list);
    expect(screen.getByRole("button", { name: "Follow the run" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Follow the run" }));

    // Dragging the scrollbar: a press on the list itself, then it scrolls.
    fireEvent.pointerDown(list);
    fireEvent.scroll(list);
    expect(screen.getByRole("button", { name: "Follow the run" })).toBeInTheDocument();
  } finally {
    mq.restore();
  }
});

// ---- the plan in the run dialog ----

const planOf = (phases: number[][], resets: unknown[]) =>
  ({ order: phases.flat(), phases, resets, counts: null, saved: false }) as never;

test("with more than one phase the dialog lists the phases and the reset line", async () => {
  mockIPC(() => null, { shouldMockEvents: true });
  render(
    <ReplayPane
      org="acme"
      project="Web"
      pbiId={42}
      cases={[
        { id: 1, title: "Publish the cycle" },
        { id: 2, title: "Open the report" },
      ]}
      plan={planOf([[1], [2]], [{ before_case_id: 2, names: ["Cycle"], changed_by: [["Cycle", [1]]] }])}
      onClose={vi.fn()}
      onFinished={vi.fn()}
    />,
  );
  const plan = await screen.findByLabelText("Run plan");
  expect(within(plan).getByText("Phase 1: 1 case")).toBeInTheDocument();
  expect(within(plan).getByText("Phase 2: 1 case")).toBeInTheDocument();
  expect(
    within(plan).getByText('Reset: revert "Cycle" (changed by #1 Publish the cycle)'),
  ).toBeInTheDocument();
});

test("with one phase the dialog shows nothing extra", async () => {
  mockIPC(() => null, { shouldMockEvents: true });
  render(
    <ReplayPane
      org="acme"
      project="Web"
      pbiId={42}
      cases={[{ id: 1, title: "A" }, { id: 2, title: "B" }]}
      plan={planOf([[1, 2]], [])}
      onClose={vi.fn()}
      onFinished={vi.fn()}
    />,
  );
  await screen.findByRole("button", { name: "Start" });
  expect(screen.queryByLabelText("Run plan")).not.toBeInTheDocument();
  expect(screen.queryByText(/Phase \d/)).not.toBeInTheDocument();
});
