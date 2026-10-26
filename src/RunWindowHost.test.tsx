// The run window is lazy-loaded by App's `RunWindowHost`; nothing else
// mounts that path, so a broken import path or a renamed export in
// screens/AutoRun/ReplayPane would otherwise pass every suite.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { RunWindowHost } from "./App";
import { openRunSetup, resetBackgroundRun } from "./lib/backgroundRun";
import { resetModulesStore, setModulesForTest } from "./lib/modules";

beforeEach(() => setModulesForTest(["db_access", "auto_run"]));

afterEach(() => {
  resetModulesStore();
  resetBackgroundRun();
  clearMocks();
  localStorage.clear();
  vi.restoreAllMocks();
});

test("opening a run's window loads and shows it, and nothing shows before", async () => {
  mockIPC(() => null, { shouldMockEvents: true });
  resetBackgroundRun();
  render(<RunWindowHost />);
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  act(() => {
    openRunSetup({
      org: "acme",
      project: "Web",
      pbi: { id: 42, title: "Login work", work_item_type: "Product Backlog Item", project: "Web" },
      cases: [{ id: 1, title: "Alpha check" }],
      plan: null,
    });
  });
  const dialog = await screen.findByRole("dialog");
  expect(dialog).toHaveTextContent("Unattended run");
});
