// The sidebar's "new" mark (lib/newNav) clears however its screen is
// opened, not only by a click on the row: Ctrl+number and the command
// palette both count. And starting the app - Rust's answer to which modules
// are on - never marks anything.
//
// Its own file: a whole-app mount, kept out of App.test.tsx's long run.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { configure, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import App from "./App";
import { ALL_MODULES, resetModulesStore } from "./lib/modules";
import { NEW_NAV_KEY, isNewNav, newNavSnapshot, resetNewNavForTest } from "./lib/newNav";

// A whole-app mount, as in App.test.tsx: its clocks, for its reasons.
vi.setConfig({ testTimeout: 15_000 });
configure({ asyncUtilTimeout: 5_000 });

afterEach(() => {
  clearMocks();
  localStorage.clear();
  resetModulesStore();
  resetNewNavForTest();
});

function start(marks: Record<string, string[]> | null) {
  localStorage.setItem("tcm-v2-tour-done", "yes");
  if (marks) localStorage.setItem(NEW_NAV_KEY, JSON.stringify(marks));
  mockIPC((cmd) => {
    if (cmd === "modules_get") return [...ALL_MODULES];
    if (cmd === "resume_session") return { signed_in: true, account: "a@b.com" };
    if (cmd === "check_update") return null;
    if (cmd === "list_orgs") return [{ name: "acme", url: "" }];
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "plugin:event|listen") return 1;
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <App />
    </QueryClientProvider>,
  );
}

test("starting the app with every module on marks nothing", async () => {
  start(null);
  await screen.findByText("a@b.com");
  expect(await screen.findByRole("button", { name: "Auto Run" })).toBeInTheDocument();
  expect(newNavSnapshot()).toEqual({});
  expect(screen.queryByRole("button", { name: /\(new\)/ })).not.toBeInTheDocument();
});

test("Ctrl+number opening a marked tab clears its mark", async () => {
  start({ autorun: ["auto_run"] });
  await screen.findByText("a@b.com");
  expect(await screen.findByRole("button", { name: "Auto Run (new)" })).toBeInTheDocument();

  // Auto Run is the sixth numbered tab with every module on.
  fireEvent.keyDown(window, { key: "6", ctrlKey: true });
  expect(await screen.findByRole("heading", { name: "Auto Run" })).toBeInTheDocument();
  await waitFor(() => expect(isNewNav("autorun")).toBe(false));
  expect(screen.getByRole("button", { name: "Auto Run" })).not.toHaveClass("nav-new");
});

test("the command palette opening a marked tab clears its mark", async () => {
  start({ ai: ["writing_style"] });
  await screen.findByText("a@b.com");
  expect(await screen.findByRole("button", { name: "AI Bridge (new)" })).toBeInTheDocument();

  fireEvent.keyDown(window, { key: "k", ctrlKey: true });
  fireEvent.click(await screen.findByRole("option", { name: /AI Bridge/ }));
  expect(await screen.findByRole("heading", { name: "AI Bridge" })).toBeInTheDocument();
  await waitFor(() => expect(isNewNav("ai")).toBe(false));
  expect(screen.getByRole("button", { name: "AI Bridge" })).not.toHaveClass("nav-new");
});
