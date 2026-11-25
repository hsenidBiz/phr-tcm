// The top bar's Suite mode: finds a suite among the plans Search Suites
// browses, lists it as plan > folder > suite, and holds the pick as a chip.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ContextBar from "../../src/components/ContextBar";
import type { SuiteScope } from "../../src/lib/scope";
import { resetAppSimpleModeStore, setAppSimpleModeForTest } from "../../src/lib/appSimpleMode";

afterEach(() => {
  resetAppSimpleModeStore();
  clearMocks();
  localStorage.clear();
});

const PLANS = [
  {
    plan: { id: 9, name: "Auth Plan", area_path: "Web", root_suite_id: 90 },
    suites: [
      { id: 91, name: "Login folder", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
      { id: 92, name: "Password reset", suite_type: "requirementTestSuite", requirement_id: 42, parent_id: 91 },
    ],
  },
];

function bar(over: Partial<React.ComponentProps<typeof ContextBar>> = {}) {
  mockIPC((cmd) => {
    if (cmd === "list_orgs") return [];
    if (cmd === "list_projects") return [];
    if (cmd === "pr_overview") return { mine: [], awaiting: [] };
    if (cmd === "recent_mentions") return [];
    if (cmd === "list_plans_with_suites") return PLANS;
  });
  const props = {
    org: "acme",
    setOrg: () => {},
    project: "Web",
    setProject: () => {},
    allProjects: false,
    homeProject: "Web",
    pbi: null,
    setPbi: vi.fn(),
    suite: null as SuiteScope | null,
    setSuite: vi.fn(),
    account: null,
    workMode: false,
    onToggleWork: () => {},
    onOpenSettings: () => {},
    ...over,
  };
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ContextBar {...props} />
    </QueryClientProvider>,
  );
  return props;
}

test("Suite mode finds a suite by name and picks it with its plan and project", async () => {
  const props = bar();
  fireEvent.click(screen.getByRole("button", { name: "Suite" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Find suite" }), { target: { value: "reset" } });
  // Listed as plan > folder > suite.
  const hit = await screen.findByRole("button", { name: "Auth Plan › Login folder › Password reset" });
  fireEvent.click(hit);
  expect(props.setSuite).toHaveBeenCalledWith({
    planId: 9,
    suiteId: 92,
    title: "Password reset",
    planName: "Auth Plan",
    project: "Web",
  });
});

test("nothing is read until the box is used", async () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "list_orgs" || cmd === "list_projects" || cmd === "recent_mentions") return [];
    if (cmd === "pr_overview") return { mine: [], awaiting: [] };
    if (cmd === "list_plans_with_suites") return PLANS;
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ContextBar
        org="acme" setOrg={() => {}} project="Web" setProject={() => {}} allProjects={false} homeProject="Web"
        pbi={null} setPbi={() => {}} suite={null} setSuite={() => {}} account={null} workMode={false}
        onToggleWork={() => {}} onOpenSettings={() => {}}
      />
    </QueryClientProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Suite" }));
  await new Promise((r) => setTimeout(r, 50));
  expect(calls).not.toContain("list_plans_with_suites");
  fireEvent.focus(screen.getByRole("textbox", { name: "Find suite" }));
  await waitFor(() => expect(calls).toContain("list_plans_with_suites"));
});

test("a held suite shows as a chip with its folder, title and plan, and clearing it clears the suite", () => {
  const props = bar({ suite: { planId: 9, suiteId: 92, title: "Password reset", planName: "Auth Plan", project: "Web" } });
  expect(screen.getByText("Password reset")).toBeInTheDocument();
  expect(screen.getByText("Auth Plan")).toBeInTheDocument();
  // The switch is gone while a scope is held; the chip's clear is the way back.
  expect(screen.queryByRole("group", { name: "Scope kind" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Clear suite" }));
  expect(props.setSuite).toHaveBeenCalledWith(null);
});

test("the PBI box stays the default and is untouched by the switch until Suite is chosen", () => {
  bar();
  expect(screen.getByRole("textbox", { name: "Find PBI" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "PBI" })).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(screen.getByRole("button", { name: "Suite" }));
  expect(screen.queryByRole("textbox", { name: "Find PBI" })).not.toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Find suite" })).toBeInTheDocument();
});

test("Suite mode still works with app Basic mode on", () => {
  setAppSimpleModeForTest(true);
  bar();
  fireEvent.click(screen.getByRole("button", { name: "Suite" }));
  expect(screen.getByRole("textbox", { name: "Find suite" })).toBeInTheDocument();
});

test("with no setSuite (Auto Test off) the bar has no switch", () => {
  bar({ setSuite: undefined });
  expect(screen.queryByRole("group", { name: "Scope kind" })).not.toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Find PBI" })).toBeInTheDocument();
});
