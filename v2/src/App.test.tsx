import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import App from "./App";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function renderApp() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <App />
    </QueryClientProvider>,
  );
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

  fireEvent.click(screen.getByRole("button", { name: /Work Manager \(Beta\)/ }));
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
  renderApp();
  await screen.findByText("a@b.com");
  await vi.waitFor(() => expect(pushes.length).toBeGreaterThan(0));
  expect(pushes[pushes.length - 1]).toMatchObject({ organization: "acme", project: "Web" });
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
