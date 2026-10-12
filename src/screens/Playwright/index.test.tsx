// The Playwright tab: the clone folder, the health line, what the clone
// holds, and the Export dialog Auto Run's menu item opens here.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { RepoStructure } from "../../bindings";
import { clearPlaywrightExportRequest, playwrightExportSnapshot, requestPlaywrightExport } from "../../lib/playwrightExport";
import { HEALTH_TEXT } from "./HealthLine";
import Playwright from "./index";

const pick = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: pick, save: vi.fn() }));

afterEach(() => {
  clearPlaywrightExportRequest();
  clearMocks();
  vi.clearAllMocks();
});

const HEALTHY: RepoStructure = {
  features: [
    {
      country: "sl",
      side: "admin",
      module: "performance",
      feature: "rating-methods",
      spec_count: 1,
      case_count: 3,
      case_ids: [135152, 135179, 135180],
    },
    {
      country: "sl",
      side: "self",
      module: "smoke",
      feature: "framework-smoke",
      spec_count: 2,
      case_count: 0,
      case_ids: [],
    },
  ],
  generated_ids: [135152, 135179],
  user_keys: ["A", "B"],
  navigation_keys: ["performance/rating-methods"],
  health: [],
};

type Call = { cmd: string; args: Record<string, unknown> };

function mount(clone: string, structure: RepoStructure | (() => RepoStructure) = HEALTHY) {
  const calls: Call[] = [];
  let saved = clone;
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === "get_app_settings") return { playwright_clone: saved };
    if (cmd === "set_playwright_clone") {
      saved = (args as { path: string }).path;
      return { playwright_clone: saved };
    }
    if (cmd === "pw_repo_structure") return typeof structure === "function" ? structure() : structure;
    if (cmd === "pw_export_preview") throw "no preview in this test";
    return null;
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <Playwright org="acme" project="Web" />
    </QueryClientProvider>,
  );
  return calls;
}

test("a healthy clone shows it is ready, and what it holds", async () => {
  const calls = mount("C:/repo");
  expect(await screen.findByText("The clone is ready.")).toBeInTheDocument();
  expect(screen.getByText("C:/repo")).toBeInTheDocument();
  expect(
    screen.getByText("2 features · 3 specs · 3 test cases · 2 raw specs · 2 accounts · 1 navigation entry"),
  ).toBeInTheDocument();
  expect(screen.getByText("sl / admin / performance / rating-methods")).toBeInTheDocument();
  expect(screen.getByText("1 spec, 3 test cases")).toBeInTheDocument();
  expect(calls.find((c) => c.cmd === "pw_repo_structure")?.args).toEqual({ clone: "C:/repo" });
});

test("the health line names each issue", async () => {
  mount("C:/repo", { ...HEALTHY, health: ["NoNodeModules", "NoNode", "NoQaOptions"] });
  const line = await screen.findByRole("status", { name: "Repo health" });
  expect(line).toHaveTextContent(HEALTH_TEXT.NoNodeModules);
  expect(line).toHaveTextContent(HEALTH_TEXT.NoNode);
  expect(line).toHaveTextContent(HEALTH_TEXT.NoQaOptions);
  expect(HEALTH_TEXT.NoNodeModules).toContain("npm install");
  expect(HEALTH_TEXT.NoQaOptions).toContain("default");
});

test("a folder that is not the repo says so and shows no structure", async () => {
  mount("C:/elsewhere", { ...HEALTHY, features: [], health: ["NotTheRepo"] });
  expect(await screen.findByText(HEALTH_TEXT.NotTheRepo)).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Repo structure" })).not.toBeInTheDocument();
});

test("with no folder chosen nothing is read, and Choose sets one", async () => {
  const calls = mount("");
  expect(await screen.findByText("No folder chosen yet.")).toBeInTheDocument();
  expect(calls.some((c) => c.cmd === "pw_repo_structure")).toBe(false);

  pick.mockResolvedValueOnce("D:/clone");
  fireEvent.click(screen.getByRole("button", { name: "Choose…" }));
  expect(await screen.findByText("D:/clone")).toBeInTheDocument();
  expect(calls.find((c) => c.cmd === "set_playwright_clone")?.args).toEqual({ path: "D:/clone" });
  await screen.findByText("The clone is ready.");
  expect(calls.find((c) => c.cmd === "pw_repo_structure")?.args).toEqual({ clone: "D:/clone" });
  expect(screen.getByRole("button", { name: "Change…" })).toBeInTheDocument();
});

test("Auto Run's Export request opens the Export dialog here, and closing it clears the request", async () => {
  mount("C:/repo");
  await screen.findByText("The clone is ready.");
  expect(screen.queryByRole("heading", { name: "Export to Playwright" })).not.toBeInTheDocument();

  act(() => requestPlaywrightExport({ pbiId: 7, caseIds: [10], modules: [[10, "Wizard"]] }));
  expect(await screen.findByRole("heading", { name: "Export to Playwright" })).toBeInTheDocument();
  expect(screen.getByText(/passing scripts of PBI 7/)).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("heading", { name: "Export to Playwright" })).not.toBeInTheDocument());
  expect(playwrightExportSnapshot()).toBeNull();
});
