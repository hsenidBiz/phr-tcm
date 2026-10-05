// Walking a selection of cases in one supervised session.
//
// The invariant under test: a selection is ONE run. Verdicts are banked as
// the person moves through the cases and written once at the end (or when
// they walk away), never one file per case.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "../../lib/toast";
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
  const counted: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_count_evidence") {
      counted.push(args);
      return true;
    }
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
  return { saved, launched, closes, counted };
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
  expect(s.counted, "nothing on disk, nothing counted").toEqual([]);
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

// The case too: the app reads its script on this machine to decide whether
// the browser must stop the page's saves (Must not save).
test("a step is sent with the project and the case it belongs to", async () => {
  const s = mockSignIn({ 1: { case_id: 1, title: "s", steps: STEPS } }, () => OK);
  renderPane([{ id: 1, title: "Public page" }]);
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(await screen.findByRole("button", { name: /Run step 1/ }));
  await waitFor(() =>
    expect(s.stepped).toEqual([{ organization: "acme", project: "Web", caseId: 1, step: STEPS[0] }]),
  );
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

test("the verdict row is the shared picker: one labelled group of three toggles", async () => {
  mockSession();
  renderPane([{ id: 1, title: "Valid login" }]);

  const group = await screen.findByRole("group", { name: "Your verdict" });
  const buttons = within(group).getAllByRole("button");
  expect(buttons.map((b) => b.textContent)).toEqual(["Passed", "Failed", "Blocked"]);
  for (const b of buttons) expect(b).toHaveAttribute("aria-pressed", "false");

  fireEvent.click(within(group).getByRole("button", { name: "Failed" }));
  expect(within(group).getByRole("button", { name: "Failed" })).toHaveAttribute("aria-pressed", "true");
  expect(within(group).getByRole("button", { name: "Passed" })).toHaveAttribute("aria-pressed", "false");
});

test("the verdict group is named once, by its visible label", async () => {
  mockSession();
  renderPane([{ id: 1, title: "Valid login" }]);

  const group = await screen.findByRole("group", { name: "Your verdict" });
  // Named BY the visible "Your verdict" text (aria-labelledby), not by an
  // aria-label repeating it - otherwise a screen reader reads it twice.
  expect(group).not.toHaveAttribute("aria-label");
  const labelId = group.getAttribute("aria-labelledby");
  expect(labelId).toBeTruthy();
  expect(document.getElementById(labelId!)).toHaveTextContent("Your verdict");
});

/// The quirks' evidence is counted from a supervised run exactly once,
/// right after the run is on disk (a failed save counts nothing - see the
/// failed-write test above).
test("a saved run is counted toward the project's quirks once", async () => {
  const s = mockSession();
  const onClose = renderPane([{ id: 1, title: "Valid login" }]);
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(await screen.findByRole("button", { name: "Passed" }));
  fireEvent.click(screen.getByRole("button", { name: /Save result/ }));
  await waitFor(() => expect(onClose).toHaveBeenCalled());
  expect(s.saved).toHaveLength(1);
  expect(s.counted).toEqual([{ organization: "acme", project: "Web", runId: "run-1" }]);
});

/** A session whose one case has preconditions, answering the check with
 * `answer`. Records the checks and sign-ins asked. */
function mockPreconditions(answer: { blocked: string | null; notice: string | null }) {
  const checked: unknown[] = [];
  const asked: unknown[] = [];
  const saved: { cases: Record<string, unknown>[] }[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_load_script")
      return {
        case_id: 1,
        title: "s",
        steps: STEPS,
        account: "hr.admin",
        preconditions: [{ flow: "pms-performance-cycle", stage: "publish", value: 274 }],
      };
    if (cmd === "auto_run_check_preconditions") {
      checked.push(args);
      return answer;
    }
    if (cmd === "auto_run_sign_in") {
      asked.push(args);
      return OK;
    }
    if (cmd === "auto_run_new_id") return "run-1";
    if (cmd === "auto_run_save_run") {
      saved.push((args as { run: { cases: Record<string, unknown>[] } }).run);
      return null;
    }
    return null;
  });
  return { checked, asked, saved };
}

const NOT_MET = "precondition not met: Publish for 274 (Performance cycle wizard)";

test("a case whose precondition is not met is Blocked before its sign-in and runs no step", async () => {
  const s = mockPreconditions({ blocked: NOT_MET, notice: null });
  renderPane([{ id: 1, title: "Open a published cycle" }]);
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));

  expect(await screen.findByText(`Blocked before step 1: ${NOT_MET}`)).toBeInTheDocument();
  expect(s.checked).toEqual([{ organization: "acme", project: "Web", caseId: 1, dbReadAccess: true }]);
  expect(s.asked).toEqual([]);
  expect(screen.getByRole("button", { name: /Run step 1/ })).toBeDisabled();

  // The verdict is still the person's; the record carries the reason.
  fireEvent.click(screen.getByRole("button", { name: "Blocked" }));
  fireEvent.click(screen.getByRole("button", { name: /Save result/ }));
  await waitFor(() => expect(s.saved).toHaveLength(1));
  expect(s.saved[0].cases[0]).toMatchObject({ verdict: "Blocked", proposed: "Blocked", reason: NOT_MET });
});

test("a case whose preconditions are met signs in as usual", async () => {
  const s = mockPreconditions({ blocked: null, notice: null });
  renderPane([{ id: 1, title: "Open a published cycle" }]);
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));

  expect(await screen.findByText("signed in as HR Admin from a saved session")).toBeInTheDocument();
  expect(s.checked).toHaveLength(1);
  expect(s.asked).toHaveLength(1);
  expect(screen.queryByText(/Blocked before step 1/)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Run step 1/ })).toBeEnabled();
});

test("a case with no preconditions is never checked", async () => {
  const seen: string[] = [];
  mockIPC((cmd) => {
    seen.push(cmd);
    if (cmd === "auto_run_load_script") return { case_id: 1, title: "s", steps: STEPS, account: "hr.admin" };
    if (cmd === "auto_run_sign_in") return OK;
    return null;
  });
  renderPane([{ id: 1, title: "Leave request" }]);
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  expect(await screen.findByText("signed in as HR Admin from a saved session")).toBeInTheDocument();
  expect(seen).not.toContain("auto_run_check_preconditions");
});

const NOT_CHECKED = "preconditions were not checked: Database Read Access is off on the AI Bridge tab";

test("with Database Read Access off the case says its preconditions were not checked and carries on", async () => {
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["db_lookup", "db_query"]));
  const s = mockPreconditions({ blocked: null, notice: NOT_CHECKED });
  renderPane([{ id: 1, title: "Open a published cycle" }]);
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));

  const notice = await screen.findByText(NOT_CHECKED);
  expect(notice).toHaveClass("text-warning");
  expect(s.checked).toEqual([{ organization: "acme", project: "Web", caseId: 1, dbReadAccess: false }]);
  // Not a block: the case signs in and its steps can run.
  expect(await screen.findByText("signed in as HR Admin from a saved session")).toBeInTheDocument();
  expect(s.asked).toHaveLength(1);
  expect(screen.queryByText(/Blocked before step 1/)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Run step 1/ })).toBeEnabled();

  // The record carries it, as an unattended run's does.
  fireEvent.click(screen.getByRole("button", { name: "Passed" }));
  fireEvent.click(screen.getByRole("button", { name: /Save result/ }));
  await waitFor(() => expect(s.saved).toHaveLength(1));
  expect(s.saved[0].cases[0]).toMatchObject({ verdict: "Passed", notice: NOT_CHECKED });
  expect(s.saved[0].cases[0]).not.toHaveProperty("proposed");
});


const NO_GUARD = "the no-save guard could not be set up: Fetch.enable was refused";

test("a no-save case whose guard cannot start is Blocked with the sentence, as an unattended run records it", async () => {
  const saved: { cases: Record<string, unknown>[] }[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_load_script") return { case_id: 1, title: "s", steps: STEPS, no_save: true };
    if (cmd === "auto_run_step") throw NO_GUARD;
    if (cmd === "auto_run_new_id") return "run-1";
    if (cmd === "auto_run_save_run") {
      saved.push((args as { run: { cases: Record<string, unknown>[] } }).run);
      return null;
    }
    return null;
  });
  renderPane([{ id: 1, title: "Read a shared draft" }]);
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  fireEvent.click(await screen.findByRole("button", { name: /Run step 1/ }));

  expect(await screen.findByText(`Blocked before step 1: ${NO_GUARD}`)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Run step 1/ })).toBeDisabled();

  fireEvent.click(screen.getByRole("button", { name: "Blocked" }));
  fireEvent.click(screen.getByRole("button", { name: /Save result/ }));
  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0].cases[0]).toMatchObject({ verdict: "Blocked", proposed: "Blocked", reason: NO_GUARD });
});

// ---- Replay to step N, started from Past runs or the review ----

const THREE = [1, 2, 3].map((n) => ({ step_number: n, actions: [{ kind: "click", target: `Button ${n}` }] }));

/** A pane opened to replay case 1. The replay command waits for `finish`
 * with what it answers, so a test can look at the pane mid-replay. */
function mockReplay(opts: { account?: string } = {}) {
  const calls: { cmd: string; args: unknown }[] = [];
  let finish: (answer: unknown) => void = () => {};
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "auto_run_load_script") {
        return { case_id: 1, title: "s", steps: THREE, ...(opts.account ? { account: opts.account } : {}) };
      }
      if (cmd === "auto_run_replay_to_step") return new Promise((resolve) => (finish = resolve));
      return null;
    },
    { shouldMockEvents: true },
  );
  return {
    named: (cmd: string) => calls.filter((c) => c.cmd === cmd),
    finish: (answer: unknown) =>
      act(async () => {
        finish(answer);
      }),
  };
}

function renderReplay(step: number) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <RunPane
        org="acme"
        project="Web"
        pbiId={42}
        cases={[{ id: 1, title: "Valid login" }]}
        replayTo={step}
        onClose={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

const READY = {
  end: { kind: "ready", detail: { case_id: 1, step: 3, notice: null } },
  sentence: "replayed case 1 to step 3 - the browser is on the page before step 3 runs",
};

/** The row of step `n` in the pane's step list. */
async function stepRow(n: number) {
  return (await screen.findByRole("button", { name: `Run step ${n}` })).closest("li")!;
}

test("a pane opened for a replay starts it at once, for that case and step, without Open browser", async () => {
  const r = mockReplay();
  renderReplay(3);

  await waitFor(() => expect(r.named("auto_run_replay_to_step")).toHaveLength(1));
  expect(r.named("auto_run_replay_to_step")[0].args).toEqual({
    organization: "acme",
    project: "Web",
    caseId: 1,
    step: 3,
    dbReadAccess: true,
  });
  // The replay opens the browser itself.
  expect(r.named("auto_run_open_browser")).toHaveLength(0);
});

test("the replay is told Database Read Access is off", async () => {
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["db_lookup", "db_query"]));
  const r = mockReplay();
  renderReplay(2);

  await waitFor(() => expect(r.named("auto_run_replay_to_step")).toHaveLength(1));
  expect((r.named("auto_run_replay_to_step")[0].args as { dbReadAccess: boolean }).dbReadAccess).toBe(false);
});

test("the pane says which step the replay is on, from its progress", async () => {
  const r = mockReplay();
  renderReplay(3);
  // Progress is said once the replay has started.
  await waitFor(() => expect(r.named("auto_run_replay_to_step")).toHaveLength(1));

  const { emit } = await import("@tauri-apps/api/event");
  await act(async () => {
    await emit("autorun-replay-progress", { case_id: 1, step: 1, of: 2 });
  });
  expect(await screen.findByText("replaying step 1 of 2")).toBeInTheDocument();

  // Another case's replay is not this pane's.
  await act(async () => {
    await emit("autorun-replay-progress", { case_id: 9, step: 7, of: 8 });
  });
  expect(screen.queryByText("replaying step 7 of 8")).not.toBeInTheDocument();

  await act(async () => {
    await emit("autorun-replay-progress", { case_id: 1, step: 2, of: 2 });
  });
  expect(await screen.findByText("replaying step 2 of 2")).toBeInTheDocument();
});

test("Stop replay asks the replay to stop", async () => {
  const r = mockReplay();
  renderReplay(3);

  fireEvent.click(await screen.findByRole("button", { name: "Stop replay" }));
  await waitFor(() => expect(r.named("auto_run_stop_replay")).toHaveLength(1));

  // Gone once the replay has answered.
  await r.finish({ end: { kind: "stopped", detail: { step: 2 } }, sentence: "the replay was stopped at step 2" });
  expect(await screen.findByText("the replay was stopped at step 2")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Stop replay" })).not.toBeInTheDocument();
});

test.each([
  [READY, "text-success"],
  [
    {
      end: { kind: "stopped_at", detail: { phase: "step", step: 2, why: 'button "Save" not found', outcomes: [] } },
      sentence: 'replay stopped at step 2: button "Save" not found',
    },
    "text-danger",
  ],
  [{ end: { kind: "stopped", detail: { step: 2 } }, sentence: "the replay was stopped at step 2" }, "text-muted"],
  [
    { end: { kind: "refused", detail: "case 1 has no saved script" }, sentence: "case 1 has no saved script" },
    "text-warning",
  ],
  [
    {
      end: { kind: "blocked", detail: "precondition not met: Publish for 274" },
      sentence: "precondition not met: Publish for 274",
    },
    "text-warning",
  ],
])("the replay's answer is shown as its own sentence (%#)", async (answer, tone) => {
  const r = mockReplay();
  renderReplay(3);
  await waitFor(() => expect(r.named("auto_run_replay_to_step")).toHaveLength(1));

  await r.finish(answer);
  expect(await screen.findByText(answer.sentence)).toHaveClass(tone);
});

test("after a replay to step 3, steps 1 and 2 are marked and step 3 is next, with no second sign-in", async () => {
  const r = mockReplay({ account: "hr.admin" });
  renderReplay(3);
  await waitFor(() => expect(r.named("auto_run_replay_to_step")).toHaveLength(1));

  await r.finish(READY);

  for (const n of [1, 2]) expect(within(await stepRow(n)).getByText("replayed")).toBeInTheDocument();
  const next = await stepRow(3);
  expect(within(next).queryByText("replayed")).not.toBeInTheDocument();
  expect(next).toHaveAttribute("aria-current", "step");
  expect(within(next).getByRole("button", { name: "Run step 3" })).toBeEnabled();

  // The replay signed the case in: the pane shows its browser as open and
  // does not sign in over it.
  expect(screen.queryByRole("button", { name: "Open browser" })).not.toBeInTheDocument();
  await act(async () => {});
  expect(r.named("auto_run_sign_in")).toHaveLength(0);
  expect(r.named("auto_run_check_preconditions")).toHaveLength(0);
});

test("a replay that is ready with a notice shows the notice", async () => {
  const r = mockReplay();
  renderReplay(3);
  await waitFor(() => expect(r.named("auto_run_replay_to_step")).toHaveLength(1));

  await r.finish({
    end: {
      kind: "ready",
      detail: { case_id: 1, step: 3, notice: "preconditions not checked: Database Read Access is off" },
    },
    sentence: READY.sentence,
  });
  expect(await screen.findByText("preconditions not checked: Database Read Access is off")).toBeInTheDocument();
});

test("a replay stopped at step 2 marks step 1, shows step 2's outcomes as failed, and leaves step 2 to run", async () => {
  const r = mockReplay();
  renderReplay(3);
  await waitFor(() => expect(r.named("auto_run_replay_to_step")).toHaveLength(1));

  await r.finish({
    end: {
      kind: "stopped_at",
      detail: {
        phase: "step",
        step: 2,
        why: 'button "Save" not found',
        outcomes: [
          { ok: true, detail: "clicked Edit" },
          { ok: false, detail: 'button "Save" not found', screenshot: "shot-1-2.png" },
        ],
      },
    },
    sentence: 'replay stopped at step 2: button "Save" not found',
  });

  expect(within(await stepRow(1)).getByText("replayed")).toBeInTheDocument();
  const failed = await stepRow(2);
  expect(within(failed).queryByText("replayed")).not.toBeInTheDocument();
  expect(within(failed).getByText(/^button "Save" not found/)).toHaveClass("text-danger");
  expect(within(failed).getByRole("button", { name: "View screenshot for action 2" })).toBeInTheDocument();
  expect(within(failed).getByRole("button", { name: "Run step 2" })).toBeEnabled();
  expect(within(await stepRow(3)).queryByText("replayed")).not.toBeInTheDocument();
});

test("a replay whose browser would not open says so and offers Open browser", async () => {
  mockIPC(
    (cmd) => {
      if (cmd === "auto_run_load_script") return { case_id: 1, title: "s", steps: THREE };
      if (cmd === "auto_run_replay_to_step") return Promise.reject("Edge could not be started");
      return null;
    },
    { shouldMockEvents: true },
  );
  const error = vi.spyOn(toast, "error");
  renderReplay(3);

  await waitFor(() => expect(error).toHaveBeenCalledWith("Could not open the browser: Edge could not be started"));
  expect(await screen.findByRole("button", { name: "Open browser" })).toBeEnabled();
  expect(screen.queryByRole("button", { name: "Stop replay" })).not.toBeInTheDocument();
});

test("the first progress is heard: the pane listens before the replay starts", async () => {
  const { emit } = await import("@tauri-apps/api/event");
  mockIPC(
    (cmd) => {
      if (cmd === "auto_run_load_script") return { case_id: 1, title: "s", steps: THREE };
      if (cmd === "auto_run_replay_to_step") {
        // Said the moment the replay begins, before anything else happens.
        void emit("autorun-replay-progress", { case_id: 1, step: 1, of: 2 });
        return new Promise(() => {});
      }
      return null;
    },
    { shouldMockEvents: true },
  );
  renderReplay(3);

  expect(await screen.findByText("replaying step 1 of 2")).toBeInTheDocument();
});

test("saving after a replay keeps each replayed step with one outcome, never an empty step", async () => {
  const r = mockReplay();
  renderReplay(3);
  await waitFor(() => expect(r.named("auto_run_replay_to_step")).toHaveLength(1));
  await r.finish(READY);
  await stepRow(3);

  fireEvent.click(screen.getByRole("button", { name: "Failed" }));
  fireEvent.click(screen.getByRole("button", { name: /Save result/ }));

  await waitFor(() => expect(r.named("auto_run_save_run")).toHaveLength(1));
  const run = (r.named("auto_run_save_run")[0].args as {
    run: { cases: { steps: { step_number: number; outcomes: unknown[] }[] }[] };
  }).run;
  expect(run.cases[0].steps).toEqual([
    { step_number: 1, outcomes: [{ ok: true, detail: "replayed before healing" }] },
    { step_number: 2, outcomes: [{ ok: true, detail: "replayed before healing" }] },
    { step_number: 3, outcomes: [] },
  ]);
});

/** The steps the pane hands to `auto_run_save_run`, after Failed and Save. */
async function saveFailed(r: ReturnType<typeof mockReplay>) {
  fireEvent.click(screen.getByRole("button", { name: "Failed" }));
  fireEvent.click(screen.getByRole("button", { name: /Save result/ }));
  await waitFor(() => expect(r.named("auto_run_save_run")).toHaveLength(1));
  return (r.named("auto_run_save_run")[0].args as {
    run: { cases: { proposed?: string; reason?: string; steps: { step_number: number; outcomes: unknown[] }[] }[] };
  }).run.cases[0];
}

test("a replay stopped while signing in keeps its outcomes under the sign-in, never step 1", async () => {
  const r = mockReplay({ account: "hr.admin" });
  renderReplay(3);
  await waitFor(() => expect(r.named("auto_run_replay_to_step")).toHaveLength(1));

  const failed = { ok: false, detail: "nothing matched #go", screenshot: "shot-1-0.png" };
  await r.finish({
    end: { kind: "stopped_at", detail: { phase: "sign_in", step: 1, why: "nothing matched #go", outcomes: [failed] } },
    sentence: "replay stopped while signing in: nothing matched #go",
  });
  expect(await screen.findByText("replay stopped while signing in: nothing matched #go")).toHaveClass("text-danger");
  // The sign-in box says it, with the way to try again.
  expect(screen.getByRole("button", { name: "Sign in again" })).toBeInTheDocument();
  expect(within(await stepRow(1)).queryByText(/nothing matched/)).not.toBeInTheDocument();

  const saved = await saveFailed(r);
  expect(saved.steps).toEqual([
    { step_number: 0, outcomes: [failed] },
    { step_number: 1, outcomes: [] },
    { step_number: 2, outcomes: [] },
    { step_number: 3, outcomes: [] },
  ]);
});

test("a replay stopped going to the area keeps its outcomes under the module step", async () => {
  const r = mockReplay();
  renderReplay(3);
  await waitFor(() => expect(r.named("auto_run_replay_to_step")).toHaveLength(1));

  const failed = { ok: false, detail: "the Leave link never appeared" };
  await r.finish({
    end: { kind: "stopped_at", detail: { phase: "area", step: 1, why: failed.detail, outcomes: [failed] } },
    sentence: "replay stopped while going to the case's area: the Leave link never appeared",
  });
  await screen.findByText("replay stopped while going to the case's area: the Leave link never appeared");
  expect(screen.getByText("Going to the area").closest("li")).toHaveTextContent("the Leave link never appeared");

  const saved = await saveFailed(r);
  expect(saved.steps[0]).toEqual({ step_number: -1, outcomes: [failed] });
  expect(saved.steps.find((s) => s.step_number === 1)?.outcomes).toEqual([]);
});

test("a replay blocked by a precondition blocks the case, as a watched start does", async () => {
  const r = mockReplay({ account: "hr.admin" });
  renderReplay(3);
  await waitFor(() => expect(r.named("auto_run_replay_to_step")).toHaveLength(1));

  await r.finish({
    end: { kind: "blocked", detail: "precondition not met: Publish for 274" },
    sentence: "precondition not met: Publish for 274",
  });
  expect(await screen.findByText("Blocked before step 1: precondition not met: Publish for 274")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Run step 1" })).toBeDisabled();

  const saved = await saveFailed(r);
  expect(saved.proposed).toBe("Blocked");
  expect(saved.reason).toBe("precondition not met: Publish for 274");
  expect(r.named("auto_run_sign_in")).toHaveLength(0);
});

test("a refused replay leaves the shared browser alone: closing the pane neither stops nor closes it", async () => {
  const r = mockReplay();
  const onClose = vi.fn();
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <RunPane org="acme" project="Web" pbiId={42} cases={[{ id: 1, title: "Valid login" }]} replayTo={3} onClose={onClose} />
    </QueryClientProvider>,
  );
  await waitFor(() => expect(r.named("auto_run_replay_to_step")).toHaveLength(1));

  await r.finish({
    end: { kind: "refused", detail: "a replay is already running - wait for it to finish" },
    sentence: "a replay is already running - wait for it to finish",
  });
  await screen.findByText("a replay is already running - wait for it to finish");

  fireEvent.click(screen.getByRole("button", { name: /Close/ }));
  await waitFor(() => expect(onClose).toHaveBeenCalled());
  expect(r.named("auto_run_close_browser")).toHaveLength(0);
  expect(r.named("auto_run_stop_replay")).toHaveLength(0);
});

test("a pane closed while its replay goes stops the replay and closes no browser", async () => {
  const r = mockReplay();
  const view = renderReplay(3);
  await waitFor(() => expect(r.named("auto_run_replay_to_step")).toHaveLength(1));

  view.unmount();
  await waitFor(() => expect(r.named("auto_run_stop_replay")).toHaveLength(1));
  expect(r.named("auto_run_close_browser")).toHaveLength(0);
});

test("a browser another replay opened is shown as open, and never opened again", async () => {
  const calls: string[] = [];
  mockIPC(
    (cmd) => {
      calls.push(cmd);
      if (cmd === "auto_run_load_script") return { case_id: 1, title: "s", steps: STEPS };
      return null;
    },
    { shouldMockEvents: true },
  );
  renderPane([{ id: 1, title: "Valid login" }]);
  expect(await screen.findByRole("button", { name: "Open browser" })).toBeInTheDocument();

  const { emit } = await import("@tauri-apps/api/event");
  await act(async () => {
    await emit("autorun-session-changed", { opened: true, account: null });
  });
  expect(await screen.findByRole("button", { name: "Run step 1" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Open browser" })).not.toBeInTheDocument();
  expect(calls).not.toContain("auto_run_open_browser");
});

test("a replay that signs the browser in as another account is said, and this case signs in again before its next step", async () => {
  const order: string[] = [];
  mockIPC(
    (cmd) => {
      if (cmd === "auto_run_load_script") return { case_id: 1, title: "s", steps: STEPS, account: "hr.admin" };
      if (cmd === "auto_run_sign_in") {
        order.push("sign in");
        return OK;
      }
      if (cmd === "auto_run_step") {
        order.push("step");
        return [{ ok: true, detail: "ok" }];
      }
      return null;
    },
    { shouldMockEvents: true },
  );
  renderPane([{ id: 1, title: "Valid login" }]);
  fireEvent.click(await screen.findByRole("button", { name: "Open browser" }));
  // The pane's own sign-in, once the browser opened.
  await waitFor(() => expect(order).toEqual(["sign in"]));
  const run = await screen.findByRole("button", { name: "Run step 1" });
  await waitFor(() => expect(run).toBeEnabled());

  const { emit } = await import("@tauri-apps/api/event");
  await act(async () => {
    await emit("autorun-session-changed", { opened: true, account: "clerk" });
  });
  expect(
    await screen.findByText(
      "the Auto Run browser was signed in as clerk by a replay - sign in again before the next step",
    ),
  ).toBeInTheDocument();

  fireEvent.click(run);
  await waitFor(() => expect(order).toEqual(["sign in", "sign in", "step"]));
  await waitFor(() =>
    expect(screen.queryByText(/was signed in as clerk by a replay/)).not.toBeInTheDocument(),
  );

  // Once signed in again, the next step needs no second sign-in.
  await waitFor(() => expect(run).toBeEnabled());
  fireEvent.click(run);
  await waitFor(() => expect(order).toEqual(["sign in", "sign in", "step", "step"]));
});
