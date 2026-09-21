// Walking a selection of cases in one supervised session.
//
// The invariant under test: a selection is ONE run. Verdicts are banked as
// the person moves through the cases and written once at the end (or when
// they walk away), never one file per case.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
function mockSession(opts: { saveFails?: boolean } = {}) {
  const saved: Saved[] = [];
  const launched: string[] = [];
  const closes: number[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_load_script") return { case_id: 1, title: "s", steps: STEPS };
    if (cmd === "auto_run_open_browser") {
      launched.push((args as { browserName: string }).browserName);
      return null;
    }
    if (cmd === "auto_run_close_browser") {
      closes.push(1);
      return null;
    }
    if (cmd === "auto_run_new_id") return "run-1";
    if (cmd === "auto_run_save_run") {
      if (opts.saveFails) throw new Error("disk full");
      saved.push((args as { run: Saved }).run);
      return null;
    }
    return null;
  });
  return { saved, launched, closes };
}

function renderPane(cases: { id: number; title: string }[], onClose = vi.fn()) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <RunPane org="acme" project="Web" pbiId={42} cases={cases} onClose={onClose} />
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

test("each case in a selection gets a fresh browser, so none inherits the last one's state", async () => {
  const s = mockSession();
  renderPane([
    { id: 1, title: "Valid login" },
    { id: 2, title: "Locked account" },
  ]);

  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  await waitFor(() => expect(s.launched).toHaveLength(1));

  fireEvent.click(screen.getByRole("button", { name: "Passed" }));
  fireEvent.click(screen.getByRole("button", { name: /Save and next case/ }));
  await screen.findByText("case 2 of 2");

  // Case 1 signed in; case 2 must not start signed in. The old window is
  // closed and a new one - new profile, no cookies - takes its place.
  await waitFor(() => expect(s.launched).toEqual(["edge", "edge"]));
  expect(s.closes.length).toBeGreaterThanOrEqual(1);
});

test("a failed write keeps the pane open instead of closing over lost verdicts", async () => {
  const s = mockSession({ saveFails: true });
  const onClose = renderPane([
    { id: 1, title: "Valid login" },
    { id: 2, title: "Locked account" },
  ]);

  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(screen.getByRole("button", { name: "Passed" }));
  fireEvent.click(screen.getByRole("button", { name: /Save and next case/ }));
  await screen.findByText("case 2 of 2");

  fireEvent.click(screen.getByRole("button", { name: /Close/ }));

  // The write failed, so the pane stays put with the verdict still in
  // hand - closing here would destroy it with no way back.
  await waitFor(() => expect(screen.getByText("case 2 of 2")).toBeInTheDocument());
  expect(onClose).not.toHaveBeenCalled();
  expect(s.saved).toHaveLength(0);
});

test("Close keeps a verdict marked on the case in front of you", async () => {
  const s = mockSession();
  renderPane([
    { id: 1, title: "Valid login" },
    { id: 2, title: "Locked account" },
  ]);

  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  // Marked but never saved - the buttons show it as chosen, so dropping
  // it silently would contradict what is on screen.
  fireEvent.click(screen.getByRole("button", { name: "Blocked" }));
  fireEvent.click(screen.getByRole("button", { name: /Close/ }));

  await waitFor(() => expect(s.saved).toHaveLength(1));
  expect(s.saved[0].cases.map((c) => [c.case_id, c.verdict])).toEqual([[1, "Blocked"]]);
});

test("the remembered browser is used on the next run without picking it again", async () => {
  localStorage.setItem("tcm-v2-autorun-browser", "chrome");
  const s = mockSession();
  renderPane([{ id: 1, title: "Valid login" }]);

  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  await waitFor(() => expect(s.launched).toEqual(["chrome"]));
});

test("a stored browser the picker cannot show falls back to Edge on screen and in the launch", async () => {
  localStorage.setItem("tcm-v2-autorun-browser", "netscape");
  const s = mockSession();
  renderPane([{ id: 1, title: "Valid login" }]);

  // A blank trigger with Rust quietly starting Edge would have the screen
  // and the run disagreeing about what was tested.
  expect(await screen.findByRole("combobox", { name: "Browser to run in" })).toHaveTextContent(
    "Microsoft Edge",
  );
  fireEvent.click(screen.getByRole("button", { name: "Open browser" }));
  await waitFor(() => expect(s.launched).toEqual(["edge"]));
});

test("a failed action offers the screenshot taken when it failed", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_load_script") return { case_id: 1, title: "s", steps: STEPS };
    if (cmd === "auto_run_new_id") return "run-1";
    if (cmd === "auto_run_step")
      return [
        { ok: true, detail: "loaded https://app.example/" },
        { ok: false, detail: 'waited 15000ms: button "Save" not found', screenshot: "shot-1-000001.jpg" },
      ];
    if (cmd === "auto_run_shot") {
      expect((args as { name: string }).name).toBe("shot-1-000001.jpg");
      return "data:image/jpeg;base64,AAAA";
    }
    return null;
  });
  const onClose = renderPane([{ id: 1, title: "Valid login" }]);

  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  const run = await screen.findByRole("button", { name: "Run step 1" });
  await waitFor(() => expect(run).toBeEnabled());
  fireEvent.click(run);

  expect(await screen.findByText(/button "Save" not found/)).toBeInTheDocument();
  // Only the failed action has one, and its name says which action.
  expect(screen.getAllByRole("button", { name: /View screenshot/ })).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "View screenshot for action 2" }));
  const img = await screen.findByRole("img", { name: "Screenshot of the failed action" });
  expect(img).toHaveAttribute("src", "data:image/jpeg;base64,AAAA");

  // The preview is a Modal nested inside the run pane's own Modal. Escape
  // must close only the preview - not the whole run underneath it.
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("img", { name: "Screenshot of the failed action" })).not.toBeInTheDocument();
  expect(onClose).not.toHaveBeenCalled();
});

test("a second Save while the first is still writing does not write twice", async () => {
  const s = mockSession();
  renderPane([{ id: 1, title: "Valid login" }]);

  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(screen.getByRole("button", { name: "Passed" }));

  // `new_run_id` is millisecond-resolution, so two writes a few ms apart
  // can land on one filename - and the shorter record list would win.
  const saveBtn = screen.getByRole("button", { name: /Save result/ });
  fireEvent.click(saveBtn);
  fireEvent.click(saveBtn);

  await waitFor(() => expect(s.saved).toHaveLength(1));
  // Settle any second write that a broken guard would have let through.
  await waitFor(() => expect(s.saved).toHaveLength(1));
});

/** A session whose scripts name accounts, recording every sign-in asked for. */
function mockSignIn(scripts: Record<number, unknown>, outcome: (key: string, nth: number) => unknown) {
  const asked: { organization: string; project: string; accountKey: string }[] = [];
  const forgotten: string[] = [];
  const stepped: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_load_script") return scripts[(args as { caseId: number }).caseId] ?? null;
    if (cmd === "auto_run_sign_in") {
      const a = args as { organization: string; project: string; accountKey: string };
      asked.push(a);
      return outcome(a.accountKey, asked.length);
    }
    if (cmd === "auto_run_forget_session") {
      forgotten.push((args as { accountKey: string }).accountKey);
      return null;
    }
    if (cmd === "auto_run_step") {
      stepped.push(args);
      return [{ ok: true, detail: "ok" }];
    }
    if (cmd === "auto_run_new_id") return "run-1";
    return null;
  });
  return { asked, forgotten, stepped };
}

const OK = { ok: true, detail: "signed in as HR Admin from a saved session", used_saved_session: true, steps: [] };

test("a case that names an account is signed in once the browser opens", async () => {
  const s = mockSignIn({ 1: { case_id: 1, title: "s", steps: STEPS, account: "hr.admin" } }, () => OK);
  renderPane([{ id: 1, title: "Leave request" }]);
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  expect(await screen.findByText("signed in as HR Admin from a saved session")).toBeInTheDocument();
  expect(s.asked).toEqual([{ organization: "acme", project: "Web", accountKey: "hr.admin" }]);
});

test("a case with no account signs nobody in", async () => {
  const s = mockSignIn({ 1: { case_id: 1, title: "s", steps: STEPS } }, () => OK);
  renderPane([{ id: 1, title: "Public page" }]);
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(await screen.findByRole("button", { name: /Run step 1/ }));
  await waitFor(() => expect(s.stepped).toHaveLength(1));
  expect(s.asked).toEqual([]);
});

test("the next case signs its own account into its own fresh browser", async () => {
  const s = mockSignIn(
    {
      1: { case_id: 1, title: "a", steps: STEPS, account: "employee" },
      2: { case_id: 2, title: "b", steps: STEPS, account: "supervisor" },
    },
    (key) => ({ ...OK, detail: `signed in as ${key}` }),
  );
  renderPane([{ id: 1, title: "Submit" }, { id: 2, title: "Approve" }]);
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  expect(await screen.findByText("signed in as employee")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Passed" }));
  fireEvent.click(screen.getByRole("button", { name: /Save and next case/ }));
  expect(await screen.findByText("signed in as supervisor")).toBeInTheDocument();
  expect(s.asked.map((a) => a.accountKey)).toEqual(["employee", "supervisor"]);
});

test("a sign-in that fails is shown with its steps, and the steps can still be run", async () => {
  const failed = {
    ok: false,
    detail: "sign-in stopped at step 3: button \"Login\" not found",
    used_saved_session: false,
    steps: [{ ok: true, detail: "loaded https://hr.example.internal/" }, { ok: false, detail: "button \"Login\" not found" }],
  };
  const s = mockSignIn({ 1: { case_id: 1, title: "s", steps: STEPS, account: "hr.admin" } }, (_k, nth) => (nth === 1 ? failed : OK));
  renderPane([{ id: 1, title: "Leave request" }]);
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  expect(await screen.findByText(/sign-in stopped at step 3/)).toBeInTheDocument();
  expect(screen.getByText("loaded https://hr.example.internal/")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Run step 1/ })).toBeEnabled();

  fireEvent.click(screen.getByRole("button", { name: "Sign in again" }));
  expect(await screen.findByText("signed in as HR Admin from a saved session")).toBeInTheDocument();
  expect(s.forgotten).toEqual(["hr.admin"]);
  expect(s.asked).toHaveLength(2);
});

test("a step is sent with the project it belongs to", async () => {
  const s = mockSignIn({ 1: { case_id: 1, title: "s", steps: STEPS } }, () => OK);
  renderPane([{ id: 1, title: "Public page" }]);
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(await screen.findByRole("button", { name: /Run step 1/ }));
  await waitFor(() => expect(s.stepped).toEqual([{ organization: "acme", project: "Web", step: STEPS[0] }]));
});

test("a verdict waits for the case's own sign-in to finish; Close stays available regardless", async () => {
  let resolveSignIn: (out: unknown) => void = () => {};
  mockIPC((cmd) => {
    if (cmd === "auto_run_load_script") return { case_id: 1, title: "s", steps: STEPS, account: "hr.admin" };
    if (cmd === "auto_run_sign_in") {
      // Never resolves until the test says so - the Rust side takes one
      // SESSION mutex per browser, so this stands in for a slow sign-in
      // without the test racing real IPC timing.
      return new Promise((resolve) => {
        resolveSignIn = resolve;
      });
    }
    if (cmd === "auto_run_new_id") return "run-1";
    return null;
  });
  renderPane([{ id: 1, title: "Leave request" }]);

  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  await screen.findByText("Signing in as hr.admin");

  // A verdict can be picked while the sign-in is still running - only
  // banking it is blocked.
  fireEvent.click(screen.getByRole("button", { name: "Passed" }));
  expect(screen.getByRole("button", { name: /Save result/ })).toBeDisabled();
  // Walking away from a slow sign-in must still work: the Rust-side mutex
  // makes it safe, and a person must never be stuck waiting on it.
  expect(screen.getByRole("button", { name: /Close/ })).toBeEnabled();

  await act(async () => {
    resolveSignIn({ ok: true, detail: "signed in as HR Admin from a saved session", used_saved_session: true, steps: [] });
    await Promise.resolve();
  });
  expect(await screen.findByText("signed in as HR Admin from a saved session")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Save result/ })).toBeEnabled();
});

test("closing while a sign-in is in flight does not break when the result lands afterward", async () => {
  let resolveSignIn: (out: unknown) => void = () => {};
  const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
  mockIPC((cmd) => {
    if (cmd === "auto_run_load_script") return { case_id: 1, title: "s", steps: STEPS, account: "hr.admin" };
    if (cmd === "auto_run_sign_in") {
      return new Promise((resolve) => {
        resolveSignIn = resolve;
      });
    }
    if (cmd === "auto_run_new_id") return "run-1";
    if (cmd === "auto_run_save_run") return null;
    return null;
  });
  const onClose = renderPane([{ id: 1, title: "Leave request" }]);

  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  await screen.findByText("Signing in as hr.admin");

  fireEvent.click(screen.getByRole("button", { name: /Close/ }));
  await waitFor(() => expect(onClose).toHaveBeenCalled());

  // The sign-in was still in flight when Close was pressed (the Rust mutex
  // queued the browser close behind it) - its result landing afterward
  // must not throw or update state outside of React's control.
  await act(async () => {
    resolveSignIn({ ok: true, detail: "signed in as HR Admin from a saved session", used_saved_session: true, steps: [] });
    await Promise.resolve();
  });
  expect(consoleError).not.toHaveBeenCalled();
});
