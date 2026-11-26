import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { resetAppSimpleModeStore, setAppSimpleModeForTest } from "../../src/lib/appSimpleMode";
import type { Module } from "../../src/lib/modules";

// The screens themselves are not under test: each becomes an empty stand-in.
vi.mock("../../src/app/lazyScreens", () => {
  const Stub = () => null;
  return {
    AiBridge: Stub, ApiTemplates: Stub, AutoRun: Stub, CrystalReports: Stub, EditCases: Stub, ImportFile: Stub,
    ManageCases: Stub, ManualEntry: Stub, Playwright: Stub, RunTests: Stub, Settings: Stub, Suites: Stub, ViewCases: Stub,
  };
});

import CaseScreens from "../../src/app/CaseScreens";

afterEach(() => {
  clearMocks();
  resetAppSimpleModeStore();
});

function renderAutoRunTitle() {
  mockIPC((cmd) => (cmd === "get_app_settings" ? { autorun_mode: "simple" } : null));
  const noop = () => {};
  const props = {
    section: "autorun", org: "acme", project: "Web", homeProject: "Web", allProjects: false, pbi: null,
    modules: new Set<Module>(["auto_run"]), caseSelection: null, runFolder: null, manageFocus: null,
    settingsFill: false, startOnModules: false, setPbi: noop, setCaseSelection: noop, setRunFolder: noop,
    setManageFocus: noop, setSection: noop, setWorkMode: noop, setSettingsFill: noop, setStartOnModules: noop,
    goToSection: noop, changeHomeProject: noop,
  } as unknown as Parameters<typeof CaseScreens>[0];
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <CaseScreens {...props} />
    </QueryClientProvider>,
  );
}

test("app Basic mode hides Auto Test's Basic/Advanced switch but keeps the Settings button", () => {
  // The header's right side now always holds the Settings button (Basic mode
  // shows it too), so the wrapper stays; only the switch is gone.
  setAppSimpleModeForTest(true);
  renderAutoRunTitle();
  expect(screen.queryByRole("group", { name: "Auto Test mode" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Settings" })).toBeInTheDocument();
});

test("the Settings button opens the Auto Test settings dialog", async () => {
  setAppSimpleModeForTest(false);
  renderAutoRunTitle();
  expect(screen.queryByRole("heading", { name: "Auto Test settings" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(await screen.findByRole("heading", { name: "Auto Test settings" })).toBeInTheDocument();
  expect(screen.getByText("Browser page size")).toBeInTheDocument();
  expect(screen.getByText("Browsers at once")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(screen.queryByRole("heading", { name: "Auto Test settings" })).not.toBeInTheDocument();
});

test("with app Basic mode off the switch shows as before", () => {
  setAppSimpleModeForTest(false);
  renderAutoRunTitle();
  expect(screen.getByRole("group", { name: "Auto Test mode" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Advanced" })).toBeInTheDocument();
});
