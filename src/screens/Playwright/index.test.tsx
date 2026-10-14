// The Playwright tab: the clone folder, the health line, what the clone
// holds, and the Export dialog Auto Run's menu item opens here.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, expect, test, vi } from "vitest";
import type { RepoStructure } from "../../bindings";
import { clearPlaywrightExportRequest, playwrightExportSnapshot, requestPlaywrightExport } from "../../lib/playwrightExport";
import { HEALTH_LABEL, HEALTH_TEXT } from "./HealthLine";
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
    if (cmd === "pw_list") return { tests: [], projects: [], last_report: null, load_errors: [] };
    if (cmd === "pw_reports" || cmd === "pw_users_list" || cmd === "pw_sync_states") return [];
    return null;
  });
  const view = render(
    <StrictMode>
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <Playwright org="acme" project="Web" />
      </QueryClientProvider>
    </StrictMode>,
  );
  return Object.assign(calls, { unmount: view.unmount });
}

test("a healthy clone shows one All good badge, and what it holds", async () => {
  const calls = mount("C:/repo");
  const health = await screen.findByRole("status", { name: "Repo health" });
  expect(await within(health).findByText("All good")).toBeInTheDocument();
  expect(screen.getByText("C:/repo")).toHaveAttribute("title", "C:/repo");
  const structure = screen.getByRole("region", { name: "Repo structure" });
  const stat = (label: string) => within(structure).getByText(label).previousElementSibling?.textContent;
  expect(stat("features")).toBe("2");
  expect(stat("specs")).toBe("3");
  expect(stat("test cases")).toBe("3");
  expect(stat("raw specs")).toBe("2");
  expect(stat("accounts")).toBe("2");
  expect(stat("navigation entry")).toBe("1");
  const table = within(structure).getByRole("table", { name: "Features" });
  expect(within(table).getByRole("row", { name: "sl admin performance rating-methods 1 3" })).toBeInTheDocument();
  expect(within(table).getByRole("row", { name: "sl self smoke framework-smoke 2 0" })).toBeInTheDocument();
  expect(calls.find((c) => c.cmd === "pw_repo_structure")?.args).toEqual({ clone: "C:/repo" });
});

test("the health badges name each issue, with what to do on hover", async () => {
  mount("C:/repo", { ...HEALTHY, health: ["NoNodeModules", "NoNode", "NoQaOptions"] });
  const line = await screen.findByRole("status", { name: "Repo health" });
  for (const issue of ["NoNodeModules", "NoNode", "NoQaOptions"] as const) {
    expect(within(line).getByText(HEALTH_LABEL[issue]).closest("[title]")).toHaveAttribute("title", HEALTH_TEXT[issue]);
  }
  expect(line).not.toHaveTextContent("All good");
  expect(HEALTH_TEXT.NoNodeModules).toContain("npm install");
  expect(HEALTH_TEXT.NoQaOptions).toContain("default");
  // The Runner and Accounts say why they cannot work, in full; a missing
  // qa-options.json does not stop them.
  fireEvent.click(screen.getByRole("tab", { name: "Runner" }));
  const runner = screen.getByRole("tabpanel");
  expect(within(runner).getByText(HEALTH_TEXT.NoNodeModules)).toBeInTheDocument();
  expect(within(runner).getByText(HEALTH_TEXT.NoNode)).toBeInTheDocument();
  expect(within(runner).queryByText(HEALTH_TEXT.NoQaOptions)).not.toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Runner" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("tab", { name: "Overview" }));
});

test("the four areas are tabs, and the tab last chosen is kept for the session", async () => {
  const view = mount("C:/repo");
  await screen.findByText("All good");
  expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual(["Overview", "Runner", "Accounts 2", "Sync"]);
  expect(screen.getByRole("tab", { name: "Overview" })).toHaveAttribute("aria-selected", "true");
  fireEvent.click(screen.getByRole("tab", { name: "Sync" }));
  expect(screen.getByRole("tab", { name: "Sync" })).toHaveAttribute("aria-selected", "true");
  expect(screen.queryByRole("region", { name: "Repo structure" })).not.toBeInTheDocument();
  // The arrow keys move along the tabs.
  fireEvent.keyDown(screen.getByRole("tab", { name: "Sync" }), { key: "ArrowRight" });
  expect(screen.getByRole("tab", { name: "Overview" })).toHaveAttribute("aria-selected", "true");
  fireEvent.keyDown(screen.getByRole("tab", { name: "Overview" }), { key: "ArrowLeft" });
  view.unmount();

  mount("C:/repo");
  await screen.findByText("All good");
  expect(screen.getByRole("tab", { name: "Sync" })).toHaveAttribute("aria-selected", "true");
  fireEvent.click(screen.getByRole("tab", { name: "Overview" }));
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
  await screen.findByText("All good");
  expect(calls.find((c) => c.cmd === "pw_repo_structure")?.args).toEqual({ clone: "D:/clone" });
  expect(screen.getByRole("button", { name: "Change…" })).toBeInTheDocument();
});

test("Auto Run's Export request opens the Export dialog here, and closing it clears the request", async () => {
  mount("C:/repo");
  await screen.findByText("All good");
  expect(screen.queryByRole("heading", { name: "Export to Playwright" })).not.toBeInTheDocument();

  act(() => requestPlaywrightExport({ pbiId: 7, caseIds: [10], modules: [[10, "Wizard"]] }));
  expect(await screen.findByRole("heading", { name: "Export to Playwright" })).toBeInTheDocument();
  expect(screen.getByText(/passing scripts of PBI 7/)).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("heading", { name: "Export to Playwright" })).not.toBeInTheDocument());
  expect(playwrightExportSnapshot()).toBeNull();
});

test("a request left pending is cleared when the tab unmounts, so it cannot reopen the dialog later", async () => {
  act(() => requestPlaywrightExport({ pbiId: 7, caseIds: [10], modules: [[10, "Wizard"]] }));
  const view = mount("C:/repo");
  // StrictMode mounts, unmounts and mounts again: the request survives that.
  expect(await screen.findByRole("heading", { name: "Export to Playwright" })).toBeInTheDocument();
  expect(playwrightExportSnapshot()).not.toBeNull();

  view.unmount();
  await waitFor(() => expect(playwrightExportSnapshot()).toBeNull());

  mount("C:/repo");
  await screen.findByText("All good");
  expect(screen.queryByRole("heading", { name: "Export to Playwright" })).not.toBeInTheDocument();
});

test("Refresh reads every Playwright query again; nothing is read again unasked", async () => {
  const calls = mount("C:/repo");
  await screen.findByText("All good");
  for (const name of ["Runner", "Accounts 2", "Sync"]) fireEvent.click(screen.getByRole("tab", { name }));
  const PW = ["pw_repo_structure", "pw_list", "pw_reports", "pw_users_list", "pw_sync_states"];
  await waitFor(() => expect(PW.every((cmd) => calls.some((c) => c.cmd === cmd))).toBe(true));
  const count = (cmd: string) => calls.filter((c) => c.cmd === cmd).length;
  const before = Object.fromEntries(PW.map((cmd) => [cmd, count(cmd)]));

  // Going back and forth between the tabs reads nothing again.
  fireEvent.click(screen.getByRole("tab", { name: "Overview" }));
  fireEvent.click(screen.getByRole("tab", { name: "Runner" }));
  for (const cmd of PW) expect(count(cmd)).toBe(before[cmd]);

  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  await waitFor(() => {
    for (const cmd of PW) expect(count(cmd)).toBeGreaterThan(before[cmd]);
  });
  fireEvent.click(screen.getByRole("tab", { name: "Overview" }));
});

test("a tab is read only once it is opened, and stays mounted after", async () => {
  const calls = mount("C:/repo");
  await screen.findByText("All good");
  expect(screen.getByRole("tab", { name: "Overview" })).toHaveAttribute("aria-selected", "true");
  const read = (cmd: string) => calls.some((c) => c.cmd === cmd);
  for (const cmd of ["pw_list", "pw_reports", "pw_users_list", "pw_sync_states"]) expect(read(cmd)).toBe(false);

  fireEvent.click(screen.getByRole("tab", { name: "Runner" }));
  await waitFor(() => expect(read("pw_list")).toBe(true));
  expect(read("pw_users_list")).toBe(false);
  expect(read("pw_sync_states")).toBe(false);

  // Back to Overview: the Runner stays mounted, hidden.
  fireEvent.click(screen.getByRole("tab", { name: "Overview" }));
  expect(screen.getByRole("region", { name: "Runner", hidden: true })).toBeInTheDocument();
});
