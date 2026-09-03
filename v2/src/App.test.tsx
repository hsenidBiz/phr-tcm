import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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

/** The tour has to be visibly running before we assert on it. */
async function startTour() {
  await act(async () => {
    window.dispatchEvent(new Event(START_TOUR_EVENT));
  });
  await screen.findByRole("dialog", { name: "Interface tour" });
}

const next = () => fireEvent.click(screen.getByRole("button", { name: "Next" }));

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
    if (i < TOUR_STEPS.length - 1) next();
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
    for (let i = 0; i < 4; i++) next();

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
    expect(screen.getByText(/Queue for PBI #99/)).toBeInTheDocument();

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
  for (let i = 0; i < 6; i++) {
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
  }
  expect(await screen.findByRole("heading", { name: "Update Test Cases" })).toBeInTheDocument();
  expect(await screen.findByText(/Guest checkout - a guest can pay by card/)).toBeInTheDocument();

  // Keep going, through View Test Cases, to Run Tests (ninth stop) - the
  // screen with its own suite-seed writer (RunPanel), separate from the
  // one App gates in its own warm-up effect. A walk that stopped short of
  // here is exactly what let that second writer go unnoticed.
  for (let i = 0; i < 2; i++) {
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
  }
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

  // Far enough in that the tour has driven the app somewhere else.
  for (let i = 0; i < 3; i++) next();
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
  // the handoff really has to have been cleared and then restored.
  for (let i = 0; i < 6; i++) next();
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

  // The shell is inert, so nothing under the overlay can be reached. jsdom
  // does not implement inert semantics (a click still "reaches" a button
  // inside one) - this only proves the attribute made it onto the DOM; the
  // Ctrl+2 and Ctrl+K assertions below are what actually prove the shell
  // is unusable.
  const nav = screen.getByRole("navigation");
  expect(nav.closest("[inert]")).not.toBeNull();

  // And the tab shortcuts are off: Ctrl+2 would normally open Import File.
  await act(async () => {
    fireEvent.keyDown(window, { key: "2", ctrlKey: true });
  });
  expect(screen.getByRole("heading", { name: "Manual Entry" })).toBeInTheDocument();

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
