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
  cases: { id: number; title: string }[],
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
      { case_id: 1, title: "A" },
      { case_id: 2, title: "B" },
    ],
    browserName: "edge",
    watch: false,
  });
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
