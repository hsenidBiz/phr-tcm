// The App shell as a RELEASE build sees it: Auto Run and API Templates
// exist only while this machine's optional extras are unlocked.
//
// Its own file because the dev flag (lib/extras' AUTO_RUN_DEV) is read once,
// at module load: the app has to be imported after the flag is stubbed.
// Doing that inside App.test.tsx took `vi.resetModules()`, and every screen
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

// API Templates is offered exactly where Auto Run is. Resetting the extras
// while it is the open tab must not leave its heading over a blank body:
// the same redirect that moves off Auto Run moves off it.
test("resetting the extras while API Templates is open lands on Manual Entry", async () => {
  const { default: App } = await import("./App");
  const extras = await import("./lib/extras");
  const { QueryClient, QueryClientProvider } = await import("@tanstack/react-query");
  expect(extras.AUTO_RUN_DEV).toBe(false);

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
    if (cmd === "get_extras_unlocked") return true;
    if (cmd === "set_extras_unlocked") return null;
    if (cmd === "api_templates_overview") return { origin: null, templates: [] };
    return undefined;
  });

  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <App />
    </QueryClientProvider>,
  );
  // Unlocked: the saved tab survives the restart and opens.
  expect(await screen.findByRole("heading", { name: "API Templates" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "API Templates" })).toBeInTheDocument();

  await act(() => extras.setExtrasUnlocked(false));
  expect(await screen.findByRole("heading", { name: "Manual Entry" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "API Templates" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Auto Run/ })).not.toBeInTheDocument();
  extras.resetExtrasStore();
});
