import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import Settings from "../../src/screens/Settings";

// A changelog of ten versions or fewer has nothing to hold back, so
// Settings shows no "Show older versions" button.
vi.mock("../../src/lib/changelog", async (orig) => {
  const real = await orig<typeof import("../../src/lib/changelog")>();
  const entries = Array.from({ length: 10 }, (_, i) => ({
    version: `2.0.${10 - i}`,
    date: "2026-01-01",
    items: [`Change ${10 - i}`],
  }));
  return { ...real, CHANGELOG: entries, changelogFor: () => entries };
});
vi.mock("../../src/lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  for (const k of Object.keys(localStorage)) if (k.startsWith("tcm-v2-cache")) localStorage.removeItem(k);
});

test("ten versions or fewer show no Show older versions button", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Settings org="acme" project="Web" homeProject="Web" onHomeProjectChange={() => {}} onAllProjectsChange={() => {}} />
    </QueryClientProvider>,
  );
  fireEvent.click(await screen.findByRole("button", { name: "Show more (9 earlier versions)" }));
  expect(screen.getAllByText(/^Version 2\.0\.\d+$/)).toHaveLength(10);
  expect(screen.queryByRole("button", { name: /^Show older versions/ })).not.toBeInTheDocument();
});
