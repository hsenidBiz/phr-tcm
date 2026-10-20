// The Playwright tab: the clone folder, the health line, what the clone
// holds, and the Export dialog Auto Run's menu item opens here.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { COUNT_MS } from "../../components/CountTo";
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
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** prefers-reduced-motion is on: the summary's numbers are there at once. */
function reduceMotion() {
  vi.spyOn(window, "matchMedia").mockImplementation(
    (query: string) =>
      ({
        matches: query.includes("reduce"),
        media: query,
        onchange: null,
        addListener: () => {},
        removeListener: () => {},
        addEventListener: () => {},
        removeEventListener: () => {},
        dispatchEvent: () => false,
      }) as unknown as MediaQueryList,
  );
}

/** The header's Refresh (the Runner and Accounts tabs have their own). */
const headerRefresh = () =>
  screen.getAllByRole("button", { name: "Refresh" }).find((b) => !b.closest('[role="tabpanel"]')) as HTMLElement;

const HEALTHY: RepoStructure = {
  features: [
    {
      path: ["sl", "admin", "performance", "definition-wizard", "competencies"],
      navigation_key: "definition-wizard/competencies",
      spec_count: 1,
      case_count: 1,
      case_ids: [135300],
    },
    {
      path: ["sl", "admin", "performance", "rating-methods"],
      navigation_key: "performance/rating-methods",
      spec_count: 1,
      case_count: 3,
      case_ids: [135152, 135179, 135180],
    },
    {
      path: ["sl", "self", "smoke", "framework-smoke"],
      navigation_key: "smoke/framework-smoke",
      spec_count: 2,
      case_count: 0,
      case_ids: [],
    },
  ],
  generated_ids: [135152, 135179],
  user_keys: ["A", "B"],
  navigation_keys: ["performance/rating-methods", "definition-wizard/competencies"],
  health: [],
};

type Call = { cmd: string; args: Record<string, unknown> };

function mount(clone: string, structure: RepoStructure | (() => RepoStructure) = HEALTHY, opts: { listFails?: boolean } = {}) {
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
    if (cmd === "pw_list") {
      if (opts.listFails) throw "Playwright did not finish listing the tests.";
      return { tests: [], projects: [], last_report: null, load_errors: [] };
    }
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
  reduceMotion();
  const calls = mount("C:/repo");
  const health = await screen.findByRole("status", { name: "Repo health" });
  expect(await within(health).findByText("All good")).toBeInTheDocument();
  expect(screen.getByText("C:/repo")).toHaveAttribute("title", "C:/repo");
  const structure = screen.getByRole("region", { name: "Repo structure" });
  // The summary's tiles, not the tree's counts (which say "specs" too).
  const stat = (label: string) =>
    within(structure).getByText(label, { ignore: "ul *" }).previousElementSibling?.textContent;
  expect(stat("features")).toBe("3");
  expect(stat("specs")).toBe("4");
  expect(stat("test cases")).toBe("4");
  expect(stat("raw specs")).toBe("2");
  expect(stat("accounts")).toBe("2");
  expect(stat("navigation entries")).toBe("2");
  expect(calls.find((c) => c.cmd === "pw_repo_structure")?.args).toEqual({ clone: "C:/repo" });
});

test("the features are a folder tree: each folder under its parent, each feature under its folder", async () => {
  mount("C:/repo");
  await screen.findByText("All good");
  const tree = screen.getByRole("list", { name: "Features" });
  /** The list item a name is the row of. */
  const item = (name: string) => within(tree).getByText(name).closest("li") as HTMLElement;
  /** The names of the items directly under one (or at the top). */
  const under = (li: HTMLElement | null) =>
    [...(li ? (li.querySelector(":scope > ul")?.children ?? []) : tree.children)].map(
      (c) => c.querySelector(":scope > div span[title]")?.textContent,
    );
  expect(under(null)).toEqual(["sl"]);
  expect(under(item("sl"))).toEqual(["admin", "self"]);
  expect(under(item("admin"))).toEqual(["performance"]);
  expect(under(item("performance"))).toEqual(["definition-wizard", "rating-methods"]);
  expect(under(item("definition-wizard"))).toEqual(["competencies"]);
  expect(under(item("self"))).toEqual(["smoke"]);
  expect(under(item("smoke"))).toEqual(["framework-smoke"]);
  expect(under(item("rating-methods"))).toEqual([]);
  // Each row's counts, with what they count for a screen reader; a folder's
  // are the totals under it.
  const counts = (name: string) => item(name).querySelector(":scope > div")?.textContent?.replace(name, "");
  expect(counts("rating-methods")).toBe("1 spec3 test cases");
  expect(counts("framework-smoke")).toBe("2 specs0 test cases");
  expect(counts("performance")).toBe("2 specs4 test cases");
  expect(counts("sl")).toBe("4 specs4 test cases");
  // The full path is on hover.
  expect(within(tree).getByText("competencies")).toHaveAttribute("title", "sl/admin/performance/definition-wizard/competencies");

  // A folder folds and unfolds.
  const fold = screen.getByRole("button", { name: "Fold performance" });
  expect(fold).toHaveAttribute("aria-expanded", "true");
  fireEvent.click(fold);
  expect(within(tree).queryByText("rating-methods")).not.toBeInTheDocument();
  expect(within(tree).getByText("framework-smoke")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Unfold performance" }));
  expect(within(tree).getByText("rating-methods")).toBeInTheDocument();
  // A feature has nothing to fold.
  expect(screen.queryByRole("button", { name: "Fold rating-methods" })).not.toBeInTheDocument();
});

test("the summary counts up from 0 when Overview opens, and again each time it is opened", async () => {
  const frames = new Map<number, FrameRequestCallback>();
  let id = 0;
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
    id += 1;
    frames.set(id, cb);
    return id;
  });
  vi.stubGlobal("cancelAnimationFrame", (n: number) => frames.delete(n));
  const run = (at: number) =>
    act(() => {
      const now = [...frames.values()];
      frames.clear();
      for (const cb of now) cb(at);
    });
  mount("C:/repo");
  await screen.findByText("All good");
  const structure = await screen.findByRole("region", { name: "Repo structure" });
  const value = () => within(structure).getByText("features").previousElementSibling as HTMLElement;
  /** The digits on screen, and what a screen reader gets. */
  const seen = () => value().querySelector("[aria-hidden]")?.textContent ?? value().textContent;
  const heard = () => value().querySelector(".sr-only")?.textContent ?? value().textContent;
  expect(seen()).toBe("0");
  expect(heard()).toBe("3");
  run(0);
  run(COUNT_MS);
  expect(value().textContent).toBe("3");

  // Away and back: it counts again.
  fireEvent.click(screen.getByRole("tab", { name: "Sync" }));
  fireEvent.click(screen.getByRole("tab", { name: "Overview" }));
  expect(seen()).toBe("0");
  expect(heard()).toBe("3");
  run(1000);
  run(1000 + COUNT_MS);
  expect(value().textContent).toBe("3");
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
  // No count beside a tab's name.
  expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual(["Overview", "Runner", "Accounts", "Sync"]);
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
  for (const name of ["Runner", "Accounts", "Sync"]) fireEvent.click(screen.getByRole("tab", { name }));
  const PW = ["pw_repo_structure", "pw_list", "pw_reports", "pw_users_list", "pw_sync_states"];
  await waitFor(() => expect(PW.every((cmd) => calls.some((c) => c.cmd === cmd))).toBe(true));
  const count = (cmd: string) => calls.filter((c) => c.cmd === cmd).length;
  const before = Object.fromEntries(PW.map((cmd) => [cmd, count(cmd)]));

  // Going back and forth between the tabs reads nothing again.
  fireEvent.click(screen.getByRole("tab", { name: "Overview" }));
  fireEvent.click(screen.getByRole("tab", { name: "Runner" }));
  for (const cmd of PW) expect(count(cmd)).toBe(before[cmd]);

  // Refresh waits for the Accounts tab's counts (a repo command) to end.
  await waitFor(() => expect(headerRefresh()).toBeEnabled());
  fireEvent.click(headerRefresh());
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

test("pointing at the Runner tab starts its listing; a clone that cannot run is not listed", async () => {
  const calls = mount("C:/repo");
  await screen.findByText("All good");
  const listed = () => calls.filter((c) => c.cmd === "pw_list").length;
  fireEvent.pointerEnter(screen.getByRole("tab", { name: "Accounts" }));
  fireEvent.focus(screen.getByRole("tab", { name: "Sync" }));
  expect(listed()).toBe(0);
  fireEvent.pointerEnter(screen.getByRole("tab", { name: "Runner" }));
  await waitFor(() => expect(listed()).toBe(1));
  expect(calls.find((c) => c.cmd === "pw_list")?.args).toEqual({ clone: "C:/repo", refresh: false });
  // Pointing again asks for nothing more.
  fireEvent.pointerEnter(screen.getByRole("tab", { name: "Runner" }));
  expect(listed()).toBe(1);
  calls.unmount();

  const blocked = mount("C:/repo", { ...HEALTHY, health: ["NoNodeModules"] });
  await screen.findByRole("status", { name: "Repo health" });
  fireEvent.pointerEnter(screen.getByRole("tab", { name: "Runner" }));
  fireEvent.focus(screen.getByRole("tab", { name: "Runner" }));
  expect(blocked.some((c) => c.cmd === "pw_list")).toBe(false);
});

test("after a failed listing, pointing at the Runner tab does not list again", async () => {
  const calls = mount("C:/repo", HEALTHY, { listFails: true });
  await screen.findByText("All good");
  const listed = () => calls.filter((c) => c.cmd === "pw_list").length;
  fireEvent.click(screen.getByRole("tab", { name: "Runner" }));
  expect(await screen.findByText("Playwright did not finish listing the tests.")).toBeInTheDocument();
  expect(listed()).toBe(1);
  fireEvent.click(screen.getByRole("tab", { name: "Overview" }));
  fireEvent.pointerEnter(screen.getByRole("tab", { name: "Runner" }));
  fireEvent.focus(screen.getByRole("tab", { name: "Runner" }));
  await new Promise((r) => setTimeout(r, 50));
  expect(listed()).toBe(1);
});

test("Refresh waits while an account change or a Runner command is going", async () => {
  let release: () => void = () => {};
  const calls = mount("C:/repo");
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === "get_app_settings") return { playwright_clone: "C:/repo" };
    if (cmd === "pw_repo_structure") return HEALTHY;
    if (cmd === "pw_list") return { tests: [], projects: [], last_report: null, load_errors: [] };
    if (cmd === "pw_users_refs") return new Promise((r) => (release = () => r([])));
    if (cmd === "pw_reports" || cmd === "pw_users_list" || cmd === "pw_sync_states") return [];
    return null;
  });
  await screen.findByText("All good");
  const refresh = headerRefresh;
  expect(refresh()).toBeEnabled();
  // The Accounts tab's counts are a repo command: Refresh waits for it.
  fireEvent.click(screen.getByRole("tab", { name: "Accounts" }));
  await waitFor(() => expect(calls.some((c) => c.cmd === "pw_users_refs")).toBe(true));
  await waitFor(() => expect(refresh()).toBeDisabled());
  await act(async () => release());
  await waitFor(() => expect(refresh()).toBeEnabled());
  fireEvent.click(screen.getByRole("tab", { name: "Overview" }));
});
