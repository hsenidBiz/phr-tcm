// The App shell as a RELEASE build sees it: Auto Run, API Templates and
// Playwright exist only while their module is switched on - a release build
// starts with none on, and there is no build-kind shortcut in the webview
// (Rust starts a development build with every module on instead).
//
// Its own file because the app has to be imported after the dev flag is
// stubbed. Doing that inside App.test.tsx took `vi.resetModules()`, and every screen
// that file lazily loaded afterwards came from a second module graph with
// its own stores - the tour's "is it running" among them, which let a later
// tour test write through a guard it could no longer see.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, configure, render, screen } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, expect, test, vi } from "vitest";

// A whole-app mount, as in App.test.tsx: its clocks, for its reasons.
vi.setConfig({ testTimeout: 15_000 });
configure({ asyncUtilTimeout: 5_000 });

beforeAll(() => {
  vi.stubEnv("DEV", false);
});

afterAll(() => {
  vi.unstubAllEnvs();
});

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

// Switching a module off while one of its screens is the open tab must not
// leave its heading over a blank body: the redirect moves off it. API
// Templates hangs off Auto Run, so switching Auto Run off takes it too (Rust
// answers with what is left on).
test("switching Auto Run off while API Templates is open lands on Manual Entry", async () => {
  const { default: App } = await import("./App");
  const modules = await import("./lib/modules");
  const { QueryClient, QueryClientProvider } = await import("@tanstack/react-query");
  expect(import.meta.env.DEV).toBe(false);

  localStorage.setItem("tcm-v2-tour-done", "yes");
  localStorage.setItem(
    "tcm-v2-prefs",
    JSON.stringify({ org: "acme", project: "proj", section: "apitemplates", pbi: null, workMode: false }),
  );
  mockIPC((cmd) => {
    if (cmd === "resume_session") return { signed_in: true, account: "a@b.com" };
    if (cmd === "check_update") return null;
    if (cmd === "list_orgs") return [{ name: "acme", url: "" }];
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "modules_get") return ["db_access", "auto_run", "api_templates"];
    if (cmd === "modules_set") return ["db_access"];
    if (cmd === "api_templates_overview") return { origin: null, templates: [] };
    return undefined;
  });

  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <App />
    </QueryClientProvider>,
  );
  // On: the saved tab survives the restart and opens.
  expect(await screen.findByRole("heading", { name: "API Templates" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "API Templates" })).toBeInTheDocument();

  await act(() => modules.setModule("auto_run", false));
  expect(await screen.findByRole("heading", { name: "Manual Entry" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "API Templates" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Auto Run/ })).not.toBeInTheDocument();
  modules.resetModulesStore();
});

// The same restart on a release build whose modules are all off (a fresh
// install): the saved tab is left for Manual Entry once Rust has answered.
test("a saved API Templates tab is left on a fresh install", async () => {
  const { default: App } = await import("./App");
  const modules = await import("./lib/modules");
  const { QueryClient, QueryClientProvider } = await import("@tanstack/react-query");

  localStorage.setItem("tcm-v2-tour-done", "yes");
  localStorage.setItem(
    "tcm-v2-prefs",
    JSON.stringify({ org: "acme", project: "proj", section: "apitemplates", pbi: null, workMode: false }),
  );
  mockIPC((cmd) => {
    if (cmd === "resume_session") return { signed_in: true, account: "a@b.com" };
    if (cmd === "check_update") return null;
    if (cmd === "list_orgs") return [{ name: "acme", url: "" }];
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "modules_get") return [];
    return undefined;
  });

  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <App />
    </QueryClientProvider>,
  );
  expect(await screen.findByRole("heading", { name: "Manual Entry" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "API Templates" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Auto Run/ })).not.toBeInTheDocument();
  modules.resetModulesStore();
});
