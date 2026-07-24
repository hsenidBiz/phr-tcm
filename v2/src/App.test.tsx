import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
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

test("the app pushes org/project context to the AI bridge", async () => {
  const pushes: Array<Record<string, unknown>> = [];
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
    if (cmd === "bridge_status") return { port: 1, mcp_exe: "x" };
  });
  renderApp();
  await screen.findByText("a@b.com");
  await vi.waitFor(() => expect(pushes.length).toBeGreaterThan(0));
  expect(pushes[pushes.length - 1]).toMatchObject({ organization: "acme", project: "Web" });
});

test("update banner appears when a newer version exists", async () => {
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: false, account: null };
    if (cmd === "check_update") return "0.5.0";
  });
  renderApp();
  expect(await screen.findByText(/Version 0.5.0 is available/)).toBeInTheDocument();
});
