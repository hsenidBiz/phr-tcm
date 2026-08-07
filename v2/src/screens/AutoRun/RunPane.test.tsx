// Walking a selection of cases in one supervised session.
//
// The invariant under test: a selection is ONE run. Verdicts are banked as
// the person moves through the cases and written once at the end (or when
// they walk away), never one file per case.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import RunPane from "./RunPane";

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.restoreAllMocks();
});

const STEPS = [{ step_number: 1, actions: [{ kind: "check_text", value: "ok" }] }];

type Saved = { id: string; pbi_id: number; cases: { case_id: number; verdict: string }[] };

/** Records every run written to disk and every browser launch, so a test
 * can assert on how MANY of each happened, not just that they did. */
function mockSession() {
  const saved: Saved[] = [];
  const launched: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_load_script") return { case_id: 1, title: "s", steps: STEPS };
    if (cmd === "auto_run_open_browser") {
      launched.push((args as { browserName: string }).browserName);
      return null;
    }
    if (cmd === "auto_run_close_browser") return null;
    if (cmd === "auto_run_new_id") return "run-1";
    if (cmd === "auto_run_save_run") {
      saved.push((args as { run: Saved }).run);
      return null;
    }
    return null;
  });
  return { saved, launched };
}

function renderPane(cases: { id: number; title: string }[], onClose = vi.fn()) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <RunPane pbiId={42} cases={cases} onClose={onClose} />
    </QueryClientProvider>,
  );
  return onClose;
}

test("two cases are one run: the browser opens once and one file is written", async () => {
  const s = mockSession();
  const onClose = renderPane([
    { id: 1, title: "Valid login" },
    { id: 2, title: "Locked account" },
  ]);

  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  await waitFor(() => expect(s.launched).toEqual(["edge"]));

  // Case 1: mark, then advance. Nothing may reach disk yet.
  fireEvent.click(screen.getByRole("button", { name: "Passed" }));
  fireEvent.click(screen.getByRole("button", { name: /Save and next case/ }));
  expect(await screen.findByText("case 2 of 2")).toBeInTheDocument();
  expect(s.saved).toHaveLength(0);

  // The verdict does not carry over to the next case - each is decided
  // on its own, so Save is unavailable until this one is marked too.
  expect(screen.getByRole("button", { name: /Save result/ })).toBeDisabled();

  fireEvent.click(screen.getByRole("button", { name: "Failed" }));
  fireEvent.click(screen.getByRole("button", { name: /Save result/ }));

  await waitFor(() => expect(s.saved).toHaveLength(1));
  expect(s.saved[0].cases.map((c) => [c.case_id, c.verdict])).toEqual([
    [1, "Passed"],
    [2, "Failed"],
  ]);
  // One launch for the whole selection, one close at the end.
  expect(s.launched).toHaveLength(1);
  await waitFor(() => expect(onClose).toHaveBeenCalled());
});

test("closing part-way keeps the verdicts already marked", async () => {
  const s = mockSession();
  renderPane([
    { id: 1, title: "Valid login" },
    { id: 2, title: "Locked account" },
  ]);

  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(screen.getByRole("button", { name: "Passed" }));
  fireEvent.click(screen.getByRole("button", { name: /Save and next case/ }));
  await screen.findByText("case 2 of 2");

  // Walking away with case 2 unmarked must not throw away case 1.
  fireEvent.click(screen.getByRole("button", { name: /Close/ }));
  await waitFor(() => expect(s.saved).toHaveLength(1));
  expect(s.saved[0].cases.map((c) => c.case_id)).toEqual([1]);
});

test("closing with nothing marked writes no run at all", async () => {
  const s = mockSession();
  renderPane([{ id: 1, title: "Valid login" }]);

  fireEvent.click(await screen.findByRole("button", { name: /Close/ }));
  await waitFor(() => expect(s.saved).toHaveLength(0));
});

test("the browser choice is remembered and sent to the launcher", async () => {
  const s = mockSession();
  renderPane([{ id: 1, title: "Valid login" }]);

  // The app's Select is a themed listbox, not a native <select> - drive it
  // the way a person does: open the trigger, pick the option.
  fireEvent.click(await screen.findByRole("combobox", { name: "Browser to run in" }));
  fireEvent.click(screen.getByRole("option", { name: "Google Chrome" }));
  fireEvent.click(screen.getByRole("button", { name: "Open browser" }));

  await waitFor(() => expect(s.launched).toEqual(["chrome"]));
  expect(localStorage.getItem("tcm-v2-autorun-browser")).toBe("chrome");
});

test("a single case shows no progress counter and saves on the first verdict", async () => {
  const s = mockSession();
  renderPane([{ id: 1, title: "Valid login" }]);

  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  expect(screen.queryByText(/case 1 of/)).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Blocked" }));
  fireEvent.click(screen.getByRole("button", { name: /Save result/ }));
  await waitFor(() => expect(s.saved).toHaveLength(1));
  expect(s.saved[0].cases).toHaveLength(1);
});
