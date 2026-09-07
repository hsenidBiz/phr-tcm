import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, configure, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import App from "./App";
import { TOUR_STEPS } from "./tour/tourScript";
import { START_TOUR_EVENT } from "./tour/tourState";
import { commands } from "./bindings";

// Every test here mounts the WHOLE app - sidebar, context bar, screens,
// queries - and several walk the tour across most of its stops. Idle, they
// land between 2 and 4.5 seconds; sharing a laptop with the other 90 test
// files they routinely cross vitest's 5s default, and it arrives as a bare
// timeout on a test whose assertions are all fine. Two different tests
// have failed that way on three full runs, which makes the suite useless
// as a release gate - and a gate people learn to wave through is worse
// than no gate.
//
// Raised for this file only: the 5s default still holds for the ~90 files
// of unit tests, so a genuinely hung one there still fails fast. A timeout
// is for catching a hang, and nothing in this file takes 15s honestly.
vi.setConfig({ testTimeout: 15_000 });

// A SECOND clock, and the one that actually bit: Testing Library's
// `findBy*` gives up after 1s by default, which vitest's testTimeout above
// does nothing about. Under full-suite load this app's first render can
// take longer than that, so a `findByText` for something that does arrive
// fails at 1s - and reports "Unable to find an element", which reads like
// a broken assertion rather than the machine being busy. That cost a real
// diagnosis: the failure moved between tests run to run and vanished when
// the file ran alone.
configure({ asyncUtilTimeout: 5_000 });

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function renderApp() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const result = render(
    <QueryClientProvider client={qc}>
      <App />
    </QueryClientProvider>,
  );
  // Most tests never need the client back; the ones that force a refetch
  // (e.g. flipping auth mid-test) grab it off the return value.
  return { ...result, qc };
}

function signedInMocks(extra: (cmd: string, args: unknown) => unknown = () => undefined) {
  mockIPC((cmd, args) => {
    if (cmd === "auth_status") return { signed_in: true, account: "a@b.com" };
    if (cmd === "check_update") return null;
    if (cmd === "list_orgs") return [{ name: "acme", url: "" }];
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "plugin:event|listen") return 1;
    return extra(cmd, args);
  });
}

test("signed out: sign-in view only, no sidebar tabs", async () => {
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: false, account: null };
    if (cmd === "check_update") return null;
  });
  renderApp();
  expect(
    await screen.findByRole("button", { name: /sign in with microsoft/i }),
  ).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Manual Entry" })).not.toBeInTheDocument();
});

test("sidebar shows the v1 tabs and switches screens", async () => {
  signedInMocks((cmd) => {
    if (cmd === "list_plans_with_suites") return [];
  });
  renderApp();
  expect(await screen.findByText("a@b.com")).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Manual Entry" })).toBeInTheDocument();

  for (const tab of ["Import File", "Update Test Cases", "Run Tests", "Test Suites"]) {
    fireEvent.click(screen.getByRole("button", { name: tab }));
    expect(screen.getByRole("heading", { name: tab })).toBeInTheDocument();
  }
});

test("settings opens from the gear, not the sidebar", async () => {
  signedInMocks();
  renderApp();
  await screen.findByText("a@b.com");
  const { within } = await import("@testing-library/react");
  const nav = screen.getByRole("navigation");
  expect(within(nav).queryByRole("button", { name: "Settings" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByLabelText("Settings"));
  expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument();
  expect(screen.getByText("Appearance")).toBeInTheDocument();

  // Clicking the gear again exits settings, back to the previous tab.
  fireEvent.click(screen.getByLabelText("Close settings"));
  expect(screen.queryByRole("heading", { name: "Settings" })).not.toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Manual Entry" })).toBeInTheDocument();
});

test("work pill toggles the board and a tab click returns", async () => {
  signedInMocks((cmd) => {
    if (cmd === "fetch_board") return { items: [], states_by_type: {} };
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
    if (cmd === "list_repos") return [];
  });
  renderApp();
  await screen.findByText("a@b.com");

  fireEvent.click(screen.getByRole("button", { name: /Work Manager/ }));
  expect(screen.getByRole("heading", { name: "Board" })).toBeInTheDocument();

  // Work Manager swaps the rail: its own sections, no test-case tabs.
  expect(screen.getByRole("button", { name: "Pull Requests" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Run Tests" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Pull Requests" }));
  expect(await screen.findByRole("heading", { name: "Pull Requests" })).toBeInTheDocument();

  // The pill is the way back, and it lands on the case tabs again.
  fireEvent.click(screen.getByRole("button", { name: /Test Case Manager/ }));
  expect(screen.getByRole("heading", { name: "Manual Entry" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Run Tests" })).toBeInTheDocument();
});

test("prefs restore section, scope and selected PBI", async () => {
  localStorage.setItem(
    "tcm-v2-prefs",
    JSON.stringify({
      org: "acme",
      project: "Web",
      section: "run",
      pbi: { id: 42, title: "Login flow", work_item_type: "Product Backlog Item" },
      workMode: false,
    }),
  );
  signedInMocks((cmd) => {
    if (cmd === "list_projects") return [{ id: "p1", name: "Web" }];
    if (cmd === "ensure_pbi_suite") return { plan_id: 9, plan_name: "Plan", suite_id: 91 };
    if (cmd === "list_test_points") return [];
  });
  renderApp();
  expect(await screen.findByRole("heading", { name: "Run Tests" })).toBeInTheDocument();
  // The chosen PBI is restored into the context bar chip.
  expect(await screen.findByText("Login flow")).toBeInTheDocument();
});

test("Ctrl+2 jumps to Import File; Ctrl+Shift+M toggles Work Manager", async () => {
  signedInMocks((cmd) => {
    if (cmd === "fetch_board") return { items: [], states_by_type: {} };
  });
  renderApp();
  await screen.findByText("a@b.com");

  fireEvent.keyDown(window, { key: "2", ctrlKey: true });
  expect(screen.getByRole("heading", { name: "Import File" })).toBeInTheDocument();

  fireEvent.keyDown(window, { key: "m", ctrlKey: true, shiftKey: true });
  expect(screen.getByRole("heading", { name: "Board" })).toBeInTheDocument();

  fireEvent.keyDown(window, { key: "m", ctrlKey: true, shiftKey: true });
  expect(screen.getByRole("heading", { name: "Import File" })).toBeInTheDocument();
});

// The sidebar's CASE_ITEMS order is manual, import, edit, view, run,
// autorun, suites, ai (8 rows) - the Ctrl+N shortcut order must match it
// row for row, or a number opens the wrong screen and the last row loses
// its shortcut entirely.
test("Ctrl+6 jumps to Auto Run and Ctrl+8 jumps to AI Bridge", async () => {
  signedInMocks();
  renderApp();
  await screen.findByText("a@b.com");

  fireEvent.keyDown(window, { key: "6", ctrlKey: true });
  expect(screen.getByRole("heading", { name: "Auto Run" })).toBeInTheDocument();
  // Shipped early, and the screen says so - beside the heading, not in it.
  expect(screen.getByText("In Development")).toBeInTheDocument();

  fireEvent.keyDown(window, { key: "7", ctrlKey: true });
  expect(screen.getByRole("heading", { name: "Test Suites" })).toBeInTheDocument();
  // The pill belongs to Auto Run alone.
  expect(screen.queryByText("In Development")).not.toBeInTheDocument();

  fireEvent.keyDown(window, { key: "8", ctrlKey: true });
  expect(screen.getByRole("heading", { name: "AI Bridge" })).toBeInTheDocument();
});

test("signing in starts the AI bridge and pushes org/project context", async () => {
  const pushes: Array<Record<string, unknown>> = [];
  let bridgeStarted = 0;
  localStorage.setItem(
    "tcm-v2-prefs",
    JSON.stringify({
      org: "acme",
      project: "Web",
      section: "manual",
      pbi: null,
      workMode: false,
    }),
  );
  signedInMocks((cmd, args) => {
    if (cmd === "list_projects") return [{ id: "p1", name: "Web" }];
    if (cmd === "set_bridge_context") {
      pushes.push(args as Record<string, unknown>);
      return null;
    }
    if (cmd === "bridge_status") {
      bridgeStarted += 1;
      return { port: 1, mcp_exe: "x" };
    }
  });
  localStorage.setItem("tcm-v2-working-dir", "D:\\repo");
  renderApp();
  await screen.findByText("a@b.com");
  await vi.waitFor(() => expect(pushes.length).toBeGreaterThan(0));
  expect(pushes[pushes.length - 1]).toMatchObject({
    organization: "acme",
    project: "Web",
    workingDir: "D:\\repo",
  });
  // The bridge must come up WITHOUT visiting the AI Bridge tab - an AI
  // tool connecting right after sign-in gets a live listener.
  expect(bridgeStarted).toBeGreaterThan(0);
});

/// The last manual step in the AI loop. `begin_test_case_writing` already
/// makes the developer say where the JSON goes, so the app can start
/// watching that path before the assistant has written a line - and the
/// existing watcher then folds the file in by itself.
test("the path from begin_test_case_writing starts being watched", async () => {
  localStorage.setItem(
    "tcm-v2-prefs",
    JSON.stringify({
      org: "acme",
      project: "Web",
      section: "ai",
      pbi: { id: 42, title: "Login flow", work_item_type: "Product Backlog Item" },
      workMode: false,
    }),
  );
  // `shouldMockEvents` is the supported way to make the mock's `emit` reach
  // handlers registered by `listen`; without it the two never connect, and
  // `plugin:event|listen` must be left to the mock rather than stubbed here.
  mockIPC(
    (cmd) => {
      if (cmd === "auth_status") return { signed_in: true, account: "a@b.com" };
      if (cmd === "check_update") return null;
      if (cmd === "list_orgs") return [{ name: "acme", url: "" }];
      if (cmd === "list_projects") return [{ id: "p1", name: "Web" }];
      if (cmd === "list_test_case_fields") return [];
      return undefined;
    },
    { shouldMockEvents: true },
  );

  renderApp();
  await screen.findByText("a@b.com");

  // Nothing is watched until intake settles on a path.
  expect(localStorage.getItem("tcm-v2-watch:acme/42")).toBeNull();

  const { emit } = await import("@tauri-apps/api/event");
  const deliver = (path: string) =>
    act(async () => {
      await emit("intake-output-path", { path });
    });

  await deliver("C:/drafts/login-cases.json");
  await vi.waitFor(() => {
    const raw = localStorage.getItem("tcm-v2-watch:acme/42");
    expect(raw).toBeTruthy();
    expect(JSON.parse(raw as string)).toEqual([
      { path: "C:/drafts/login-cases.json", stamp: "", snapshot: [] },
    ]);
  });

  // Re-running begin with the same answers must not reset a file already
  // being followed: that would drop its snapshot, and the next edit would
  // read as though every case in the file were new.
  localStorage.setItem(
    "tcm-v2-watch:acme/42",
    JSON.stringify([
      { path: "C:/drafts/login-cases.json", stamp: "abc", snapshot: [{ title: "kept" }] },
    ]),
  );
  await deliver("C:/drafts/login-cases.json");
  const after = JSON.parse(localStorage.getItem("tcm-v2-watch:acme/42") as string);
  expect(after[0].stamp).toBe("abc");
  expect(after[0].snapshot).toHaveLength(1);
});
test("update banner appears when a newer version exists", async () => {
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: false, account: null };
    if (cmd === "check_update") return { available: "0.5.0", blocked: null };
  });
  renderApp();
  expect(await screen.findByText(/Version 0.5.0 is available/)).toBeInTheDocument();
});

// The apply happens after the app has exited, so a failed one used to be
// invisible: the app restarted on the old version and the banner just came
// back, as if the click had done nothing. When the backend reports that the
// last attempt did not land, the banner says WHY - files in use - and what
// to do about it, instead of blandly re-offering the same version.
test("a failed update attempt is explained, not silently re-offered", async () => {
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: false, account: null };
    if (cmd === "check_update")
      return { available: "0.5.0", blocked: null, failed_attempt: "0.5.0" };
  });
  renderApp();
  expect(await screen.findByText(/couldn't finish - another program was using/)).toBeInTheDocument();
  // The way out is still one click away.
  expect(screen.getByRole("button", { name: /restart to update/i })).toBeInTheDocument();
  expect(screen.queryByText(/Version 0.5.0 is available/)).not.toBeInTheDocument();
});

/// The app is left open for days, so a launch-only check means a release
/// lands and nobody hears about it until they next restart. It re-checks
/// every hour, and silently: nothing appears until there is something to
/// say, and the banner is still the only thing that says it.
test("a release published while the app is open is noticed within the hour", async () => {
  // Only the scheduling primitives. Faking microtasks/rAF/performance as
  // well deadlocks React's scheduler against `act`, which reads as a
  // five-second timeout with no clue attached.
  vi.useFakeTimers({
    toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"],
  });
  let checks = 0;
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: false, account: null };
    if (cmd === "check_update") {
      checks += 1;
      // Nothing on launch; a release lands between the first and second.
      return checks === 1 ? { available: null, blocked: null } : { available: "9.9.9", blocked: null };
    }
  });
  // The clock is driven by hand rather than by `findBy*`: under fake
  // timers those two both want to own it and neither makes progress.
  const tick = async (ms: number) => {
    await act(async () => {
      await vi.advanceTimersByTimeAsync(ms);
    });
  };
  try {
    renderApp();
    await tick(50);
    expect(screen.getByRole("button", { name: /sign in/i })).toBeInTheDocument();
    expect(checks).toBe(1);
    expect(screen.queryByText(/is available/)).not.toBeInTheDocument();

    await tick(60 * 60 * 1000 + 1_000);
    expect(checks).toBeGreaterThan(1);
    expect(screen.getByText(/Version 9.9.9 is available/)).toBeInTheDocument();
  } finally {
    vi.useRealTimers();
  }
});

/// The download is the part of an update that takes real time, and a
/// spinner says nothing about how much of it is left. The bar and the byte
/// pair both come from the SAME event - so what is checked here is that a
/// progress event reaches the screen, and that the two states either side
/// of it (nothing yet, finished) do not read as bytes that never arrived.
test("the update banner shows how much of the package has downloaded", async () => {
  mockIPC(
    (cmd) => {
      if (cmd === "auth_status") return { signed_in: false, account: null };
      if (cmd === "check_update") return { available: "0.5.0", blocked: null };
      // Never resolves: the download is still in flight for the whole test,
      // which is exactly the window the bar exists for.
      if (cmd === "apply_update") return new Promise(() => {});
    },
    { shouldMockEvents: true },
  );
  renderApp();
  fireEvent.click(await screen.findByRole("button", { name: /restart to update/i }));

  // Before the first event there is no size to report - and 0% would be a
  // claim, not a measurement.
  const bar = await screen.findByRole("progressbar");
  expect(bar).not.toHaveAttribute("aria-valuenow");
  expect(screen.getByText("Preparing…")).toBeInTheDocument();

  // The bar replaces the offer text on the same row: once the button is
  // clicked, "is available" has been answered, and the banner should not
  // grow a second line mid-download.
  expect(screen.queryByText(/is available/)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /updating/i })).toBeDisabled();

  const { emit } = await import("@tauri-apps/api/event");
  const send = (percent: number, downloaded: number, total: number) =>
    act(async () => {
      await emit("update-progress", { percent, downloaded, total });
    });

  await send(35, 8_678_112, 24.8 * 1024 ** 2);
  await vi.waitFor(() => {
    expect(screen.getByText("8.3 MB of 24.8 MB")).toBeInTheDocument();
  });
  expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "35");

  // At 100% the bytes are all in and the wait is the install, not a
  // download - saying "24.8 MB of 24.8 MB" while nothing moves reads as a
  // stall.
  await send(100, 24.8 * 1024 ** 2, 24.8 * 1024 ** 2);
  await vi.waitFor(() => {
    expect(screen.getByText("Installing…")).toBeInTheDocument();
  });
});

// Azure DevOps throttles per user, not per app - the same slowdown can be
// hitting the person's browser tabs with no explanation, so the app has to
// say so when it happens. `note_server_delay` only re-fires the backing
// Rust event for a hold that is new or longer, but that guard lives on the
// Rust side; the frontend gets no such promise from the wire and must not
// assume it - a long import can still deliver two events close together
// (e.g. a slightly shorter one racing a longer one that already logged),
// so the toast itself has to debounce.
test("a slowdown toast appears once, and is debounced against a second event", async () => {
  mockIPC(
    (cmd) => {
      if (cmd === "auth_status") return { signed_in: false, account: null };
      if (cmd === "check_update") return null;
    },
    { shouldMockEvents: true },
  );
  renderApp();
  await screen.findByRole("button", { name: /sign in/i });

  const { emit } = await import("@tauri-apps/api/event");
  await act(async () => {
    await emit("slowdown-requested", { secs: 12 });
  });

  expect(await screen.findByText(/asked this app to slow down/i)).toBeInTheDocument();
  expect(screen.getByText(/12s/)).toBeInTheDocument();
  expect(screen.getByText(/Azure DevOps request rate/)).toBeInTheDocument();

  // A second event straight after must not produce a second toast.
  await act(async () => {
    await emit("slowdown-requested", { secs: 5 });
  });
  expect(screen.getAllByText(/asked this app to slow down/i)).toHaveLength(1);
});

/// A check that could not run is not the same as being up to date, and the
/// banner must not treat it as either - it has nothing to offer. The
/// difference is told to the person who ASKED, in the toast.
test("a failed update check shows no banner", async () => {
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: false, account: null };
    if (cmd === "check_update") return { available: null, blocked: "Could not reach the update feed" };
  });
  renderApp();
  await screen.findByRole("button", { name: /sign in/i });
  expect(screen.queryByText(/is available/)).not.toBeInTheDocument();
  expect(screen.queryByText(/Could not reach/)).not.toBeInTheDocument();
});

// DevOps said this user cannot read the releases repo. That is not an
// error to hide: it is the one thing the user can fix, and the notice says
// how. It is independent of whether GitHub still served an update.
//
// Signed IN, not out: `no_access` can only ever come back true when a token
// was sent (no token means `sources()` omits DevOps entirely, so the flag
// stays false) - mocking signed-out here would assert a state the real
// backend can never produce.
test("no access to the releases repo shows the notice with the next step", async () => {
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: true, account: "a@b.com" };
    if (cmd === "check_update")
      return { available: null, blocked: null, failed_attempt: null, no_access: true };
    if (cmd === "list_orgs") return [{ name: "acme", url: "" }];
    if (cmd === "plugin:event|listen") return 1;
  });
  renderApp();
  expect(await screen.findByText(/updates have moved to Azure DevOps/)).toBeInTheDocument();
  expect(screen.getByText(/Redmine ticket/)).toBeInTheDocument();
  expect(screen.queryByText(/is available/)).not.toBeInTheDocument();
});

test("the notice and an update from the fallback show together", async () => {
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: true, account: "a@b.com" };
    if (cmd === "check_update")
      return { available: "0.5.0", blocked: null, failed_attempt: null, no_access: true };
    if (cmd === "list_orgs") return [{ name: "acme", url: "" }];
    if (cmd === "plugin:event|listen") return 1;
  });
  renderApp();
  expect(await screen.findByText(/Version 0.5.0 is available/)).toBeInTheDocument();
  expect(screen.getByText(/updates have moved to Azure DevOps/)).toBeInTheDocument();
});

test("the notice can be dismissed, and stays away until the next launch", async () => {
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: true, account: "a@b.com" };
    if (cmd === "check_update")
      return { available: null, blocked: null, failed_attempt: null, no_access: true };
    if (cmd === "list_orgs") return [{ name: "acme", url: "" }];
    if (cmd === "plugin:event|listen") return 1;
  });
  const { unmount } = renderApp();
  await screen.findByText(/updates have moved/);
  fireEvent.click(screen.getByRole("button", { name: /dismiss update notice/i }));
  expect(screen.queryByText(/updates have moved/)).not.toBeInTheDocument();
  unmount();
  // A fresh mount is a fresh launch: the dismissal was never written down.
  renderApp();
  expect(await screen.findByText(/updates have moved/)).toBeInTheDocument();
});

test("with access, no notice", async () => {
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: false, account: null };
    if (cmd === "check_update")
      return { available: null, blocked: null, failed_attempt: null, no_access: false };
  });
  renderApp();
  await screen.findByRole("button", { name: /sign in/i });
  expect(screen.queryByText(/updates have moved/)).not.toBeInTheDocument();
});

// The app runs for days and re-checks hourly, so access can be granted and
// then lost again within one session (revoked, or a transient auth failure
// that recurs). A dismiss must only cover the episode of missing access
// that was on screen when it was clicked - once that episode ends because
// access came back, a later loss has to be told again, not swallowed by a
// dismiss from episodes ago.
test("dismissing, then losing access again after it was restored, shows the notice again", async () => {
  let noAccess = true;
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: true, account: "a@b.com" };
    if (cmd === "check_update")
      return { available: null, blocked: null, failed_attempt: null, no_access: noAccess };
    if (cmd === "list_orgs") return [{ name: "acme", url: "" }];
    if (cmd === "plugin:event|listen") return 1;
  });
  const { qc } = renderApp();
  await screen.findByText(/updates have moved/);
  fireEvent.click(screen.getByRole("button", { name: /dismiss update notice/i }));
  expect(screen.queryByText(/updates have moved/)).not.toBeInTheDocument();

  // Access restored: the notice hides on its own, and the spent dismissal
  // stops mattering.
  noAccess = false;
  await act(async () => {
    await qc.invalidateQueries({ queryKey: ["update"] });
  });
  await waitFor(() => {
    expect(screen.queryByText(/updates have moved/)).not.toBeInTheDocument();
  });

  // Access lost again: this is a new episode, so the old dismiss must not
  // apply to it.
  noAccess = true;
  await act(async () => {
    await qc.invalidateQueries({ queryKey: ["update"] });
  });
  expect(await screen.findByText(/updates have moved/)).toBeInTheDocument();
});

// Regression guard for the sign-in fix: the launch-time `["update"]` check
// necessarily runs signed out (tokens are in-memory only, so every launch
// starts that way), which means it can never reach DevOps. Signing in has
// to give the query a second, real chance rather than leaving it parked on
// the launch-time answer for up to an hour.
test("signing in triggers a second update check", async () => {
  let signedIn = false;
  let checkUpdateCalls = 0;
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: signedIn, account: signedIn ? "a@b.com" : null };
    if (cmd === "check_update") {
      checkUpdateCalls += 1;
      return { available: null, blocked: null, failed_attempt: null, no_access: false };
    }
    if (cmd === "sign_in") {
      signedIn = true;
      return { signed_in: true, account: "a@b.com" };
    }
    if (cmd === "list_orgs") return [{ name: "acme", url: "" }];
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "plugin:event|listen") return 1;
  });
  // `mockIPC` fakes `window.__TAURI_INTERNALS__` but not the `globalThis.isTauri`
  // flag the sign-in mutation guards on - without it every click would hit
  // the "plain browser" early-throw instead of ever calling `sign_in`.
  (globalThis as unknown as { isTauri?: boolean }).isTauri = true;
  try {
    renderApp();
    const signInButton = await screen.findByRole("button", { name: /sign in with microsoft/i });
    await waitFor(() => expect(checkUpdateCalls).toBe(1));

    fireEvent.click(signInButton);
    await screen.findByText("a@b.com");

    await waitFor(() => expect(checkUpdateCalls).toBeGreaterThan(1));
  } finally {
    delete (globalThis as unknown as { isTauri?: boolean }).isTauri;
  }
});

/** The tour has to be visibly running before we assert on it. */
async function startTour() {
  await act(async () => {
    window.dispatchEvent(new Event(START_TOUR_EVENT));
  });
  await screen.findByRole("dialog", { name: "Interface tour" });
}

const next = () => fireEvent.click(screen.getByRole("button", { name: "Next" }));

/** The tour never moves the app: a stop that lives somewhere else waits
 * for the user to click their way there. */
const tourWaiting = () =>
  screen.getByRole("dialog", { name: "Interface tour" }).getAttribute("data-waiting") === "true";

/** The ONE control the tour has left live - a rail row, or the pill that
 * crosses into the Work Manager. Throws if the lock has leaked. */
function liveControl(): HTMLElement {
  const nav = screen.getByRole("navigation");
  const rows = within(nav)
    .getAllByRole("button")
    .filter((b) => !(b as HTMLButtonElement).disabled);
  if (rows.length > 1) throw new Error(`${rows.length} rail controls are live, expected one`);
  if (rows.length === 1) return rows[0];
  return screen.getByRole("button", { name: /^(Work Manager|Test Case Manager)$/ });
}

/** Next when the tour offers it, otherwise the click it is waiting for. */
const advance = () => (tourWaiting() ? fireEvent.click(liveControl()) : next());

/** Walk the tour to stop `n` (1-based), doing whatever each stop asks. */
async function walkToStop(n: number) {
  const label = `${n} / ${TOUR_STEPS.length}`;
  for (let guard = 0; guard < 3 * TOUR_STEPS.length; guard++) {
    if (screen.queryByText(label) && !tourWaiting()) return;
    advance();
    await act(async () => {});
  }
  throw new Error(`the tour never reached stop ${n}`);
}

// Stop 5 rings the queue, and the queue is the one area on the route that
// is NOT fed by a command the tour stands in for - it comes off the saved
// draft, which is empty for everyone. The stop drew a ~12px ring around
// nothing. This walks the whole route and checks each ringed area is
// there AND has something in it.
//
// jsdom does no layout, so "an area with non-zero size" cannot be measured
// here: every element reports 0x0 whatever it holds. The honest proxy is
// content - an anchor wrapping a component that returned null renders
// empty, which is exactly the shape of the bug this catches.
test("every stop rings an area that is there, with something in it", async () => {
  signedInMocks((cmd) => {
    if (cmd === "list_projects") return [{ id: "p1", name: "Payments" }];
    if (cmd === "list_plans_with_suites") return [];
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
  });
  renderApp();
  await screen.findByText("a@b.com");
  await startTour();

  for (let i = 0; i < TOUR_STEPS.length; i++) {
    const step = TOUR_STEPS[i];
    const at = `stop ${i + 1} ("${step.title}")`;
    // Walking is the user's job now: a stop that lives elsewhere asks, and
    // this clicks the one control it left live.
    await walkToStop(i + 1);
    if (step.anchor) {
      await waitFor(() => {
        const found = document.querySelector(`[data-tour="${step.anchor}"]`);
        expect(found, `${at} rings nothing - no [data-tour="${step.anchor}"] on screen`).not.toBeNull();
        const el = found as HTMLElement;
        // The gear is the one anchor with no words in it by design.
        if (step.anchor === "settings") {
          expect(el.querySelector("svg"), `${at} rings an EMPTY box`).not.toBeNull();
        } else {
          expect(el.textContent?.trim(), `${at} rings an EMPTY box`).not.toBe("");
        }
      });
    }
  }

  fireEvent.click(screen.getByText("Skip tour"));
  // Eighteen stops, each waiting for a screen to mount: comfortably under
  // the 5s default on its own, but not while the whole suite is running.
}, 30_000);

// The queue is served from a draft in storage, not from a command, so the
// tour has to hand it sample cases itself - and it must do that WITHOUT
// reading, writing or clearing the user's own draft. This proves both:
// the sample queue is on screen at the Manual Entry stop, and the real
// draft is byte-identical afterwards - including at every instant in
// between, which is where a seed-plus-guard would have clobbered it.
test("the tour's queue is populated and the real draft is never touched", async () => {
  const draft = [
    {
      title: "My own queued case",
      steps: [{ action: "Open the app", expected: "It opens" }],
      tags: "smoke",
      automation_status: "Not Automated",
      module_value: "",
      preconditions: "",
      update_id: null,
    },
  ];
  const REAL_KEY = "tcm-v2-draft:acme/99";
  const raw = JSON.stringify(draft);
  localStorage.setItem(REAL_KEY, raw);
  localStorage.setItem(
    "tcm-v2-prefs",
    JSON.stringify({
      org: "acme",
      project: "Payments",
      section: "manual",
      pbi: { id: 99, title: "Checkout", work_item_type: "Product Backlog Item" },
      workMode: false,
    }),
  );

  // Every value the real key is ever given, not just the one it ends on.
  const writes: string[] = [];
  const setItem = Storage.prototype.setItem;
  const removeItem = Storage.prototype.removeItem;
  const spy = vi
    .spyOn(Storage.prototype, "setItem")
    .mockImplementation(function (this: Storage, k: string, v: string) {
      if (k === REAL_KEY) writes.push(v);
      setItem.call(this, k, v);
    });
  const rmSpy = vi
    .spyOn(Storage.prototype, "removeItem")
    .mockImplementation(function (this: Storage, k: string) {
      if (k === REAL_KEY) writes.push("<removed>");
      removeItem.call(this, k);
    });

  try {
    signedInMocks((cmd) => {
      if (cmd === "list_projects") return [{ id: "p1", name: "Payments" }];
      if (cmd === "list_plans_with_suites") return [];
      if (cmd === "ensure_pbi_suite") return { plan_id: 9, plan_name: "Plan", suite_id: 91 };
      if (cmd === "pbi_test_cases") return [];
      if (cmd === "pr_overview") return { awaiting: [], mine: [] };
    });
    renderApp();
    await screen.findByText("a@b.com");
    // The user's own queue is on screen before the tour starts.
    expect(await screen.findByText(/Queue for PBI #99/)).toBeInTheDocument();

    await startTour();
    // Welcome, scope, item, form, batch - stop 5 is the queue.
    await walkToStop(5);

    const queue = await waitFor(() => {
      const el = document.querySelector('[data-tour="queue"]') as HTMLElement;
      expect(el?.textContent ?? "").toContain("Queue for PBI #4821");
      return el;
    });
    // A batch worth showing: several cases, and at least one update, so
    // the main button has both halves of its wording to say.
    expect(queue.textContent).toMatch(/Review \d+ test cases/);
    expect(queue.textContent).toContain("UPDATE #");

    fireEvent.click(screen.getByText("Skip tour"));
    await screen.findByRole("heading", { name: "Manual Entry" });
    // The user's own queue is back - waited for, because the reload lands
    // a render after the scope changes (which is precisely why the save
    // has to sit that render out).
    expect(await screen.findByText("My own queued case")).toBeInTheDocument();
    // Awaited, not read synchronously: the heading and the case body do
    // not necessarily land in the same render, so a sync query here races
    // the one above. And given longer than this file's 5s query budget,
    // because this is the heaviest test in the suite - it walks the whole
    // tour and then waits on a teardown that restores the stand-in calls,
    // three overrides, seven pieces of state and a reloaded draft. Under
    // full-suite load that has taken over five seconds. The failure reads
    // as "the queue never came back" rather than "the machine was busy",
    // which has misdiagnosed this suite more than once.
    expect(
      await screen.findByText(/Queue for PBI #99/, undefined, { timeout: 8_000 }),
    ).toBeInTheDocument();

    // The sample queue was never saved anywhere...
    expect(localStorage.getItem("tcm-v2-draft:Northwind/4821")).toBeNull();
    // ...and the user's own draft came back exactly as it went in - at no
    // point did it hold anything else. Asserted with the spies still on,
    // so a late write would be caught too.
    expect(localStorage.getItem(REAL_KEY)).toBe(raw);
    for (const v of writes) expect(v).toBe(raw);
  } finally {
    spy.mockRestore();
    rmSpy.mockRestore();
  }
});

test("the tour shows sample data, then hands the app back untouched", async () => {
  localStorage.setItem(
    "tcm-v2-prefs",
    JSON.stringify({ org: "acme", project: "Payments", section: "manual", pbi: null, workMode: false }),
  );
  signedInMocks((cmd) => {
    if (cmd === "list_projects") return [{ id: "p1", name: "Payments" }];
    if (cmd === "list_plans_with_suites") return [];
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
  });
  const realOrgs = commands.listOrgs;
  renderApp();
  await screen.findByText("a@b.com");

  await startTour();
  // Walk to the seventh stop (Update Test Cases), where the sample cases
  // are on screen: welcome, scope, item, form, batch, import, update.
  // The last two live on other tabs, so the tour asks instead of moving -
  // walkToStop clicks whichever row it left live.
  await walkToStop(7);
  expect(await screen.findByRole("heading", { name: "Update Test Cases" })).toBeInTheDocument();
  expect(await screen.findByText(/Guest checkout - a guest can pay by card/)).toBeInTheDocument();

  // Keep going, through View Test Cases, to Run Tests (ninth stop) - the
  // screen with its own suite-seed writer (RunPanel), separate from the
  // one App gates in its own warm-up effect. A walk that stopped short of
  // here is exactly what let that second writer go unnoticed.
  await walkToStop(9);
  expect(await screen.findByRole("heading", { name: "Run Tests" })).toBeInTheDocument();

  fireEvent.click(screen.getByText("Skip tour"));

  // Everything is back: real calls, real scope, and the saved context was
  // never overwritten with the sample one.
  expect(commands.listOrgs).toBe(realOrgs);
  expect(await screen.findByRole("heading", { name: "Manual Entry" })).toBeInTheDocument();
  expect(screen.queryByText(/Guest checkout/)).not.toBeInTheDocument();
  expect(JSON.parse(localStorage.getItem("tcm-v2-prefs")!)).toMatchObject({
    org: "acme",
    project: "Payments",
    pbi: null,
    workMode: false,
  });
  expect(localStorage.getItem("tcm-v2-repositories")).toBeNull();
  // Update Test Cases (the screen this walk passed through) reads its own
  // module/preconditions field refs straight from org/project props, not
  // through anything App gates - the guard has to be in saveFieldPrefs
  // itself, and this proves it held from here too. (A real
  // tcm-v2-fields:acme/Payments entry is fine - that one was written for
  // the actual scope, before the tour ever started.)
  expect(localStorage.getItem("tcm-v2-fields:Northwind/Website")).toBeNull();
  // Run Tests resolves and caches a plan/suite for the PBI on screen -
  // RunPanel writes that seed directly, not through anything App gates,
  // so this is the assertion that would have caught it.
  expect(localStorage.getItem("tcm-v2-suite:Northwind/4821")).toBeNull();
});

// The walk above starts from Manual Entry, which stop 2 navigates to
// anyway - so it never actually asked whether the section is put back.
// Settings is where most people press "Show UI tour", which makes it the
// section that most often has to come back.
test("the tour hands the app back to the section it was started from", async () => {
  signedInMocks((cmd) => {
    if (cmd === "list_plans_with_suites") return [];
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
  });
  renderApp();
  await screen.findByText("a@b.com");

  fireEvent.click(screen.getByLabelText("Settings"));
  expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument();

  // Started the way a user starts it: Settings' own button.
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Show UI tour" }));
  });
  await screen.findByRole("dialog", { name: "Interface tour" });

  // Stop 2 lives on Manual Entry, and the app is on Settings - so the tour
  // asks for that click rather than making it. Nothing moves until it comes.
  next();
  await waitFor(() => expect(tourWaiting()).toBe(true));
  expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument();

  // Far enough in that the app has been walked somewhere else.
  await walkToStop(4);
  expect(await screen.findByRole("heading", { name: "Manual Entry" })).toBeInTheDocument();

  fireEvent.click(screen.getByText("Skip tour"));
  expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
});

// Test Suites can hand a set of cases to Update Test Cases; that handoff
// is App state, not a saved preference, and the tour clears it on its way
// through (every "cases" stop does). It is captured and put back with
// everything else - which nothing was asserting.
test("the tour gives the Test Suites handoff back", async () => {
  localStorage.setItem(
    "tcm-v2-prefs",
    JSON.stringify({
      org: "acme",
      project: "Payments",
      section: "suites",
      pbi: null,
      workMode: false,
    }),
  );
  signedInMocks((cmd, args) => {
    if (cmd === "list_projects") return [{ id: "p1", name: "Payments" }];
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: { id: 9, name: "Auth - Test Plan", area_path: "Proj", root_suite_id: 90 },
          suites: [
            {
              id: 95,
              name: "Regression",
              suite_type: "staticTestSuite",
              requirement_id: null,
              parent_id: null,
            },
          ],
        },
      ];
    if (cmd === "list_test_points")
      return (args as { suiteId: number }).suiteId === 95
        ? [
            {
              point_id: 7,
              test_case_id: 201,
              test_case_name: "Valid login",
              config_name: "W10",
              tester: "",
              last_outcome: "",
              last_run_id: null,
              last_result_id: null,
            },
          ]
        : [];
    if (cmd === "test_cases_by_ids") return [];
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
  });
  renderApp();
  await screen.findByText("a@b.com");
  await screen.findByText("Regression");

  fireEvent.click(screen.getAllByText("Edit cases")[0]);
  const handedOver = "Showing cases handed over from the Test Suites browser.";
  expect(await screen.findByText(handedOver)).toBeInTheDocument();

  await startTour();
  // Stop 7 is Update Test Cases - the same screen, on the sample data, so
  // the handoff really has to have been cleared and then restored. Getting
  // there means clicking the tabs the tour asks for, one at a time.
  await walkToStop(7);
  expect(await screen.findByRole("heading", { name: "Update Test Cases" })).toBeInTheDocument();
  expect(screen.queryByText(handedOver)).not.toBeInTheDocument();

  fireEvent.click(screen.getByText("Skip tour"));
  expect(await screen.findByText(handedOver)).toBeInTheDocument();
});

test("the app is locked while the tour runs", async () => {
  signedInMocks((cmd) => {
    if (cmd === "list_plans_with_suites") return [];
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
  });
  renderApp();
  await screen.findByText("a@b.com");
  await startTour();

  // Stops 1-5 all live on Manual Entry, where the app already is; stop 6
  // lives on Import File, so the tour stops and asks for that one click.
  await walkToStop(5);
  next();
  await waitFor(() => expect(tourWaiting()).toBe(true));

  // The screens are inert, so nothing under the overlay can be reached.
  // jsdom does not implement inert semantics (a click still "reaches" a
  // button inside one) - this only proves the attribute made it onto the
  // DOM; the disabled-row and Ctrl+2 assertions below are what actually
  // prove the shell is unusable.
  const main = document.querySelector("main")!;
  expect(main.closest("[inert]")).not.toBeNull();
  // The rail deliberately sits OUTSIDE that region - `inert` is inherited,
  // so a hole cannot be punched through it for the row being asked for.
  // It locks itself instead, which is what the next block checks.
  const nav = screen.getByRole("navigation");
  expect(nav.closest("[inert]")).toBeNull();

  // Exactly one row answers: the one the stop is waiting for. Every other
  // row - and the collapse toggle - is disabled, and a disabled button
  // does not run its handler however it is pressed.
  const row = (name: string) => within(nav).getByRole("button", { name });
  expect(row("Import File")).toBeEnabled();
  expect(row("Update Test Cases")).toBeDisabled();
  expect(row("Manual Entry")).toBeDisabled();
  expect(within(nav).getByRole("button", { name: "Close sidebar" })).toBeDisabled();
  expect(liveControl()).toBe(row("Import File"));

  fireEvent.click(row("Update Test Cases"));
  await act(async () => {});
  expect(screen.getByRole("heading", { name: "Manual Entry" })).toBeInTheDocument();
  // ...and the tour did not take that for the click it was waiting for.
  expect(tourWaiting()).toBe(true);
  expect(screen.getByText(`6 / ${TOUR_STEPS.length}`)).toBeInTheDocument();

  // And the tab shortcuts are off: Ctrl+2 would normally open Import File.
  // The click above is the only way past this stop - a shortcut must not
  // skip the sequence it enforces.
  await act(async () => {
    fireEvent.keyDown(window, { key: "2", ctrlKey: true });
  });
  expect(screen.getByRole("heading", { name: "Manual Entry" })).toBeInTheDocument();
  expect(tourWaiting()).toBe(true);

  // The command palette is the third lock surface - Ctrl+K must not open
  // it either.
  await act(async () => {
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
  });
  expect(screen.queryByPlaceholderText(/Type a command/)).not.toBeInTheDocument();

  fireEvent.click(screen.getByText("Skip tour"));
  await act(async () => {
    fireEvent.keyDown(window, { key: "2", ctrlKey: true });
  });
  expect(await screen.findByRole("heading", { name: "Import File" })).toBeInTheDocument();
});

// The other half of the lock: the one row it does leave live has to
// actually work, and the tour has to pick itself up when the app arrives.
test("clicking the tab the tour asks for moves the app and carries the tour on", async () => {
  signedInMocks((cmd) => {
    if (cmd === "list_plans_with_suites") return [];
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
  });
  renderApp();
  await screen.findByText("a@b.com");
  await startTour();

  await walkToStop(5);
  next();
  await waitFor(() => expect(tourWaiting()).toBe(true));
  // While it waits, the card asks for the row by the name the rail shows.
  expect(screen.getByText("Go to Import File")).toBeInTheDocument();
  // ...and takes Next away, so the ask cannot be shrugged off.
  expect(screen.queryByRole("button", { name: "Next" })).not.toBeInTheDocument();

  fireEvent.click(within(screen.getByRole("navigation")).getByRole("button", { name: "Import File" }));

  expect(await screen.findByRole("heading", { name: "Import File" })).toBeInTheDocument();
  await waitFor(() => expect(tourWaiting()).toBe(false));
  expect(screen.getByText("Bring cases in from a file")).toBeInTheDocument();
  // Arrived, so Next is back.
  expect(screen.getByRole("button", { name: "Next" })).toBeInTheDocument();

  fireEvent.click(screen.getByText("Skip tour"));
});

// The overlay only renders `tourOpen && signedIn` - a session that ends
// mid-tour (an expired token noticed on reconnect, a manual sign-out)
// makes it vanish on its own, taking the Skip button with it while the
// shell stayed locked. Nothing short of a restart used to clear that.
test("signing out mid-tour ends the tour and unlocks the app", async () => {
  let signedIn = true;
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: signedIn, account: signedIn ? "a@b.com" : null };
    if (cmd === "check_update") return null;
    if (cmd === "list_orgs") return [{ name: "acme", url: "" }];
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "list_plans_with_suites") return [];
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
  });
  const { qc } = renderApp();
  await screen.findByText("a@b.com");
  await startTour();

  signedIn = false;
  await act(async () => {
    await qc.invalidateQueries({ queryKey: ["auth"] });
  });

  // The overlay is gone, and so is everything it was holding: no dialog,
  // no inert shell, and the sign-in screen is reachable rather than stuck
  // behind a lock nothing can lift. The cache updates a beat before the
  // component re-renders off it, so this waits for that rather than
  // asserting on the instant right after the invalidate settles.
  const signInButton = await screen.findByRole("button", { name: /sign in with microsoft/i });
  expect(signInButton).toBeInTheDocument();
  expect(screen.queryByRole("dialog", { name: "Interface tour" })).not.toBeInTheDocument();
  expect(document.querySelector("[inert]")).toBeNull();
});
