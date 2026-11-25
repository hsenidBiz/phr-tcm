// The top bar holds a PBI or a test suite, never both; both are remembered
// across restarts; the AI bridge is told about the suite on every push; and
// Search Suites' Auto Test opens Auto Test on the suite it was pressed on.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { configure, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import App from "../src/App";
import { ALL_MODULES, resetModulesStore } from "../src/lib/modules";
import { resetForTests as resetNotifications } from "../src/lib/notifications";

vi.setConfig({ testTimeout: 15_000 });
configure({ asyncUtilTimeout: 5_000 });

afterEach(() => {
  clearMocks();
  localStorage.clear();
  resetModulesStore();
  resetNotifications();
});

const PLANS = [
  {
    plan: { id: 9, name: "Auth Plan", area_path: "Web", root_suite_id: 90 },
    suites: [{ id: 92, name: "Password reset", suite_type: "staticTestSuite", requirement_id: null, parent_id: null }],
  },
];
const storedPrefs = () => JSON.parse(localStorage.getItem("tcm-v2-prefs") ?? "{}");

function mount(
  saved: Record<string, unknown>,
  extra: (cmd: string, args: unknown) => unknown = () => undefined,
  modules: string[] = [...ALL_MODULES],
) {
  localStorage.setItem("tcm-v2-tour-done", "yes");
  localStorage.setItem("tcm-v2-prefs", JSON.stringify({ org: "acme", project: "Web", section: "manual", workMode: false, ...saved }));
  const pushes: Array<{ switches: { pbi: unknown; suite: unknown } }> = [];
  mockIPC((cmd, args) => {
    if (cmd === "modules_get") return modules;
    if (cmd === "resume_session") return { signed_in: true, account: "a@b.com" };
    if (cmd === "check_update") return null;
    if (cmd === "list_orgs") return [{ name: "acme", url: "" }];
    if (cmd === "list_projects") return [{ id: "p1", name: "Web" }];
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "bridge_status") return { port: 1, mcp_exe: "x" };
    if (cmd === "set_bridge_context") {
      pushes.push(args as never);
      return null;
    }
    if (cmd === "list_plans_with_suites") return PLANS;
    if (cmd === "search_pbis" || cmd === "search_pbis_all") return [{ id: 5, title: "Login work", work_item_type: "Product Backlog Item", project: "Web" }];
    return extra(cmd, args);
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <App />
    </QueryClientProvider>,
  );
  return pushes;
}

const SUITE = { planId: 9, suiteId: 92, title: "Password reset", planName: "Auth Plan", project: "Web" };

test("a remembered suite comes back as the chip, and every bridge push carries it", async () => {
  const pushes = mount({ pbi: null, suite: SUITE });
  expect(await screen.findByRole("button", { name: "Clear suite" })).toBeInTheDocument();
  expect(screen.getByText("Password reset")).toBeInTheDocument();
  await waitFor(() => expect(pushes.length).toBeGreaterThan(0));
  expect(pushes[pushes.length - 1].switches.suite).toEqual({
    plan_id: 9,
    suite_id: 92,
    title: "Password reset",
    plan_name: "Auth Plan",
    project: "Web",
  });
  expect(pushes[pushes.length - 1].switches.pbi).toBeNull();
});

test("choosing a PBI drops the suite, and choosing a suite drops the PBI", async () => {
  const pushes = mount({ pbi: null, suite: SUITE });
  fireEvent.click(await screen.findByRole("button", { name: "Clear suite" }));
  await waitFor(() => expect(storedPrefs().suite).toBeNull());
  // The box is still in Suite mode; switch to PBI and pick one.
  fireEvent.click(screen.getByRole("button", { name: "PBI" }));
  fireEvent.change(await screen.findByRole("textbox", { name: "Find PBI" }), { target: { value: "login" } });
  fireEvent.click(await screen.findByRole("button", { name: /Login work/ }));
  await waitFor(() => expect(storedPrefs().pbi?.id).toBe(5));
  expect(storedPrefs().suite).toBeNull();
  await waitFor(() => expect(pushes[pushes.length - 1].switches.suite).toBeNull());

  // And back: clear the PBI, pick the suite.
  fireEvent.click(screen.getByRole("button", { name: "Clear PBI" }));
  fireEvent.click(await screen.findByRole("button", { name: "Suite" }));
  fireEvent.change(await screen.findByRole("textbox", { name: "Find suite" }), { target: { value: "password" } });
  fireEvent.click(await screen.findByRole("button", { name: "Auth Plan › Password reset" }));
  await waitFor(() => expect(storedPrefs().suite?.suiteId).toBe(92));
  expect(storedPrefs().pbi).toBeNull();
  await waitFor(() => expect(pushes[pushes.length - 1].switches.suite).toMatchObject({ suite_id: 92 }));
  expect(pushes[pushes.length - 1].switches.pbi).toBeNull();
});

test("Search Suites' Auto Test sets the suite and opens Auto Test on its cases", async () => {
  const asked: Array<Record<string, unknown>> = [];
  mount({ pbi: null, suite: null, section: "suites" }, (cmd, args) => {
    if (cmd === "suite_test_cases_full") {
      asked.push(args as Record<string, unknown>);
      return [];
    }
  });
  fireEvent.click(await screen.findByRole("button", { name: "More actions for Password reset" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Auto Test" }));
  await waitFor(() => expect(storedPrefs().section).toBe("autorun"));
  expect(storedPrefs().suite).toMatchObject({ planId: 9, suiteId: 92, title: "Password reset", planName: "Auth Plan" });
  await waitFor(() => expect(asked.length).toBeGreaterThan(0));
  expect(asked[0]).toMatchObject({ org: "acme", project: "Web", planId: 9, suiteId: 92 });
});

test("with Auto Test off there is no suite switch, and a remembered suite is dropped from the prefs", async () => {
  mount({ pbi: null, suite: SUITE }, () => undefined, ["playwright"]);
  await screen.findByText("a@b.com");
  await waitFor(() => expect(storedPrefs().suite).toBeNull());
  expect(screen.queryByRole("button", { name: "Clear suite" })).not.toBeInTheDocument();
  expect(screen.queryByRole("group", { name: "Scope kind" })).not.toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Find PBI" })).toBeInTheDocument();
});
